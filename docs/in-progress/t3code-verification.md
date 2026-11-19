# Verificación: eficiencia (T3 Code), delegación y Environment

Resultados iniciales del verificador antes de la revisión de integración.
Revisión del checkout del verificador con los cambios de t1 (eficiencia / T3 Code),
t2 (delegación a monos y worker temporal) y t3 (control Environment) ya aplicados
(commit base `46dfd1d3`, 16 archivos, +1842 −169). Fecha: 2026-11-19, macOS.
No se editó código: solo este documento.

**Resultado: todo pasa.** Ningún fallo que asignar a @efficiency, @delegation ni
@environment. Abajo, observaciones que no rompen nada pero conviene conocer.

## Comandos y resultados

| # | Comando | Resultado |
| --- | --- | --- |
| 1 | `cargo fmt --all -- --check` | Pasa (salida vacía, código 0) |
| 2 | `cargo xtask check` (fmt + clippy `-D warnings` + tests de xtask) | Pasa. Clippy sin avisos; xtask `48 passed; 0 failed`. Único aviso: `block v0.1.6` tendrá código rechazado en un Rust futuro (dependencia, previo) |
| 3 | `cargo xtask cargo test -p monocode-core` | Pasa: `1495 passed; 0 failed; 11 ignored` (+ doctest: 1 ignorado) |
| 4 | `cargo xtask cargo test -p monocode-gpui --profile native-dev <filtro>` | Ver tabla siguiente |
| 5 | `cargo xtask test` (core + GPUI completos) | Pasa: core `1495 passed; 11 ignored`; GPUI `1236 passed; 0 failed; 17 ignored` (317 s); otro binario `3 passed` |

Filtros de GPUI:

| Filtro | Resultado |
| --- | --- |
| `environment` | 7 passed |
| `orchestration` | 16 passed |
| `agents_pane` | 2 passed |
| `right_panel` | 3 passed |
| `transcript` | 167 passed, 1 ignored |
| `monos` | 127 passed |
| `simulator` (extra) | 15 passed |
| `context_meter` (extra) | 1 passed |

## Revisión por lectura

### Worker temporal fuera del roster y sin memoria — correcto

- `control worker` se convierte en un `delegate` sin campo `mono`
  (`crates/monocode-core/src/orchestration/control.rs:733`); el validador rechaza
  `mono` en la entrada (test `a_temporary_worker_is_a_delegate_on_the_chats_harness`).
- El run lo abre el propio chat, con `requester: None`. La vinculación de una tarea
  a un mono sale en `crates/monocode-gpui/src/shell/orchestration.rs:798`
  (`live.run.requester?;`): en un run sin requester (no es de Chief) nunca se
  asigna un mono, así que no hay identidad ni memoria de mono. El prompt del worker
  es la tarea tal cual; no lleva la instrucción «Save to your memory» que sí añade
  `hand_task` a los monos.
- El test `a_temporary_worker_belongs_to_its_chat_and_never_joins_the_roster`
  comprueba `t.mono == None`, `roster.monos` vacío, que la sesión del worker no
  entra en `hidden_sessions` y que `managed_by()` es el chat.
- Ni un worker, ni un mono, ni Chief pueden delegar a su vez
  (`delegation_refusal`, `shell/orchestration.rs:1534`), así que no hay bucles.

### El resultado llega una vez al chat originador — correcto

- Mono: `mono_task_done` hace `mono_handoffs.remove(...)` antes de entregar
  (`shell/orchestration.rs:1711`); un segundo aviso no encuentra destinatario.
  Un reintento de `control mono` con el mismo request ID devuelve el primer recibo
  (`handoffs.receipt`, `:1643`) y no encola la tarea dos veces. Tests:
  `a_finished_mono_task_reaches_the_chat_that_gave_it_once`,
  `a_chat_gives_monos_tasks_only_when_one_is_free_and_never_from_a_worker`.
- Worker temporal: usa el flag `delivered` de la tarea del run; el test comprueba
  «one result for the chat» y «never delivered twice» tras volver a bombear el run.

### Environment unificado conserva el medidor de contexto — correcto

- El anillo y el botón de Environment son ahora un solo control
  (`environment_control`, `crates/monocode-gpui/src/conversation/environment.rs:169`,
  usado en `crates/monocode-gpui/src/conversation.rs:3413`). Sigue pintando
  `context_meter::ring`, el tooltip con porcentaje y tokens, y la sección de
  contexto del frame (`context_section`, `environment.rs:588`).
