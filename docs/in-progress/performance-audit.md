# Audit: resources, voice and calls, the notch, macOS 27

An audit on Nov 9, 2026 of what the desktop app costs at rest and at work,
whether voice and live calls do what they say, the notch's frames and
transitions, and what macOS 27 adds that the app could use. Four read-only
reviews ran beside measurements of the optimized build on this Mac (Apple
silicon, macOS 27.3 beta). Findings marked **read** were confirmed by reading
the code path; **measured** ones have numbers below. Nothing here was checked
by hand in the running app unless a line says so.

## Measured

Optimized build (`cargo xtask build`), a fresh profile in a temporary `HOME`
so no session's hooks reach it, music paused.

| Case | Result |
| --- | --- |
| Start | 5–26 % of one core for about 20 s, then quiet |
| At rest, window open | 0.7–1.2 % CPU, 122 MB footprint (64 MB of it GPU), 16–18 threads, 1–2 frames every 10 s |
| At rest, the notch | 0 renders in 32 s |
| At rest, a 10 s `sample` | Every thread parked; the only work seen is the notch's levels thread asking CoreAudio 5 times a second |
| A replayed reply (`cargo xtask perf frame-run`, 21 s) | 59 frames/s on average, 0.76 ms per draw, main thread 7.6 %; 20–23 % while the text streams (the window and the notch each draw about 56 frames/s) |
| Closed notch while any agent works | About 56 paint-only frames/s and 4–6 % of the main thread, for as long as a session works. Claude Code and Codex sessions outside the app count: their hooks keep the wings up |

The rest state is healthy. The standing costs are the loops that run while an
agent works, and a few polls that run whether or not anything changed.

## Fixed in this pass

| What | Where | Checked by |
| --- | --- | --- |
| **A live call never heard the user.** The session was put in a shared `Option` for the microphone thread and taken out again a few lines later, so every frame was dropped. The microphone now keeps its own way in (`Session::microphone`). Audio heard while the call connects is dropped instead of arriving late in one burst | `crates/live/src/session.rs`, `monos/group_call_live.rs` | `a_microphone_keeps_its_way_in_after_the_session_moves` |
| **Mute did not mute a live call.** The panel said "Muted" while frames kept going up. The gate now has a mute, and the call sets it | `crates/voice/src/duplex.rs`, `monos/group_call.rs` | `a_muted_call_sends_nothing_up_even_with_headphones` |
| **Interrupt, unmute and a turn's end opened this Mac's own ear inside a live call**, a second microphone that transcribed the call. The local ear now stays shut in a live call, and Interrupt stops the host's voice and drops the rest of that answer | `monos/group_call.rs`, `crates/voice/src/duplex.rs` | `a_live_call_keeps_the_local_ear_shut_and_says_why_it_ended` |
| **Why a live call ended was never shown.** The reason was set on a state that hanging up then dropped. It now stays on the panel | `monos/group_call_live.rs` | Same test |
| **Each `cargo xtask perf` run left 9 hooks in `~/.claude/settings.json` and 8 in `~/.codex/hooks.json`**, for a socket in its temporary folder, for good. Every hook event then ran one more command per past run. Profiling runs now install none (`MONOCODE_NOTCH_HOOKS=off`), and installing forgets entries whose socket is gone | `crates/monocode-core/src/notch/hooks.rs`, `shell/notch.rs`, `xtask/src/perf.rs` | `entries_for_sockets_that_are_gone_leave_and_everything_else_stays`, `installing_forgets_a_deleted_copys_hooks` |
| **An open file was read whole, and Git asked about it with about 8 processes, every 2 s**, changed or not. The poll now costs one `stat` off the interface thread until the size or time changes; Git's own changes already arrive as `GitChanged` | `file_editor.rs` | `the_poll_reads_the_file_again_only_when_it_changed_on_disk` |
| **The notch's content blinked whenever it got a little smaller**: a shorter state word, music pausing, a shorter page. It faded out with the fold and popped back. Content now leaves only when the shape becomes another kind, as the fade-in already assumed | `notch/mod.rs` (`leaves`) | Not covered by a test (tests run under reduced motion) |
| **Closing a panel that was still opening** snapped its content to part faded. The fade is measured from where the fold began | `notch/mod.rs` | Not covered by a test |
| **The face's two timers woke the interface 12 times a second on a Mac with a camera housing**, where no face is worn. They start only when one is | `notch/face.rs` | Notch tests |
| **Agents at rest hopped at 60 frames/s** in the Agents page and the chat's header. They stand still unless they work | `notch/mod.rs` (`face_or_provider`) | Notch tests |
| **A chat left open kept being gathered and redrawn with the panel closed.** The notch watches a chat only while the panel shows | `notch/mod.rs` (`set_open`) | Notch tests |

