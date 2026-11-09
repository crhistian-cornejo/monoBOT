# What MonoBOT adds around a coding agent

MonoBOT runs each provider's own agent CLI (see [providers.md](providers.md)).
The CLI owns the model, its tools and its own compaction. MonoBOT owns what it
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
  When a turn reports a completed `contextCompaction`, MonoBOT clears the
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
- **The same tool call with the same input, many times in a row** depends on
  the provider:
  - A provider that takes a message mid-turn (`Kind::can_steer`: Codex, Claude
    Code, Cursor, Pi, omp, Hermes and OpenCode) is reminded at the 3rd and 5th
    repeat and stopped at the 8th. The reminder asks the agent to read the last
    result, then change its approach or finish. It travels as a steer on the
    turn's control channel. The chat shows it as the turn's status, not as a
    message from the user.
  - Any other provider is stopped at the 6th repeat.

The stop reason stays as the turn's error row.

## Not done yet

- `/goal`: bounded automatic continuation. See item 8 in
  [ivy-review.md](../in-progress/ivy-review.md).
- Project actions that run tests after edits (item 5 there).
