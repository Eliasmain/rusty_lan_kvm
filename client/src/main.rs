use arboard::{Clipboard, SetExtLinux};
use ashpd::desktop::remote_desktop::{DeviceType, KeyState, RemoteDesktop};
use ashpd::desktop::PersistMode;
use ashpd::WindowIdentifier;
use protocol::{read_event, write_event, InputEvent};
use std::env;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::TcpStream;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    let server_ip = args.get(1).map(|s| s.as_str()).unwrap_or("192.168.50.1");
    let addr = format!("{}:9999", server_ip);

    println!("Initializing...");

    let remote_desktop = Arc::new(RemoteDesktop::new().await?);
    let session = Arc::new(remote_desktop.create_session().await?);
    let identifier = WindowIdentifier::default();

    remote_desktop
        .select_devices(
            &session,
            DeviceType::Keyboard | DeviceType::Pointer,
            None,
            // accept evrytime without prompting the user
            PersistMode::DoNot,
        )
        .await?;

    remote_desktop.start(&session, &identifier).await?;
    println!("Portal session active.");

    let virtual_x = Arc::new(tokio::sync::Mutex::new(0.0));
    let max_width = 1920.0;
    let min_width = 0.0;

    let switch_sent = Arc::new(AtomicBool::new(false));
    let has_entered = Arc::new(AtomicBool::new(false));

    // Shared writer handle for the background clipboard task across reconnections
    let current_writer: Arc<tokio::sync::Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Spawn clipboard monitoring task once
    let writer_for_clipboard = current_writer.clone();
    tokio::spawn(async move {
        let mut clipboard = match Clipboard::new() {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Failed to initialize clipboard: {}", e);
                return;
            }
        };
        let mut last_text = String::new();

        loop {
            if let Ok(current_text) = clipboard.get_text() {
                if !current_text.is_empty() && current_text != last_text {
                    last_text = current_text.clone();
                    let mut guard = writer_for_clipboard.lock().await;
                    if let Some(w) = &mut *guard {
                        let event = InputEvent::Clipboard {
                            text: current_text.clone(),
                        };
                        if let Err(e) = write_event(w, &event).await {
                            eprintln!("✗ Failed to send clipboard to server: {}", e);
                        } else {
                            println!(
                                "✓ Clipboard synced TO server: {}...",
                                if last_text.len() > 50 {
                                    format!("{}...", &last_text[..50])
                                } else {
                                    last_text.clone()
                                }
                            );
                        }
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    });

    // Outer infinite loop for auto-reconnection
    loop {
        println!("Attempting to connect to KVM server at 192.168.50.1:9999...");
        let stream = match tokio::time::timeout(
            Duration::from_secs(2),
            TcpStream::connect("192.168.50.1:9999"),
        )
        .await
        {
            Ok(Ok(s)) => {
                if let Err(e) = s.set_nodelay(true) {
                    eprintln!("Failed to set nodelay: {}", e);
                }
                println!("Connected to KVM server!");
                s
            }
            Ok(Err(e)) => {
                println!("Connection failed ({}), retrying in 2 seconds...", e);
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
            Err(_) => {
                println!("Connection timed out. Retrying in 2 seconds...");
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let (mut reader, writer) = stream.into_split();

        // Assign the writer handle so the clipboard task can use it
        {
            let mut guard = current_writer.lock().await;
            *guard = Some(writer);
        }

        // Inner event loop
        let mut disconnected = false;
        while let Some(event) = match read_event(&mut reader).await {
            Ok(ev) => ev,
            Err(_) => {
                disconnected = true;
                None
            }
        } {
            if disconnected {
                break;
            }

            match event {
                InputEvent::MouseMove { dx, dy } => {
                    let mut vx = virtual_x.lock().await;

                    if dx > 0.0 {
                        has_entered.store(true, Ordering::Relaxed);
                    }

                    *vx = (*vx + dx).clamp(min_width, max_width);

                    if *vx <= min_width + 1.0 && has_entered.load(Ordering::Relaxed) {
                        if !switch_sent.load(Ordering::Relaxed) {
                            println!(
                                ">>> Mouse hit LEFT edge. Switching back to LOCAL (Server) <<<"
                            );
                            switch_sent.store(true, Ordering::Relaxed);
                            has_entered.store(false, Ordering::Relaxed);

                            let switch_event = InputEvent::MouseDown { button: 999 };
                            let mut guard = current_writer.lock().await;
                            if let Some(w) = &mut *guard {
                                if write_event(w, &switch_event).await.is_err() {
                                    disconnected = true;
                                    drop(guard);
                                    break;
                                }
                            }
                            *vx = 0.0;
                        }
                    } else if *vx > 100.0 {
                        switch_sent.store(false, Ordering::Relaxed);
                    }

                    let rd = remote_desktop.clone();
                    let ses = session.clone();
                    tokio::spawn(async move {
                        let _ = rd.notify_pointer_motion(&ses, dx, dy).await;
                    });
                }
                InputEvent::MouseDown { button } => {
                    if button == 999 {
                        continue;
                    }
                    let _ = remote_desktop
                        .notify_pointer_button(&session, button as i32, KeyState::Pressed)
                        .await;
                }
                InputEvent::MouseUp { button } => {
                    if button == 999 {
                        continue;
                    }
                    let _ = remote_desktop
                        .notify_pointer_button(&session, button as i32, KeyState::Released)
                        .await;
                }
                InputEvent::Scroll { dx, dy } => {
                    let _ = remote_desktop
                        .notify_pointer_axis(&session, dx, -dy, false)
                        .await;
                }
                InputEvent::KeyDown { keycode } => {
                    let _ = remote_desktop
                        .notify_keyboard_keycode(&session, keycode as i32, KeyState::Pressed)
                        .await;
                }
                InputEvent::KeyUp { keycode } => {
                    let _ = remote_desktop
                        .notify_keyboard_keycode(&session, keycode as i32, KeyState::Released)
                        .await;
                }
                InputEvent::Clipboard { text } => {
                    println!("Applying clipboard from Server...");
                    // Change 'cb' to 'mut cb' here:
                    if let Ok(mut cb) = Clipboard::new() {
                        std::thread::spawn(move || {
                            let _ = cb.set().wait().text(text);
                        });
                        println!("✓ Client clipboard updated from server");
                    } else {
                        println!("✗ Failed to access client clipboard");
                    }
                }
            }
        }

        {
            let mut guard = current_writer.lock().await;
            *guard = None;
        }

        println!("Connection lost. Reconnecting in 2 seconds...");
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
