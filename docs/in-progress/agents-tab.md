# Agents tab and effort shimmer — built Nov 11, pending checks

Two pieces landed together; both compile, pass their tests and are clippy
clean. What is left is runtime verification, not code.

## Agents tab

An **Agents** tab in the right panel (and the Classic sidebar and project
rail) shows the Claude chat in view's subagents and workflow runs, live.
`monocode_core::claude_runs` reads the files Claude Code writes under
`<CLAUDE_CONFIG_DIR>/projects/<cwd>/<session>/`; `agents_pane.rs` polls it on
a background thread (1.5 s on screen, 6 s off). See
[desktop.md](../done/desktop.md#agents-tab).

**Pending:** try it against a real session. Open a Claude chat and have it
start subagents or a `/swarm`-style workflow; the tab should appear, list each
run's phases in script order with their agents' states and latest step, and
disappear when the agents do. Worth checking too: the Classic layout's rail
and sidebar entries, switching between chats with and without agents, and an
agent that stops writing (Stopped after ~20 min).

## Effort shimmer

Strong reasoning efforts in a model menu — Max, Ultra, Ultracode, Ultrathink —
get a tile shimmer behind the row while it is highlighted, one colour per
effort (`effort_tiles.rs`, `TONE_*` in theme.rs). Reduced motion draws a
static layer.

**Pending:** open a model menu and hover a strong effort row; the shimmer
should fill from the row's centre, pulse the edge three times and fade after
about five seconds. Check it under reduced motion too.

## Unrelated failures already on main

Four tests fail without any of the above changes, still open:

- `classic_surface_tests`: closing with `cmd-w`.
- The terminal's dock.
- CRLF line breaks in `orchestration` tests.