A second pass the same day, on what the list below first held:

| What | Where | Checked by |
| --- | --- | --- |
| **Lyrics were fetched with Lyrics off**, so the title of whatever played went to lrclib.net for nothing. They are asked for only while Lyrics is on; off, the player's switch turns them on and asks then | `notch/media.rs` (`want_lyrics`), `notch/mod.rs` | `lyrics_are_not_asked_for_while_lyrics_is_off` |
| **Typed text in a live call took this Mac's path.** It goes to the model as a turn of the user's (`Session::send_text`, Gemini's `clientContent`); Phonic takes none, and the panel says so | `crates/live`, `monos/group_call.rs` | `typed_text_goes_up_as_a_turn_of_the_users`, the live call test |
| **No microphone permission check before a live call.** It asks, or says where to turn it on, before connecting | `monos/group_call_live.rs` | Not covered by a test |
| **Hanging up could leave the microphone on for 15–30 s** while the session still connected. The audio stops at hang up (`Duplex::stop`) | `crates/voice/src/duplex.rs`, `monos/group_call_live.rs` | Not covered by a test |
| **A network away for a moment ended the call.** A connection that fails after the call was live is tried again within the allowance (now 5 a minute), waiting 0.5 s, then twice as long each time up to 4 s; a refused model still steps down at once | `crates/live/src/session.rs` | `a_network_that_is_away_for_a_moment_does_not_end_the_call`, `a_call_that_cannot_begin_says_so_at_once` |
| **`in_ears` was read once.** It is asked again when a call starts and each time the members finish speaking | `monos/group_call.rs` (`call_check_ears`) | Not covered by a test |
| **The transcript parsed the turn's tool calls as JSON on every patch and reveal frame, and the notch's sync did so 4 times a second.** Each call is read once and kept by its first row; a patch forgets the call it touches and the later ones | `transcript.rs` (`ToolKinds`, `forget_tools_from`) | `the_steps_of_a_turn_follow_every_patch_to_its_task_list` |
| **The equalizer, the karaoke line and the gliding title redrew at the display's rate and laid the notch out each time.** They run on the shared clock: 60 a second at most, keeping the frame's measurements | `notch/media.rs` | Notch tests (reduced motion); not measured on a 120 Hz display |
| **A terminal with a job in front spawned `ps` every second.** The job's name is asked for when the job in front changes, and again every 10 s | `terminal.rs` | Terminal tests |
| **Each track's decoded cover stayed for the session.** A cover that is replaced or gone is evicted | `notch/media.rs` (`forget_cover`) | Not covered by a test |
| **The GPUI tests read the user's real Keychain**, and macOS asked to let `monocode_gpui-<hash>` use what is stored in "com.monocode.desktop". A crate's `cfg(test)` is off when it is built as a dependency, so core's and the mail crate's "tests use memory" never held for the app's tests. Secrets now reach the system's store only after the app's `main` says so; see [architecture.md](../done/architecture.md#secrets). On macOS the items are also named "monoBOT" | `crates/email/src/base.rs` (`keychain`), `monocode_core::secrets`, `main.rs` | `until_the_app_says_so_secrets_live_in_memory_and_are_found_again`; the full suite ran without the dialog being reported. The naming was not seen in a real dialog |

After both passes: `cargo xtask check` clean; `cargo xtask test` 1317 (core) and
1098 (GPUI) passed; `monobot-email` 101, `monobot-live` 27, `monobot-voice` 6.

A third pass the same night, on the notch opening late under the pointer.
Measured with `MONOBOT_NOTCH_TRACE=tour` (the tour now rests the pointer,
waits `OPEN_DELAY` and opens, five times, after the app settles) on the
`native-dev` build `cargo xtask dev` runs, in a temporary `HOME`.

| What | Where | Checked by |
| --- | --- | --- |
| **The panel opened up to a second late, and the whole app froze meanwhile.** Each resize of the notch's window (opening, and before this pass every change of the state word's width) made AppKit draw the layer inside its own transaction; GPUI keeps 2 drawables and both were still held, so `nextDrawable` blocked the main thread 100 ms to 1 s. A 10 s `sample` of the running app had the main thread waiting there about half of its busy time. The notch's layer now keeps 3. Hover to open went from 90 ms + 100–1000 ms to 91 ms every time, the first frame within 18 ms, 60 frames/s throughout | `notch/platform.rs` (`style`) | The tour's trace; not covered by a test (tests draw no Metal) |
| **The wings changed width with every state word** ("Running", "Delegating"…), each change a resize and a possible stall above. One width per state now; see [notch.md](notch.md) | `core/notch.rs` (`AGENT_WING`), `notch/media.rs` (`MEDIA_WING`, `music_wing`) | `agents_under_the_music_take_the_left_wing_and_keep_the_width`, `the_wings_show_who_works_and_what_the_lead_does` |
| The notch's window is not restorable: there is nothing to bring back, and AppKit no longer records it. A `sample` of the running app showed AppKit's state saving on 23 % of the samples, but `sample` counts a thread waiting for a core as busy and the Mac was compiling; real thread times (`ps -M`) of the bundled app while a reply streams give 12 % of a core in all, 11 % of it the main thread, and no other thread over 3 %. So no saving was measured | `notch/platform.rs` (`style`) | Not covered by a test |
| **With the closed notch animating (music, agents), the main thread waited for a drawable 16 % of the time** (446 of 2843 samples at rest with music). With the third drawable: 0 of 2720 | Same | `sample` of the main thread, before and after |
| The content came in a little sooner: fade delay 40 → 20 ms, the widgets' stagger 30 → 20 ms, the opening spring's response 0.34 → 0.30 s | `notch/mod.rs` | Taste; tests run under reduced motion |

