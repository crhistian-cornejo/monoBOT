# MonoBOT documentation

Documents are sorted by state. When something is finished, update its document
and move it from `in-progress/` to `done/`; when a document only records how
things were, move it to `old/`.

## done/ — works today

| Document | About |
| --- | --- |
| [desktop.md](done/desktop.md) | Running, building and checking the desktop app; data paths and fixtures |
| [architecture.md](done/architecture.md) | Crate boundaries, rendering rules and the pinned GPUI patches |
| [building.md](done/building.md) | Platform prerequisites, `cargo xtask`, packages, releases and update signing |
| [install.md](done/install.md) | Installing the Linux packages |
| [providers.md](done/providers.md) | Installing and signing in to each agent CLI |
| [code-harness.md](done/code-harness.md) | What MonoBOT adds around a coding agent: context sent once, large attachments, loop reminders, `/goal` |
| [remote-access.md](done/remote-access.md) | The remote host over SSH, its protocol and packaging |
| [agent-access.md](done/agent-access.md) | `/operator` and the `monobot app` actions agents can call |
| [agent-teams.md](done/agent-teams.md) | Specialist bots and the coordinating orchestrator |
| [research-web.md](done/research-web.md) | What agents can do on the web: search, read, download, papers |
| [jira.md](done/jira.md) | The Jira Cloud inbox |
| [mermaid.md](done/mermaid.md) | Mermaid diagrams in Markdown and chats |
| [visuals.md](done/visuals.md) | Interactive session visuals |
| [release-notes.md](done/release-notes.md) | Notes used for the next GitHub release |

## in-progress/ — open work

| Document | About |
| --- | --- |
| [tauri-gap-audit.md](in-progress/tauri-gap-audit.md) | What the removed Tauri app did that the native app still lacks, by priority |
| [port-plan.md](in-progress/port-plan.md) | The phased plan and its progress table |
| [limitations.md](in-progress/limitations.md) | Known limits and what still needs runtime checks, per area and platform |
| [mail-workspace.md](in-progress/mail-workspace.md) | Mail, bot inboxes and agenda with Cloudflare and Mailflare; deployment checklist |
| [workstreams.md](in-progress/workstreams.md) | Workstreams (beta): multi-day work run by an orchestrator |
| [ivy-review.md](in-progress/ivy-review.md) | What to take from IVY: harness and agent gaps, Decider, live and group calls, dictation |
| [notch.md](in-progress/notch.md) | The notch on macOS and Windows: what is built, checked and next |
| [voice-and-hands.md](in-progress/voice-and-hands.md) | Voice: done so far are the Neural Engine ear, call fixes, Settings › Voice, Gemini Live and Phonic calls, and Mac actions; done too are JOSIE-style turns with Smart Turn; left are speaker echo, the wake word and CUA |

## old/ — history

| Document | About |
| --- | --- |
| [verification.md](old/verification.md) | Dated records of checks, including the Tauri era |
| [ui-parity-followup.md](old/ui-parity-followup.md) | A finished round of UI parity fixes |
| [macos-review.md](old/macos-review.md) | macOS checklist written for the Tauri app |

`assets/` holds the screenshot, the visual examples used by fixtures and the
third-party license texts referenced from [NOTICE](../NOTICE).

Other READMEs: [mobile](../mobile/README.md), [remote host](../host/README.md),
[mail Worker](../deploy/cloudflare-mail/README.md),
[generators](../scripts/generators/README.md).