- Anillo y barra comparten umbrales (`context_meter.rs:6-7`, `level_color`).
- Tests: `the_ring_tells_its_shortcut_opens_the_frame_and_closes_it_again`,
  `a_new_chat_shows_the_ring_but_has_no_frame_to_open`,
  `the_context_reads_as_a_share_and_tokens`, `a_fuller_window_turns_amber_then_red`,
  `the_meter_warns_then_alarms_at_the_shared_thresholds`.

### Pestaña Simulator del panel derecho — sin cambios, funciona

- Ningún cambio toca `right_panel.rs`, `simulator_pane*` ni `shell/layout.rs`.
- Pasan `shell::layout::tests::the_default_project_panel_opens_the_simulator_when_enabled`,
  `right_panel::tests::tab_order_keeps_known_tabs_once` y los 13 tests de
  `simulator_pane`. El test del worker temporal además comprueba que la pestaña
  Swarm aparece solo en el chat que lo lidera.

### `docs/in-progress/t3code-comparison.md` no afirma ahorros de tokens — correcto

- Dice expresamente que no hay datos de consumo de T3 y que no le atribuye ahorros.
- Las dos mejoras de monoBOT se presentan como ahorro de render/escritura,
  «no de tokens», con cifras medidas en tests:
  - lote de eventos: 2 402 → 78 eventos, afirmado exacto en
    `crates/monocode-core/src/provider/batch.rs:259`;
  - salida de fallo plegada: 1 500 → 87 líneas, afirmado en
    `crates/monocode-gpui/src/transcript/tools.rs:1033`.
- Los límites (`developerInstructions` en Codex, presupuesto del traspaso) se dejan
  como no medidos.

## Observaciones (no son fallos)

1. **Umbrales del anillo cambiados.** El anillo del composer avisaba al 75 % y al
   90 %; ahora al 80 % y al 95 %, los de la barra del Environment
   (`crates/monocode-gpui/src/context_meter.rs:6-7`). Es intencionado (unificar),
   pero el usuario verá el aviso más tarde. (@environment)
2. **El anillo se ve siempre.** Antes solo con contexto conocido o en borrador; ahora
   también en un chat con mensajes sin métricas («No context used yet»). Es
   coherente con que sea la puerta al Environment. (@environment)
3. **Cifra de bytes no afirmada exacta.** El doc da 87 532 040 → 2 847 716 B, pero el
   test solo comprueba `json * 25 < raw_json` (`provider/batch.rs:260`); la cifra
   exacta sale del `eprintln!`. Si cambia el fixture, el doc puede quedar
   desactualizado sin que falle nada. (@efficiency, menor)
4. **`mono_handoffs` vive en memoria.** Si la app se cierra mientras un mono trabaja,
   su respuesta no vuelve al chat que la pidió. Ya pasaba antes de estos cambios.
   (@delegation, previo)
5. **Recibos de `control mono`.** `Handoffs` busca con un recorrido lineal sobre hasta
   512 recibos; está bien para este volumen.

## Revisión de integración en `959b1753`

- Pasaron `cargo fmt --all -- --check`, `git diff --check`, `cargo xtask check`
  y la suite de core (1 496 pasan, 11 ignorados). En GPUI pasaron
  `orchestration` (17), `environment` (7), `transcript` (167, 1 ignorado),
  `swarm::tests` (6) y la regresión de la pestaña Simulator (1).
- Se reprodujo y corrigió el arranque sin aislamiento: `control worker`
  rechaza una carpeta sin Git o un run que comparte checkout. El fixture
  comprueba el worktree separado, el modelo del turno, el recibo estable
  aunque cambie el modelo y que cancelar mientras se prepara el worker no
  deja una sesión desconectada del chat.
- El agrupador conserva los eventos terminales y no mueve deltas a través
  de un cambio de estado. En el fixture actualizado aplica 85 de los 2 402
  eventos originales; el JSON de herramientas baja de 87 532 040 a
  2 847 716 bytes. No cambia el payload enviado al proveedor.
- Se comprobó la geometría de Environment a 600 px y el cierre por teclado
  con GPUI. No hubo prueba visual manual en claro/oscuro ni ejecución de
  una delegación exitosa contra un proveedor real.
