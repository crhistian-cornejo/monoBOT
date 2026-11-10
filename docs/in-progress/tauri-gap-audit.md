# Auditoría: lo que Tauri tiene y la nativa todavía no

Fecha: 8 de noviembre de 2026. Referencia: la app Tauri en el commit `5880742c`
(`src/`, `src-tauri/src`), comparada con `crates/monocode-gpui/src` y
`crates/monocode-core/src` en `main` (`7aa61ccf`).

Método: se sacaron las etiquetas visibles en inglés, los `aria-label`, las claves
`monocode.*` y los comandos `invoke` de cada feature de Tauri, y se buscaron en
los crates nativos. Cada etiqueta que faltaba se revisó a mano en el código. Más
del 95 % de las etiquetas ya existen en la nativa, así que los huecos están
concentrados. No se usó la documentación como prueba ni de presencia ni de
ausencia. Tampoco se ejecutó ninguna de las dos apps: un «falta» quiere decir
que no se encontró en el código.

Este documento no repite los huecos ya conocidos: enviar más tarde, chips de
cita y citas, pestañas News/Bots/Sub-agents, lista y tarjetas de noticias, el
grupo para exportar, importar y sincronizar bots, girar y voltear formas, el
título del eje Y, la paginación de Word, los archivos remotos, producción
(instalador, firma, actualizador) y `host/`.

Tamaño: **S** cabe en una tarde, **M** lleva de uno a tres días, **L** es más
de tres días o toca el diseño.

Rutas: `T:` es Tauri (`src/…` o `src-tauri/src/…`), `G:` es
`crates/monocode-gpui/src` y `C:` es `crates/monocode-core/src`.

## 1. Orquestación y workstreams

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 1.1 | ~~**El orquestador no dirige la ejecución.**~~ **Hecho.** En Tauri, el líder usa `monobot control` con doce acciones: list, delegate, get, wait, steer, message, retry, cancel, respond, answer, review y finish. Las aprobaciones y preguntas de los trabajadores le llegan a él como `needsInput`. | `T: src-tauri/src/control.rs`, `control_cli.rs`, `features/orchestration/model/orchestration.ts` | `C: orchestration/control.rs` tiene la ejecución, las doce acciones con sus validaciones, los recibos por request ID y los turnos de continuación del líder. El líder las llama con `monocode-gpui control ACTION`, por el mismo canal autenticado que `app` (`C: mono/bridge.rs`); `G: shell/orchestration.rs` las ejecuta y `wait` espera a que cambie la ejecución. Las aprobaciones y preguntas de cada trabajador se leen de su chat (`G: conversation/orchestration.rs`) y el líder las resuelve con respond y answer. | L |
| 1.2 | ~~**Los trabajadores no se aíslan.**~~ **Hecho.** En Tauri cada trabajador va en su propio worktree, se sigue en una pestaña de solo lectura y sus cambios se aplican a la copia del líder. | `T: AgentTabView.tsx`, `checkpoint.rs` (`session_checkpoint_apply`, `cleanup_safe`) | `C: orchestration/isolation.rs`: cada trabajador recibe un worktree separado que parte de una instantánea de la copia del líder, con los archivos sin commit y los no rastreados. Al aceptarlo con review, solo su diferencia se aplica a la copia del líder; se rechaza si escribió fuera de su alcance o si ya no aplica, y en ese caso se conserva el worktree. Un proyecto sin Git comparte la copia, como antes. El chat del trabajador muestra «managed by the orchestrator» y su compositor es de solo lectura. | L |
| 1.3 | ~~**La tarjeta del plan no se puede editar.**~~ **Hecho.** En Tauri se elige modelo, harness y esfuerzo por tarea, se editan Task e Instructions, se fija «Parallel workers» y hay «Show fewer tasks». | `T: OrchestrationPreview.tsx` | `G: conversation/orchestration.rs` (`PlanCard`): cada tarea abre Task e Instructions, elige harness y modelo con el `ModelList` del compositor y su esfuerzo entre los niveles de ese modelo. «Parallel workers» va de 1 a 4, y los planes largos se pliegan con «Show N more tasks» / «Show fewer tasks». | M |
| 1.4 | ~~**Una ejecución interrumpida no se puede reanudar.**~~ **Hecho.** Tauri guarda el estado (interrupted, start_unknown, etapas) y muestra el banner «Continue interrupted and queued work». | `T: orchestrationState.ts`, `OrchestrationSidebarAgents.tsx` | La ejecución se guarda en orden tras cada cambio (`orchestration-run:<lead>`, `C: store.rs`). Si la app se cerró con trabajo en marcha, o si el turno del líder se interrumpe, vuelve en pausa: la tarjeta muestra «Continue interrupted and queued work» y Resume continúa a cada trabajador en su sesión y su worktree con un turno de recuperación. Las etapas intermedias de Tauri (`start_unknown`, `stop_unknown`) no hacen falta: un worktree creado para un intento que falla se borra. | M |
| 1.5 | ~~**No se pueden abrir los trabajadores.**~~ **Hecho.** Faltan «Open this agent beside the orchestrator» y abrir todos en pestañas. | `T: orchestration/ui/OrchestrationActions.ts` | Cada agente del rail tiene «Open beside» (`G: shell/rails.rs`) y la tarjeta del líder tiene «Open agents», que los abre a todos junto a él. | S |
| 1.6 | ~~**Workstreams no tiene interfaz.**~~ Hecho: el recorrido (`workstream_intro_seen` en el roster; el «?» junto a la cabecera Workstreams lo reabre cuando se quiera, terminando en Done y sin tocar esa marca), «New workstream» (nombre, objetivo, repositorio y dónde corre el orquestador), la sección Workstreams del rail con Beta y las pestañas Agents (Waiting on you/Working/Idle), PRs (Open/Draft/Merged/Closed) y Files. El Mono guarda su `workstream`, el orquestador recibe `brief` y su chat empieza en su checkout. | `T: features/workstreams/ui/*`, `app/shell/MonoRailSection.tsx` | `G: monos/workstreams.rs`, `monos/panels.rs`; `C: mono/workstreams.rs`, `mono.rs` | L |

