# Una sola interfaz: código y agentes personales juntos, al estilo de Capy

Plan del 2026-11-17. El usuario quiere que monoBOT tenga una única interfaz,
tomada de Capy 0.9. Esa interfaz reemplaza los layouts Classic y Focused.
Las sesiones de código y los monos (agentes personales) viven en la misma
barra lateral, y cualquier proveedor puede lanzar monos como subagentes,
también en un swarm.

La referencia es la app instalada (`/Applications/Capy.app`, 0.9.2, Electron).
Se analizó en estático y en solo lectura: REA sobre `app.asar`, y el renderer
desempaquetado, que trae los atributos `data-track-owner` con el nombre de
cada componente. También hay capturas de la app abierta. No se copió código
ni recursos: lo que sigue son patrones adaptados al lenguaje visual de
monoBOT, como pide `CLAUDE.md`.

## Lo que ya está hecho

| Pieza | Dónde |
| --- | --- |
| Una sola barra lateral en el layout Focused. Desaparece el cambio Code/Agents: arriba van los **Captains** (los agentes personales: el Principal, los bots y los grupos), y debajo los **Monos**, los Workstreams y los hilos. La cabecera lleva el proyecto, «Nuevo hilo» y buscar. Una barra al pie pone Ajustes a la izquierda e Inbox, Notes y Automations como iconos. El carrusel de proyectos se elimina: el proyecto se cambia en la cabecera | `shell.rs` (`sidebar`), `shell/sidebar_list.rs` (`sidebar_bottom_bar`), `monos/mod.rs` (`unified_rail`), `monos/agents.rs` (`Section::Captains`) |
| Esquinas con cuatro valores (Square, Subtle, Rounded, Soft) en Apariencia, y otros cuatro propios del notch en Look. Una caja con estilo y borde en los cuatro lados que no pide radio recibe 6 px multiplicados por la escala, para que nada quede como un rectángulo pelado. Lo pintado a mano, como los trozos de un mensaje largo, conserva sus radios | `vendor/gpui-pre/src/window.rs` (`scaled_corners`, `Window::set_corner_scale`), `theme.rs` (`CORNERS`), `notch/settings.rs`, `monocode-core/src/notch.rs` (`Look::corners`) |
| Selector de modelos como el de Capy: una lista compacta con los favoritos y su esfuerzo y número (⌥1–⌥9) y después los modelos agrupados por quién los hace (`catalog::model_maker`), o por agente con «By provider». Al pasar el puntero por un modelo se abre a su lado una tarjeta con Reasoning en un deslizador con tramado, Fast, Thinking, Tier, los controles del proveedor, Runs on y Favorite, y un tramado en la esquina. La tarjeta tarda 180 ms en irse, para cruzar el hueco | `pickers/sections.rs`, `conversation.rs` (`ComposerMenu::Model`, `apply_model_option`, `pick_favorite`) |
| Cualquier proveedor puede pasar trabajo a Chief y a los monos. La oferta es un prompt más el CLI `control chief`, y antes solo se activaba en Claude | `conversation.rs` (`chief_offer`), `settings/pages/chat.rs` |
| Hilos en dos líneas, como los de Capy: título y edad arriba; espacio de trabajo, modelo y proveedor debajo. Es la vista por defecto. Los workspaces guardados antes pasan a ella una sola vez (`two_line_cards`), y «Compact session cards» vuelve a la de una línea y se mantiene | `shell.rs` (`session_row_with`), `workspace.rs` |
| El dictado dibuja su onda como una trama de puntos de 2 px en el acento, con el mismo dither de Bayer 4×4 que el fondo del chat. Lleva el tiempo y un botón ✓ que termina y conserva el texto | `monocode-core/src/dictation.rs` (`dither_cells`), `dictation.rs` (`dither_wave`) |
| Los avisos (actualizaciones de los harness, errores) aparecen debajo de la barra de título y ya no la tapan | `shell/notifications.rs` |
| Captains más grandes, como en Capy: cara de 40 px, el nombre y debajo lo último que dijo (o su especialidad) | `monos/mod.rs` (`captain_subtitle`, filas de `Section::Captains`) |
| «New Captain»: una ventana con la cara que tendrá, su nombre y el rol con que empieza (o en blanco), y Create Captain. Reemplaza el menú de roles del `+` | `monos/new_captain.rs`, `monocode-core/src/mono.rs` (`create_captain`) |
| La página de un Captain antes de su primer mensaje: cara de 112 px, su nombre en grande y «View Captain», que abre sus ajustes | `conversation/mono.rs`, evento `ViewCaptain` |
| Workstreams es una página propia, abierta desde un botón de la barra inferior, y deja la barra lateral | `monos/workstreams.rs` (`WorkstreamsPage`), `AppView::Workstreams` |
| «Hand to your crew» en el menú de un hilo: el equivalente al «Promote to Captain» de Capy. Chief toma la última petición del hilo y la reparte entre tus monos. La pestaña del panel derecho se llama **Crew** cuando muestra un run de Chief, y **Swarm** cuando es un swarm | `shell/orchestration.rs` (`hand_to_crew`), `swarm.rs` (`is_crew`), `right_panel.rs` |
| Los controles del compositor (+, modelo, acceso) ya no llevan marco: muestran un fondo al pasar el puntero, como en Capy | `conversation.rs` (`tool_pill`) |
| Un hilo que trabaja muestra en su fila un cronómetro en vivo (`0:42`), como T3 Code, en lugar de «Working…». La barra lateral se redibuja una vez por segundo solo mientras algún hilo trabaja | `shell.rs` (`track_working`, `working_clock`) |
| Las esquinas del notch cambian también su forma al abrirse: el radio del contorno va de 4 px (Square) a unos 23 px (Soft), y sus botones segmentados se redondean igual | `notch/mod.rs` (`corner_radius`), `notch/settings.rs` |
| El marco Environment se pinta siempre por encima del chat. Antes, con el panel derecho abierto, quedaba debajo del transcript | `conversation/environment.rs` |
| Menús contextuales nativos de macOS, SF Pro, esquinas redondeadas, acento del sistema y colores de estado | `native_menu.rs`, `theme.rs`; ver `ui-ux-audit.md` |
| Revisión del sistema de agentes (18 nov). Codex llega al puente de la app cuando se le ofrece Chief o swarm, o tiene la capacidad de la app; antes su sandbox lo bloqueaba (`Settings::reaches_app`). Una petición nueva ya no se da por empezada si el chat tiene un run de Chief anterior: se le dice que espere o lo pare. «Hand to your crew» abre el hilo si no estaba cargado y muestra los errores que llegan tarde. Borrar el chat de Chief termina su run. Chief ya no se ofrece a sí mismo en su primer turno | `monocode-core/src/agent.rs`, `shell/orchestration.rs`, `shell/session_groups.rs` |

