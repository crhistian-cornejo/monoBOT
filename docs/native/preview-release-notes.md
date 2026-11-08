# MonoBOT 0.16.0 preview

First MonoBOT release built only with Rust + GPUI. The earlier React/Tauri
desktop app is no longer part of MonoBOT.

## What's in it

- Native desktop app for Claude Code, Codex, Cursor, Grok Build, OpenCode,
  Antigravity, Pi, omp, fx, Hermes Agent, Devin and Droid, using each CLI's
  own sign-in.
- Monos (personal agents) with groups, news, habits, memory, documents and
  cited sources; native PDF, Word, Excel and PowerPoint viewers.
- Mail workspace with bot inboxes, agenda and optional Cloudflare/Mailflare
  integration.
- Mobile companion pairing over encrypted QUIC.

## Before you install

- macOS is the main verified platform. Linux and Windows packages still need
  wider runtime testing; see [preview limitations](parity.md).
- Updates are signed with MonoBOT's own update key. Builds signed with the old
  Tauri key cannot update to this release; install it fresh.

**Updates:** Settings → General → About checks for and installs the next preview.
