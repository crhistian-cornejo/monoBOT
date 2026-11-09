# Notch

The MonoBOT notch on macOS and Windows, started Nov 8, 2026. It takes the idea from
IVY's notch (`~/Developer/IVY/apps/desktop/macos/Sources/Notch/`, about 15.7k
lines of Swift) and keeps only what helps someone working with agents. The
findings it builds on are in [ivy-review.md](ivy-review.md).

## What is built

The notch is all GPUI and Rust, with no Swift helper and no web view.

**Rules.** The rules are in `monocode-core/src/notch.rs` and its tests:

- One saved look (`notch.look`), always normalized.
- The shelf's widgets, their widths and first-fit rows.
- The geometry from the screen: the camera between the menu bar's auxiliary areas, or none.
- The notices: an ask stays until answered, a finished turn goes by itself, and turns that had finished before the app started are not news.

**Interface.** It is in `monocode-gpui/src/notch/`.

- **Shapes.**
  - Quiet: the camera itself, or a slim handle on the top edge.
  - Busy: ears with the dot-matrix work mark and the count.
  - Notice: Allow and Deny, Answer, or Open.
  - Open: a panel. On macOS the notch style hangs from the camera; Island floats under the menu bar.
- **Pages.**
  - Home: the shelf.
  - Agents: what needs you, what works, what finished, recent chats.
  - Chat: asks a new agent in the last project, or opens the quick composer.
  - Settings: a sidebar of General, Notices, Widgets and Look.
- **Widgets.** Agents, recent chats, needs you, reminders, call, clock, system and new chat. Each one can be shown or hidden, widened and reordered, from the pencil or from Settings › Widgets. Every change is saved at once.
- **Motion.**
  - The window takes the room of both shapes once, and shrinks back when the island rests.
  - Inside, the island springs between shapes at the display's rate (response 0.34 and damping 0.8 opening; 0.26 and 1.0 folding). The content fades in 40 ms later, over 120 ms.
  - Under the pointer the closed notch swells by 14 points.
  - Reduced motion draws every final state at once, and `follow_reduced_motion` now reads the system setting for the whole app.
- **Look.** Square corners softened to 4 px, a 4 px flare where the notch meets the top edge, and a 1 px hairline while it shows something (Settings › Look). It is black in every theme, and the accent comes from the palette.

**Window.** The open panel's window holds the room of its tallest page, so
changing pages never resizes it. Closed, the window is the island plus room
for its swell, so the resting loops repaint a small window. Clicks pass
through the transparent room on macOS (`setIgnoresMouseEvents`, driven by the
pointer watch). GPUI counts a panel above the menu bar as on screen
(`on_screen` in `vendor/gpui-pre-macos`, see
[architecture](../done/architecture.md)). Before that, AppKit reported it
occluded and its frames stopped for up to two seconds.

**Platform.** It is in `notch/platform.rs`.

- **macOS.** The notch reads `NSScreen`'s safe area and auxiliary areas, and is a borderless panel at `mainMenu + 3` on every Space and over full-screen apps. Global and local `NSEvent` monitors watch the pointer, and a latency-critical `NSProcessInfo` activity holds while the notch is in use, because App Nap delayed timers by about a second.
- **Windows.** A topmost tool window on the primary display, moved with `SetWindowPos`. It is not compiled here: `ring`'s C build stops the cross-check.

**Claude Code and Codex in a terminal** (`notch/hooks.rs`).

- **The hook.** Each hook is one shell command per event. It sends `source app\n` plus the hook's JSON to `notch.sock` in the data folder with `nc -U`, and always succeeds; with MonoBOT closed, `nc` fails at once.
- **Sessions MonoBOT runs itself** carry `MONOCODE_HARNESS_PARENT`, so their hooks stay quiet.
- **Install.** MonoBOT adds its entries beside everybody else's in `~/.claude/settings.json` and `~/.codex/hooks.json`. It keeps a `.monobot-backup` of each file, writes through a temporary file, and replaces its own older command for the same socket.
- **Codex runs a new hook only after the user trusts it** in Codex's `/hooks`.
- **In the notch.** Sessions show as "folder · Claude Code" with their state word. Clicking one brings forward the app it runs in (`open -b` with the hook's `__CFBundleIdentifier`).

**System levels** (`notch/system.rs`, `notch/levels.rs`).

- A thread reads the output volume and mute (CoreAudio) and the built-in display's brightness (DisplayServices) five times a second, and reports only changes.
- A change opens the wings for 1.6 s with a drawn mark, a bar and the percent, and lights the edge in the accent as it fades.
- On macOS the notch answers the volume, mute and brightness keys itself (an event tap, with Accessibility permission), so macOS's indicator does not show; Settings › Look › Volume and brightness gives them back to the system. On Windows a low-level keyboard hook takes the volume keys (not built here).

**App.** In `shell/notch.rs`, the shell gathers the feed at most four times a second. It covers MonoBOT's sessions with their task progress, the Monos' chats with their faces, group members in their seats, pending asks, the call, recent chats, reminders and the terminal sessions. It carries out what the notch asks.

## Checked

| Date | What | Result |
| --- | --- | --- |
| Nov 8 | `cargo xtask cargo test -p monocode-core … notch::` | 14 passed |
| Nov 8 | `cargo xtask cargo test -p monocode-gpui … notch::` (real GPUI events: hover, clicks, pages, settings, shelf) | 9 passed |
| Nov 8 | Clippy with `-D warnings` on core and GPUI | Clean |
| Nov 8 | Dev app on a 14-inch MacBook Pro | Window exactly over the camera at level 27. Hover opens in about 120 ms and the panel settles by about 460 ms |
| Nov 8 | `MONOBOT_NOTCH_TRACE=tour` | After the GPUI patch every page change runs at about 60 fps (largest gap 18 ms) |
| Nov 8 | Hook commands run live (bash, zsh -lc) | Delivered; quiet for MonoBOT's own sessions; succeed with MonoBOT closed (4 ms) |
| Nov 8 | Installed into the real settings | Earlier hooks and settings untouched, backups written, reinstall replaces without duplicating |
| Nov 8 | Simulated Claude Code session through the installed command | Wings "Editing", then the Done notice |
| Nov 8 | `cargo xtask cargo test -p monocode-gpui` (full) | 1040 passed; 2 known flaky under load passed alone |

## Next

- Windows: compile and try it on a Windows machine (the volume, its keys and the window code are written against windows 0.62 but not built here), then the hooks over a named pipe and the brightness.
- Look at it on an external display and over a full-screen app.
- Measure the CPU at rest with `cargo xtask perf`.
- A row in the app's Settings that opens the notch's settings, and a way to hide the notch.