Tried and left: keeping the notch's window at the panel's size for good also
removed the stall, but doubled the app's CPU at rest and grew its footprint.

Seen and left:

- The main window keeps GPUI's 2 drawables, and waits for one about 11 % of
  the time while a reply streams (88 of about 770 samples). A third would
  cost one more window of pixels, about 25 MB at this size; frames already
  hold 53 a second, so only latency would change. Yours to decide.
- The first frosted surface of a kind (`MPSImageGaussianBlur` for a new
  sigma) can hold the main thread about 0.5 s while Apple's library warms
  up, once. Seen once with a sampler attached, not again without it.
- While a reply streams, `native-dev` spends 40–48 % of the main thread
  (the optimized build 20–23 %) on about 53 frames a second per window:
  the session marks, the composer's runner, the transcript and the notch
  each ask for them. That is the 60 frames/s choice above.

The `native-dev` build (opt-level 1) is what `cargo xtask dev` runs; draws in
it cost about twice the optimized build's (0.6–1.5 ms against 0.6–0.9 ms for
the notch), still far inside a frame.

A fourth pass on Nov 10 (Windows), on the resources list below. Not
measured: no frame or memory log was taken before and after.

| What | Where | Checked by |
| --- | --- | --- |
| **A provider turn's loop woke 62 times a second while the provider was silent.** It now waits up to 100 ms when it has nothing batched, and a tick only while events wait to go out or a stop is under way; a stop, an approval or a steer is still seen within 100 ms | `core/provider.rs` (`IDLE_WAIT`) | Core tests |
| **The composer's runner kept its QuickJS runtime for the life of the chat.** The runner, and its runtime, go once a turn is over and its last jump landed; in a window behind another it runs at 30 frames/s | `composer_runner.rs` (`resting`), `conversation.rs` | Clippy; GPUI tests |
| **Resting Mono faces breathed at 60 frames/s.** A working hop runs at 30, a breath and a blink at 15 | `monos/avatar.rs` | GPUI tests |
| **The mobile catalog was built every 3 s with nothing paired.** It is built only while a phone is paired or an invitation waits (`MobileHost::serving`) | `core/mobile/host.rs`, `shell/mobile_host.rs` | Core tests |
| **The background's fade on Windows and Linux painted one quad per row of pixels on every frame.** The fade is straight between its stops, so it is now one gradient per gap (5–8 quads) | `core/project_appearance.rs` (`background_fade_stops`), `project_background.rs` | Not covered by a test; not seen in the app |
| **Nothing slowed on a locked screen.** On Windows the shared clock now paces loops as for a hidden app while the lock screen is in front (asked at most once a second) | `pulse.rs` (`screen_locked`) | Not covered by a test (tests leave it off) |