## 2. Chat y transcript

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 2.1 | ✅ **Hecho.** Barra «Find in conversation»: Cmd/Ctrl+F la abre, Enter, ↓, F3 y Cmd/Ctrl+G avanzan (Shift+Enter, ↑ y las variantes con Shift retroceden), Esc la cierra, con contador. Funciona en las sesiones de proyecto y en los chats de agentes, grupos y paneles flotantes; cambiar de agente o grupo la cierra. Busca en mensajes, tools, planes e imágenes generadas. En un Mono paginado solo cubre las páginas ya cargadas. | `T: sessions/ui/TranscriptFind.tsx`, `model/transcriptFind.ts` | `C: transcript/find.rs`, `G: conversation/find.rs` (reutiliza `reveal_search` para saltar y marcar). | M |
| 2.2 | ✅ **Hecho** (la tarjeta). Fila `plan` en el transcript: la escriben `ExitPlanMode` de Claude y el ítem `plan` de Codex, y si no, la última respuesta de un turno de plan que parezca un plan. La tarjeta tiene Open (lector sobre el chat), Build (envía «Build approved plan» con el plan como fuente) y el menú «Build with another model», y pasa a Building…/Built. **Sigue faltando** editar el plan (Preview/Source) antes de construirlo. | `T: PlanPreview.tsx`, `model/plan.ts`, `SecondOpinionButton.tsx`, `files/ui/FilePane.tsx` (PlanFilePane) | `C: plan.rs`, `G: transcript/plans.rs`, `G: conversation/remote_plans.rs` (lector y Build compartidos con los planes remotos), `TransferKind::Build` en el menú de proveedores. | M |
| 2.3 | ✅ **Hecho.** Un párrafo que es solo `![alt](ruta-de-la-imagen)`, o solo un enlace a una imagen o un vídeo local, muestra la imagen (se abre a pantalla completa) o una tarjeta que abre el vídeo en el reproductor de monoBOT. Las rutas relativas se resuelven contra la carpeta de la sesión. Como en Tauri, las imágenes web no se descargan: se ve su texto alternativo. | `T: DocumentImage.tsx`, `LocalMedia.tsx`, `documentImageSources.ts` | `C: transcript/media.rs`, `G: markdown_model.rs` (`Block::Image`), `G: transcript/reply_media.rs`. | M |
| 2.4 | ✅ **Hecho.** El `imageGeneration` de Codex se guarda en `generated-images` y Antigravity, cuando el turno usa `generate_image`, espera hasta dos minutos a que Gemini escriba las imágenes. Ambas quedan como fila `image` en el chat, con su prompt debajo. | `T: GeneratedImage.tsx`, `harness/core/apply.ts` (`image.generated`), `codexProtocol.ts`, `antigravityImages.ts` | `C: images.rs` (`save_base64`, `REPLY_ROLE`), `C: provider.rs` (`Event::Image`), `G: runtime.rs`, `G: transcript/reply_media.rs`. | M |
| 2.5 | ✅ **Hecho.** Las filas Edit y Write tienen un desplegable con el diff de esa edición, sin git: `Edit`/`MultiEdit`/`Write` de Claude, `fileChange` de Codex y el contenido `diff` de ACP. En Tauri era un popover al pasar el ratón; aquí se abre bajo la fila. | `T: ToolDiffPreview.tsx` | `C: tool/edit_preview.rs`, `G: transcript/tools.rs`. | M |
| 2.6 | **El medidor de contexto no tiene «Compact now».** En Tauri es un popover con esa acción. | `T: ContextMeter.tsx` | `G: conversation.rs:~3267` solo muestra un tooltip. `/compact` sí funciona. | S |
| 2.7 | **Borrar una sesión no ofrece «Also delete the unused worktree».** | `T: DeleteSessionDialog.tsx` | `G: shell/project_removal.rs` (`confirm_delete_session`) es un aviso simple y nunca borra el worktree. | S |
| 2.8 | **Falta «Commit these changes»** en la tarjeta de revisión. | `T: SessionReview.tsx` (`onCommit`) | `G: session_review.rs` solo tiene Review, Undo y Keep. | S |
| 2.9 | **La vista previa de la sesión está incompleta.** Faltan la fila Changes (+/−), la de Issue/PR vinculado y el estado «Removed» del worktree. | `T: SessionCardPreview.tsx` | `G: conversation.rs:~470` solo muestra Branch y Worktree. | S |
| 2.10 | **Al picker `/mcp` le faltan piezas:** búsqueda, «Manage MCP Servers…», los estados de carga, vacío y error, y el estado en vivo de `claude mcp list`. | `T: McpServerPicker.tsx`, `mcpPicker.ts` | `G: composer_completion.rs` lista los servidores, sin nada de lo anterior. | S |
| 2.11 | **No se muestra el bloque desplegable «CI context»** de los prompts de reparación de CI. | `T: AgentTranscript.tsx:2793` | `C: store/shared/edit.rs` conoce la clave, pero nada la pinta. | S |
| 2.12 | **No se puede crear una skill desde el picker del compositor** («New skill»). | `T: SkillPicker.tsx`, `Composer.tsx` | Solo se crean desde `G: settings/pages/skills.rs`. | S |
| 2.13 | **Falta la tarjeta para instalar las skills de documentos de Anthropic** (docx/xlsx/pptx/pdf) desde el marketplace de Claude. | `T: DocumentSkillsCard.tsx`, `harness.rs` (`install_document_skills`) | Falta. | S |

