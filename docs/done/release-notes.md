# monoBOT 0.0.1

The first monoBOT release under its own name, logo and update service. The
app is built only with Rust + GPUI.

## What's in it

- Native desktop app for Claude Code, Codex, Cursor, Grok Build, OpenCode,
  Antigravity, Pi, omp, fx, Hermes Agent, Devin and Droid, using each CLI's
  own sign-in.
- Monos (personal agents) with groups, news, habits, memory, documents and
  cited sources; native PDF, Word, Excel and PowerPoint viewers.
- Mail workspace with bot inboxes, agenda and optional Cloudflare/Mailflare
  integration.
- Mobile companion pairing over encrypted QUIC.

## Updates

- monoBOT checks for updates when it opens and every four hours. A new
  version downloads by itself; restart to install it. On Windows it also
  installs when you quit.
- If an update can't finish, the sidebar offers the installer to download by
  hand.
- Every update is signed with monoBOT's update key and checked before it
  installs.

## Before you install

- Versions start again at 0.0.1. A 0.16 preview cannot update itself to this
  release: install it from the download page. Your data is kept.
- macOS is the main verified platform. Linux and Windows packages still need
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
