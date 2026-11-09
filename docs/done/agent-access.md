# Agent access with /operator

## Enable access

Type `/operator` at the start of a composer message to enable MonoBOT access in that thread. For example, `/operator start two Codex sessions: one to inspect the API and one to review the UI`, or `/operator list my notes`. The slash picker also offers this command. The transcript shows only the request text in a translucent amber bubble; MonoBOT removes the command from the request sent to the agent and supplies the local `app` CLI path and instructions once per provider conversation: Claude Code reads them in its system prompt, and other providers get them with the first message of the conversation and again after the provider compacts it (see [code-harness.md](code-harness.md)). Later turns in the same thread can use the CLI without repeating `/operator`; other threads receive no CLI instructions or app access. The CLI can act only during an active agent turn. The agent can run the shown `app --help` command for the exact JSON input fields.

## CLI actions

Sessions started by an ordinary Operator remain visible in project history.
Monos use the same project actions, with their **Show started sessions** setting
controlling whether their launches appear in the user's history. This is
independent of navigation: `reveal` defaults to `false`, so a background launch
can stay visible in history without switching the user's current conversation.

- `models.list` shows available providers, models, settings, and permission modes.
- `sessions.start` creates a session in the current project with a prompt. Set `reveal: true` to open its tab, or `placement: "right"` / `placement: "down"` to split the calling session's pane; `besideSessionId` selects another visible session pane in the project. Reuse the returned session ID as the next `besideSessionId` to build nested layouts. By default it submits the prompt; set `draft: true` to save it unsent without starting an agent turn. It accepts a provider, model, effort or other model settings, permission mode, and current checkout or new worktree choice. Set `worktreeCwd` to a path from `worktrees.list` for a specific existing checkout. Use `worktrees.create` to create a worktree on a named new or existing local branch, then pass its path as `worktreeCwd`. Omit `runtimeMode` to inherit the calling session's permission mode, or set it explicitly to override. It returns the new session ID as soon as the pane and prompt are accepted, so the agent can move it into a folder immediately.
- `sessions.list` shows project sessions and their archived status. `sessions.read` returns up to three recent user/assistant exchanges, with a cursor for older exchanges and a per-message character cap. `sessions.send` submits a follow-up to an idle session, while `sessions.draft` saves an unsent message for the user to review. `folders.list` and `folders.move` organize project sessions in sidebar folders, including a new folder.
- `sessions.stop`, `sessions.archive` and `sessions.delete` take a `sessionId` to manage another session in the project. Monos can select an assigned project with `project`. Stop cancels the current turn and pauses queued messages; archive stops and saves the conversation for later restoration; delete stops and permanently removes the conversation. Open files, terminals and worktrees are kept. These actions cannot target the caller, Mono chats, habit runs or orchestration workers. Reuse the same request ID when retrying a call.
- When a Mono successfully stops, archives or deletes a session it is monitoring, its pending completion report for that session is dismissed, including any queued report. The Mono confirms the action in its current reply. Reports for other sessions and other Monos are kept. Rejected launches or follow-ups return a CLI error without a later completion report.
- `notes.list` returns titles and short previews; `notes.read` returns one full note by ID.
- `notes.list` accepts `limit` and `offset`. `notes.write` creates a note from its
  `body`, with optional title and tags; pass an existing note `id` to update just
  its title, body or tags while preserving the other fields.

### The user's Mac (macOS)

These actions act on the user's Mac (`monocode-core/src/hands.rs`). A bot
needs the **Use this Mac** access in its settings; the user's own Monos may use
them.

| Action | What it does |
| --- | --- |
| `apps.open {name}` / `apps.quit {name}` | Opens or quits an app by its name, never by a path. |
| `web.open {url, browser?}` | Opens an http or https address. |
| `web.search {query, site?}` | Searches Google or YouTube. |
| `media.control {command, app?}` | Plays, pauses, toggles, skips forward or goes back in Spotify or Music. |
| `media.play {query}` | Opens a Spotify search. |
| `media.now {app?}` | Says what is playing. |
| `system.volume {level}` | Sets the volume. |
| `clipboard.read {}` / `clipboard.write {text}` | Reads or writes the clipboard. |
| `keys.type {text, app?}` | Types into the front window or into `app`. Needs Accessibility. |
| `screen.look {}` | Saves a picture of the screen for the agent to open. Needs Screen Recording. |

Nothing runs through a shell, and AppleScript values are escaped.

Retry an uncertain mutation with the same request ID and identical input.
Unknown fields and invalid options produce errors instead of being ignored.

## Orchestration workers

Orchestration workers keep their existing scoped `control` workflow and do not receive this app access.

Return to the [README](../../README.md) for a quick start.
