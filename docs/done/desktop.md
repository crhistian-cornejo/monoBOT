# monoBOT desktop (GPUI)

monoBOT's desktop app is written in Rust with GPUI. It is the only desktop app:
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

The character is a signed distance field marched in software (the
`monobot-persona` crate in `crates/persona`, which the core re-exports as
`mono::persona`), so there is no shader and it is the same picture on every
platform. The app renders each pose (resting, blinking, talking) once per size
on a background thread and paints it as a picture from then on; the hop, the
breath and the blink only move or swap that picture.

The phone draws the same faces. The desktop sends each Mono's `face` with its
mascot, and the phone's native link (`crates/mobile-link`, operation
`persona`) renders each pose with the same crate into the app's cache, a few
at a time and every resting picture first; a file is drawn once and named by
the renderer's revision, so a new renderer draws them again. Until a picture
lands the avatar shows a soft dot in the bot's colour, and where none can be
drawn (an older desktop, a build without the renderer) it wears its pixel
mascot. Pixel mascots and personas move as on the desktop, on the UI thread:
they hop at work, breathe and blink at rest except in lists, and hold still
when the Mono needs you or with Reduce Motion on.

To look at every species and outfit, write a contact sheet:

```sh
MONOBOT_PERSONA_SHEET=sheet.png cargo xtask cargo test -p monocode-core mono::persona -- --ignored
```

## Menu bar, tray and quick composer

monoBOT's icon sits in the macOS menu bar and in the Windows tray. A click
opens the same menu on both (`menu_bar::status_menu`): each Mono, which opens
its floating chat; **Beside the Icon**, the live item (a provider's usage, the
agents at work, what plays, the system's load); **Quick Composer**; and
**Quit monoBOT**, which takes the regular quit path, so a downloaded update
installs. On Windows the menu also has **Open Notch** and **Open monoBOT**.
In the tray the logo is the executable's own icon; a live item replaces it with
its ring or mark, and its rows become the tooltip. Settings › Monos › Menu bar
icon (Tray icon on Windows) turns it off.

Closing the workspace window hides it instead of quitting, on macOS and
Windows, while Settings › General › **Keep monoBOT running in the background
when the window closes** is on (the default; stored as `close_to_tray`). The
shell, the notch, the tray and the agents keep going. Unsaved file edits still
ask first, as quitting does. The window comes back from the notch, the tray's
**Open monoBOT**, a notification, the Dock (macOS) or by launching monoBOT
again (Windows: one copy runs per data folder, and a second launch shows the
first one's window and exits). Ctrl+Q / ⌘Q and the tray's or menu's Quit
still quit. With the setting off, or on Linux, closing the window quits. The
decision and the Windows single instance live in `monocode_core::app_window`;
hiding and showing in `monocode-gpui/src/app_window.rs`.

Windows keeps a new tray icon behind the overflow arrow, and no app can move
it out: drag it onto the taskbar, or turn monoBOT on in Settings ›
Personalization › Taskbar › Other system tray icons. The Tray icon row's
**Taskbar settings** button opens that page.

The quick composer floats over other apps. Its global shortcut is ⌘⇧Space on
macOS (HIToolbox) and Ctrl+Shift+Space on Windows (`RegisterHotKey`,
`quick_composer/hotkey_windows.rs`). Change it in Keybindings; Settings ›
General shows an error when another app already owns it.

Windows toasts show monoBOT's logo: at startup the app writes it to
`%LOCALAPPDATA%\monoBOT\notification-icon.png` and registers it as the
`IconUri` of its AppUserModelID (`dev.usemono.gpui`), which GPUI registers by
name only.

## Agents tab

When the Claude chat in view starts subagents or a workflow, the right panel
(and the Classic sidebar) gets an **Agents** tab. It lists each workflow run
with its phases in the script's order, and under each phase its agents. Every
agent shows whether it is running, done, failed or stopped, how long it has
taken, and its latest step (`Bash · cargo test …`, `Edit · system.rs`) or, once
done, the first line of its report. Subagents started outside a workflow are
listed below the runs. Running runs open by default and finished runs fold;
clicking a run's header flips it.

Claude Code does not stream this work to the app; it writes it under
`<CLAUDE_CONFIG_DIR>/projects/<cwd>/<session>/` (`subagents/agent-*.jsonl` and
`.meta.json`, `subagents/workflows/<run>/journal.jsonl`, and the run's script
in `workflows/scripts/`). `monocode_core::claude_runs` reads those files and
only rereads the ones whose size or time changed. The pane
(`crates/monocode-gpui/src/agents_pane.rs`) polls it off the UI thread: every
1.5 s while the tab is on screen and every 6 s otherwise, which is only to
know whether the tab should be there. An unfinished agent whose transcript
has not moved for 20 minutes counts as stopped.

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
repaints every view that leased it, so loaders, mascots and bot faces move at
60fps and share their frames. `Rate::Steady`, 30fps, is for the small loops
that turn in place for a whole turn or a whole song: the 3×3 working spinner
everywhere it shows, and on the closed notch the sweep of light on the state
word, the equalizer, the gliding title and the lyric's fill. `Rate::Low`,
15fps, is for pictures that only trade one still pose for another; a hidden
app ticks a few times a second. A view says its pace again on every frame, so
one that moves at 60 for a moment (a word changing, a lyric rising in) falls
back to its loop's. Entrances and one-off transitions run on GPUI's own
per-frame animations. Panes keep their spring in `pane_motion.rs` and tabs
their widths in `tab_motion.rs`. Everything draws
its final state under reduced motion.

A short label that changes morphs instead of jumping (`text_morph.rs`): the
letters both texts share slide to their new places and the rest trade places,
over `motion::NORMAL`, on the shared clock. The notch uses it for its state
word, counts, clock, row labels and pills; see
[notch.md](../in-progress/notch.md).

## Trying a theme on

In Settings › Appearance, each workspace theme is tried on as the pointer
reaches it in the list: the whole app repaints with it at once, with no blend.
(The arrow keys do not walk that list yet: its filter field keeps them.) Nothing is saved until one is chosen; closing the list, or
Settings, puts the saved theme back. The list says which option is highlighted
(`SettingsEvent::PreviewTheme`) and the shell paints with it in place of the
saved theme (`Shell::theme_preview`), skipping the work a real change does
(keybindings, the menu, the store). A project with colors of its own keeps
them, tried on or chosen, since the app's theme does not show there.

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
installed monoBOT database, and changes save there.** No extra flags are needed.

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
