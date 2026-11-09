# What to take from IVY

A review on Nov 8, 2026 of `~/Developer/IVY` against MonoBOT. It covers the
code harness, agents, Perplexity Decider, live calls, group calls and
dictation. The notch has its own plan in [notch.md](notch.md). Paths that start
with `IVY/` are in that repository; everything else is in this one.

## Harness and agents, by value per effort

Every gap below was checked by searching `monocode-core` and `monocode-gpui`.

| # | Change | IVY source | MonoBOT today | Effort |
| --- | --- | --- | --- | --- |
| 1 | **Harden every Git call** against a hostile repo: no fsmonitor, hooks, external diff, pager or `protocol.ext`; strip `GIT_DIR`/`GIT_CONFIG`; neutralize dangerous `.git/config` keys | `IVY/crates/core/src/code/git.rs` (`hardened`, `base`, `ALWAYS`, `ENV_REMOVED`, `guard`) | `git.rs:35` and `scm.rs:1152` set only `GIT_TERMINAL_PROMPT=0`; only `checkpoint.rs:899` disables fsmonitor; ten separate `Command::new("git")` | S–M |
| 2 | **Cap the access of agent-started sessions** at the access of the session that started them | `IVY/crates/core/src/agents/subagents.rs`, `spawn.rs` | `app_access.rs:240` copies `runtimeMode` with no ceiling, so a Supervised agent can start a full-access session | S |
| 3 | **Per-project trust**: until the user trusts a repo, run Claude with `--setting-sources user` and ask once | `IVY/crates/core/src/code/trust.rs` | `provider.rs:1717` always passes `user,project,local`, so a cloned repo's hooks and `.mcp.json` load silently | S–M |
| 4 | **Loop guards**: stop a turn that repeats the same text, or the same tool call with the same input six times | `IVY/crates/core/src/agents/guard.rs` | Missing | S |
| 5 | **Project actions**: detect run/test/build from manifests and lockfiles as buttons that type into the terminal on click | `IVY/crates/core/src/code/actions.rs` | Missing | S |
| 6 | **Agent presence** (ready, working, unavailable) in `agents.list`, so the orchestrator stops assigning work to an agent without a provider or quota | `IVY/crates/core/src/agents/presence.rs` | `mono/agents.rs:278` lists agents without state | S |
| 7 | **Command classes on approval cards** (read, write, network, install, push, destructive), with "Always allow" refused for dangerous commands and interpreters | `IVY/crates/core/src/agents/sessions/classify.rs`, `always.rs`; `IVY/crates/rules/src/risk.rs` | `provider/acp.rs:1484` turns Session into `allow_always` unfiltered | M |
| 8 | **`/goal`**: bounded automatic continuation (eight turns at most), never after a failure or a stop, and a waiting user message goes first | `IVY/crates/core/src/code/goal.rs` | Missing | M |
| 9 | **Silent provider notice**: mark a thread silent after five minutes without output | `IVY/crates/core/src/code/reliability.rs` (`watch`) | Turn recovery exists (`checkpoint.rs:377`); `provider/acp.rs:180` deliberately has no deadline | S–M |
| 10 | **Learned skills**: after a turn with three or more tools, the cheapest model proposes a skill for approval; unused ones are archived after 60 days | `IVY/crates/core/src/agents/learning.rs` | `mono/groups/learning.rs` saves facts only | M |

Smaller candidates, in rough order: prompt improvement that keeps `@`, `$skill`
and `/command` intact (`prompt_improve.rs`), a durable team board and profiles
(`board.rs`, `profiles.rs`, also listed as next in
[agent-teams.md](../done/agent-teams.md)), a visible agent browser instead of
Playwright MCP over `npx` (`code/browser.rs`), external Claude and Codex
sessions through hooks (`crates/hook`), Ollama with its own tool loop, and a
Seatbelt sandbox for providers without one.

IVY has nothing to add on memory, agent messages, groups with rounds and
mentions, commit messages, checkpoints, provider switching or worktrees. Its
memory was copied from MonoBOT's.

**Done Nov 8 (1–4):**