## Cómo se corresponde Capy con monoBOT

| Capy | monoBOT hoy | Hacia dónde va |
| --- | --- | --- |
| **Captain**: un hilo promovido que lleva una *crew* de hilos (`CaptainDetail.crew: ThreadId[]`, «Crew runs on») | Los agentes personales de la zona Agents: el Principal, los bots y los grupos | **Los Captains son los agentes personales.** Los monos de código (y los bots `coder` y `chief`) son los «capys de hilo»: la crew que cualquier proveedor lanza como subagentes, a través de Chief o del swarm |
| **Thread** con `status: working, waiting, idle, failed` y `ReadSignals` (`needsYou`, `ask.kind`, `crewNeedsYou`) | Sesión de código con `session_mark::State` (Idle, Working, Done, Attention) | Se añaden Failed y la regla «hasta que lo veas». El motivo de la espera también: aprobación, pregunta o credenciales |
| Barra lateral **Hybrid**: Captains arriba (los fijados como caras de 48 px en celdas de 64 px), Threads debajo con Working y Settled (colapsable, con contador) | Zona Code con secciones Working y Done, y la zona Agents aparte (`agents_zone`) | Una sola barra: Monos arriba, sesiones debajo. Se quita el cambio de zona |
| **Navegación inferior** editable: New thread, Threads, Issues, Pull requests, Automations, Explore, Drive, Memory | Filas fijas arriba: New, Inbox, Notes, Automations, y la sección Workstreams | Una barra de iconos abajo, editable («Sidebar pages»): Inbox, Notes, Automations, Workstreams, Swarm, Memory, Pull requests |
| **Glifo de estado**: una rejilla de 3×3 puntos en 14 px. Working gira (800 ms), waiting late en la fila central, needs-you y failed laten por niveles. Con movimiento reducido queda estático | `session_mark` dibuja puntos | Rejilla de 3×3 con esos estados; animación por pasos y estática con movimiento reducido |
| **Compositor**: chips de alcance encima (Project, Machine o Run on: checkout actual, worktree nuevo o existente, nube), luego +, ajustes, chip de modelo, micro y enviar con el acento | Compositor con +, Codex y Supervised en cajas con borde. Workspace va debajo | Chips de alcance arriba (proyecto, workspace, host remoto); controles sin borde, con fondo al pasar el puntero; enviar con el acento |
| **Peticiones como «takeover» del compositor**: una por tipo de petición, con Dismiss y Later. Las aclaraciones van con Back, Next y Submit | Tarjetas de aprobación en el transcript | Las aprobaciones y las preguntas ocupan el sitio del compositor y dicen qué esperan |
| **Teclas de envío** a mitad de turno: steer, interrupt, queue y newline, configurables para ↩, ⌘↩ y ⌥↩. Cola con «Send now» y «Edit in composer» | `follow_up` = steer o queue | Las tres teclas configurables y la cola editable |
| **Grupos de trabajo**: etiquetas en presente y en pasado («Reading X» → «Read X»); la fila más nueva con shimmer; las lecturas repetidas se juntan con un contador; un grupo anidado sin cabecera | Fold «Worked for» y calm work | Etiquetas en presente y en pasado, fila viva con shimmer (estática con movimiento reducido) y lecturas agrupadas |
| **Panel derecho con pestañas**: Chat, Diffs, Terminal, Browser, Desktop, Phones, Machine files, Drive, Subagents/Crew, Pull requests y Side threads, con división a la derecha o abajo | Panel derecho con Changes, Swarm, Agents y Environment. La pestaña Agents ya lista los subagentes de un chat de Claude con su estado, su tiempo y su último paso, leyendo los archivos que escribe Claude Code | Se añade la pestaña **Crew**: los monos que trabajan para esta sesión y su estado. También faltan los subagentes de los otros proveedores, que hoy no los exponen |
| **Pantalla vacía**: fondo con imagen y efecto (Dither, Grain, Vignette) y el compositor en el centro | `chat_background` con Dither, ASCII, Halftone… | Ya existe; queda centrar el compositor y los chips |