## 3. Proveedores

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 3.1 | **Los subagentes no se anidan en ACP, Devin, Pi/omp ni Cursor.** Tauri lee `parentToolCallId` (también dentro de `_meta`), `cognition.ai/subagent_*` y TaskToolDetails. | `T: harness/core/acpSubagents.ts`, `devinSubagents.ts`, `piSubagents.ts`, `cursorSubagents.ts` | Solo se anidan los de Claude (`parent_tool_use_id`) y Codex (`C: provider/tasks.rs`). `parentToolCallId` no aparece en `C:`. | M–L |
| 3.2 | **No se atienden las extensiones de Cursor y Grok:** `cursor/create_plan`, `cursor/update_todos`, `cursor/task` y `x.ai/exit_plan_mode`. Sus planes y listas de tareas se pierden. | `T: cursor.ts:497-535`, `grok.ts:537` | `C: provider/acp.rs:~816` responde «Unsupported client method». | M |
| 3.3 | **No se recupera nada del almacén local de Cursor** (progreso de subagentes y detalle de tools). | `T: src-tauri/src/cursor_store.rs`, `cursorStore.ts` | Falta. | M |
| 3.4 | **fx no muestra el detalle de sus tools.** Tauri traduce sus títulos en gerundio y saca ruta, query y comando del resultado. | `T: fx/fxTool.ts` | No se encontró equivalente. Probablemente falta, pero no se probó con fx. | S |