- Every background Git call goes through `git::safe`. Its fixed settings run nothing, hooks go to `/dev/null`, the app's Git variables are removed, and the repository's own programs give way to the user's or to nothing. Those programs are fsmonitor, filters, textconv, credential helpers, signers, ssh and core.worktree; an external diff of the repository's own is refused. A commit or a push the user asks for runs as in their terminal, with the repository's hooks. A test runs a hostile repository: plain Git runs its fsmonitor, `git::safe` does not.
- A session an agent starts never has more access than that agent (`AccessMode::at_most` in `app_access::launch_settings`).
- A project whose repository brings `.claude/settings*.json` or `.mcp.json` runs Claude Code with `--setting-sources=user` until the user trusts it (`project_trust`). The chat shows a "Trust project" notice.
- `loop_guard` stops a turn that makes the same call with the same input 6 times in a row, or that keeps writing the same passage. The reason stays as the turn's error row.

**Order for the rest:** 5–6, then 7–9, then 10.

## Perplexity Decider

Decider for Mac (`/Applications/Decider.app`, v0.1.0) runs from
`~/Library/LaunchAgents/ai.perplexity.decider.plist` at login. With no
`config.toml` it listens on `127.0.0.1:8080`, in per-question mode, with no API
key. On Nov 8 `GET /health` answered `ready`. Its log shows a median of 2.4 s
per one-question decision, 4.6 s at p90 and 13 s at most.

**API (protocol v3).** It has two endpoints:

- `GET /health`: 503 while the model loads.
- `POST /v1/decisions`, with the body `{"model":"pplx-decider-flash-v1.2","state":…,"questions":{key:Q}}` and 1 to 128 questions. Unknown fields are rejected. Each question is one of three types:
  - `noul`: a yes/no probability.
  - `choice`: one option out of `criteria`, with probabilities and a confidence.
  - `score`: a weighted level out of two or more ordered `criteria`.

The reference schema is `IVY/crates/decider/src/protocol.rs`.

**How IVY uses it** (`IVY/crates/core/src/agents/decider.rs`, `risk_gate.rs`, `crates/rules/src/{risk,triage,money}.rs`):

- A four-level risk score for commands and edits, before approval.
- "Does this text contain a secret?" before outgoing text.
- Phishing, scam or prompt injection in incoming messages.
- Mail triage into priority, routine or waiting.
- Finance screening: is this money, and is this the same charge.

When Decider is down, destructive actions fail closed (ask the user) and the
rest fail open. Only verdicts, sources and timings are logged, never the text.

**How MonoBOT should integrate it:**

- **Placement.** Put the client in `monocode-core/src/decider/` (`client.rs`, `gate.rs`, `questions.rs`). Question texts and thresholds go in plain functions that are tested without a network. Use `monobot_http` with no redirects. It never runs on the GPUI thread.
- **Discovery.** macOS only at first. Read the port from the user's setting, then `DECIDER_PORT`, then `config.toml`, then 8080. The host is always `127.0.0.1`.
  - Probe `/health` with a 1–2 s timeout and cache the result for 15 s when ready and 4 s otherwise.
  - Confirm that `/v1/models` lists the Decider model, because many dev servers use 8080.
  - Don't launch the process: the LaunchAgent keeps it alive.
- **Authority.** Decider only ever restricts. It can hold a mail or force an approval card, never grant access. When it is absent or slow, the current rules decide.
- **Load.** Two requests at a time:
  - Critical lane: waits for a slot, about 8 s timeout.
  - Background lane: gives up when busy, about 15 s timeout.
  - Cache per message id.
- **First decisions:**
  1. Phishing or injection check on inbound mail before `inbox/workspace/inbound.rs:124` delivers it to a bot.
  2. Risk score on tool approvals in `provider.rs:1039-1100` when access is Auto or Full; a high score forces the approval card.
  3. Inbox triage after `email/src/triage.rs:97` keyword rules, only to promote Routine when confidence ≥ 0.5.
  4. Secret check before a bot sends or drafts mail.
  5. Group calls: one `noul` per member ("has something new to add") instead of paying an LLM call to get `PASS` (`mono/groups/call.rs:104-123`).
- **Tests:**
  - A fake server on `127.0.0.1:0` checks the request body and parses replies.
  - A `choice` outside the options is rejected.
  - "Nobody listening" is an error, not an answer.
  - The background lane gives up when busy.
  - An `#[ignore]` test runs against the real Decider.

## Group calls and dictation: checked Nov 8

Both are wired end to end: microphone, Silero, Parakeet, turn, member seats,
sentences, Supertonic and output. The engines always run off the UI thread, and
the downloads are verified by size and SHA-256. All tests pass:

| Command | Result |
| --- | --- |
| `cargo test -p monobot-dictation -p monobot-voice` | 8 + 6 passed, 4 ignored (network, model, microphone) |
| `cargo xtask cargo test -p monocode-core --profile native-dev dictation` | 11 passed |
| `… -p monocode-core … groups::call` | 6 passed |
| `… -p monocode-gpui … dictation` | 11 passed, 2 ignored |
| `… -p monocode-gpui … call` | 15 passed |

Problems found, most important first:

1. **Fixed: the voice model could unload mid-call.** Every hang-up released Supertonic 120 s later with no use count, so a new call started within those two minutes lost the model between sentences. `VoiceUses` in `monos/group_call.rs` now counts holds and uses, and a release runs only when no call holds the voices and none started since.
2. **Fixed: a disconnected microphone hung the session.** Stream errors were ignored. `dictation/src/capture.rs` now records the first fatal error (an underrun is only a glitch), lets the stream go, and the session sends `Event::Lost`. The dictation keeps what was heard and shows why.
3. **Fixed: painting another group hung up the live call.** `call_panel` now shows "You are in a call with …" with Hang up for any other group, and only hanging up ends the call.
4. **Fixed: echo tail** (see [voice-and-hands.md](voice-and-hands.md), phase 1). Calls are half duplex, so there is no echo, but the mic reopens only 100 ms after playback drains (`voice/src/player.rs:139`). IVY waits 400 ms, which matters on Bluetooth and in rooms with reverb.
5. **Fixed: AirPods** (built-in mic with a headset on, one output stream per call). The microphone and the output reopen on every turn, which flips Bluetooth between A2DP and HFP and clips the start of sentences.
6. **Fixed: gaps between sentences** (synthesis one ahead). `voice/src/queue.rs:95` synthesizes the next sentence only after the current one finishes playing; synthesizing one ahead removes the pause.
7. **Turn end of at least 1.1 s** (0.45 s VAD silence plus `TURN_GAP_MS` 600 ms, plus transcription). A pause mid-sentence ends the turn. IVY uses Smart Turn for this.
8. **Fixed: text cleanup** (¿¡, capitals, pauses, misheard names; fillers not yet). There is only `text::join`. IVY's `tidy` adds the opening «¿¡», capitals at sentence start, and fillers.
9. **Stale docs.** `port-plan.md:277` still lists group call voices as missing, and `dictation/engine.rs:1-2` mentions Tauri.

## Live calls with Gemini

This is feasible, at roughly 2–3k lines plus echo cancellation on macOS.

**How IVY does it** (`IVY/crates/voice/src/{gemini,gemini_live}.rs`):

- The connection is a WebSocket to `BidiGenerateContent`, with the key in `x-goog-api-key`.
- The audio is 16 kHz PCM16 in 20 ms frames in, and 24 kHz out with a 100 ms buffer.
- The setup asks for input and output transcription, session resumption and context compression, and falls back across models.
- `interrupted` flushes the playback queue.
- Agents are reached through a function call. A call that takes over 8 s answers "running" with `WHEN_IDLE`, and the result arrives later as a note.
- Echo cancellation on macOS comes from VoiceProcessingIO in the Swift host, and on Windows from `aec.rs`.

IVY's group calls do **not** use Gemini Live. They run on the local pipeline:
Parakeet with partials, Smart Turn, and WeSpeaker to tell echo from the user.

**Proposal:**

- **Placement.** A new crate `crates/live` holds the protocol, the session and the audio, without GPUI. The prompt, the tools and the member mapping go in `monocode-core/src/mono/groups/live.rs`, and a "Live" mode goes in `monos/group_call.rs`.
- **Key.** The Gemini key goes in the keychain (service `com.monocode.desktop`), entered in Settings.
- **Group calls.** Gemini is the host with one voice. It calls an `ask_member {member, request}` tool that sends to the member's seat (`monos/groups.rs:271`) and returns its answer when ready. Running one Live session per member costs more and makes echo between voices hard to separate.
- **Reuse.** The cpal capture, the player logic, tokio, rustls through `monobot-http`, base64 and keyring are already in the lockfile.
- **Port from IVY.** The setup and frames from `gemini.rs`, and the `gemini_live.rs` loop: waiting, interruption, reconnect on `goAway`.
- **Rewrite.** The sink, microphone ownership and echo cancellation on macOS, which can come from `AVAudioEngine` voice processing through `objc2-avf-audio`. Until then, the call stays half duplex with a 400 ms tail.