Left from item 4 below: the Monos' 10 s tick still notifies every time; what
it refreshes notifies on its own only in some paths, so dropping it needs
those paths checked first. Item 6 on macOS (a locked screen) is still open.

## Left to do, by value

### Yours to decide

- Decided on Nov 10: the working spinner everywhere, and the closed notch's
  loops (the sweep on the state word, the equalizer, the gliding title, the
  lyric's fill), run at 30 frames/s (`Rate::Steady`). Changes of a moment (a
  word trading letters, a lyric rising in) keep 60, and a view now says its
  pace on every frame: before, one frame at 60 kept it at 60 for as long as
  it animated. The main window still draws about 53 frames/s while the chat
  in view streams, since the composer's runner and the transcript ask for 60.

### Resources

1. **The composer's runner calls QuickJS 60 times a second for the whole
   turn** and its runtime is never released (`composer_runner.rs:103`,
   `conversation.rs:2863`). Read.
2. **A provider turn's loop wakes 62 times a second even when the provider is
   silent**, plus a forwarder thread at 10 (`core/provider.rs:61`,
   `runtime.rs:141`). Wait longer while nothing is pending. Read.
3. **The mobile catalog is rebuilt on the interface thread every 3 s** with no
   phone connected and nothing changed (`shell/mobile_host.rs:254`). Read.
4. **The Monos' 10 s tick** notifies and rescans cards whether or not anything
   changed (`monos/scheduler.rs:1364`); resting avatars in the Monos' panels
   breathe at 60 frames/s (`monos/avatar.rs:129`). Read.
5. **The notch's levels thread polls 5 times a second and its media thread
   wakes every 250 ms, for good** (`core/notch/system.rs:76`,
   `notch/platform.rs:680`). CoreAudio property listeners and IOKit's power
   source notifications would make volume and battery event-driven. Read.
6. **Nothing in the notch pauses on a locked screen or a sleeping display**,
    and the shared clock does not slow for a window that is visible but not
    focused (`pulse.rs:167`). Suspected for the lock screen; read for the clock.
7. **Clipboard pictures are never evicted** when their row leaves the list.
   Read.
8. Smaller: three separate 30 s usage clocks; a 1 Hz thread that outlives the
    parked provider pool (`core/provider/parked.rs:359`); `platform::screen`
    asked twice per notch render; the notch's body tessellated as two paths
    every frame.

Looked at and left alone: the resource monitor's 500 ms tick measures
interface stalls by its own lateness, so slowing it would change what it
measures.

### Privacy

