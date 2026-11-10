# Contributing

monoBOT is early and I’m the only maintainer, so small and focused lands much faster than large and ambitious. Past that, the door is open - bug reports and fixes are genuinely welcome.

Please don’t open PRs that add a new provider right now. The existing harnesses still need to agree on a few patterns, and a new adapter would copy whatever is there today. See [New providers](#new-providers).

## Get it running

You need a current stable Rust toolchain and at least one provider CLI installed and logged in. The desktop tooling is Rust too (`cargo xtask`); Node.js and pnpm are only for the mobile app and the remote host. See [provider setup](docs/done/providers.md) for installation and login instructions, and [building from source](docs/done/building.md) for platform prerequisites and packaging.

```bash
cargo xtask dev
```

`cargo xtask help` lists the other commands. The mobile app is a separate pnpm project in `mobile/` (`pnpm --dir mobile …`); see its [README](mobile/README.md).

## Where things live

- `crates/monocode-core/` - everything without UI: provider transports, sessions and SQLite storage, Git and files, bots, mail, the mobile link. No GPUI dependency.
- `crates/monocode-gpui/` - the desktop app: shell, composer, transcript, editor, settings and platform integration. All Rust crates share the root Cargo workspace and lockfile.
- `crates/{mobile-link,email,http,dictation,voice,visual}/` - shared crates used by core and the app.
- `mobile/` - the Expo companion app.
- `host/` - the remote host installed over SSH ([remote access](docs/done/remote-access.md)).
- `deploy/cloudflare-mail/` - the optional inbound mail Worker.
- `xtask/` - the desktop tooling behind `cargo xtask`: dev runner, builds, macOS bundles, installers, update signing.
- `scripts/generators/` - JavaScript sources of the bundles the app runs in QuickJS; `scripts/perf/` - the profiling replay.

`crates/monocode-core/src/provider/` is the most useful place to start if you want to fix something real. Each provider transport translates its CLI's protocol into monoBOT's own events, and the parsers have unit tests beside them, so you can fix a Codex parsing bug with only Claude Code installed. That's for the providers we already ship - please don't add a new one yet.

## Before you push

```bash
cargo xtask check
cargo xtask test
```

That runs what CI runs: `cargo fmt`, strict `cargo clippy`, the xtask tests and the core and GPUI tests. If it's green locally it should be green on GitHub. See [verification](docs/old/verification.md) for focused commands.

## New providers

I’m pausing new harnesses until the current ones share the same patterns - session lifecycle, catalog probes, usage, approvals, and how slash commands and skills are wired. A PR that adds another provider will be closed for now, even if the work is good. Fixes, tests, and protocol bugs on Claude, Codex, Cursor, Grok, OpenCode, Antigravity, Pi, omp, fx, Hermes Agent, and Devin are still the best kind of contribution.

When the pause lifts, this section goes away.

## Pull requests

Keep a PR to one thing, and say what changed and why. The [PR template](.github/pull_request_template.md) covers the rest. If it changes the UI, a before/after screenshot helps a lot.

For anything that moves product direction - a new surface, new provider behavior, a refactor that changes the shape of the app - open an issue first. That’s not gatekeeping, I’d just rather you hear “I’m already halfway through that” before you write it than after. New providers are the exception: don’t send the adapter, even from an issue, until the pause above is gone.

I might close a PR, ask you to shrink it, or end up implementing the idea differently. That’s a call about scope and timing, not about you or the quality of your work.

Be kind: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security reports: [SECURITY.md](SECURITY.md).
