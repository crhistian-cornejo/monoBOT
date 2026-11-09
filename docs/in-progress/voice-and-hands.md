# Voice, live calls and hands: plan to match IVY natively

A review on Nov 8, 2026 of what IVY (`~/Developer/IVY`) does for voice, calls
in the JOSIE-Voice style, realtime speech and acting on the computer, against
what MonoBOT does today. It extends [ivy-review.md](ivy-review.md), which
already covers the harness, Decider and the Gemini Live design for group
calls. Paths that start with `IVY/` are in that repository.

## Where MonoBOT stands

Group calls and dictation work end to end and their tests pass (see
ivy-review.md), but the scope is narrow:

| Area | MonoBOT today | IVY |
| --- | --- | --- |
| Ear (STT) | Parakeet v3 int8 + Silero, one phrase at a time, no partials | Parakeet, Whisper turbo, Qwen3-ASR (hotwords), CoreML Parakeet; ElevenLabs Scribe in the cloud; partial every 320 ms |
| Voice (TTS) | Supertonic 3, en/es, one synth at a time | Phonon (local `ptts`), ElevenLabs, Gradium, Phonic, Gemini TTS, system voice fallback; synth ahead |
| Realtime speech | None | Gemini Live (`BidiGenerateContent`) and Phonic STS, both with tools |
| Calls | Group calls only, half duplex, a "Done talking" button, mic and speaker reopen every turn | Calls with any agent or group, open mic, Smart Turn v3.2, WeSpeaker echo check, barge-in, acks, wake word «Hey IVY» |
| Models | Two hardcoded packs, no delete, installed from the composer or the call panel | Catalog pinned by size and SHA-256, resumable Range download, repair, delete, "Voz completa" one-click |
| Keys | Keychain only for mail | Keychain for ElevenLabs, Gradium, Phonic, Gemini; validated, redacted |
| Settings | No voice page | Ear, voice, live provider, keys, models |
| Hands | `monocode_app` MCP (sessions, mail, web read…), Playwright over `npx`, Poki for the iPhone | Native open app/page/search, media and Spotify, windows, volume, clipboard, typing, read window (AX), look at screen, CUA through `cua-driver`, Office files written directly |
| iPhone | `mobile-link`: the phone drives desktop sessions; Poki taps the phone | Same direction plus remote desktop input, phone calls with Gemini Live, Siri intents |

So the answer to "is it all implemented?" is no: only the local group call is.

**Done Nov 8: the ear on the Neural Engine.** The user asked for only the
Parakeet IVY uses, not the whole catalog. That is Parakeet v3 converted to
Core ML (FluidInference, the files IVY pins), now run straight from Rust in
`crates/dictation/src/neural.rs` through `objc2-core-ml`. There is no Swift;
the TDT decoding is ported from FluidAudio's `TdtDecoderV3`.

- On Apple silicon it is the preferred build (`model::Engine`).
- A Mac with the CPU build downloads it at launch (about 460 MB), keeps
  dictating with the CPU meanwhile, then deletes the CPU model (over 650 MB).
- It is released after 45 s idle instead of 3 min, since it reloads in about
  0.3 s.
- Measured on this Mac: 7 s of Spanish transcribed exactly in 0.1–0.3 s, a
  0.28 s load once Core ML has cached its compile (the first compile took
  30 s), and a 71 MB process.
- Other platforms keep the CPU build.

The other ears in the table (Whisper, Qwen3, Scribe) are out of scope.

## Principles

- **Nothing heavy by default.** Every engine, key and model is opt-in from
  Settings › Voice. Nothing loads until used and everything is released when
  idle (dictation already does this after 3 min, voices after 120 s).
- **One audio owner.** A single `AudioSession` in `crates/voice` owns the mic
  and the output for the whole call, instead of opening cpal streams per turn.
  Calls, dictation and live sessions borrow it. This fixes the AirPods
  A2DP/HFP flip and the clipped sentence starts.
- **Core decides, GPUI paints.** Protocols, state machines and timings go in
  crates without GPUI and are tested with fake audio. GPUI shows state and
  forwards clicks.
- **One tool surface.** New actions are added to the existing `monocode_app`
  bridge (`mono/bridge.rs`, already a localhost TCP server with a secret, no
  `npx`), so every CLI and every live session sees the same actions and the
  same approval rules. No second MCP server.
- **Speech never authorizes.** Destructive or outward actions (send, delete,
  shut down, CUA clicks) still need a card click.

