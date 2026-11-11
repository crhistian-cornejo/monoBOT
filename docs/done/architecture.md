# Native architecture

The GPUI app is monoBOT's only desktop app. The React/Tauri app it replaced was
removed from the repository; [tauri-gap-audit.md](../in-progress/tauri-gap-audit.md) lists the
behavior it had that the native app still lacks. CI, releases and the updater
feed build the native app.

## Code layout

| Location | Responsibility |
| --- | --- |
| `crates/monocode-core/` | Provider transports, persistence, accounts, Git/files, remote connections and shared services |
| `crates/monocode-gpui/` | Native shell, composer, transcript, editor, settings and platform UI |
| `Cargo.toml` (root) | Cargo workspace for every crate, profiles and dependency patches |
| `xtask/` | `cargo xtask`: development runner with manual restart (`dev`), optimized build and macOS packaging (`build`), installers, update signing |
| `scripts/generators/native-editor/`, `scripts/generators/native-settings/` | Generators for embedded JavaScript assets |
| `vendor/gpui-pre*/` | Pinned framework and platform patches |

`monocode-core` has no Tauri or GPUI dependency. Keep provider and storage
behavior in core services, with UI work in GPUI views. Development uses isolated
data; release builds use the installed desktop database. See the
[native guide](desktop.md#data-and-configuration) for paths and commands.

Provider transports use Codex app-server, Claude stream-json, OpenCode HTTP/SSE,
ACP for Cursor/Grok/Antigravity/fx/Hermes/Devin, and Pi/omp RPC. They reuse the
providers' CLI authentication and conversation IDs.

## Secrets

Every secret (mail passwords and tokens, voice keys, a sign-in's refresh token,
the phone link's identity) goes through one door, `monocode_core::secrets::entry`
(`crates/email/src/base.rs`, `keychain`), to the system's store: the Keychain
on macOS, Credential Manager on Windows, the secret service on Linux. The
service ids stay internal (`com.monocode.desktop`, `com.monocode.mobile`),
because changing them would lose what users saved.

The system's store is used only after the app says so
(`secrets::use_system_keychain()`, the first line of its `main`). Until then
secrets live in the process's memory. A test binary never calls it, so no test
of any crate reads the user's secrets or makes macOS ask for them; the same
switch keeps tests away from Claude Code's own Keychain item
(`usage/claude.rs`). Before this, the GPUI tests reached the real Keychain,
because a crate's `cfg(test)` is off when it is built as a dependency, and
macOS asked the user to let `monocode_gpui-<hash>` in.

On macOS the system's dialog must name the app, not an internal id
("monoBOT wants to use … stored in “monoBOT”"). macOS words that dialog from
the access list an item is created with, which keeps the name the item had
then: naming the item afterwards changes what Keychain Access shows and not
what the dialog says (checked on a real Keychain with a throwaway item). So an
item not made under the name is made again the first time it is touched in a
run (`keychain::remake`): the secret is read, which asks the user exactly as
the read that follows was about to and for the last time under the old id,
then the item is deleted and added again named "monoBOT" with a note that marks
it as done. If the named item cannot be added, the secret goes back as it was.
The service and account that find the secret never change.

A development build is signed anew on every build, so macOS asks again after
each one; an installed, signed release is asked once.

## Rendering and responsiveness

Keep provider I/O, parsing, file access and persistence off the UI thread.
Composer input should not rebuild the transcript. Transcript rows have stable
identities, bounded blocks and virtualized layout; streaming updates should
preserve the reader's position. Cancellation must remain responsive under load.

The editor embeds generated analysis and formatting bundles. After changing their
source services or dependencies, run `pnpm install` and `pnpm build` inside
`scripts/generators/native-editor/` (its sources live in that folder). Generated bundles and their license notices belong
in the repository.

## Framework patches

GPUI and its platform crate are pinned to `gpui-pre` 0.3.9. Local Apple renderer
patches cover alpha blending, font matching, backdrop blur and shadows. The
headless Metal regression is documented in [verification](../old/verification.md).

On macOS a window above the menu bar (level 25 and up, as the notch's panel)
counts as on screen whenever it is ordered in (`on_screen` in
`vendor/gpui-pre-macos/src/window.rs`). AppKit reports such a mostly
transparent panel occluded, which stopped its display link: frames paused for
up to a second after a resize, or for good in a large fixed panel.

Two patches let the workspace window close to the background and come back:
on Windows, activating a hidden window shows it first (`activate` in
`vendor/gpui-pre-windows/src/window.rs`), as AppKit's `makeKeyAndOrderFront`
does; on macOS, the reopen callback runs on every Dock click, not only with
no visible window, because the notch's panel counts as one
(`should_handle_reopen` in `vendor/gpui-pre-macos/src/platform.rs`).

The vendored framework/platform crates remove the unused `zed-scap` dependency.
The native build rejects lockfiles that reintroduce it, and the `screen-capture`
feature remains disabled. Preserve these guards and the licenses in [NOTICE](../../NOTICE).

See [preview limitations](../in-progress/limitations.md) for current release work.
