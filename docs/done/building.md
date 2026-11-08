# Build from source

MonoBOT's desktop app is Rust + GPUI (`crates/monocode-gpui`). You need a current
stable Rust toolchain and at least one signed-in agent CLI ([provider setup](providers.md)).
The build, packaging and release tooling is Rust as well: the `xtask` crate,
run as `cargo xtask <command>` (`cargo xtask help` lists them). No Node.js or
JavaScript dependencies are needed for the desktop build.

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
cargo xtask dev                   # development build, separate database
cargo xtask dev --cwd /path/to/project
cargo xtask build                 # optimized build (and the .app on macOS)
cargo xtask check                 # fmt, clippy -D warnings, xtask tests
cargo xtask test                  # core and GPUI tests
cargo xtask cargo <args>          # plain cargo, with the dictation libraries found
```

`cargo xtask dev` rebuilds and restarts when you type `r` then Enter. See the
[desktop guide](desktop.md) for data paths, fixtures and dictation
libraries.

## Packages

CI packages every platform with `cargo xtask package` after a release build:

```sh
cargo build --locked -p monocode-gpui --release
MONOCODE_NATIVE_VERSION=0.16.0-preview.1 \
  cargo xtask package <macos|linux|rpm|windows> target/release/monocode-gpui native-dist <arch>
```

It writes a signed/notarized `.app` and `.dmg` on macOS (it needs the Apple
credentials), a `.deb` and AppImage on Linux, an `.rpm` on Enterprise Linux and
an NSIS installer on Windows, all under `native-dist/` and named
`MonoBOT_<version>_…`, `monobot_<version>_amd64.deb` and
`monobot-<version>.x86_64.rpm`. The tools that only exist as programs
(`codesign`, `notarytool`, `hdiutil`, `dpkg-deb`, `appimagetool`, `rpmbuild`,
`makensis`) must be installed; the NSIS script template is
`xtask/packaging/native-installer.nsi`. `cargo xtask bundle-macos` wraps an
existing build in `MonoBOT Native.app` without packaging it.

## Releases and updates

Pushing a tag `native-vX.Y.Z` runs [native-release.yml](../../.github/workflows/native-release.yml).
It builds all platforms, signs the update packages and the `latest.json`
manifest (`cargo xtask release native-dist`), verifies them with the app's own
update client (`cargo xtask verify-release native-dist`), publishes a GitHub
release and uploads the files to the `monobot-releases` R2 bucket under
`native-preview/`. The app reads its feed from
`<R2_PUBLIC_BASE_URL>/native-preview/latest.json`.

Repository secrets the workflow needs:

| Secret | Value |
| --- | --- |
| `R2_ACCOUNT_ID`, `R2_BUCKET_NAME`, `R2_PUBLIC_BASE_URL` | Cloudflare account, `monobot-releases`, its public `r2.dev` URL |
| `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY` | An R2 API token limited to `monobot-releases` (Object Read & Write) |
| `MONOBOT_UPDATER_PRIVATE_KEY`, `MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD`, `MONOBOT_UPDATER_PUBKEY` | The minisign key pair that signs updates (`cargo xtask update-keys <dir>` creates one; the password comes from `MONOBOT_UPDATER_PRIVATE_KEY_PASSWORD`) |
| `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_KEY`, `APPLE_API_ISSUER`, `APPLE_API_KEY_P8` | Developer ID signing and notarization |

`cargo xtask bump-version <x.y.z>` sets the version of the app, its crates and
the remote host before a release.

The remote host has its own [build and packaging instructions](remote-access.md#release-packaging).
The mobile app builds from `mobile/` ([mobile README](../../mobile/README.md)).

Read [CONTRIBUTING.md](../../CONTRIBUTING.md) for contribution guidelines.
