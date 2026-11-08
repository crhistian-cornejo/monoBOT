# MonoBOT desktop (GPUI)

MonoBOT's desktop app is written in Rust with GPUI. It is the only desktop app:
the earlier React/Tauri app has been removed. It builds from the Cargo workspace at
the repository root (`Cargo.toml`, `Cargo.lock`, output in `target/`).

## Requirements

- Node.js 22+ and a current stable Rust toolchain.
- On macOS, Xcode with macOS components.
- Linux core checks need D-Bus headers and pkg-config (`libdbus-1-dev` and
  `pkg-config` on Debian/Ubuntu).
- At least one installed, signed-in agent CLI. See [provider setup](providers.md).

The desktop commands need no JavaScript dependencies or frontend build.
macOS is the primary preview target. A Linux ARM64 source build has been
smoke-tested in a VM; Linux x64 packages and Windows GUI interaction still need
runtime testing. See [preview limitations](../in-progress/limitations.md).

Fenced `mermaid` blocks render as native diagrams in Markdown previews and
conversations. See [Mermaid diagrams](mermaid.md) for examples and limitations.
For interactive visuals, use `/visualize` in an idle local desktop conversation
or ask a Mono directly. See [session visuals](visuals.md) and the
[preview release notes](release-notes.md).

## Monos

Each kind of agent has its own side panel. The **Images** bot opens on
**Gallery**, which shows every picture in its chat, newest first, in a masonry
wall that keeps each picture's shape. You can filter it by portrait, landscape
or square. Hovering a picture shows its brief, style, format and date, along
with three actions: change it in the chat (it attaches the picture as the
source), reuse its style, or reveal the file. Clicking it opens it full screen.
**Styles** lists the preset formats (Square 1:1, Portrait 4:5, Story 9:16,
Poster 2:3, Classic 3:2, Standard 4:3, Landscape 16:9, Ultrawide 21:9) and 16
looks (`/screenprint`, `/anime`, `/watercolor`…). Picking one puts its slash
word at the start of the draft, replacing any other of the same kind. The
bot's slash menu offers the same words, and `images.generate` takes the style
id and adds its art direction to the brief. The gallery is read from the chat
itself, so it needs no store of its own. Sub-agents, Documents and Habits come
after the gallery and styles.

A Mono's **Overview** tab brings together its current activity, a contribution
calendar, today's work, upcoming habits and new documents. **Sub-agents** lists
the sessions it launched, with their model, provider, checkout and state. Monos
are prompted to delegate longer tasks to background sessions, whose final
reports return to the Mono chat.

Contribution counts come from the Mono's sessions, including saved history:
commits and pull requests opened, merged or reviewed through supported agent
tools. Opened pull requests are checked for later merges through the GitHub CLI.
Records are stored in `monocode.db.gpui.sqlite3` alongside native preferences
when using the installed desktop database.

In the Mono's **Settings → General → Heartbeat** section, **Check in on its own**
controls inactivity checks. They are enabled by default with a 30-minute quiet
interval; choose 15 minutes, 30 minutes, 1 hour or 2 hours, or turn them off.
While the native app is running, an idle Mono with an existing conversation can
review sessions, notes and inbox changes in its assigned projects and suggest
up to two follow-ups. It can also perform recurring checks already described
in its soul's standing instructions. New work is proposed for the user to accept.

Heartbeats keep the Mono's selected permission mode and use its provider account,
with a smaller model when its catalog offers one. Quiet checks leave no report
or retained side session, and repeated quiet results extend the interval up to
four hours. Recent suggestions and the last check time are saved in the Mono's
`heartbeat.json` file to avoid repeating
the same follow-ups after a restart. Scheduled habits take priority over
heartbeats. Live scheduler and provider behavior still need broader testing;
see [verification](../old/verification.md).

## Develop

From the repository root:

```sh
cargo xtask dev
```

Development uses a separate database. To open a particular project:

```sh
cargo xtask dev --cwd /path/to/project
```

Type `r` then Enter in the terminal to rebuild and restart. The current app stays
open until the new build succeeds. Ctrl+C stops the app and runner. There is no
hot reload or automatic rebuild on save.

