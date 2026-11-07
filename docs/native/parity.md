# Native preview limitations

The native app is still experimental. A feature being present in source does not
establish runtime or platform parity with the production React/Tauri app. See
[verification](verification.md) for recorded checks.

## Providers

All eleven providers have discovery, model selection and native transports.
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
| Files and Git | Platform visual/IME checks, Linux/Windows media hosting, non-Git checkpoints, per-file checkpoint Undo/Keep and orchestration checkpoint integration |
| Papers and PDF | The PDF page rasterizer is Windows-only (`file_editor/pdf.rs` renders with `Windows.Data.Pdf`). Detection by `%PDF-` signature, per-page text extraction, the shared paper cache, the `papers.*` app actions and citation passage matching are platform-neutral. On macOS and Linux the viewer shows the extracted-text view and offers "Open externally"; there is no page image and `papers.figure` fails with a platform error. Citation focus selects the quoted text in the text view — the rendered page has no x/y highlight yet. A macOS renderer (PDFKit/Quartz) and a Linux one (pdfium/poppler) are still needed, plus real-PDF visual checks on Windows |
| Accounts and settings | Live OAuth/MCP authentication checks and remaining platform-specific settings behavior |
| Inbox | Runtime/visual checks, inline video, linked-update notices and richer CI evidence/repair progress |
| Notes | Runtime/visual checks and composer note-card presentation |
| Automations | Scheduler, provider and recovery checks; current implementation has source review only |
| Monos | Existing-data checks, live heartbeat and background delegation checks, native visual/document accessibility, floating-document interactions and durable orchestration recovery |
| Remote connections | Real-machine installation/update, existing-data restoration and cross-platform checks; the existing host's feature limits still apply |
| Integrations | Completion/approval notifications, sounds and Dock badges; Quick Composer attachments/shortcut checks, MCP/skills checks and remaining app-control endpoints. macOS reminder delivery and click routing passed on Oct 22 |
| Shared data | Concurrent Tauri/native use with active scheduled tasks or phone connections remains unverified and deferred |

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
