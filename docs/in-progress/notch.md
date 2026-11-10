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
  - Agents: the user's own agents (Monos), each with its face and what it does now (needs you, the step it is on, or resting), their approvals, and any other agent at work (a habit, a group member's seat).
  - Chat: the coding chats. Ask a new one in the last project or open the quick composer, answer their approvals, follow what works and finished, and pick up one of the last five. The Monos' own chats stay out of it.
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
- **Windows.** A topmost tool window on the primary display, moved with `SetWindowPos`, which then tells GPUI its new bounds (the `WM_SIZE` it sends arrives mid-render and GPUI drops it; without this the open panel drew empty at the old size). It is `WS_EX_NOACTIVATE`, so Windows never hands it activation when the workspace minimizes, and DWM draws no Windows 11 corners, border or shadow around its transparent room. Whether the pointer is over the island is asked of the system (`GetCursorPos`), since GPUI's hover goes stale when the window resizes under a still pointer.
- **Windows look.** No camera to sit on and often a dark taskbar on top, so the resting handle is larger (156 × 18 with its face, 156 × 10 with the face off), keeps a light hairline and wears the face, or a short grip line in its middle. A system change lights the body with a soft blurred bloom in the accent (two drop shadows, no hard ring) that eases out; the window takes room for it only while it shows, since that room blocks clicks to the taskbar.
- **On top.** Windows raises the taskbar, the Start menu and a clicked window above other topmost windows, so the notch slipped under a taskbar docked at the top. A `WinEvent` hook on foreground changes puts it back on top without activating it.

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

**What plays** (`monocode-core/src/notch/media.rs`, `notch/media.rs`).

- On Windows a thread reads the system's media session every half second (`GlobalSystemMediaTransportControlsSessionManager`, the session the media flyout shows: Spotify, a browser's video, the media player) and reports only a new track, play or pause, or a seek. The playhead runs on from the app's last stamp.
- Synced lyrics come from LRCLIB (`lrclib.net/api/get`, then `/api/search`; free, no key), once per track, off the interface thread, through `monobot-http`.
- The playhead: Spotify reports a new position about every 4.5 s and restamps in between without moving it, so the notch takes a new position (plus the time since its stamp) as the truth and runs its own clock until the next one (`Playhead` in core). Measured against Spotify's reports it stays within about 0.05 s and never goes back.
- When LRCLIB's exact lookup misses, its search can return other cuts of the song; the one as long as the track (within 3 s) wins, since an extended mix's lines run late against the radio edit.
- The cover comes from the session's thumbnail stream, read once per track (a few tries, since Spotify and browsers hand it over late) and decoded once into one `gpui::Image`.
- While something plays and no agent works, the closed notch grows wings: the cover, the lyric line at the playhead and the waveform in the accent. Each line starts 0.3 s early and rises in over 0.32 s with a short motion blur (fading copies trailing it) while the one before rises out, and a line too long to fit then glides through. It is all worked out from the playhead, so a seek or a pause draws the right moment. Without timed lyrics, or with Lyrics off, the title and artist glide past instead. Only then does it ask for frames; at rest it draws none.
- While an agent works, the agent keeps the wings and the right one keeps a corner for what plays: the small cover and the equalizer.
- Open, the player leads Home whenever there is a track: the cover with the app's drawn mark (Spotify's, a globe for browsers), the gliding title, the artist in the accent (Spotify's green for Spotify), the album and the Lyrics switch (whether the closed notch shows them), a bar you drag to seek (`TryChangePlaybackPositionAsync`), the times, and previous / play-pause / next. Its glyphs are Fluent System Icons (`assets/fluent/`, from `cargo xtask icons fluent`), the family Windows draws its own flyout in, so they look the same on every system. The lyrics never show under the open player, only on the closed notch. Play, pause and seeks show at once and the next read confirms them.
- On macOS the track comes from the system's now playing service (MediaRemote, what Control Center shows: Spotify, Music, a browser's video). Since macOS 15.4 it answers only Apple's own programs, so a long-lived `osascript` (JavaScript for Automation) loads the framework and prints one JSON line whenever what plays changes (`media::parse_report` in core); it looks twice a second while something plays and every 1.5 s otherwise, and ends by itself when MonoBOT does. Play, pause, next, previous and seeks are each one short `osascript` (`MRMediaRemoteSendCommand`, `MRMediaRemoteSetElapsedTime`). The service keeps the cover to other programs, so it comes from Apple's catalog search (`itunes.apple.com/search`, free, no key), only when the title and the lead artist match (`media::cover_from_search`): no cover rather than a wrong one.