On macOS, the command packages and launches `MonoBOT Native.app`. Other
platforms launch the executable directly.

Dictation links sherpa-onnx, whose build script downloads its prebuilt libraries
from GitHub into each target folder. A network that inspects TLS refuses that
download, so `cargo xtask` (`dev`, `build`, `test`, `check`) points `SHERPA_ONNX_LIB_DIR` at libraries already
fetched under `target/sherpa-onnx-prebuilt`, in this checkout or, from a git
worktree, the main one. Running cargo by hand, use `cargo xtask cargo <args>`
in its place, or set `SHERPA_ONNX_LIB_DIR` yourself.

## Build

```sh
cargo xtask build
```

This creates an optimized build without launching it. **The release app uses the
installed MonoBOT database, and changes save there.** No extra flags are needed.

On macOS, open:

```sh
open "target/release/MonoBOT Native.app"
```

The app has a separate name and application ID (`dev.usemono.gpui`) so it can be
installed alongside MonoBOT. Local source builds are signed locally, without
Apple notarization or configured automatic updates. Linux and Windows builds
produce a standalone executable
under `target/release/`. `CARGO_TARGET_DIR` and
`CARGO_BUILD_TARGET` can change these output paths.

The **MonoBOT Native Preview** GitHub Actions workflow runs checks and build
verification.

## Data and configuration

Development stores `monocode.db` under:

| Platform | Development directory |
| --- | --- |
| macOS | `~/Library/Application Support/MonoBOT Development/` |
| Linux | `$XDG_DATA_HOME/MonoBOT Development/`, or `~/.local/share/MonoBOT Development/` |
| Windows | `%LOCALAPPDATA%\MonoBOT Development\` |

`MONOCODE_NATIVE_DATA_DIR` overrides this development directory.

Release builds open `com.monocode.desktop/monocode.db` under macOS Application
Support, Linux's XDG data directory, or Windows' `%APPDATA%`. The app uses the
existing desktop schema and session IDs. GPUI preferences are saved beside the
database in `monocode.db.gpui.sqlite3`.

Supported desktop preferences and workspace state are read from the installed
app's webview localStorage on macOS and Linux. The Windows WebView2 reader is
implemented and covered by synthetic fixtures on Linux; reading a real Windows
profile remains unverified. Native preference edits are stored separately.
Some desktop layout concepts still need native support; see
[preview limitations](../in-progress/limitations.md).

These readers import preferences left by the old Tauri app, if it was ever
installed. The Windows reader always opens a temporary copy of localStorage;
close the old app before the first launch so the copy is consistent. See [verification](../old/verification.md) for
recorded checks.

To inspect the installed database without opening a window or changing it, use
the executable from a release build:

```sh
target/release/monocode-gpui --check-data
```

On Windows, the terminal prefers PowerShell 7 (`pwsh.exe`), then Windows
PowerShell (`powershell.exe`), with `COMSPEC`/`cmd.exe` as the fallback. A `SHELL`
environment variable pointing to an installed native executable overrides this
order. Missing executables and Unix-only shell paths are ignored.

Provider CLI paths can be changed in Settings → Providers. CLI authentication
continues to use each provider's existing sign-in.

## Checks and fixtures

```sh
cargo xtask check
cargo xtask test
```

`check` runs Rust formatting, strict Clippy and the xtask tests; `test` runs the
core and GPUI tests.
See [verification](../old/verification.md) for individual commands, optional provider
checks and the latest recorded results.

A synthetic long-history fixture uses development data and makes no provider
request:

```sh
MONOCODE_NATIVE_DATA_DIR=/tmp/monocode-native-lab cargo xtask dev --fixture
```

Fixtures are refused with production data. Use a fresh temporary directory when
you want an empty fixture store.

## Contributing

See [architecture](architecture.md) for the crate boundaries and renderer patches,
and [preview limitations](../in-progress/limitations.md) for remaining work. The
[session visuals guide](visuals.md) documents the agent-facing visual API and
examples.