## 4. Inbox, correo y control de código

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 4.1 | **Los checks de los PR son muy básicos.** Tauri muestra los pasos del job, anotaciones con el código del commit, el filtro «Needs attention / All checks», «Refresh checks», sondeo mientras están pendientes y el aviso de caché. | `T: InboxPrChecks.tsx`, `CheckEvidence.tsx`, `githubPrChecks.ts`, `git_github_check_details` | `G: inbox/ui.rs:2115` (`checks_view`) es una lista plana. No se piden jobs ni anotaciones en `C: inbox/github.rs`. | L |
| 4.2 | **No existe el flujo para reparar CI.** Tauri deja elegir los checks, enviarlos a un chat nuevo o a uno existente y seguir el progreso (Awaiting CI, Still failing, Out of date…), guardado en `monocode.ciRepairs.v1`. | `T: CheckRepairForm.tsx`, `CheckRepairProgress.tsx`, `ciRepair*.ts` | Un único botón «Fix with agent» con un prompt fijo (`G: inbox/ui.rs:2124`). | L |
| 4.3 | **Falta el panel lateral «Ask» de cada elemento,** con su sesión `inbox_ask` y «Restart conversation». | `T: InboxDiscussionPanel.tsx`, `inboxAsk.ts`, `instructions/inbox.md` | El core solo filtra esas sesiones (`C: store/shared.rs:529`). | M–L |
| 4.4 | **Empezar una sesión desde un elemento funciona distinto.** En Tauri el elemento queda como mini-tarjeta en el compositor y el usuario escribe su instrucción. | `T: InboxMiniCard.tsx`, `App.tsx` (`onStartInboxItem`) | `G: shell/app_views.rs:457` envía un prompt fijo sin pasar por el compositor. | M |
| 4.5 | **Cambiar de rama con cambios sin commit falla con el error de git.** Tauri ofrece «Commit & switch» (con mensaje de IA) y «Stash & switch». | `T: SwitchBranchDialog.tsx`, `BranchPicker.tsx` | `G: git_picker.rs:259` muestra el error tal cual. No hay stash en `C: git.rs`/`scm.rs`. | M |
| 4.6 | **El code del PR no tiene el interruptor «Hunks / Full file»** y el resumen no lista los «Changed files» con salto al diff. | `T: InboxView.tsx`, `InboxPrDiff.tsx`, `InboxPrOverview.tsx` | Faltan en `G: inbox/ui.rs` (`diff_view`, `summary_view`). | S |
| 4.7 | **El inbox no tiene menú de notificaciones** (silenciar todos con duración, reanudar, ajustes). | `T: InboxNotificationMenu.tsx` | La lógica existe en `G: settings/notification_preferences.rs`, pero no hay menú. | S |
| 4.8 | **Lo que hace el propio usuario puede marcar elementos como no leídos** (sus comentarios y los commits de sus Monos). | `T: inboxSelfActivity.ts` | `C: inbox.rs:396` solo marca como visto lo seleccionado. | S |
| 4.9 | **Los vídeos dentro de issues y PR no se reproducen** (mp4/webm/mov). | `T: InboxMedia.tsx`, `inboxMedia.ts` | `G: inbox/document.rs:129` solo decodifica imágenes. Probable, sin comprobar en la app. | S–M |
| 4.10 | **Cuando un agente pide conectar una cuenta de correo, no pasa nada.** Tauri abre el diálogo ya rellenado («{bot} asked to connect this account») y permite dejar la cuenta solo para ese agente o abrirla a todos. | `T: MailConnectDialog.tsx`, `mailAccess.ts` | `C: inbox/mail/agent.rs:335` devuelve `"opened": false`. Nada escribe `monocode.mail.private`. | M |
| 4.11 | **No se puede elegir ni quitar la foto de una cuenta de correo.** | `T: MailAccounts.tsx` (AccountPhoto) | `G: inbox/mail.rs:362` lee `monocode.mail.photo.*`, pero nada la escribe. | S |
| 4.12 | **El panel de cambios no tiene grafo de historial** plegable y redimensionable. | `T: GitChangesPanel.tsx`, `GitHistoryGraph.tsx` | Solo existe la pestaña History (`G: history.rs`). | S–M |
| 4.13 | **A la página de worktrees le faltan detalles:** «Refresh worktrees» y el tooltip que explica por qué Delete está bloqueado. | `T: WorktreesPage.tsx` | `G: settings/pages/worktrees.rs` | S |