## Phases

Each phase ships on its own and leaves the app working.

### 1. Fix the current call (done Nov 8)

From ivy-review.md items 4–6 and 8:

- **One output stream for the whole call.** `voice/src/player.rs` keeps one
  cpal stream on the call's player thread, from the first sentence to
  hang-up. It plays silence between sentences, so a Bluetooth link stays
  awake. A fresh stream starts with 150 ms of silence, and the stream
  reopens when it fails or the default output changes.
- **Synthesis one sentence ahead.** `voice/src/queue.rs` synthesizes on one
  thread and plays on another, with one ready sentence waiting at most, so an
  interruption wastes little.
- **Echo tail at the end of a round:** 400 ms on speakers and 120 ms on
  headphones, before `Drained` reopens the mic. Between sentences there is
  none. The old wait was 100 ms after every sentence.
- **Mic choice.** `dictation/src/devices.rs` never opens a Bluetooth
  headset's mic when the computer has its own, so AirPods stay in A2DP. The
  mic still reopens each turn, but on the built-in device that costs nothing.
  `in_ears` tells headphones and Bluetooth headsets from speakers.
- **Text.** For Spanish only, `dictation/src/text.rs` adds the opening ¿¡,
  sentence capitals and `rejoin_pauses` (". y luego" becomes ", y luego").
  English stays as Parakeet wrote it.
- **Misheard names.** `call::spoken_address` (in `mono/groups/call.rs`)
  rewrites «Hola yares, …» as «Jarvis, …» before the turn order is chosen.

Left for phase 3: the turn end (item 7, Smart Turn) and keeping the mic open
across turns with barge-in.

### 2. Settings › Voice, models and keys (done Nov 8)

- **Keys.** `monocode-core/src/voice/keys.rs` keeps the Gemini and Phonic
  keys in the keychain (service `com.monocode.desktop`, account
  `voice-<provider>`).
  - A pasted key is trimmed and checked (16 to 200 printable characters).
  - `ApiKey` never prints itself.
  - `keep_in_memory` makes the app's own tests use memory, never the real
    keychain.
- **Models.** `Dictation::remove` and `Voice::remove` delete a model
  (refused while it is in use), and `disk_bytes` says what it takes.
- **The Voice page** (`settings/pages/voice.rs`) sits in the App group. It
  has three groups:
  - Dictation: the Parakeet build and its size, with Download, "Get the
    faster model" and Delete (asks once).
  - Call voices: Supertonic, with the same controls.
  - Live calls: how calls talk (This Mac, Gemini Live or Phonic) and a secret
    field per key. The field clears once the key is saved. Removing the
    chosen provider's key sends calls back to this Mac.
- Not done, on purpose: the cloud voices for the local call (ElevenLabs,
  Gradium, Gemini TTS). The user asked for fewer models, and a live provider
  covers the cloud case.
- Tests: `settings/voice_tests.rs` (six, with clicks), plus the core, model
  and dictation engine tests.

### 3. Call mode in the JOSIE-Voice style (done Nov 8, except speaker echo and wake word)

- **Turn rules** (`dictation/src/turn.rs`, ported from IVY's
  `rules/call.rs`):
  - The pause depends on the pace (patient 1400 ms, normal 800 ms, quick
    450 ms).
  - It is ×0.6 after a question or an exclamation, and ×1.8 after an open
    ending (a comma, «y», «pero», a hesitation, or "and" and "um" in
    English).
  - When Smart Turn finds a turn unfinished, it waits 1 s more.
  - A turn needs at least 300 ms of voice and lasts at most 30 s.
  - `Tracker` follows a turn through the voice detector, counting the
    detector's 450 ms lag in the silence.
  - Backchannels («sí», «ajá», "yeah", a laugh) never cut in, and
    `echo_of` tells the members' own words heard back.
- **Smart Turn v3.2** (`dictation/src/smart_turn.rs`): IVY's port as is
  (Whisper log-mel features, tract, rustfft, pure Rust).
  - The model is pinned by size and SHA-256 (32 MB) and downloaded at
    launch with the ear.
  - It is loaded only for calls, and released with the ear.
  - On this Mac: a 0.08 s load and 0.04 s per verdict.
  - It gave 0.90 for a finished sentence. macOS's synthetic voice also read
    as finished when cut off mid-sentence, so judge it with real speech.
