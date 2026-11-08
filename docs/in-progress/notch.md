# Notch

A plan from Nov 8, 2026 for a MonoBOT notch on macOS. It takes the idea from
IVY's notch (`~/Developer/IVY/apps/desktop/macos/Sources/Notch/`, about 15.7k
lines of Swift) and keeps only what helps someone working with agents. The
findings it builds on are in [ivy-review.md](ivy-review.md).

## What it is

The notch is a quiet strip around the camera housing. It says one thing at a
time, at most: what the agents are doing, or what needs the user. Nothing in it
moves while nothing happens.

IVY's notch has seven modes, three styles, six tabs and many feeds: the mascot,
WhatsApp, music, football, the agenda, volume and brightness. MonoBOT keeps
**three states** and **four sources**.

| State | Size | Shows | When |
| --- | --- | --- | --- |
| **Rest** | The physical notch, plus a 28 pt ear on each side | Left: a dot in the accent color while an agent works, plus a count ("2"). Right: the dictation or call level when the mic is live. With nothing running, the ears hide and the notch looks like hardware. | Always |
| **Notice** | 360 pt wide, one or two lines, hangs below the notch | An approval ("Claude wants to run `cargo test` · Allow · Deny"), a finished turn ("Done · auth refactor"), a call turn | Pushed by an event, folds back after 4 s unless it needs an answer |
| **Open** | 420 × up to 320 pt | The list of live sessions (provider icon, title, current step, elapsed time), pending approvals, and the call controls when a call is live | Hover for 150 ms or click; closes 300 ms after the pointer leaves |

**Sources** (all already exist; the notch only reads them):

- Live sessions and their step: `shell/live_agents.rs`, `session_mark.rs` and the aggregate in `shell/keep_awake.rs`.
- Approvals and questions: `notifications/approvals.rs` (`PendingInput`, `Decision`).
- Group calls: `monos/group_call.rs` (`CallState`, phase, mic, voice queue).
- Dictation: `dictation.rs` (level, words).

Mail, agent messages and the mascot stay out of the first version.

**Interactions:**

- Clicking a session opens it in the main window.
- Allow and Deny answer through the same `Decision` path as the in-app card.
- The call has mute and hang up.
- Esc folds the notch.
- There is no hotkey of its own; the quick composer already has one.

## Look

- The surface is black, so it merges with the hardware notch, with `content(alpha)` text from `theme.rs`. The accent from the active palette is the only color: the working dot, the level bars and the primary button. No glow from edge to edge.
- It reuses `theme.rs` (icon buttons, `primary_action`, `section_label`) and the approval card's layout from `notifications/approvals.rs`; the card is not rewritten.
- **Motion:**
  - The surface grows from the notch with one spring when it opens or a notice arrives, and shrinks back.
  - The working dot pulses slowly, about 2 s.
  - Nothing else animates: no eyes and no shimmer.
  - With reduced motion, every change is drawn in its final state and the dot doesn't pulse.
- **Macs without a notch:** a 120 × 24 pt pill centered under the menu bar, with the same states.

## How it's built

A GPUI window patched through objc2, the way `traffic_lights.rs` already patches
the floating chat (`floating_panel`, `square_corners`, `fit_height`). The notch
reads GPUI entities directly, so it needs no IPC, no Swift toolchain and no
second signed binary. A Swift helper would get SwiftUI springs and the safe
area for free. It would also cost a new build and signing chain, a socket for
state, and a second copy of the styles.

**The window:**

