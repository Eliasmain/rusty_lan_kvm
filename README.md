# Rusty KVM 🦀⚡

A lightning-fast, headless cross-platform network KVM (Keyboard, Video, Mouse) switch built in **Rust**. It allows you to seamlessly share one mouse and keyboard across multiple Linux machines over an Ethernet LAN connection using raw kernel inputs (`evdev`) and Wayland XDG Desktop Portals.

---

## Features
- **Zero UI / Headless Architecture:** Runs silently as a background daemon.
- **Edge-Triggered Switching:** Move your mouse off the edge of your screen to transition control instantly.
- **Bi-directional Clipboard Sync:** Seamlessly copy text on one machine and paste it on the other.
- **Auto-Reconnection:** The client persistently tries to reconnect if the network drops or the server restarts.
- **Optimized for Speed:** Bypasses TCP Nagle’s algorithm (`nodelay`) for sub-millisecond input response.

---

## Prerequisites
- **Rust Toolchain** (latest stable via [rustup.rs](https://rustup.rs/))
- **Server Machine:** Linux with user permissions for the `input` group.
- **Client Machine:** A Wayland compositor (e.g., GNOME, Hyprland, Sway) supporting **XDG Desktop Portals**.
