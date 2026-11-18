# UI/UX audit of the desktop app (Nov 17, 2026)

An audit of the GPUI app against two lenses: the app-screen rules of the
design-taste skill (colour on data, not chrome; honest zeros; one grey per
tier at 4.5:1; quiet navigation) and Apple's interface guidelines (system
face, native geometry, the system accent, reduced motion). The references
were [NativeGPUI](https://github.com/Cafekery/NativeGPUI) (AppKit inside
GPUI), [T3 Code](https://github.com/pingdotgg/t3code), its fork
[Synara](https://github.com/Emanuele-web04/synara) and
[Capy](https://capy.ai/changelog).

The app was captured with `--dev-data --chat-fixture` under a temporary
`HOME` at 1180×800 pt, dark glass. The Mac was locked during the audit, so
only the first screen was captured, before and after; the rest comes from
reading the views.

## Fixed in this round

| Finding | Change |
| --- | --- |
| About 150 status colours were Tailwind 400 literals scattered through the views (`#f87171` alone 74 times), in four reds, four ambers and three greens. On the light theme they fall to 1.6–2.8:1 on white, so errors, approvals and checks were close to unreadable. | `theme::{danger, warning, success, info}`: the 400 tone on dark surfaces and a 700 tone on light ones (5.1–5.9:1 on white). Every view now uses them, and the duplicate `AMBER`, `EMERALD`, `DONE` and `RED_400` constants are gone. |
| Every corner was squared (`set_square_corners(true)`, from the Tauri radius tokens). On macOS the composer, menus, segmented controls and buttons read as a Windows or web port. Boxes framed by a border that asked for no radius stayed rectangles even with rounding on. | Appearance › Layout › **Corners** has four choices: Square, Subtle (60%), Rounded (100%, the default on macOS) and Soft (160%). Each scales the radii the views ask for. A styled box framed on all four sides that asks for none gets 6 px at that scale; shapes painted by hand, such as the chunks of a long message, keep their own. The notch has the same four choices in its own Look page, because GPUI now takes a corner scale per window (`Window::set_corner_scale`). |
| The interface was always drawn in the bundled Geist, so text did not get San Francisco's optical sizes or Core Text's tracking tables. | Appearance › Layout › **Interface font**: System uses `.SystemUIFont`, which is SF Pro on macOS, and is the default there. Geist stays the default elsewhere. Changing it re-measures the transcript, because the style key covers the font. |
| The accent could not follow the one chosen in macOS System Settings. | A **System** swatch in the accent picker (macOS only). It reads `NSColor.controlAccentColor` in sRGB, at most once a second, so a change in System Settings shows on the next repaint. The pattern comes from NativeGPUI `src/system/tokens.rs`. |
| The title bar's diff counter painted `+0 -0` in green and red on a clean tree. It also used the pastel diff hues, which fail on light. | Each zero prints in grey. Non-zero counts use `diff_foreground`, which follows light and dark. |
| Right-click menus were drawn by GPUI, so they had no type-ahead, no VoiceOver menu semantics and not the system's look. | `native_menu::present` shows AppKit's `NSMenu` on macOS for a right-click on a file in the explorer, a link in the chat, and a title tab, session or terminal tab. It carries checks, submenus, disabled rows and key equivalents. A menu with a custom view, a flyout or a switch (project appearance, folder colours, session filters) keeps the drawn menu. Button dropdowns, tests and other platforms keep it too. The tracking loop runs from `window.spawn`, outside GPUI's borrow, as NativeGPUI does. |

Tests: `theme::composer_tests::{status_hues_darken_on_light_surfaces_to_stay_readable,
the_system_accent_setting_paints_a_colour_or_falls_back_to_the_theme,
the_interface_face_and_corner_shape_follow_appearance}` and
`native_menu::tests`. The native menus were not exercised by hand in this
round; the views keep their drawn-menu tests, since tests always draw.

## Still open, by value

### Native macOS (from NativeGPUI)

1. **Sidebar vibrancy per region.** Put an `NSVisualEffectView` with the
   `.sidebar` material under GPUI's view, sized to the sidebar. The window
   must become `Transparent` instead of `Blurred`, and the glass palettes
   need re-checking (`src/materials/material.rs:393-408`). Estimate: 2–4 days.
2. **Follow accent changes live** with `AppleColorPreferencesChangedNotification`
   instead of the one-second re-read (`tokens.rs:363-460`).
3. **SF Symbols in native menus** through `NSMenuItem.image`, and native
   menus for the text fields' and terminal's Copy and Paste menus
   (`input/context_menu.rs`, `terminal/context_menu.rs`).

Skip these: the AppKit split view, `NSOutlineView`, `NSToolbar` and the
private APIs (`_setCornerRadius`, `NSScrollPocket`). They are too invasive
or too fragile across OS updates.

### Sessions and attention (T3 Code, Synara, Capy)

5. **One status per session row, picked in priority order.** The order is
   approval, then question, then working (stepped pulse, 2 s in 6 steps, with
   a solid dot under reduced motion), then failed until seen, then done until
   seen. `session_mark::State` has Idle, Working, Done and Attention; it lacks
   Failed and the "until seen" rule.
6. **The row's right slot.** It shows a compact relative time ("5m", "3h"),
   which becomes a live `m:ss` timer while the agent works. Holding ⌘ swaps
   it for 1–9 jump hints.
7. **A "Needs you" section** above the project groups, ranked waiting, then
   unread, then working, with its order frozen while visible. Add a "Next
   session needing attention" command.
8. **Send keys and labels.** The button reads Send, Steer or Queue by state.
   ↩ steers, ⌥↩ queues and ⌘↩ interrupts. Queued messages stack above the
   composer and fold behind a count from two up.
9. **The "Worked for" fold counts failures** ("Worked for 42s · 1 failed")
   and never hides a failed or running tool.
10. **The diff panel gets a "Reviewed" checkbox per file**, which collapses
    the file. Lockfiles and generated files are greyed and left out of the
    totals. ⌥↓ and ⌥↑ jump between hunks.

### Seen on the first screen, to check

- The first frames are a loading state that does not look like one. The
  harness picker shows an empty icon slot before "Codex", and the status bar
  shows `··· ··· ⟳` until the providers answer. Draw the final layout with
  neutral bars at their real size, as the app-screen rules ask.
- The sidebar said "No sessions yet" while the fixture's session was open
  in a title tab.
- "Notes 0" puts a count on a nav item that needs no action. A count should
  appear only on a queue that waits on the user, and never at zero.
- The composer's controls (+, Codex, Supervised) are each a bordered box.
  Native and T3 Code controls are borderless, with a hover fill, which
  lowers the chrome around the one field that matters.
- The Beta badge is tracked uppercase in the accent. Sentence-case "Beta" in
  grey would be quieter, and it would leave the accent for actions.
- Sidebar rows are 32 pt with 14 pt medium labels. The macOS source list is
  13 pt regular on 28 pt rows. Try a compact density instead of relying on
  interface scale.
- The git-status letters in Changes (`right_panel::AMBER_400`, `SKY_400`)
  and the pastel `DIFF_ADD` and `DIFF_DEL` still fail contrast on light
  themes.
