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
bot's slash menu offers the same words. A format's word is its own name
(`/portrait`, `/ultrawide`); older words such as `/cinema` still mean the same
ratio. Typing `/format` lists only the formats with their ratios.
`images.generate` takes the style id and adds its art direction to the brief. The gallery is read from the chat
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

### Faces

A coding Mono wears its pixel mascot. A personal bot wears a small 3D
character instead, so the two kinds are told apart at a glance. Nothing new is
stored: the bot's colour is the character's body, in the same pastel the pixel
mascots wear, its mascot is its species (a cat's ears, a robot's screen, a
mushroom's cap) and its role dresses it (the Principal's crown, the Inbox's
cap, the Analyst's glasses). `Mono::face()` names what the avatar draws, and
the look picker previews each mascot as the character it would become.

One body carries every species and each role adds one prop, as
[momo](https://github.com/mortspace/momo)'s characters do. Two rules keep
them tidy: what a species grows on top of its head (antennae, a crest, a nose
cone) gives way to a hat, and what a role carries (the Planner's flag, the
Writer's quill, the Phone bot's phone) is held in the hand. Below 40 pixels
the picture is a portrait of the head and shoulders with larger eyes, so the
species still reads beside a name in a list. A working bot hops and trades its
resting picture for one with its mouth open and its hands up, three times a
second, as the pixel mascots trade frames.

The character is a signed distance field marched in software
(`monocode-core/src/mono/persona.rs`), so there is no shader and it is the same
picture on every platform. The app renders each pose (resting, blinking,
talking) once per size on a background thread and paints it as a picture from
then on; the hop, the breath and the blink only move or swap that picture.
The phone still receives the mascot name and draws its own pixel mascots.
To look at every species and outfit, write a contact sheet:

```sh
MONOBOT_PERSONA_SHEET=sheet.png cargo xtask cargo test -p monocode-core mono::persona -- --ignored
```

## Dictation

Dictation runs on this machine: the microphone feeds a Silero voice detector
and Parakeet v3 transcribes each phrase as the detector closes it. The
microphone opens at once. Parakeet loads on the session's own decoding thread
(`crates/dictation/src/session.rs`), so words said while it loads are kept and
transcribed when it is ready, and listening never waits on a phrase being
transcribed. Spanish and English are told apart phrase by phrase: Spanish
gets its opening «¿» and «¡» and its pauses rejoined, and English is left as
Parakeet wrote it, also inside the same dictation.

## Motion

Shared timings, curves and entrances live in `crates/monocode-gpui/src/motion.rs`:
the modal card, the notices that slide in from the window's edge, the image
preview's fade and the tick that answers a copy all take them from there. A
new transition picks from that file instead of adding its own numbers.
Looping decoration keeps its clock in `pulse.rs`: one tick, 60 times a second,
repaints every view that leased it, so spinners, loaders, mascots and bot
faces move at 60fps and share their frames (`Rate::Low`, 15fps, is for
pictures that only trade one still pose for another; a hidden app ticks a few
times a second). Entrances and one-off transitions run on GPUI's own
per-frame animations. Panes keep their spring in `pane_motion.rs` and tabs
their widths in `tab_motion.rs`. Everything draws
its final state under reduced motion.

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

On macOS, the command packages and launches `monoBOT.app`. Other
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
open "target/release/monoBOT.app"
```

The app has its own application ID (`dev.usemono.gpui`) but no longer a separate
name: on macOS it cannot share a folder with the earlier `MonoBOT.app`, which it
replaces. Local source builds are signed locally, without
Apple notarization or configured automatic updates. Linux and Windows builds
produce a standalone executable
under `target/release/`. `CARGO_TARGET_DIR` and
`CARGO_BUILD_TARGET` can change these output paths.

The **monoBOT Preview** GitHub Actions workflow runs checks and build
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
