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

**Platform.** It is in `notch/platform.rs`.

- **macOS.**
  - The notch reads `NSScreen`'s safe area and auxiliary areas.
  - The panel is borderless at `mainMenu + 3`, on every Space and over full-screen apps.
  - It is placed with `setFrame`. A borderless panel is not pushed below the menu bar, so GPUI needed no patch.
  - The pointer is watched with global and local `NSEvent` monitors. GPUI sends no hover to a pop-up while another app is active, and its stale hover reopened the panel.
  - Opening and closing check the pointer's live position.
  - While the notch is in use it holds a latency-critical `NSProcessInfo` activity. App Nap otherwise delayed every timer by about a second.
- **Windows.** A topmost tool window on the primary display, moved with `SetWindowPos`. This was not compiled here: `cargo check --target x86_64-pc-windows-msvc` stops at `ring`'s C build on this Mac.

**App.** In `shell/notch.rs`, the shell feeds the notch on every session change and every 3 seconds. It sends the live agents, pending asks, the group call, recent chats and reminders, and carries out what the notch asks.

## Checked

| Date | What | Result |
| --- | --- | --- |
| Nov 8 | `cargo xtask cargo test -p monocode-core … notch::` | 14 passed |
| Nov 8 | `cargo xtask cargo test -p monocode-gpui … notch::` (real GPUI events: hover, clicks, pages, settings, shelf) | 9 passed |
| Nov 8 | Clippy with `-D warnings` on core and GPUI | Clean |
| Nov 8 | Dev app on a 14-inch MacBook Pro | Window exactly over the camera at level 27. Hover opens in about 120 ms and the panel settles by about 460 ms |

## Next

- Look at it on Windows, an external display and a full-screen app.
- Measure the CPU at rest with `cargo xtask perf`.
- Keep the old content during the fold, so the panel fades out instead of emptying.
- Add a Settings row in the app that opens the notch's settings, and a way to hide the notch.