## 5. Editor y archivos

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 5.1 | **El editor no cierra paréntesis ni comillas** ni resalta los paréntesis y etiquetas emparejados. | `T: files/editor/editorEditing.ts` | `G: input/code.rs` no tiene nada de esto. | M |
| 5.2 | **Find no busca en la vista previa de Markdown.** | `T: FilePreviewSearch.tsx` | Find cambia a Source (lo fija un test en `G: file_editor.rs:1583`). | M |
| 5.3 | **No se puede comentar una línea cambiada desde el gutter de git.** | `T: editorGit.ts` (`onComment`), `DiffCommentComposer.tsx` | `G: input.rs:82` (`CodeAction`) solo tiene Stage y Revert. Los comentarios solo existen en `diff_view.rs`. | S–M |
| 5.4 | **El frontmatter no se muestra como bloque plegable «Properties»** en la vista previa. | `T: FileEditor.tsx:477` | Solo se separa en `G: settings/skill_document.rs`. | S |
| 5.5 | **Las pestañas de archivo no tienen menú contextual:** Close Others, Open in Default App, Reveal, Copy Path, Copy Relative Path y Copy File Name. | `T: workspace/ui/SurfaceTabs.tsx:86` | No hay destino de pestaña en `MenuTarget` (`G: shell.rs:358`). | S |
| 5.6 | **El explorador no tiene «Open in Terminal».** | `T: files/ui/FileTree.tsx:242` | Falta en `G: explorer.rs`. | S |

