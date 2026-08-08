use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode};
use protocol::{read_event, send_event, InputEvent};
use std::env;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

fn is_mouse_button(code: u16) -> bool {
    (272..=287).contains(&code)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Optional CLI argument for screen max_width (defaults to 1920.0)
    let args: Vec<String> = env::args().collect();
    let max_width: f64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(1920.0);

    let listener = TcpListener::bind("0.0.0.0:9999").await?;
    println!("KVM Server listening on port 9999...");
    println!("Layout: Server [Left] <---> Client [Right]");
    println!("Server screen max width set to: {}px", max_width);

    loop {
        let (socket, addr) = listener.accept().await?;
        socket.set_nodelay(true)?;

        println!("Client connected from {}", addr);

        let client_active = Arc::new(AtomicBool::new(false));
        let client_active_reader = client_active.clone();

        let virtual_x = Arc::new(AtomicU64::new((max_width / 2.0).to_bits()));
        let virtual_x_reader = virtual_x.clone();

        let (mut reader, mut writer) = socket.into_split();

        // Listen for switch-back signal from client (Left edge hit on client)
        tokio::spawn(async move {
            loop {
                match read_event(&mut reader).await {
                    Ok(Some(event)) => {
                        if let InputEvent::MouseDown { button: 999 } = event {
                            client_active_reader.store(false, Ordering::Relaxed);
                            virtual_x_reader.store((max_width / 2.0).to_bits(), Ordering::Relaxed);
                            println!(
                                ">>> Switched back to LOCAL (Server Mode) via Client Left Edge <<<"
                            );
                        }
                    }
                    Ok(None) => {
                        println!("Client gracefully disconnected.");
                        break;
                    }
                    Err(e) => {
                        eprintln!("✗ Server read error from client: {}", e);
                        break;
                    }
                }
            }
        });

        let mut devices = Vec::new();
        for i in 0..32 {
            let path = format!("/dev/input/event{}", i);
            if Path::new(&path).exists() {
                if let Ok(dev) = Device::open(&path) {
                    let is_mouse = dev
                        .supported_relative_axes()
                        .map_or(false, |axes| axes.contains(RelativeAxisCode::REL_X));
                    let is_keyboard = dev
                        .supported_keys()
                        .map_or(false, |keys| keys.contains(KeyCode::KEY_A));

                    if is_mouse || is_keyboard {
                        println!(
                            "Captured input device: {} ({})",
                            path,
                            dev.name().unwrap_or("Unknown")
                        );
                        devices.push(dev);
                    }
                }
            }
        }

        if devices.is_empty() {
            eprintln!("No input devices found! Ensure your user is in the 'input' group.");
            continue;
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel::<InputEvent>(16);

        for mut dev in devices {
            let tx_clone = tx.clone();
            let client_active_clone = client_active.clone();
            let virtual_x_thread = virtual_x.clone();

            std::thread::spawn(move || {
                let mut last_grabbed_state = false;
                let mut last_switch_time = Instant::now();
                let cooldown = Duration::from_millis(400);

                loop {
                    let should_be_active = client_active_clone.load(Ordering::Relaxed);
                    if should_be_active != last_grabbed_state {
                        if should_be_active {
                            if dev.grab().is_ok() {
                                println!("Device grabbed for remote control.");
                            }
                        } else {
                            if dev.ungrab().is_ok() {
                                println!("Device released back to local control.");
                            }
                        }
                        last_grabbed_state = should_be_active;
                    }

                    match dev.fetch_events() {
                        Ok(events) => {
                            for ev in events {
                                match ev.destructure() {
                                    EventSummary::RelativeAxis(
                                        _,
                                        RelativeAxisCode::REL_X,
                                        value,
                                    ) => {
                                        if !client_active_clone.load(Ordering::Relaxed) {
                                            let mut vx = f64::from_bits(
                                                virtual_x_thread.load(Ordering::Relaxed),
                                            );
                                            vx = (vx + value as f64).clamp(0.0, max_width);
                                            virtual_x_thread.store(vx.to_bits(), Ordering::Relaxed);

                                            if vx >= max_width - 1.0
                                                && last_switch_time.elapsed() > cooldown
                                            {
                                                client_active_clone.store(true, Ordering::Relaxed);
                                                println!(
                                                    ">>> Switched to CLIENT (Remote Mode) <<<"
                                                );
                                                virtual_x_thread
                                                    .store(100.0f64.to_bits(), Ordering::Relaxed);
                                                last_switch_time = Instant::now();
                                            }
                                        } else {
                                            let _ = tx_clone.try_send(InputEvent::MouseMove {
                                                dx: value as f64,
                                                dy: 0.0,
                                            });
                                        }
                                    }
                                    EventSummary::RelativeAxis(
                                        _,
                                        RelativeAxisCode::REL_Y,
                                        value,
                                    ) => {
                                        if client_active_clone.load(Ordering::Relaxed) {
                                            let _ = tx_clone.blocking_send(InputEvent::MouseMove {
                                                dx: 0.0,
                                                dy: value as f64,
                                            });
                                        }
                                    }
                                    EventSummary::RelativeAxis(
                                        _,
                                        RelativeAxisCode::REL_WHEEL,
                                        value,
                                    ) => {
                                        if client_active_clone.load(Ordering::Relaxed) {
                                            let _ = tx_clone.blocking_send(InputEvent::Scroll {
                                                dx: 0.0,
                                                dy: (value * 10) as f64,
                                            });
                                        }
                                    }
                                    EventSummary::Key(_, code, state) => {
                                        let raw_code = code.code();
                                        if client_active_clone.load(Ordering::Relaxed) {
                                            let event = if is_mouse_button(raw_code) {
                                                if state == 1 {
                                                    InputEvent::MouseDown {
                                                        button: raw_code as u32,
                                                    }
                                                } else {
                                                    InputEvent::MouseUp {
                                                        button: raw_code as u32,
                                                    }
                                                }
                                            } else {
                                                if state == 1 {
                                                    InputEvent::KeyDown {
                                                        keycode: raw_code as u32,
                                                    }
                                                } else {
                                                    InputEvent::KeyUp {
                                                        keycode: raw_code as u32,
                                                    }
                                                }
                                            };
                                            let _ = tx_clone.blocking_send(event);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }

        while let Some(event) = rx.recv().await {
            if let Err(e) = send_event(&mut writer, &event).await {
                eprintln!("Connection lost: {}", e);
                break;
            }
        }
    }
}
