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

- [Rust](https://rustup.rs/) (stable)
- [Bun](https://bun.sh/)
- macOS with Xcode (for macOS native builds and iOS simulator) or Linux / Windows with standard build tools.
- At least one installed and authenticated agent CLI (e.g. `claude`, `codex`, `agy`).

---

## Quick Start

All scripts run via **Bun**:

### Desktop App (GPUI)

```bash
# Launch native app in development mode
bun run dev

# Open directly into a specific project folder
bun run dev -- --cwd /path/to/project

# Build release bundle
bun run build
```

> In development mode, press `r` + `Enter` in the terminal to rebuild and reload the application.

### Mobile App (Expo)

```bash
# Install mobile dependencies
bun run mobile:install

# Start Metro bundler
bun run mobile:dev

# Build native Rust transport and launch simulator
bun run mobile:ios       # iOS Simulator
bun run mobile:android   # Android Emulator
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
├── vendor/                  # Pinned and patched GPUI framework crates
├── scripts/                 # Build, packaging, and developer automation scripts
└── docs/native/             # Architecture, porting notes, and native guides
```

---

Optional mail infrastructure, Mailflare MCP, agent inboxes and agenda:
[setup and remaining deployment steps](docs/native/mail-workspace.md).

## License

[MIT](LICENSE). Provider names and logos are trademarks of their respective owners. See [NOTICE](NOTICE).
