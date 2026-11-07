<h1 align="center">MonoBOT</h1>

<p align="center">
  <strong>A GUI for your coding agents.</strong>
</p>

<p align="center">
  <img width="1680" height="1050" alt="MonoBOT with agent sessions open side by side" src="https://github.com/user-attachments/assets/89e04066-69de-473d-9aab-88b9ca3e3b7b" />
</p>

> [!NOTE]
> **Try the new [MonoBOT Native preview](https://github.com/hardbeat920/monocode/releases)**, our experimental Rust + GPUI app for macOS, Linux and Windows. Read the [preview details](#monobot-native-preview) before installing.

Works with your subscriptions on Claude Code, Codex, Cursor, Grok Build, OpenCode, Antigravity, Pi, omp, fx, Hermes Agent, and Devin. If they’re installed and logged in, MonoBOT can run them. Tabs are sessions. The composer is the input. MonoBOT does not sell tokens.

## Install

Install and sign in to at least one supported agent first. See [provider setup](docs/providers.md) for instructions.

| Platform | Native | Tauri |
| --- | --- | --- |
| macOS, Apple Silicon | [MonocodeNative_aarch64.dmg](https://github.com/hardbeat920/monocode/releases/download/native-v0.16.0-preview.13/MonocodeNative_0.16.0-preview.13_aarch64.dmg) | [MonoBOT.dmg](https://dl.usemono.dev/MonoBOT.dmg) |
| macOS, Intel | [MonocodeNative_x64.dmg](https://github.com/hardbeat920/monocode/releases/download/native-v0.16.0-preview.13/MonocodeNative_0.16.0-preview.13_x64.dmg) | [MonoCode_x64.dmg](https://dl.usemono.dev/MonoCode_x64.dmg) |
| Linux, x86_64 | [.deb, .rpm, or AppImage](https://github.com/hardbeat920/monocode/releases/tag/native-v0.16.0-preview.13) | [.deb, .rpm, or AppImage](https://github.com/hardbeat920/monocode/releases/latest) |
| Windows, x86_64 | [Installer](https://github.com/hardbeat920/monocode/releases/download/native-v0.16.0-preview.13/MonocodeNative_0.16.0-preview.13_x64-setup.exe) | [Installer](https://github.com/hardbeat920/monocode/releases/latest) |

On macOS, open the DMG and drag MonoBOT to Applications. On Windows, run the installer. See [Linux setup](docs/install.md) for dependencies and package instructions.

## Get started

Open a project folder, choose an agent, and send a message. Each tab is a session. Split panes to work with sessions side by side.

- Keep conversations organized by project.
- Use separate Git worktrees for parallel tasks.
- Edit files and review changes in the app.
- Start a message with `/operator` to let an agent open sessions and manage worktrees and notes.

Monos are experimental agents that work across your projects. They keep a memory and can run scheduled tasks you set up with them.

MonoBOT is still early. If something breaks, [report a bug](https://github.com/hardbeat920/monocode/issues).

## Guides

- [Provider setup](docs/providers.md)
- [Linux installation](docs/install.md)
- [Agent access with /operator](docs/agent-access.md)
- [Remote sessions, experimental](docs/remote-access.md)

## Build from source

You’ll need Node.js 20+, [pnpm](https://pnpm.io/installation) (`corepack enable` picks the pinned version) and a stable Rust toolchain. See [build setup](docs/building.md) for platform dependencies and packaging.

The mobile app is a separate Expo project in [`mobile/`](mobile/README.md). Run it with `cd mobile && pnpm install && pnpm start`.

```bash
pnpm install
pnpm tauri dev
```

Small, focused contributions are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## MonoBOT Native preview

The Rust + GPUI app is an experimental preview. React/Tauri remains the
production app.

Download a packaged preview for macOS, Linux or Windows from the
[releases page](https://github.com/hardbeat920/monocode/releases). See
[known limitations](docs/native/parity.md) before testing.

You can install it alongside MonoBOT, but **the preview uses your existing
MonoBOT database and saves changes there**. Back up your data with the app
closed before testing, and avoid editing the same conversations in both apps
at once.

To try GPUI from this checkout with Node.js 22+, Rust and Xcode on macOS:

```sh
pnpm native:dev     # launch with a separate development database
pnpm native:build   # create an optimized app using installed MonoBOT data
```

Type `r` then Enter in the development terminal to rebuild and restart; there is
no hot reload. The release app saves changes to your installed MonoBOT data.
See the [native guide](docs/native/README.md) for prerequisites, build output,
checks and preview limitations.

## Contributors

Thanks to everyone who contributes to MonoBOT!

[![MonoBOT contributors](https://contrib.rocks/image?repo=hardbeat920/monocode)](https://github.com/hardbeat920/monocode/graphs/contributors)

Contributor image by [contrib.rocks](https://contrib.rocks).

## License

[MIT](LICENSE). Provider names and logos are trademarks of their owners. See [NOTICE](NOTICE).
