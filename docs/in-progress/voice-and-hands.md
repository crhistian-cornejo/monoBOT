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

### 1. Fix the current call (small)

From ivy-review.md items 4–8, all in `crates/voice` and `crates/dictation`:

- Keep one input and one output stream open for the whole call.
- Echo tail of 400 ms (700 ms without headphones), not 100 ms.
- Synthesize the next sentence while the current one plays (`queue.rs`).
- Prefer the built-in mic over a Bluetooth headset mic; treat headphones as
  echo free.
- Text: `tidy` (¿¡, capitals), `rejoin_pauses`, the misheard name fold
  (`IVY/crates/core/src/agents/groups.rs` `sound`, `spoken_address`).

### 2. Settings › Voice, model manager and keys (medium)

- `crates/voice/src/catalog.rs`: one catalog for every local model (Parakeet,
  Whisper turbo, Qwen3-ASR, Silero v6, Smart Turn v3.2, WeSpeaker, Supertonic)
  pinned by size and SHA-256, ported from `IVY/crates/voice/src/models.rs`
  (free space check, staging folder, 4 MiB Range chunks, archive fallback,
  atomic rename, `remove`).
- `monocode-core/src/voice/keys.rs`: Gemini, Phonic, ElevenLabs and Gradium
  keys in the keychain (service `com.monocode.desktop`, account
  `voice-<provider>`), mirroring `inbox/workspace/secrets.rs` with its test
  mock. Settings only stores "saved" flags.
- A Voice page in the App group: ear, voice, live provider, keys (secret
  inputs), models with size, Download, Delete and progress (pattern from
  `settings/pages/poki.rs`).
- Cloud voices (ElevenLabs flash, Gradium, Phonic, Gemini TTS) behind one
  `Voice` trait with the local voice as fallback and the reason shown once.

### 3. Call mode in the JOSIE-Voice style (large)

Port `IVY/crates/voice/src/call.rs` and `IVY/crates/rules/src/call.rs` into
`crates/voice/src/call/` and `monocode-core/src/voice/call.rs`:

- Open mic, Silero in 32 ms windows, partial every 320 ms and at 160 ms of
  quiet; the last partial becomes the transcript when it covers the turn.
- End of turn: patient 1400 / normal 800 / quick 450 ms, shortened after ?/!,
  lengthened after an open ending, plus Smart Turn after 200 ms.
- Barge-in: WeSpeaker voiceprint against the app's own voice and the words it
  said lately; backchannels («sí», «ajá») do not cut in.
- Calls with any session, Mono or group (not only groups), a push-to-talk
  hotkey on the existing Carbon hotkey (`quick_composer/hotkey.rs`), acks and
  a filler at 1.5 s.
- Warm the agent before the first turn (the parked pool already exists in
  `provider/parked.rs`; add a prewarm when a call opens).

### 4. Live speech: Gemini Live and Phonic (large)

As designed in ivy-review.md: a `crates/live` crate with the WebSocket
protocols, the model ladder with cooldowns, `goAway` resumption, interruption
and the 100 ms jitter buffer. Tools: quick actions answered at once, and
`ask_agent {request}` that runs a turn in a MonoBOT session, answers
"running" after 8 s and injects the result later as a note. Up to 3 tasks at
once on Gemini, one on Phonic.

### 5. Hands on macOS (medium–large)

New `crates/hands` without GPUI, exposed as `monobot app` actions:

- `apps.open/close`, `web.open/search/play`, `media.control/play/now`,
  `spotify.search/play` (Web API with the user's own app; AppleScript
  fallback), `system.volume/dark_mode/clipboard`, `keys.type` (CGEvent through
  `objc2`), `windows.list/focus` and `window.read` (Accessibility API).
- `screen.look`: a ScreenCaptureKit capture returned as an image, behind an
  approval card.
- `computer.use`: the pinned `cua-driver` binary as one persistent process,
  with IVY's allowlist (no terminals, password managers or System Settings;
  every click needs a fresh capture; a card for each call).
- Office: write .docx/.xlsx/.pptx directly (port `IVY/crates/hook/src/office`)
  instead of relying on skills.
- Live sessions get the quick ones as direct tools, so "abre Spotify y pon
  algo de jazz" runs without an LLM turn.

### 6. iPhone (later)

Poki already taps the phone. Next: the phone as mic and speaker for a call
over `mobile-link` (it carries no audio today), and remote desktop input with
scopes.

## Order

1 → 2 → 3 and 5 in parallel (different crates) → 4 → 6. Phase 4 needs the
audio owner from 1 and the keys from 2; phase 3 needs the catalog from 2.