- `WindowKind::PopUp` (already an `NSPanel` that doesn't activate), `WindowBackgroundAppearance::Transparent`, and `inactive_frame_interval: None` so animations don't drop to 30 fps.
- A new `notch_panel()` in a `notch/platform.rs`, next to `traffic_lights.rs`:
  - Remove `Titled | FullSizeContentView` so the panel is borderless.
  - Set the level to `mainMenu + 3`.
  - Set the behavior to `CanJoinAllSpaces | Transient | FullScreenAuxiliary | IgnoresCycle`.
  - Remove the shadow.
- **Above the menu bar.** AppKit pushes windows below the menu bar through `constrainFrameRect:toScreen:`. A borderless mask usually avoids that. If not, add an override to `GPUIPanel` with `class_addMethod`, or as a patch in `vendor/gpui-pre-macos`, recorded in `docs/done/architecture.md`.
- **The panel is fixed** at 440 × (notch height + 340) and never resizes. The island is drawn inside it, as IVY does, so no resize jank and no reflow.

**Geometry** (`notch/geometry.rs`, pure functions with tests):

- Read `safeAreaInsets.top`, `auxiliaryTopLeftArea` and `auxiliaryTopRightArea` through objc2.
- The notch width is the screen width minus both auxiliary areas.
- The center sits after the left area.
- The height is the lower of the inset and the menu bar height.
- When the inset is 0, use the pill on the main screen.
- **Multiple displays:** one notch, on the built-in screen when it has one, otherwise on the main screen. Recompute on `NSApplicationDidChangeScreenParametersNotification`.

**Clicks pass through:**

- A global `NSEvent` monitor for `mouseMoved` hit-tests the island's current rect.
- It toggles `setIgnoresMouseEvents` so the rest of the strip stays clickable.
- It drives the hover timers for opening and closing.

**Reduced motion:** GPUI's `cx.reduce_motion()` exists, but the macOS host never
sets it. Read `NSWorkspace.accessibilityDisplayShouldReduceMotion` at launch
and on its change notification, and call `cx.set_reduce_motion`. Every view
gains from this, not only the notch.

**Cost at rest:** the island is static and the dot is the only animation, and
it stops when no agent works. CPU and GPU at rest need measuring with
`cargo xtask perf`. The goal is no frames requested while nothing changes.

**Settings:** a Notch switch under Settings › Appearance (off on Windows and
Linux), plus "Open on hover" or "Open on click".

## Code layout

| Where | What |
| --- | --- |
| `monocode-core/src/notch.rs` | A pure state machine: summary from sessions, approvals, call and dictation → `Rest/Notice/Open` and what each shows; notice queue and timeouts; priority (approval > call > done > working). No GPUI. |
| `monocode-gpui/src/notch/mod.rs` | The window, its entity subscriptions and rendering. |
| `monocode-gpui/src/notch/platform.rs` | objc2 patches: panel, level, mouse pass-through, screen notification, reduced motion. |
| `monocode-gpui/src/notch/geometry.rs` | Notch rect from the screen values, with tests. |

## Steps

1. **Reduced motion from the system.** Small, and every view gains from it.
2. **State machine in core** with tests: priorities, notice expiry, an approval that stays until answered, call and dictation levels.
3. **The window on the notch, empty.**
   - Geometry, the patched panel, pass-through.
   - Check it on a Mac with a notch, on one without, on an external display, in a full-screen app and across Spaces.
4. **Rest and Open.** Live sessions and opening a session in the main window.
5. **Notices.** Approvals with Allow and Deny through `Decision`, and finished turns.
6. **Calls and dictation.** Level, mute and hang up.
7. **Settings and cost.** Measure at rest, and write the GPUI interface tests with real events: hover opens, leave closes, Esc folds, an approval stays.
8. **Docs.** Move this plan into `docs/done/desktop.md`, and record any GPUI patch in `architecture.md`.

Steps 1 and 2 don't depend on each other. Step 3 carries the most risk: the
menu bar constraint and click pass-through. Try it first as a spike before
building views on top.

## Open questions

- Does a borderless GPUI panel stay above the menu bar without overriding `constrainFrameRect`? Only a spike on a Mac with a notch can tell.
- Should finished turns raise a notice, or only change the count? It is quieter without, but a turn that ended while the user was away would go unseen.
- Should the notch also show external Claude or Codex sessions? That would need IVY's hooks (`crates/hook`), so it is out of the first version.
