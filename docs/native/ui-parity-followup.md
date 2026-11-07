# Paridad útil de interfaz: seguimiento nativo

Referencia de comportamiento: la copia local de Tauri en
`.claude/worktrees/agent-a3f69eafb1397eb18/src/`.
La comparación usa su código real, no solo el inventario histórico de migración.

## Tarjetas de agentes y noticias

La regresión se reproduce con eventos de ratón en la ventana real del banco de
pruebas GPUI: abrir una tarjeta, entrar en ella, pasar por su borde de un píxel
y retirar el cursor. Antes del arreglo, la tarjeta permanecía visible.

La tarjeta ahora observa la superficie completa, no solo el contenido. La
salida también se detecta en la fase de captura, aunque la barra superior
intercepte el evento posterior. El cierre conserva los 120 ms necesarios para
cruzar el espacio entre la fila y la tarjeta; nuevos movimientos fuera no
reinician indefinidamente ese plazo. Cambiar de agente cancela el cierre viejo.

También se invalidan explícitamente los dos sidebars que pintan estas tarjetas:
cerrar el estado de `Monos` no bastaba para retirar su dibujo del sidebar
cacheado. Una señal específica de hover actualiza la vista principal y la
navegación original, incluida su versión compacta, sin invalidar el chat.

Al desplazar el sidebar, perder foco o cambiar los límites de la ventana se
descartan las tarjetas. El estado compartido también corrige la superficie del
resumen de noticias. No se añade otra animación a una interacción frecuente.

Referencia Tauri: `features/monos/ui/MonoCardPreview.tsx`, incluidos sus cierres
por scroll y blur. Código nativo: `monos/hover_cards.rs`, `popover.rs`,
`monos/mod.rs`, `shell/project_rail.rs` y los eventos existentes de `shell.rs`.

## Tutorial de iPhone/Poki

En Ajustes → MCP → iPhone (Poki) → tutorial, las siete instrucciones conservan
sus textos, comandos, navegación y estado real. Ahora incluyen las escenas
explicativas que faltaban en la versión nativa:

1. Requisitos.
2. Descarga y comprobación de la publicación.
3. Cable y confirmación de confianza.
4. Modo de desarrollador y UI Automation.
5. Inicio del servicio.
6. Selección de carpeta y estado de ejemplo.
7. Solicitud de permiso de un bot de teléfono.

Son ilustraciones, no acciones: no instalan nada, no conectan un teléfono ni
cambian ajustes de seguridad. La etiqueta «Example» y «Ready (example)» separan
los datos simulados de la línea de estado real del conector.

El movimiento se ejecuta una vez por escena y se limita a 30 fps. Con movimiento
reducido del sistema se muestra directamente el estado final. Los botones
siguientes/anterior, copia y navegación no reciben animaciones adicionales.

Referencia Tauri: `features/settings/ui/PokiTutorial.tsx` y
`features/settings/model/pokiTutorial.ts`. Código nativo:
`settings/pages/poki.rs` y `settings/pages/poki/scenes.rs`.

## Lo que no necesitaba otro port

La revisión de conversación/compositor encontró implementados el movimiento
del dock, los chips de citas, el envío posterior, los avisos de reintento,
edición del último turno, `/btw` y la respuesta visual al arrastrar adjuntos.
No se duplican esos componentes ni se considera prueba de ausencia una entrada
antigua del plan de migración.

## Comprobaciones y límites

```sh
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev mono_hover_card
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev poki::scenes
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev poki_setup_sits_under_its_switch_and_its_folder_is_stored
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev popover::tests
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev
node scripts/native-cargo.mjs check -p monocode-gpui --profile native-dev
```

Resultados focalizados: **20 pruebas aprobadas**:

- 4 de hover, incluida la matriz de sidebar principal/original/compacto y la
  comprobación de que no se reconstruye el chat.
- 7 de escenas, incluida su geometría a 320 y 640 px con movimiento normal y
  reducido.
- 1 del acceso al tutorial desde el conector en Ajustes.
- 8 regresiones del popover compartido.

La comprobación de compilación nativa (`cargo check`, sin configuración de
tests) pasa. La última suite global termina con **953 aprobadas, 7 fallidas y
17 ignoradas**. No se presenta como una suite completamente aprobada.
Los fallos adicionales que quedan sin resolver en esta entrega son:

| Prueba | Fallo observado |
| --- | --- |
| `conversation::provider_retry::tests::an_overloaded_provider_counts_down_and_the_user_can_stop_it` | No aparece el aviso de reintento. |
| `editor_analysis::parity_tests::large_tsx_analysis_highlights_the_end_of_the_current_and_original_file` | Bloqueo al esperar el análisis TSX. |
| `file_editor::spreadsheet_chart::tests::columns_are_drawn_per_point_with_axes_titles_and_legend` | Falta la etiqueta «Unidades». |
| `settings::pages::mobile::tests::pairing_dialog_matches_react_dimensions_and_scrolls_inside_small_windows` | Diferencia vertical de 0,25 px. |
| `shell::classic_surface_tests::original_collapsed_picker_selects_monos_and_projects_with_search_and_keyboard` | No queda seleccionado el agente esperado. |
| `shell::settings::tests::settings_replace_native_rail_and_tabs_and_return_to_the_same_session` | No coincide el padding esperado. |
| `shell::tests::sidebar_hover_updates_labels_without_rebuilding_the_chat` | Las coordenadas de prueba alcanzan «New», no «Inbox». |

Estos resultados describen la ejecución del árbol de trabajo, no una
comparación de base que permita atribuir el origen de esos siete fallos.
Se mantiene el aviso de compatibilidad futura de la dependencia `block 0.1.6`.

Los eventos de ratón, reloj, scroll y foco son sintéticos y pasan por GPUI, no
por una copia de la máquina de estados. Las escenas se montan con movimiento
normal y reducido. Se inspecciona el árbol dibujado y sus límites, sin capturas
rasterizadas ni revisión visual manual de esta entrega. Esto no acredita
pruebas manuales en Windows/Linux, lector de pantalla, todos los temas o un
iPhone real. No se ha activado el conector ni se ha cambiado ninguna cuenta
para probar las ilustraciones.

La app completa conserva otros límites recogidos en [parity.md](parity.md).
Esta entrega no declara paridad total con Tauri ni certifica a partir del código
el comportamiento de todas sus integraciones.
