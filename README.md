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



## Installation & Compilation

1. **Clone the repository:**
   ```bash
   git clone [https://github.com/YOUR_USERNAME/rusty-kvm.git](https://github.com/YOUR_USERNAME/rusty-kvm.git)
   cd rusty-kvm
