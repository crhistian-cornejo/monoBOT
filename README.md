<h1 align="center">MonoBOT</h1>

<p align="center">
  <strong>A fast, native GUI for your coding agents — Desktop (Rust + GPUI) & Mobile (Expo).</strong>
</p>

<p align="center">
  <img width="1680" height="1050" alt="MonoBOT with agent sessions open side by side" src="https://github.com/user-attachments/assets/89e04066-69de-473d-9aab-88b9ca3e3b7b" />
</p>

MonoBOT provides a unified native desktop and mobile interface for your existing coding agent CLI subscriptions: **Claude Code, Codex, Cursor, Grok Build, OpenCode, Antigravity, Pi, omp, fx, Hermes Agent, and Devin**.

No extra API token fees or middleman servers — it talks directly to your locally installed agent CLIs and communicates securely between desktop and mobile over peer-to-peer encrypted QUIC (TLS 1.3).

---

## Architecture

- **Desktop (`crates/monocode-gpui`)**: 100% native Rust GUI built on GPUI (GPU-accelerated, high frame rate, sub-millisecond input latency).
- **Core Engine (`crates/monocode-core`)**: Provider transports, session persistence, SQLite storage, local git/file orchestration, and background bots.
- **Mobile Companion (`mobile/`)**: React Native & Expo app for iOS and Android with live session viewing, chat composer, and project navigation.
- **Transport (`crates/mobile-link`)**: Mutual TLS 1.3 / QUIC bridge pairing mobile devices with the desktop engine.

---

## Prerequisites

- [Rust](https://rustup.rs/) (stable). The desktop app needs nothing else: its
  build, packaging and release tooling is a Rust crate (`cargo xtask`).
- [pnpm](https://pnpm.io/) and Node.js for the mobile app only.
- macOS with Xcode (for macOS native builds and iOS simulator) or Linux / Windows with standard build tools.
- At least one installed and authenticated agent CLI (e.g. `claude`, `codex`, `agy`).

---

## Quick Start

### Desktop App (GPUI)

```bash
# Launch native app in development mode
cargo xtask dev

# Open directly into a specific project folder
cargo xtask dev --cwd /path/to/project

# Build release bundle
cargo xtask build

# Every desktop command
cargo xtask help
```

> In development mode, press `r` + `Enter` in the terminal to rebuild and reload the application.

### Mobile App (Expo)

```bash
# Install mobile dependencies
pnpm --dir mobile install

# Start Metro bundler
pnpm --dir mobile start

# Build native Rust transport and launch simulator
pnpm --dir mobile ios       # iOS Simulator
pnpm --dir mobile android   # Android Emulator
```

---

## Project Structure

```text
├── assets/                  # Application icons, macOS entitlements, and bundled assets
├── crates/
│   ├── monocode-gpui/       # Native GPUI desktop application
│   ├── monocode-core/       # Business logic, provider transports, SQLite database
│   ├── mobile-link/         # Rust QUIC/TLS pairing library for desktop and mobile
│   ├── dictation/           # Local speech-to-text (sherpa-onnx)
│   ├── voice/               # Local speech synthesis playback
│   ├── email/               # IMAP mail sync, triage and SMTP/HTTPS delivery
│   └── http/                # Shared HTTP and TLS certificate verification
├── mobile/                  # React Native / Expo mobile companion app
├── host/                    # Remote host (Node) the desktop installs over SSH
├── deploy/cloudflare-mail/  # Optional inbound mail Worker (Rust → WebAssembly)
├── vendor/                  # Pinned and patched GPUI framework crates
├── xtask/                   # `cargo xtask`: dev runner, builds, packaging, releases
├── scripts/                 # JavaScript generators for embedded bundles, perf replay
└── docs/                    # done/ (guides), in-progress/ (open work), old/ (history)
```

---

Optional mail infrastructure, Mailflare MCP, agent inboxes and agenda:
[setup and remaining deployment steps](docs/in-progress/mail-workspace.md).

## License

MonoBOT is developed by Crhistian Cornejo. It started as a fork of Monocode by Nick, also MIT-licensed, whose copyright notice is kept in [LICENSE](LICENSE) as the license requires.

[MIT](LICENSE). Provider names and logos are trademarks of their respective owners. See [NOTICE](NOTICE).
