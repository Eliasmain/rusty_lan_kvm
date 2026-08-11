use arboard::{Clipboard, SetExtLinux};
use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode};
use protocol::{read_event, send_event, InputEvent, Res};
use std::collections::HashSet;
use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::net::TcpListener;

fn is_mouse_button(code: u16) -> bool {
    (272..=287).contains(&code)
}

/// Immediately grabs or ungrabs all raw device file descriptors in kernel space.
fn set_grab_all(fds: &[RawFd], grab: bool) {
    let arg: libc::c_int = if grab { 1 } else { 0 };
    for &fd in fds {
        unsafe {
            // EVIOCGRAB ioctl code = 0x40044590
            libc::ioctl(fd, 0x40044590, arg);
        }
    }
}

#[tokio::main]
async fn main() -> Res<()> {
    let listener = TcpListener::bind("0.0.0.0:9999").await?;
    println!("KVM Server Ready. Layout: Server [Left] | Client [Right]");

    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let (mut reader, mut writer) = socket.into_split();

        let client_active = Arc::new(AtomicBool::new(false));
        let keys_pressed = Arc::new(AtomicUsize::new(0));
        let virtual_x = Arc::new(Mutex::new(960.0)); // Center of server screen

        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<InputEvent>(256);
        let last_clip = Arc::new(Mutex::new(String::new()));

        // Discover and open input devices
        let mut devices = Vec::new();
        let mut seen_keyboard_names: HashSet<String> = HashSet::new();
        for i in 0..64 {
            let p = format!("/dev/input/event{}", i);
            if let Ok(d) = Device::open(&p) {
                let is_m = d
                    .supported_relative_axes()
                    .map_or(false, |a| a.contains(RelativeAxisCode::REL_X));
                let is_k = d
                    .supported_keys()
                    .map_or(false, |k| k.contains(KeyCode::KEY_A));

                if is_m || is_k {
                    if is_k && !is_m {
                        let name_str = format!("{:?}", d.name());
                        if !seen_keyboard_names.insert(name_str.clone()) {
                            println!("Skipping duplicate keyboard node {} ({})", p, name_str);
                            continue;
                        }
                    }
                    println!("Using input device {} ({:?})", p, d.name());
                    devices.push(d);
                }
            }
        }

        // Collect raw file descriptors for immediate atomic grabbing
        let raw_fds: Arc<Vec<RawFd>> = Arc::new(devices.iter().map(|d| d.as_raw_fd()).collect());

        // 1. Task: Listen for Client Signals
        let ca_t = client_active.clone();
        let vx_t = virtual_x.clone();
        let kp_t = keys_pressed.clone();
        let outbound_tx_reply = outbound_tx.clone();
        let last_clip_in = last_clip.clone();
        let fds_signal = raw_fds.clone();

        tokio::spawn(async move {
            loop {
                match read_event(&mut reader).await {
                    Ok(Some(InputEvent::MouseDown { button: 999 })) => {
                        println!("[SERVER] Received switch-back signal from client.");

                        // Immediately release kernel grab on all devices
                        set_grab_all(&fds_signal, false);
                        ca_t.store(false, Ordering::Relaxed);

                        kp_t.store(0, Ordering::Relaxed);

                        // Send safety releases for common modifiers on switch-back
                        for k in [29, 42, 56, 125, 97, 100] {
                            let _ = outbound_tx_reply
                                .send(InputEvent::KeyUp { keycode: k })
                                .await;
                        }

                        let mut vx = vx_t.lock().unwrap();
                        *vx = 960.0; // Reset to center
                        println!(">>> Switched back to SERVER <<<");
                    }
                    Ok(Some(InputEvent::Clipboard { text })) => {
                        println!("Applying clipboard from Client...");
                        {
                            let mut last = last_clip_in.lock().unwrap();
                            *last = text.clone();
                        }
                        if let Ok(mut cb) = Clipboard::new() {
                            std::thread::spawn(move || {
                                let _ = cb.set().wait().text(text);
                            });
                        }
                    }
                    Ok(None) | Err(_) => break,
                    _ => {}
                }
            }
        });

        // 2. Task: Monitor LOCAL clipboard
        let outbound_tx_clipboard = outbound_tx.clone();
        let last_clip_out = last_clip.clone();
        tokio::spawn(async move {
            let mut cb = Clipboard::new().ok();
            loop {
                if let Some(c) = cb.as_mut() {
                    if let Ok(t) = c.get_text() {
                        let should_send = {
                            let mut last = last_clip_out.lock().unwrap();
                            if !t.is_empty() && t != *last {
                                *last = t.clone();
                                true
                            } else {
                                false
                            }
                        };

                        if should_send {
                            let _ = outbound_tx_clipboard
                                .send(InputEvent::Clipboard { text: t })
                                .await;
                            println!("✓ Clipboard content detected on Server, sending to Client");
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
        });

        // 3. Spawn Physical Input Device Handlers
        let fds_boundary = raw_fds.clone();
        for mut dev in devices {
            let tx_c = outbound_tx.clone();
            let ca_c = client_active.clone();
            let vx_c = virtual_x.clone();
            let kp_c = keys_pressed.clone();
            let fds_c = fds_boundary.clone();

            std::thread::spawn(move || {
                let mut last_sw = std::time::Instant::now();
                loop {
                    if let Ok(events) = dev.fetch_events() {
                        for ev in events {
                            match ev.destructure() {
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_X, v) => {
                                    if !ca_c.load(Ordering::Relaxed) {
                                        let mut vx = vx_c.lock().unwrap();
                                        *vx = (*vx + v as f64).clamp(0.0, 1920.0);

                                        if *vx >= 1919.0
                                            && kp_c.load(Ordering::Relaxed) == 0
                                            && last_sw.elapsed() > Duration::from_millis(400)
                                        {
                                            println!(">>> Switching to CLIENT <<<");

                                            // 1. Set active state
                                            ca_c.store(true, Ordering::Relaxed);
                                            // 2. Immediately grab ALL devices in kernel space
                                            set_grab_all(&fds_c, true);

                                            last_sw = std::time::Instant::now();
                                        }
                                    } else {
                                        let _ = tx_c.try_send(InputEvent::MouseMove {
                                            dx: v as f64,
                                            dy: 0.0,
                                        });
                                    }
                                }
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_Y, v) => {
                                    if ca_c.load(Ordering::Relaxed) {
                                        let _ = tx_c.try_send(InputEvent::MouseMove {
                                            dx: 0.0,
                                            dy: v as f64,
                                        });
                                    }
                                }
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_WHEEL, v) => {
                                    if ca_c.load(Ordering::Relaxed) {
                                        let _ = tx_c.try_send(InputEvent::Scroll {
                                            dx: 0.0,
                                            dy: (v * 15) as f64,
                                        });
                                    }
                                }
                                EventSummary::Key(_, code, state) => {
                                    let raw = code.code();
                                    let raw_u32 = raw as u32;
                                    let is_m = is_mouse_button(raw);

                                    if !is_m {
                                        if state == 1 {
                                            kp_c.fetch_add(1, Ordering::Relaxed);
                                        } else if state == 0 {
                                            let current = kp_c.load(Ordering::Relaxed);
                                            if current > 0 {
                                                kp_c.fetch_sub(1, Ordering::Relaxed);
                                            }
                                        }
                                    }

                                    if ca_c.load(Ordering::Relaxed) {
                                        match state {
                                            1 => {
                                                let e = if is_m {
                                                    InputEvent::MouseDown { button: raw_u32 }
                                                } else {
                                                    InputEvent::KeyDown { keycode: raw_u32 }
                                                };
                                                let _ = tx_c.blocking_send(e);
                                            }
                                            0 => {
                                                let e = if is_m {
                                                    InputEvent::MouseUp { button: raw_u32 }
                                                } else {
                                                    InputEvent::KeyUp { keycode: raw_u32 }
                                                };
                                                let _ = tx_c.blocking_send(e);
                                            }
                                            _ => {} // Ignore state 2 (hardware repeat)
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else {
                        break;
                    }
                }
            });
        }

        // 4. Main Outbound Loop
        while let Some(event) = outbound_rx.recv().await {
            if send_event(&mut writer, &event).await.is_err() {
                break;
            }
        }

        // Clean up grab if client disconnects
        set_grab_all(&raw_fds, false);
        println!("Connection lost to client.");
    }
}
