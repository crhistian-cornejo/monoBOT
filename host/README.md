# MonoBOT remote host (`monocode-host`)

A small Node service that runs on a remote machine (installed by the desktop
app over SSH, or started by hand). It runs the provider CLIs there and serves
sessions, files and Git to the desktop over an authenticated loopback API. It
is not part of the desktop app itself, which is Rust; see
[remote access](../docs/done/remote-access.md) for setup and the protocol limits.

## Layout

| Path | What it holds |
| --- | --- |
| `src/cli.ts` | Entry point: `start`, `serve`, `pair`, `stop`, `service …`, `--version`. |
| `src/server/` | HTTP API, device pairing and tokens, session engine and store, sync transfer, attachments. |
| `src/workspace/` | File browsing, workspace commands, Git branches and worktrees. |
| `src/providers/` | Provider discovery, child processes and their sandbox guard (`provider-guard.mjs`). |
| `src/system/` | User services (systemd, launchd, Task Scheduler) and Windows ACLs; tests for the SSH bootstrap scripts in `crates/monocode-core/src/remote/`. |
| `src/desktop/` | Provider harness and models shared with the old desktop app, pruned to what the host bundle reaches. Keep it in the same shape so changes can be compared. |
| `src/testing/` | Test helpers, such as a client that speaks the desktop's protocol. |
| `scripts/` | `build.mjs` (esbuild bundle), `package.mjs` (self-contained archives with a pinned Node), `runtime-download.mjs`. |

Tests sit beside the code they test (`*.test.ts`, vitest). `build/` is generated.

## Commands

```sh
pnpm --dir host install
pnpm --dir host test        # tsc --noEmit, bundle, vitest, runtime download tests
pnpm --dir host package     # archive for this platform in host/build/host-packages/
pnpm --dir host package -- --all   # all six archives (needs zip, unzip, tar)
```