## 6. Monos, grupos, papers y documentos

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 6.1 | **La pestaña Activity del grupo está a medias.** En Tauri tiene una fila por miembro con actividad, cronómetro y subagentes, permite abrir al miembro, incluye la fila del Principal y la lista «Between members». | `T: groups/ui/GroupActivity.tsx` | `G: monos/panels.rs:1788` solo muestra los miembros que trabajan. `C: mono/groups/activity.rs` (`group_activity`, `group_exchanges`) no se usa. | M |
| 6.2 | **No se puede añadir un PDF a un paper que solo tiene resumen** («Add PDF», «Choose the paper's PDF»). También faltan el texto de editor bloqueado y la atribución «From {source}». | `T: papers/ui/PaperReader.tsx` | `G: monos/papers.rs:176` (`paper_record_page`). `PaperStore::import` solo lo llaman los agentes. | M |
| 6.3 | **Falta la cabecera del lector de papers:** autores, revista, citas, «Saved in {folder}», disponibilidad y «Open larger». También falta la lista «N passages cited in answers», con aviso cuando un pasaje no está en el texto. | `T: PaperReader.tsx`, `PaperPage.tsx` | `G: monos/papers.rs` (`paper_page`) solo tiene un título sobre el visor PDF. | S–M |
| 6.4 | **Los documentos no se pueden marcar con estrella** y sus temas no reciben nombre de un modelo pequeño (`monocode.documentStars`, `monocode.documentTopics`). | `T: artifacts/ui/DocumentFolders.tsx`, `documentTopics.ts` | `C: mono/folders.rs` agrupa por petición, sin nombres ni estrellas. | S–M |
| 6.5 | **Editar Alma o Memoria a la vez que otro no tiene salida.** Tauri ofrece «Use theirs» y «Keep mine». | `T: monos/ui/monoPanelParts.tsx:395` | `G: monos/files.rs:182` solo dice «Could not save: conflict». | S |
| 6.6 | **El panel del resumen no sigue al turno.** Tauri pasa a Sub-agents cuando empiezan subagentes y a Documents con el informe al terminar. Depende de la pestaña Sub-agents. | `T: monos/model/panelFollow.ts` | Falta: `overview_tab` solo cambia con clic. | S |
| 6.7 | **El chat flotante no abre documentos dentro de su panel** (copiar, «Open in monoBOT», cerrar con Esc). | `T: FloatingMonoChat.tsx:589-640` | `G: monos/floating.rs` no maneja documentos. No se rastreó del todo a dónde va el clic. | S–M |
| 6.8 | **La lista de sesiones del Mono no se agrupa por estado** («Waiting on you», «Working», «Idle») y no distingue sus estados vacíos. | `T: MonoSessionList.tsx:417` | Los filtros sí existen en `G: monos/filters.rs`. | S |
| 6.9 | **El visual no tiene barra de herramientas** con «Show source / Show visual» y «Reset visual». | `T: visuals/ui/VisualView.tsx` | `G: visuals.rs` solo reinicia la cámara 3D. | S |

## 7. Ventanas, shell y ajustes

| # | Hueco | Tauri | Nativa | Tamaño |
| --- | --- | --- | --- | --- |
| 7.1 | **No se pueden abrir varias ventanas principales** («New Window», ⌘⇧N, menú del Dock). | `T: src-tauri/src/menu.rs`, `window.rs` (`open_new_window`), `macos.rs` (`install_dock_menu`) | `G: settings/pages/keybindings.rs:52` lista el comando, pero nada lo atiende. ⌘⇧N está asignado a `NewClaude`. | M–L |
| 7.2 | **Windows y Linux no tienen menú dentro de la ventana** (botón «Menu (Alt)» con File, View y Terminal). | `T: app/shell/TitleBar.tsx`, `MenuBar.tsx` | `G: app_menu.rs` solo llama a `cx.set_menus`, que es la barra de macOS. | M |
| 7.3 | **El toggle «Close to tray» no hace nada en Windows.** El icono de bandeja ya existe (logo, menú con Monos, quick composer, notch y Quit; ver [desktop.md](../done/desktop.md#menu-bar-tray-and-quick-composer)); falta que cerrar la ventana la oculte en vez de salir. | `T: src-tauri/src/tray.rs` | El toggle existe (`G: settings/pages/general.rs:196`), pero `G: shell.rs:~8882` solo implementa macOS. | M |
| 7.4 | **Salir no avisa si hay chats en marcha** («N chats are still running. Quit anyway?»). | `T: window.rs` (`quit_decision`), `app/model/appLifecycle.ts` | `G: shell.rs` (`request_quit`) solo pregunta por archivos sin guardar. | S |
| 7.5 | **Una carpeta de proyecto movida o renombrada no se vuelve a encontrar.** Tauri la busca y reconecta, o avisa con «Project folder not found…». | `T: projects/model/projectLocation.ts`, `fs.rs` (`resolve_project_location`) | Falta. | M |
| 7.6 | **Faltan en Apariencia «Transparent window» y «Blur radius».** | `T: SettingsView.tsx:2361,2491`, `window.rs`, `macos.rs` | Los campos `inset_glass` y `sidebar_blur` existen (`G: settings/preferences.rs:44,52`), pero no hay fila y nada los lee. | S |
| 7.7 | **Hay atajos que no se pueden reasignar.** Faltan Find in Files, Tab: Activate 1–8 y Last, Terminal: New Tab, Session: Stop, New Window, Toggle Mono y Toggle Workspace. Además, ⌘I, ⌘⇧G y ⌘⇧\` no tienen asignación. | `T: settings/model/settings.ts:1239-1317` | `G: settings/keymap.rs` (`command()`) | S |
| 7.8 | **Faltan sonidos:** `inboxUnseen`, `updateAvailable`, `switch` y `copy`, y Linux no suena. | `T: settings/model/sounds.ts` | `G: sound.rs` solo tiene Finished, Input y LinkedActivity. | S |
| 7.9 | **No se puede cambiar de proyecto deslizando dos dedos** sobre el sidebar en el diseño Enfocado. | `T: app/shell/useProjectSwipe.ts` | Falta. | S |
| 7.10 | **Faltan detalles menores del sidebar:** el punto de «PR/issue actualizado» en la fila de sesión, «Notification settings…» en el menú del proyecto, el selector de fecha anclado para silenciar (la nativa manda a Ajustes) y el enlace «Worktree settings» del picker de workspace. | `T: Sidebar.tsx:5127`, `useProjectMenu.tsx:174`, `NotificationMuteDatePicker.tsx`, `WorkspacePicker.tsx:372` | `G: shell.rs`, `shell/project_settings.rs:357`, `git_picker.rs` | S c/u |

## 8. Backend con poco impacto visible

- **No hay bloqueo del ciclo de vida de los worktrees** (`T: worktree_lifecycle.rs`): Tauri no deja arrancar un proceso en un worktree que se está borrando, ni borrar uno mientras arranca un proceso. **S**
- **No se libera el heap tras las ejecuciones** (`T: memory.rs`, `malloc_zone_pressure_relief`/`malloc_trim`). **S**
- **La QoS en segundo plano es parcial:** Tauri usa `pthread_set_qos_class` y `setiopolicy_np` (`T: background_work.rs`), y `C: process.rs:88` solo aplica `nice`. **S**
- **Los datos de citas de Tauri no se migran:** la nativa no lee `monocode.paperPassages` ni `monocode.paperSources`, así que se pierde el historial de pasajes citados. **S**
- Remoto: cuando la máquina no responde, Tauri enlaza a «Manage machines» y la nativa muestra solo texto (`G: conversation/remote.rs:232`). **S**

## Comprobado y presente

Se buscó y existe en la nativa:

- **Ajustes:** las doce secciones con todas sus filas, salvo las del punto 7.6.
- **Integraciones:** los cinco proveedores del Inbox, el correo (leer, responder, borradores, adjuntos y búsqueda) y las conexiones SSH.
- **Control de código:** cambios, diff unificado con comentarios, crear y borrar worktrees, y el grafo de historial como pestaña.
- **Editor y archivos:** el editor con lint, autocompletado y Emmet, el explorador completo y los visores de Office y PDF.
- **Monos y agentes:** Monos con hábitos, rotación de contexto, reacciones e investigación del turno; grupos con llamadas de voz; automatizaciones; notas; la app móvil y Poki.
- **Agentes por API:** las 54 acciones `app_access` de los agentes.
- **Composición:** cola de mensajes, `/btw`, handoff, second opinion, reintentos, límites de uso, recordatorios, quick composer, terminal con arcade y búsqueda global.

Dos cosas de Tauri no se cuentan como hueco porque eran código muerto: las
acciones de `TabGroupMenu` y `collectWindowTransfer`.

## Orden propuesto

1. ~~**Orquestación de verdad (1.1–1.5).**~~ Hecho: el canal `control`, la
   reanudación, la tarjeta editable, el aislamiento en worktrees y abrir los
   trabajadores.
2. ~~**Workstreams (1.6).**~~ Hecho.
3. ~~**Lo que se ve en cada chat (2.1–2.5)**~~: hecho, salvo editar el plan
   antes de construirlo (2.2).
4. **Subagentes y extensiones de proveedores (3.1–3.3).** Sin esto se pierden
   los planes y los todos de Cursor y Grok, y la jerarquía de subagentes.
5. **Reparar CI y checks (4.1–4.3).** Los tres van juntos y comparten modelo.
6. **Ventanas y plataforma (7.1–7.4):** varias ventanas, el menú y la bandeja en
   Windows, y el aviso al salir. Hay que hacerlo antes de la fase de producción
   en Windows.
7. **Git y editor (4.5, 5.1, 5.2):** stash al cambiar de rama, cerrar
   paréntesis y buscar en la vista previa.
8. **Correo y papers (4.10, 6.2, 6.3, 6.1).**
9. **El resto de tamaño S, por zonas:** 2.6–2.13, 4.6–4.13, 5.3–5.6, 6.4–6.9,
   7.5–7.10 y la sección 8.
