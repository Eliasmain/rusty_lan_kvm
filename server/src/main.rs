use evdev::{Device, EventSummary, KeyCode, RelativeAxisCode};
use protocol::{read_event, send_event, InputEvent};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use arboard::Clipboard;
use std::time::Duration;

fn is_mouse_button(code: u16) -> bool { (272..=287).contains(&code) }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind("0.0.0.0:9999").await?;
    println!("KVM Server Ready. Layout: Server [Left] | Client [Right]");

    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let (mut reader, mut writer) = socket.into_split();

        let client_active = Arc::new(AtomicBool::new(false));
        let keys_pressed = Arc::new(AtomicUsize::new(0)); 
        let virtual_x = Arc::new(Mutex::new(960.0));

        // Create a channel specifically for outbound events (Mouse, Keys, Clipboard)
        let (outbound_tx, mut outbound_rx) = tokio::sync::mpsc::channel::<InputEvent>(256);

        // 1. Task: Listen for Client Signals (Switch-back and Clipboard coming FROM client)
        let ca_t = client_active.clone();
        let vx_t = virtual_x.clone();
        let outbound_tx_reply = outbound_tx.clone();
        tokio::spawn(async move {
            loop {
                match read_event(&mut reader).await {
                    Ok(Some(InputEvent::MouseDown { button: 999 })) => {
                        // Clean up client ghost keys by sending releases via our channel
                        for k in [29, 42, 56, 125, 97, 100] { 
                            let _ = outbound_tx_reply.send(InputEvent::KeyUp { keycode: k }).await;
                        }
                        ca_t.store(false, Ordering::Relaxed);
                        let mut vx = vx_t.lock().unwrap(); *vx = 960.0;
                        println!(">>> Switched back to SERVER <<<");
                    }
                    Ok(Some(InputEvent::Clipboard { text })) => {
                        println!("Applying clipboard from Client...");
                        if let Ok(mut cb) = Clipboard::new() { let _ = cb.set_text(text); }
                    }
                    Ok(None) | Err(_) => break,
                    _ => {}
                }
            }
        });

        // 2. Task: Monitor LOCAL clipboard and send TO client via channel
        let outbound_tx_clipboard = outbound_tx.clone();
        tokio::spawn(async move {
            let mut cb = Clipboard::new().ok();
            let mut last = String::new();
            loop {
                if let Some(ref mut c) = cb {
                    if let Ok(t) = c.get_text() {
                        if !t.is_empty() && t != last {
                            last = t.clone();
                            let _ = outbound_tx_clipboard.send(InputEvent::Clipboard { text: t }).await;
                            println!("✓ Clipboard content detected on Server, sending to Client");
                        }
                    }
                }
                tokio::time::sleep(Duration::from_millis(600)).await;
            }
        });

        // 3. Task: Physical Input Device Handlers
        let mut devices = Vec::new();
        for i in 0..64 {
            let p = format!("/dev/input/event{}", i);
            if let Ok(d) = Device::open(&p) {
                let is_m = d.supported_relative_axes().map_or(false, |a| a.contains(RelativeAxisCode::REL_X));
                let is_k = d.supported_keys().map_or(false, |k| k.contains(KeyCode::KEY_A));
                if is_m || is_k { devices.push(d); }
            }
        }

        for mut dev in devices {
            let tx_c = outbound_tx.clone();
            let ca_c = client_active.clone();
            let vx_c = virtual_x.clone();
            let kp_c = keys_pressed.clone();
            std::thread::spawn(move || {
                let mut grabbed = false;
                let mut last_sw = std::time::Instant::now();
                loop {
                    let active = ca_c.load(Ordering::Relaxed);
                    if active != grabbed {
                        if active { let _ = dev.grab(); } else { let _ = dev.ungrab(); }
                        grabbed = active;
                    }
                    if let Ok(events) = dev.fetch_events() {
                        for ev in events {
                            match ev.destructure() {
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_X, v) => {
                                    if !ca_c.load(Ordering::Relaxed) {
                                        let mut vx = vx_c.lock().unwrap();
                                        *vx = (*vx + v as f64).clamp(0.0, 1920.0);
                                        if *vx >= 1919.0 && kp_c.load(Ordering::Relaxed) == 0 && last_sw.elapsed() > Duration::from_millis(400) {
                                            ca_c.store(true, Ordering::Relaxed);
                                            last_sw = std::time::Instant::now();
                                        }
                                    } else { let _ = tx_c.try_send(InputEvent::MouseMove { dx: v as f64, dy: 0.0 }); }
                                }
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_Y, v) => {
                                    if ca_c.load(Ordering::Relaxed) { let _ = tx_c.try_send(InputEvent::MouseMove { dx: 0.0, dy: v as f64 }); }
                                }
                                EventSummary::RelativeAxis(_, RelativeAxisCode::REL_WHEEL, v) => {
                                    if ca_c.load(Ordering::Relaxed) { let _ = tx_c.try_send(InputEvent::Scroll { dx: 0.0, dy: (v * 15) as f64 }); }
                                }
                                EventSummary::Key(_, code, state) => {
                                    let raw = code.code();
                                    let is_m = is_mouse_button(raw);
                                    if !is_m {
                                        if state == 1 { kp_c.fetch_add(1, Ordering::Relaxed); }
                                        else if state == 0 { kp_c.fetch_sub(1, Ordering::Relaxed); }
                                    }
                                    if ca_c.load(Ordering::Relaxed) {
                                        let event = match state {
                                            1 => Some(if is_m { InputEvent::MouseDown { button: raw as u32 } } else { InputEvent::KeyDown { keycode: raw as u32 } }),
                                            0 => Some(if is_m { InputEvent::MouseUp { button: raw as u32 } } else { InputEvent::KeyUp { keycode: raw as u32 } }),
                                            _ => None, 
                                        };
                                        if let Some(e) = event { let _ = tx_c.blocking_send(e); }
                                    }
                                }
                                _ => {}
                            }
                        }
                    } else { break; }
                }
            });
        }

        // 4. Main Outbound Loop: One single place that writes to the network
        while let Some(event) = outbound_rx.recv().await {
            if send_event(&mut writer, &event).await.is_err() { break; }
        }
        println!("Connection lost to client.");
    }
}
