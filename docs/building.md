# Build from source

MonoBOT's desktop app is Rust + GPUI (`crates/monocode-gpui`). You need a current
stable Rust toolchain, Node.js 22+ and [Bun](https://bun.sh/) for the root
scripts, and at least one signed-in agent CLI ([provider setup](providers.md)).
No JavaScript dependencies are installed for the desktop build.

## Platform prerequisites

- **macOS:** Xcode with the macOS components (SDK 26 or newer for release builds).
- **Debian/Ubuntu:** `clang cmake ninja-build pkg-config libfontconfig1-dev
  libfreetype6-dev libxcb1-dev libxcb-shape0-dev libxcb-xfixes0-dev
  libxcb-randr0-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev
  libasound2-dev libx11-xcb-dev libegl1-mesa-dev libvulkan-dev libdbus-1-dev`.
  Packaging also needs `squashfs-tools`, `desktop-file-utils` and `appimagetool`.
- **Windows:** the `stable-x86_64-pc-windows-msvc` toolchain; packaging needs NSIS.

## Run and build

From the repository root:

```sh
bun run dev                       # development build, separate database
bun run dev -- --cwd /path/to/project
bun run build                     # optimized build (and the .app on macOS)
bun run check                     # script tests, fmt, clippy -D warnings
bun run test                      # core and GPUI tests
```

`bun run dev` rebuilds and restarts when you type `r` then Enter. See the
[desktop guide](native/README.md) for data paths, fixtures and dictation
libraries.

## Packages

CI packages every platform with `scripts/package-native.mjs` after a release
build:

```sh
cargo build --locked --manifest-path crates/monocode-gpui/Cargo.toml -p monocode-gpui --release
node scripts/package-native.mjs <macos|linux|rpm|windows> target/release/monocode-gpui native-dist <arch>
```

It writes a signed/notarized `.app` and `.dmg` on macOS (when the Apple
credentials are set), `.deb`, AppImage or `.rpm` on Linux, and an NSIS
installer on Windows, all under `native-dist/`.

## Releases and updates

Pushing a tag `native-vX.Y.Z` runs [native-release.yml](../.github/workflows/native-release.yml).
It builds all platforms, signs the update packages and the `latest.json`
manifest, verifies them with the app's own update client, publishes a GitHub
release and uploads the files to the `monobot-releases` R2 bucket under
`native-preview/`. The app reads its feed from
`<R2_PUBLIC_BASE_URL>/native-preview/latest.json`.

Repository secrets the workflow needs:

| Secret | Value |
| --- | --- |
| `R2_ACCOUNT_ID`, `R2_BUCKET_NAME`, `R2_PUBLIC_BASE_URL` | Cloudflare account, `monobot-releases`, its public `r2.dev` URL |
| `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` | An R2 API token limited to `monobot-releases` (Object Read & Write) |
| `MONOBOT_UPDATER_PRIVATE_KEY`, `MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD`, `MONOBOT_UPDATER_PUBKEY` | The minisign key pair that signs updates |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8` | Developer ID signing and notarization |

The remote host has its own [build and packaging instructions](remote-access.md#release-packaging).
The mobile app builds from `mobile/` ([mobile README](../mobile/README.md)).

Read [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines.
