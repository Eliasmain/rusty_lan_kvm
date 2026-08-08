# Rusty KVM 🦀⚡

A lightning-fast, ultra-lightweight, headless cross-platform network KVM (Keyboard, Video, Mouse) switch built in **Rust**. Share a single mouse and keyboard seamlessly across multiple Linux machines over a local LAN connection using raw kernel inputs (`evdev`) and modern Wayland XDG Desktop Portals.

---

## Why Rusty KVM?

Traditional software KVM solutions like **Input Leap** (and its predecessors, Barrier and Synergy) are robust, but they often come with distinct frustrations:
* **Configuration Overhead:** Dealing with complex text configuration files, screen coordinate maps, and layout match errors.
* **TLS Certificate Hurdles:** Forcing local network certificate generation, fingerprint matching, and security handshake failures on trusted home networks.
* **GUI Bloat & Fragility:** Requiring heavy graphical user interfaces running in the system tray, which can occasionally desync, freeze, or fail silently under Wayland compositors.

### What Rusty KVM Solves:
* **Zero UI & Headless:** Completely headless architecture designed to run silently as background system daemons. No windows, no system tray clutter, and zero graphical overhead.
* **Pure Rust & Native Linux Stack:** Intercepts hardware inputs directly from the Linux kernel (`evdev`) via exclusive device grabbing, and injects inputs cleanly into Wayland sessions using official XDG Desktop Portals.
* **Zero-Config LAN Speed:** Bypasses unnecessary TLS handshake friction for trusted local environments while disabling TCP Nagle’s algorithm (`nodelay`) to achieve sub-millisecond response times.
* **Bulletproof Auto-Reconnection:** Built with resilient asynchronous retry loops. If a network drops or a machine restarts, the client persistently auto-reconnects without dropping your desktop portal session.
* **Panic-Free Architecture:** Utilizes lock-free atomics for mouse position tracking, eliminating the risk of thread-poisoning or system crashes if rapid edge-bouncing occurs.

---

## Features
- 🖱️ **Edge-Triggered Switching:** Move your cursor past the screen edge to instantly transition control to the remote machine.
- 📋 **Cross-PC Clipboard Sync:** Seamlessly copy text on one machine and paste it on the other over the TCP bridge.
- 🔄 **Persistent Auto-Reconnect:** Automatically recovers if the network drops or the server restarts.
- ⚙️ **Systemd Native:** Designed to start automatically on boot as system and user daemons.


Build the release binaries:

Bash
cargo build --release
Binaries will be compiled to target/release/server and target/release/client.

Setting Up Systemd Services
1. Server Setup (On the Host Machine)
Add your user to the input group so it can read raw device events:

Bash
sudo usermod -aG input $USER
Create the systemd service file:

Bash
sudo nano /etc/systemd/system/rusty-server.service
Paste the following configuration (adjusting your username and paths):

Ini, TOML
[Unit]
Description=Rusty KVM Server Daemon
After=network.target

[Service]
Type=simple
User=YOUR_USERNAME
WorkingDirectory=/path/to/rusty-kvm
ExecStart=/path/to/rusty-kvm/target/release/server
Restart=always
RestartSec=3

[Install]
WantedBy=multi-user.target
Enable and start the server:

Bash
sudo systemctl daemon-reload
sudo systemctl enable --now rusty-server
2. Client Setup (On the Remote Machine)
Copy your compiled client binary to the remote machine (e.g., placing it at ~/client).

Create a systemd user service directory:

Bash
mkdir -p ~/.config/systemd/user/
nano ~/.config/systemd/user/rusty-client.service
Paste the following configuration:

Ini, TOML
[Unit]
Description=Rusty KVM Client Wayland Portal Service
After=graphical-session.target
PartOf=graphical-session.target

[Service]
Type=simple
ExecStart=/home/YOUR_CLIENT_USER/client
Restart=always
RestartSec=3

[Install]
WantedBy=graphical-session.target
Enable user lingering so the service can run before logging into a graphical session:

Bash
sudo loginctl enable-linger YOUR_CLIENT_USER
Enable and start the client user service:

Bash
systemctl --user daemon-reload
systemctl --user enable --now rusty-client
How to Watch the Logs
Because both components run silently as background systemd services, you can stream live diagnostic logs directly from your terminal:

Watch Server Logs (Host Machine):

Bash
sudo journalctl -u rusty-server -f
Watch Client Logs (Remote Machine):

Bash
journalctl --user -u rusty-client -f
License
Distributed under the MIT License. See LICENSE for more information.
## Installation & Compilation

1. **Clone the repository:**
   ```bash
   git clone [https://github.com/YOUR_USERNAME/rusty-kvm.git](https://github.com/YOUR_USERNAME/rusty-kvm.git)
   cd rusty-kvm
