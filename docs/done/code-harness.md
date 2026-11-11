# What monoBOT adds around a coding agent

monoBOT runs each provider's own agent CLI (see [providers.md](providers.md)).
The CLI owns the model, its tools and its own compaction. monoBOT owns what it
puts in the agent's messages and when it steps in during a turn. These rules
keep that part small, because every message stays in the conversation and
every later request pays for it again. Several come from DeepSeek Harness,
which spends its effort on the same thing: what the model re-reads on each
request.

## Context sent once per conversation

Standing context goes once per provider conversation, not with every message.
That covers the `/operator` rules, the visual CLI hint for providers without
MCP tools, and a Mono's rules, soul and memory.

- **Claude Code** reads the `/operator` rules and a Mono's rules in its system
  prompt (`--append-system-prompt`). The system prompt is identical on every
  request, so the provider's cache reuses it, and compaction does not drop it.
- **Other providers** get the context with the first message of each provider
  conversation. A receipt in the store (`monocode_core::introduction`) records
  which conversation already has it. Only a completed turn writes the receipt.
  A new provider conversation, a provider switch or a change to the context
  sends it again.
- **After a compaction**, the provider may have summarized that context away.
  When a turn reports a completed `contextCompaction`, monoBOT clears the
  receipts, and the next turn sends the context again.

## Large text attachments

A text attachment of up to 24 KB goes into the message whole. A larger one goes
as its first 12 KB and last 4 KB, cut at line breaks. A note between them says
how many lines were left out and asks the agent to read them from the file's
path, which the message gives. The agent reads the part it needs, and the rest
does not fill the conversation (`composer::attachment_excerpt`). Files over
128 KB still go by path only, as before.

## Loops

`monocode_core::loop_guard` watches every turn:

- **The same text over and over** stops the turn.
- **The same tool call with the same input and the same result, many times in
  a row** depends on the provider. A call is judged once it has finished:
  providers announce a call before its input streams in, and judging it then
  took any run of calls to one tool (eight different `Bash` commands, say) for
  a loop. A call whose answer changes, like checking on a background job, is
  progress and never counts. Waiting starts the count over: a waiting tool
  (`TaskOutput`, `BashOutput`, `Monitor`, `ScheduleWakeup`…), a command that
  sleeps (`sleep`, `Start-Sleep`, `timeout /t`, a blocking task read) or any
  call that ran 15 s or more. An agent that checks a slow type-check, waits
  and checks again is never stopped for it.
  - A provider that takes a message mid-turn (`Kind::can_steer`: Codex, Claude
    Code, Cursor, Pi, omp, Hermes and OpenCode) is reminded at the 3rd, 5th
    and 8th repeat and stopped at the 12th. The reminder asks the agent to read
    the last result, then change its approach, wait longer or finish. It
    travels as a steer on the turn's control channel. The chat shows it as the
    turn's status, not as a message from the user.
  - Any other provider is stopped at the 8th repeat.

The stop reason stays as the turn's error row. A stop for repeated calls also
shows the provider-retry notice above the composer (`provider_retry`'s
`Looping`), with **Continue** and no countdown: it sends the turn on, asking
the agent to read the result it kept getting and try another way.

## Goals

`/goal <objective>` keeps the agent working toward one objective over several
turns, so a long task runs on without you typing "continue". The logic lives in
`monocode_core::goal`, and `conversation/goal.rs` runs it.

- **The first turn** sends the objective, and its rules come after it. When a
  turn ends cleanly without finishing, monoBOT sends the next turn itself. The
  chat shows that turn as "Continue the goal (2 of 8)". The agent reads the
  objective, the turn's number and the rules.
- **The agent ends its final message** with `GOAL: complete` when the goal is
  done and checked, or with `GOAL: blocked: <what it needs>` when it needs you.
  Only the last non-empty line counts.
- **It is bounded:**
  - It takes at most 8 automatic turns. Then it pauses.
  - A turn that fails or that you stop pauses it. It never continues after one.
  - A message you queued goes first, and the goal continues after it.
  - Reaching the provider's usage limit pauses it.
- **The notice above the composer** shows the objective, the turn and why the
  goal paused or is blocked. It has Pause or Resume, and Clear. Resume starts a
  fresh allowance of 8 turns.
- **Commands**, as in T3 Code: `/goal` shows the goal, and `/goal pause`,
  `/goal resume` and `/goal clear` act on it. Remote projects and Mono chats do
  not have `/goal`. A provider's own `/goal`, such as Claude Code's, is still
  available as `/claude:goal`.

T3 Code hands `/goal` to the provider: Claude Code's version keeps one turn
open with a Stop hook, and Codex's (`thread/goal/set`) starts turns of its own.
monoBOT runs its own loop instead. It works the same with every provider, and
it fits its one-turn-per-message runtime, which Codex's self-started turns do
not.

## Limit resets

A subscription can hold resets for its limits. monoBOT reads them, offers one
before the limit, and uses it at the limit so the agent keeps going. The rules
live in `monocode_core::usage::resets`; the accounts center
(`accounts/resets.rs`) and the conversation (`conversation/accounts.rs`) apply
them.

- **What is read.**
  - **Codex:** its banked resets (`account/rateLimits/read`), spent with
    `account/rateLimitResetCredit/consume`.
  - **Claude Code:** its reset grants, the same ones `/limit-reset` uses. They
    come from `GET /api/oauth/usage?cedar_ember=1&skip_spend=1`. Each grant says
    how many uses it has left, which limits it refills (`five_hour`, `seven_day`,
    `seven_day_overage_included`…) and until when. A grant that gives back what
    Fable used of the weekly limit is the Fable reset; the others are 5-hour,
    weekly and full resets.
  - **At a limit:** also the weekly 5-hour reset (`?at_wall=1`, `juniper_tide`).
  - **How it is spent:** `POST /api/organizations/{org}/reset_rate_limits`, with
    a fresh `request_id`, so a retry never counts twice. The organization comes
    from `oauthAccount.organizationUuid` in Claude Code's `.claude.json`. The
    CLI's token is only read, never refreshed.
- **When.** At 98% of a limit, if a reset refills that limit, the chat offers it
  above the composer: "Claude Code is at 98% of its 5-hour limit. Use your
  5-hour reset (2 left) when you reach it?" The offer has **Use at the limit**
  and **Not now**. The reset is never spent below the limit: the last few
  percent are still the user's, and Claude's grants refuse a use before the
  limit anyway. At the limit, after a yes, the reset is used and the
  conversation goes on by itself. Without a yes, the limit notice has a **Use
  5-hour reset** button. A use that fails is said in the notice and not tried
  again by itself.
- **Which one.**
  1. The reset made for the limit you hit: 5-hour for the 5-hour limit, weekly
     for the weekly one, Fable for Fable's weekly limit.
  2. Then the narrowest: 5-hour, weekly, Fable, full.
  3. Then the one that expires first.

  The Fable reset is the last choice for the general weekly limit, because it
  only gives back what Fable used.
- **Setting.** Settings › Providers › Limit resets:
  - **Ask** (default): offers at 98% and uses at the limit after a yes.
  - **Automatic:** uses it at the limit without asking.
  - **Off.**
- **Usage footer.** The usage details list Claude's resets under "Limit
  resets", like Codex's banked resets, each with its uses left and its expiry.

## Not done yet

- Limit resets: Claude's reset endpoints are the CLI's own and undocumented.
  Reading and spending them is covered by tests against their recorded shape
  (from Claude Code 2.1.322), not against the live service.

- Project actions that run tests after edits. See item 5 in
  [ivy-review.md](../in-progress/ivy-review.md).
