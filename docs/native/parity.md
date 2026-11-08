# Native preview limitations

The native app is MonoBOT's only desktop app, still in preview. A feature being
present in source does not establish that it was checked at runtime on every
platform. See [verification](verification.md) for recorded checks and
[tauri-gap-audit.md](tauri-gap-audit.md) for behavior of the removed Tauri app
that is still missing here.

## Providers

All twelve providers have discovery, model selection and native transports.
The live results below are historical spot checks, not certification of every
provider feature or the current revision.

| Provider | Transport | Last recorded live coverage |
| --- | --- | --- |
| Claude Code | stream-json | Streaming, resume and cancellation during the initial native slice |
| Codex | app-server | Streaming, relaunch persistence, resumed context and cancellation (Oct 22) |
| Cursor | ACP | Catalog, streaming, two-turn resume and read-only Plan mode (Oct 19) |
| Grok Build | ACP | Catalog, streaming and two-turn resume (Oct 19) |
| OpenCode | HTTP/SSE v1/v2 | Current transport has source review only; earlier ACP checks do not cover it |
| Pi | RPC | Catalog, streaming and two-turn resume (Oct 19) |
| omp | RPC | Catalog, streaming and two-turn resume (Oct 19) |
| fx | ACP | Catalog, streaming and two-turn resume (Oct 19) |
| Hermes Agent | ACP | Catalog; prompt blocked by upstream account credit (Oct 19) |
| Antigravity | ACP | Protocol fixtures only; no installed server for a live check |
| Devin | ACP | Catalog, streaming, two-turn resume and supervised write approval (Oct 19) |
| Droid | ACP | Catalog, streaming, autonomy mapping and fallback models |

Changes to provider lifecycles on Oct 21 still need live checks, especially
steering, compaction, rewind, warm resume, questions and MCP confirmations.
Command discovery before the first turn is incomplete outside Pi/omp.
Provider permissions depend on the CLI: Cursor's default policy did not request
approval for every edit. Native Supervised mode does not add that guarantee.

## Application work

| Area | Remaining verification or work |
| --- | --- |
| Chat | Live attachment/steering/rewind checks, complete keyboard selection and screen-reader navigation, richer nested task artifacts and provider-specific task controls |
| History and workspace | Transcript/database paging, cache runtime checks, remaining desktop layout consumers, explicit import/export |
| Files and Git | Platform visual/IME checks, Linux/Windows media hosting, non-Git checkpoints, per-file checkpoint Undo/Keep and live checks of orchestration workers applying their worktrees back |
| Papers and PDF | The PDF page rasterizer runs natively without Chromium/PDFium on Windows (`Windows.Data.Pdf`), macOS (`CoreGraphics` + `PDFKit` word bounds) and Linux (`libpoppler-glib.so.8` + `libcairo.so.2` via runtime `dlopen`, falling back to extracted-text view when absent). Detection by `%PDF-` signature, per-page text extraction, positioned word-span extraction (`extract_page_spans`), the shared paper cache, the `papers.*` app actions, in-PDF search, on-page x/y citation/search/selection highlights, and "Add to chat" from visual or text selection are implemented. A Mono opens a cited paper in its details panel (Documents), not a code pane, and the viewer fits the page to the pane and zooms (buttons, pinch, Cmd/Ctrl+wheel; the page is redrawn wider past 100% so it stays sharp). Cited sources show each site's own icon (fetched once per host through the guarded fetcher and kept in `site-icons/` under the cache dir, letter badge while loading or when a site has none), and the Activity tab follows the latest turn and lists its searches and sources (port of `turnResearch`). Real-PDF visual checks across platforms remain |
| Accounts and settings | Live OAuth/MCP authentication checks and remaining platform-specific settings behavior |
| Inbox | Runtime/visual checks, inline video, linked-update notices and richer CI evidence/repair progress |
| Notes | Runtime/visual checks and composer note-card presentation |
| Automations | Scheduler, provider and recovery checks; current implementation has source review only |
| Monos | Existing-data checks, live heartbeat and background delegation checks, native visual/document accessibility, floating-document interactions and live checks of resumed orchestration runs |
| Remote connections | Real-machine installation/update, existing-data restoration and cross-platform checks; the existing host's feature limits still apply |
| Poki and model welcome | Poki (iPhone connector, macOS Apple Silicon only) installs the pinned release, starts and stops its daemon and shows its folder, status and seven illustrated tutorial scenes in Settings › MCP. The scenes play once, respect reduced motion and clearly label simulated status as an example. The Astra and Opus 5.5 welcome animations are painted natively. These still need checks against a real iPhone, a real Poki install. See [UI parity follow-up](ui-parity-followup.md) |
| Integrations | Completion/approval notifications, sounds and Dock badges; Quick Composer attachments/shortcut checks, MCP/skills checks and remaining app-control endpoints. macOS reminder delivery and click routing passed on Oct 22 |

## Platform and distribution

macOS is the primary verified preview target. Remaining verification includes
accessibility, IME and scroll profiling, crash/recovery testing and actual
packaged upgrades.

On October 25, a Linux ARM64 source build was launched and exercised in an
Ubuntu 24.04 Noodle VM using Rust 1.98 and a VM-specific graphics workaround.
This covered launch and basic UI interaction, not live provider requests or
installation/upgrades of the downloadable Linux x64 packages. On October 27,
a Windows 10 x64 source build passed manual title-bar interaction checks;
broader Windows GUI behavior and the downloadable packages still need runtime
testing.

On Windows, HTML panels are hosted in an InPrivate WebView2 and notifications
are delivered as toasts; both still need runtime checks on more machines. On
Linux, HTML panels use a text fallback and native notification delivery is
unavailable. Media hosting still needs Linux and Windows support, and Windows
close-to-tray behavior also needs implementation/verification.
Desktop webview preferences (theme, projects, model defaults, and related
settings) are read from the installed app on Linux as well as macOS. The Windows
WebView2 reader has synthetic fixture coverage on Linux; real Windows profile
compatibility remains unverified. The reader opens only a temporary copy;
copying an actively written profile is best-effort.

The preview workflow checks core on macOS, Linux and Windows, but checks/builds
the GUI only on macOS.