**Face at rest** (`monocode-core/src/notch/face.rs`, `notch/face.rs`). On Windows' handle and in the island style, the resting notch wears two eyes. They follow the pointer between blinks (they look for it every 80 ms and repaint only when it moved), blink every 3 to 6 s, and smile for 4 s when one more agent turn finishes than a moment ago. It asks for frames only during a blink (0.2 s); under reduced motion the eyes stay open and still. Settings › Look › Face at rest turns it off. The Mac's hardware notch keeps no face.

**Clipboard** (`monocode-core/src/notch/clipboard.rs`, `notch/clips.rs`). A fifth tab lists the last 30 copies, in memory only: text up to 64 KB, and up to 4 pictures of 8 MB each (a picture is kept only when the copy has no text, so a spreadsheet's cells stay text). Each row wears its kind's mark and names it beside its age: text, link, code, path, email, a color's swatch, a picture's thumbnail. A click copies one again, moves it to the top and marks it "Copied" for 1.4 s; each has a cross, and Clear empties the list. On Windows a thread compares the clipboard's sequence number every 400 ms and reads the clipboard only when it changed. It skips what its owner keeps out of histories (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory` = 0), as password managers ask. Settings › General › Clipboard history turns it off and forgets the list. On macOS a thread compares the pasteboard's `changeCount` every 400 ms and reads it only when it moved; a picture is kept as PNG (TIFF copies are converted), and copies marked `org.nspasteboard.ConcealedType`, `TransientType` or `AutoGeneratedType` (or 1Password's own mark) are skipped.

**Chat with an agent** (`notch/talk.rs`). A click on an agent in the Agents page slides its chat in, inside the notch, instead of bringing the app forward: the last 8 things said (the user's and the agent's, without tools or thinking), a field and Send, a back arrow, and "Open in app" as the one way that raises the window. The notch says which chat it watches (`NotchEvent::Watch`); the shell loads that Mono's chat if it was never opened, adds what was said to the feed (`Feed::talk`, `notch::said`) and delivers what is typed (`NotchEvent::Tell`): a new turn while the agent rests, a steer while it answers. What cannot be delivered (a draft left in the app's composer, a chat still loading) comes back to the field with the reason.

What was said scrolls. A short chat rests on the field; a longer one scrolls up to its first message. The chat opens at its end and stays there while the answer streams in. Scrolling up to read leaves the view where it is until the reader scrolls back down or sends something. It uses the transcript's live-phase follow (`transcript::live_phase::LiveScroll`). Each streamed update of an agent's chat reaches the notch while the turn runs, not only when it ends: `Monos` forwards it as `NotificationChanged::Content`, and the shell asks for its usual sync, at most four times a second, which the notch redraws only when what was said changed. The fields of this chat and the Chat page draw what is typed in the theme's ink over the theme's surface (`content(0.95)` on `background_base()`), so it reads in either theme.

The chat is followed only while the panel shows: closing the notch stops the watch (`NotchEvent::Watch(None)`) and opening it again resumes it, so a streaming answer never redraws a closed notch. Agents at rest stand still in the Agents page and the chat's header; only one at work hops.

**Folding.** The island's content leaves with a fold only when the shape becomes another kind (the panel closing to its wings, the wings going quiet). The same kind a little smaller, as a shorter state word or a shorter page, shows what is new at once, the way the fade-in already treated it; before, it faded out and popped back. The fade is measured from where the fold began, so closing a panel that was still opening does not start part faded. The face's timers run only where a face is worn (the island and Windows' handle), not on a camera housing.

**Words that change** (`text_morph.rs`, shared by the app). A short label that changes does not jump: the letters both texts share slide to their new places, the ones that go drift up and fade, and the new ones rise in, over 240 ms on the app's emphasized curve. "Text · 2m" becomes "Copied" and back, a clock's minute rolls, "Thinking" grows into "Editing" under the same sweep of light. In the notch it draws the lead's state word and its step count, "+N", the panel's title and summary, the clock and its date, the word on the right of every row (state, kind, "2m"), the chat's "Answering…" and every pill's label. It takes its final width at once and moves only what it paints, on the shared clock, so nothing reflows; it reads the font, size and color around it and cuts a long line with an ellipsis. Under reduced motion the new text is simply there. Left alone on purpose: the player's times, the volume and system percents, which change every second or faster.

**Lyrics off asks nothing.** With the Lyrics switch off, what plays is not looked up at lrclib.net; the switch turns them on and asks then.

**Playing next** (`monocode-core/src/notch/spotify.rs`, `notch/queue.rs`). Under the player, up to 4 tracks from Spotify's own queue (`GET /me/player/queue`); the system's media session does not give it. Notch settings › Music holds the sign-in: the user pastes the Client ID of a Spotify app of their own (Spotify gives no shared one), registered with the redirect URI `http://127.0.0.1:43917/callback`. The sign-in is OAuth with PKCE, so there is no client secret; the refresh token is in the system keychain (`spotify-refresh`) and the Client ID, which is not a secret, in the look. The queue is read off the interface thread 1.2 s after the track changes and again when the panel opens, no sooner than every 8 s, and only while Spotify is what plays.

**Battery.** The levels thread also reads the battery. Plugging in, unplugging, reaching full while plugged, and crossing 20 % or 10 % unplugged open the wings like a volume change: a drawn battery (a bolt while charging), the bar in green, red or ink, and the percent.

**Arranging.** With the pencil, widgets are picked up and dropped on another to take its place (the one under them lights up in the accent); narrower, wider and remove sit in each card's corner. The row of widgets to add scrolls when it holds more than fit (the wheel moves it), with an arrow at each end that steps through it.

**System widget** (`monocode-core/src/notch/stats.rs`). CPU, RAM and GPU, each with its percent and a bar in the accent; two columns wide it also says the memory in gigabytes. On Windows the processor comes from `GetSystemTimes`, the memory from `GlobalMemoryStatusEx` and the graphics from the `GPU Engine` performance counters (PDH), the busiest engine summed over its processes as the Task Manager counts it. It is read off the interface thread every two seconds, only while the widget shows. On macOS the memory is what Activity Monitor calls Memory Used (the apps' memory, wired and compressed, from `host_statistics64`) and the graphics load is the accelerator's `Device Utilization %` (IOKit's `PerformanceStatistics`), next to the processor and the memory pressure word.

**System history.** The widget keeps the last minute of each number (`History`, 30 readings). Two columns wide, each number has its minute drawn behind it as a faint area in the accent; the bars glide from the reading before to the new one over 0.45 s (at once under reduced motion).

**Lyrics on the closed notch only.** The open player keeps just the Lyrics switch; the three lines it used to show under it are gone, on Windows and macOS alike.

**Waveform.** On Windows the equalizer is a waveform of what the output really sounds like: 9 bars, the newest on the right, each the loudest of some 66 ms of the default endpoint's peak meter (`IAudioMeterInformation`), so the trace holds a little over half a second and a beat is seen travelling left (`media::Wave`). The meter is read about thirty times a second on a thread that lives only while the bars are asked for. Where the system has no meter the bars sway by themselves; paused or under reduced motion they rest as dots. macOS gives an output meter only to a process tap, which asks to record the computer's audio and shows the recording mark in the menu bar while it runs, so on the Mac the bars sway.

**Cover.** Opening from the wings that show what plays, the cover flies from its place in the wing to the player's, growing with the island's own spring; its place in the player waits empty until it lands. Not under reduced motion, where nothing springs.

**File tray** (`monocode-core/src/notch/tray.rs`, `notch/tray.rs`). Files or folders dragged onto the notch open it (its edge lights up in the accent) and, dropped, wait in a row on Home above the shelf, newest first, up to 24. A click opens one with its app; each has a button to show it in its folder and a cross to take it out, and Clear empties the row. Only the paths are kept, in memory. They cannot be dragged out again: GPUI does not start a drag to other apps.

**Karaoke.** On the closed notch the lyric line waits dimmed and fills with the accent from its start as it is sung, with a soft edge; a line too long to fit moves to keep that edge in view. LRCLIB times lines, not words, so a line fills evenly until the next one starts, or for as long as its words would take before a pause (`sung` in `notch/media.rs`). Reduced motion shows the line whole.

**More widgets.** Calendar (today, or the week with today in the accent) joins the catalog, and the header shows the battery's charge where there is one (`GetSystemPowerStatus`; on macOS IOKit's power sources, read by the levels thread every two seconds and kept for the header, so drawing never asks the system).

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
| Nov 9 | Built and run on Windows 11 (3440 × 1440, 60 Hz) | Opens from the top edge with its content; `MONOBOT_NOTCH_TRACE=tour` 382 frames, all ≤ 18 ms |
| Nov 9 | Media on Windows | Read the paused track from the system session; LRCLIB answered 43 synced lines through the office network |
| Nov 9 | `notch::` tests, core and GPUI, on Windows | 32 and 13 passed (new: LRC, playhead, LRCLIB URLs, wings and Music widget) |
| Nov 9 | Full GPUI suite on Windows | 1036 passed; the same 22 fail on `HEAD` without these changes (paths with `\`, CRLF, worktrees, shell) |
| Nov 9 | `notch::` tests after the face, the clipboard and the battery | Core 41 passed, GPUI 16 passed; clippy clean in the notch's files |
| Nov 9 | Built and run on Windows 11 | The resting handle shows the two eyes (screenshot). The clipboard watcher and the battery activity were not exercised by hand |
| Nov 9 | `notch::` tests after the chat with an agent, the clipboard's kinds and pictures, and Playing next | Core 48 passed, GPUI 21 passed |
| Nov 9 | Built and run on Windows 11 | The app starts with the notch. The chat with an agent, a copied picture and the Spotify sign-in were not exercised by hand |
| Nov 10 | `notch::` tests on macOS after the Mac's player, clipboard, battery, memory and graphics | Core 64 passed, GPUI 24 passed; clippy clean in the notch's files |
| Nov 10 | This Mac (macOS 27.3): levels, stats | Battery 61 % charging (as `pmset`), memory 18/24 GB (as `vm_stat`), graphics 47 % |
| Nov 10 | Dev app on this Mac with Spotify paused | MediaRemote through `osascript` gave the track and its position, the catalog its cover, LRCLIB 35 synced lines; the reader uses 8 MB and 0.11 s of processor a minute at rest, and ended when the app did. Playing, seeking and the Clipboard tab were not exercised by hand |
| Nov 9 | macOS: `cargo xtask cargo test -p monocode-gpui … -- notch live_call file_editor a_live` after the scrolling chat, streaming into the notch, the folding rule and the watch that ends on close | 75 passed. The two new chat tests fail with their fix taken out (follow off; no `NotificationChanged::Content`) |
| Nov 9 | `cargo xtask check`; `monos::` tests; `notch::hooks` in core | Clean; 116 passed; 11 passed |
| Nov 9 | Optimized build on this Mac, fresh profile, at rest | 0 notch renders in 32 s. With any agent at work the closed notch draws about 56 frames/s (4–6 % of the main thread); see [performance-audit.md](performance-audit.md) |
| Nov 9 | `text_morph` and notch tests after the words that change, lyrics only while on, and the equalizer on the shared clock | Pass (see the full run below). How the morph looks was not seen in the running app |
| Nov 9 | `cargo xtask check` and `cargo xtask test` after all of the above | Clean; core 1317 passed, GPUI 1098 passed |
| Nov 9 | By hand | Not done: the chat's scrolling, the partial answer arriving, the typed text's color and the folding without a blink were not looked at in the running app |

## Next

- By hand: click an agent in the notch, send it a message and check the app's window stays where it is; copy code, a path and a screenshot and check their marks and "Copied"; sign in to Spotify with a Client ID and check Playing next (Spotify may refuse the queue to accounts without Premium).

- Windows: confirm by hand that the panel closes when the pointer leaves, the volume keys and their bloom, and the Music widget's buttons with Spotify and a browser; then the hooks over a named pipe and the brightness.
- Windows: one click on the taskbar button of the minimized workspace still only activates it (a second click restores it, with the error sound). `WS_EX_NOACTIVATE` on the notch was not enough; trace what the shell sends on that click.
- Copy text, a password from a password manager and a large file by hand and check the Clipboard tab; plug and unplug a laptop for the battery activity.
- macOS by hand: play and pause from the notch, seek, next and previous with Spotify, Music and a browser; copy text, a screenshot and a 1Password password; the face's glance in the island style.
- By hand on Windows: drag a file from Explorer onto the closed notch (whether the small topmost window takes the drop), the cover's flight on opening, the bars against real music and silence.
- The tray: dragging a file out to another app (needs a platform drag GPUI does not have), and keeping it between runs.
- A real spectrum for the equalizer (loopback capture and an FFT) instead of the loudness.
- More from boring.notch and Coucou: the Claude and Codex limits as a widget, and the calendar's events.
- Measure the closed player's frames while a song plays (`MONOCODE_FRAME_LOG`).
- Look at it on an external display and over a full-screen app.
- Measure the CPU at rest with `cargo xtask perf`.
- A row in the app's Settings that opens the notch's settings, and a way to hide the notch.