## Tokens de Capy, como referencia para afinar los de `theme.rs`

- **Tipo:** Inter con pesos 450, 500, 560 y 700; nosotros usamos SF Pro del sistema. Escala: 11, 12, 13, 14 (base), 18, 22, 26. Tracking de −0,01 em en 13 y de −0,02 em de 14 en adelante.
- **Controles:** 24, 28, 32 y 44 de alto; filas de menú de 28; filas de paleta de 40.
- **Radios:** 2, 3, 6 (botón), 8 (tarjeta y overlay) y 12 (panel).
- **Barra lateral:** 240 por defecto, ajustable entre 224 y 330.
- **Movimiento:** 150, 200, 300 y 500 ms; curvas out-quint `(.23,1,.32,1)` y ease-out.
- **Oscuro:** fondo L 14 %, panel 18 %, tarjeta 22 %, popover 25 %. Bordes de blanco al 8, 12 y 16 %. El texto apagado mezcla el texto con el fondo al 68, 52 y 38 %.

## Fases

Cada fase deja la app funcionando y con tests.

1. **Shell único**: una barra lateral Hybrid (monos arriba, sesiones debajo
   en Needs you, Working y Settled), el glifo de rejilla y la barra de
   navegación inferior editable. Classic y Focused se retiran. Una migración
   lleva sus preferencias al shell nuevo, y la zona Agents se integra en la
   barra. Afecta a `shell/classic_sidebar.rs`, `shell/sidebar_list.rs`,
   `shell/session_groups.rs`, `shell/rails.rs`, `shell/layout.rs` y
   `session_mark.rs`.
2. **Compositor**: chips de alcance arriba, controles sin borde, enviar con
   el acento, teclas de envío configurables y la cola editable. Las
   peticiones (aprobación, pregunta, credenciales) pasan a ser takeovers del
   compositor.
3. **Monos como subagentes de cualquier proveedor**: la oferta de Chief ya
   está abierta a todos. Quedan cinco cosas:
   - que los workers de un swarm salgan de los monos de código;
   - promover una sesión a mono coordinador;
   - la pestaña Crew en el panel derecho;
   - los «needs you» de la crew en la cara del mono;
   - el `@mono` en el compositor de cualquier sesión.
4. **Transcript**: etiquetas en presente y en pasado, fila viva con
   shimmer, lecturas agrupadas y diffs largos que se abren en su sitio.
5. **Tokens**: escala de tipo, superficies y bordes como arriba, y
   comprobación de contraste en claro.

## Segunda ronda de la revisión del sistema de agentes (18 nov)

Hecho:

- OpenCode 2: un chat que llama a la app (Chief, swarm, operador) ya no usa el servicio compartido. Arranca su propio `opencode serve`, con sus credenciales, y lee la contraseña que imprime (`provider/opencode.rs`, `startup_line`).
- El resumen guardado de un run lleva quién lo pidió y el mono de cada tarea. Así el tablero sigue llamándose Crew, con sus caras, cuando el run termina, y el chat que lo pidió lo sigue viendo.
- El tope de monos nuevos por run se guarda con el run (`Run::new_monos`).
- El `SOUL.md` de un bot nuevo se escribe al guardar el roster, fuera del hilo de la interfaz. Mientras no está en disco, un trabajador recibe las instrucciones de su rol.
- Chief y los monos de código del crew salen en la sección Monos del rail.
- «View Captain» recarga los archivos y da el foco a los ajustes.
- Los agentes personales se llaman Captains en toda la interfaz: «New Captain», «Delete Captain…», «Captain name» y la ficha al pasar el puntero. Los monos de código siguen siendo monos.