- With Lyrics on, a browser video's title is still looked up as a song, and
  the cover of whatever plays is searched in Apple's catalog on macOS
  (`media::cover_from_search`), Lyrics on or off: the system keeps the cover
  to itself, so the search is how the player has one. Read.

### Voice and calls

Not run against Google or Phonic here: no call was placed. The fixes above
make audio and typed text flow by test; the first real call is still to be
made by hand. The setup's differences from IVY were left as they are for that
reason: changing what is sent to Google without a call to check it against
would be a guess.

1. **A lost microphone is silent**, and there is no stall watchdog or ping, so
   a half-open socket reads nothing for good. IVY watches for a stall
   (`IVY/crates/voice/src/gemini_live.rs`, `STALL_LIMIT`). Read.
2. **The Gemini setup differs from IVY's**: every tool is sent with
   `"behavior":"NON_BLOCKING"` (IVY sends none to 3.1 and 2.5), and
   `scheduling` sits beside `response` instead of inside it. Suspected; neither
   was run.
3. Audio quality: the live voice's resampler restarts on every chunk and there
   is no jitter buffer; an output device change drops what is queued and holds
   the microphone shut for its length; the output callback takes a lock that
   `feed` holds while it resamples. Read; how audible is not known.
4. Waste: the default output device is asked for on every chunk (25–50 times a
   second); each call turn builds a new Silero, microphone stream and three
   threads; nothing holds Parakeet through a call, so an answer over 45 s
   unloads it mid-call.

Correct as it is: 16-bit little-endian PCM at 16 kHz up and 24 kHz down,
nothing sent before `setupComplete`, the key only in the header.

### Notch transitions

- Reopening on the same tab mid-fold pops the content from nothing to full
  (`appeared` is not reset). Read.
- Reduced motion is read once at start (`shell/notch.rs:23`); it should follow
  the system's notification. Read.

## macOS 27

macOS 27 is "Golden Gate": 27.0 on Sep 14, 2026, 27.2 on Oct 26, 27.2.1 on
Nov 9 (the latest public); there is no 27.1, and 27.3 is in beta. Checked
against Apple's release notes and the 27.2 SDK's headers.

| What | Since | Needs a Swift extension | Use here |
| --- | --- | --- | --- |
| `NSStatusItem.expandedInterfaceDelegate`: a status item that opens a custom window with menu tracking | 27.0 | No | A sanctioned home for the notch's panel on a Mac without a camera housing |
| `NSGlassEffectView` in an `NSGlassEffectContainerView`; `effectIsInteractive` is new in 27 | 26.0 / 27.0 | No | Real Liquid Glass for the island style. One container batches the passes |
| `kAudioDevicePropertySuggestedReferenceDevice`, and realtime-safe blocks for `AVAudioSourceNode` and `AVAudioSinkNode` | 27.0 | No | The echo reference for calls on speakers, which today stay half duplex |
| `isLowPowerModeEnabled`, `thermalState`, `NSView.displayLink` | 12.0 / 14.0 | No | Slow the loops on battery saver or heat. Nothing new in 27, and not used yet |
| `NSPasteboard.accessBehavior` and `detect*` | 15.4 | No | 27.2 removed the privacy preview's switch, which suggests the alert is on for everyone; not confirmed. The clipboard tab reads on every change, so test it on 27.2 or later |
| Live Activities on the Mac (ActivityKit lists macOS 27.3; they appear in the menu bar) | 27.3 beta | Yes | A system-drawn agent activity. Wait for the release; the 27.2 SDK still marks it unavailable |
| Control Center controls and desktop widgets, with a new extra-large portrait family | 26.0 / 27.0 | Yes | Agents and "needs you" as widgets |
| SpeechAnalyzer's capture providers; Foundation Models' `LanguageModel` protocol | 26.0 / 27.0 | No, but Swift only | Optional ear or model behind a small Swift shim |

Not there: any public API for the area around the camera housing, and any
public way to read what the system plays. The new NowPlaying framework only
publishes an app's own sessions, so the `osascript` reader stays.