- **The session decides turns.** `Dictation::start_call` gives the
  session a `CallMode`; it sends `Event::DecidesTurns`, then
  `Event::TurnEnd` when the rules say so. The group call ends the turn on
  that event instead of its fixed 600 ms. An engine that cannot decide
  keeps the old timer.
- **Cutting in, with headphones.** When the voice goes into the user's ears
  (`devices::in_ears`), the mic stays open while members answer. A turn
  with real words that are not the members' echo interrupts them and
  becomes the next turn; a backchannel lets them go on. On speakers the
  call stays half duplex.
- **Settings › Voice › Answer after a pause** chooses the pace.
- Tests:
  - 9 rule and tracker tests and the Smart Turn feature tests.
  - The real model, as an ignored test.
  - 3 GPUI call tests: the session ends the turn, cutting in with
    headphones while a backchannel goes by, and speakers keeping the mic
    closed.
- Left:
  - Telling the user's voice from the speakers' echo (WeSpeaker), so
    cutting in works on speakers too.
  - Partials every 320 ms (today they come per phrase).
  - The wake word «Hey IVY».
  - Acknowledgements and fillers.

### 4. Live speech: Gemini Live and Phonic (done Nov 8; not tried against the real services)

- **`crates/live` (`monobot-live`)**, no GPUI:
  - `gemini.rs` and `phonic.rs` build and parse every message.
  - `session.rs` runs a call on its own thread:
    - setup, audio both ways, transcripts and interruptions;
    - tools on worker threads: a tool slower than 8 s gets "still running",
      then a late note on Gemini or repeated "still working" lines on
      Phonic; at most 3 run at once;
    - cancellation, `goAway` with the resumption handle, three reconnects a
      minute;
    - IVY's Gemini model ladder when a model is refused;
    - a clear error for a refused key.
  - `transport.rs` is a blocking `tungstenite` socket over rustls. The key
    goes only in a header.
- **`voice/src/duplex.rs`**: the microphone goes up in 16 kHz frames while
  speech plays as it arrives. The gate holds the mic while speakers play and
  for the 400 ms tail; with headphones nothing is held.
- **`monocode-core/src/mono/groups/live.rs`**: the host's instructions, the
  `ask_member` schema with the members' names, and finding a member even
  when misspelled.
- **`monos/group_call_live.rs`**: when Settings chooses a live provider,
  Start call opens the live session instead of Supertonic.
  - The user's and the host's words become lines.
  - `ask_member` sends the request to the member's own seat, and its answer
    goes back to the model.
  - Hanging up closes the session off the interface thread and saves the
    transcript.
  - Without a key, the call falls back to this Mac and says so.
- Tests:
  - `crates/live`: 23, including a real local WebSocket.
  - The duplex gate, and 5 in core.
  - Two GPUI tests: a full ask through a seat, and refusals.
- Not checked: a call against Google's and Phonic's servers with a real key,
  and the model names in the ladder (taken from IVY).

### 5. Hands on macOS (done Nov 8, except CUA)

`monocode-core/src/hands.rs` adds twelve `monobot app` actions:
`apps.open/quit`, `web.open/search`, `media.control/play/now`,
`system.volume`, `clipboard.read/write`, `keys.type` and `screen.look`.

- Each action becomes a `Plan` (program and arguments) that is checked and
  tested before it runs.
- Nothing runs through a shell. Values sent to AppleScript are escaped, only
  http and https addresses open, and app names cannot be paths or options.
- Clipboard text goes on stdin, never as an argument.
- Bots need the new **Use this Mac** access (`bot_tools::ACCESS`). The
  user's own Monos may use the actions.
- The actions are listed in `bridge.rs` HELP and checked in `api.rs`.
- Tests:
  - Eight plan tests.
  - A GPUI test that a bot without the access is refused.
  - An ignored test that runs `media.now` on this Mac; it passed.
- Left:
  - CUA through `cua-driver` (screenshot and accessibility tree, then
    click), with IVY's allowlist and a card per action.
  - Writing Office files directly.
  - Giving the live host these actions as direct tools, so «pon jazz» needs
    no agent turn.

### 6. iPhone (later)

Poki already taps the phone. Next: the phone as mic and speaker for a call
over `mobile-link` (it carries no audio today), and remote desktop input with
scopes.

## Order

1 → 2 → 3 and 5 in parallel (different crates) → 4 → 6. Phase 4 needs the
audio owner from 1 and the keys from 2; phase 3 needs the catalog from 2.
