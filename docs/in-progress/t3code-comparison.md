# T3 Code frente a monoBOT: sesión, streaming y render

Comparación con T3 Code en la revisión `803f21f6a5211dd83ca26e02f7382a2396338bee`
(`https://github.com/pingdotgg/t3code`). El repositorio se leyó como datos, sin
ejecutar nada. Las rutas `t3:` son de esa revisión; las demás, de monoBOT.

Hay dos costes que conviene no mezclar:

- **Tokens**: lo que llega al proveedor y vuelve a leer en cada petición.
- **Render**: el trabajo de la app (filas reescritas, JSON serializado,
  texto pintado). No cambia lo que paga el proveedor.

No hay datos de consumo de T3 Code. Nada de lo que sigue le atribuye un
ahorro medido: se describe qué mecanismo usa.

## Qué hace T3 Code

### Sesión nativa y continuación

- **Codex** reanuda con `thread/resume` y `excludeTurns: true`: el historial se
  queda en Codex (`t3:apps/server/src/orchestration-v2/Adapters/CodexAdapterV2.ts:3514`,
  `:7519`, `:7991`). Sus instrucciones van como `additionalContext` de cada
  `turn/start` (`:852-869`), en trozos de 4 000 bytes como máximo
  (`t3:apps/server/src/provider/CodexDeveloperInstructions.ts:10`). Según su
  comentario, Codex solo los reenvía al modelo si cambian. Tras una compactación
  los vuelve a inyectar con `thread/inject_items` (`CodexAdapterV2.ts:1960-1964`).
  En un `turn/steer` añade solo las notas que el hilo aún no tiene (`:8157-8185`).
- **Claude** usa el Agent SDK: `resume` y `resumeSessionAt` (`ClaudeAdapterV2.ts:1240-1241`,
  `:1311`, `:1503-1530`). Sus reglas van en `systemPrompt.append` (`:2356-2360`).
  `runtimeInstructions.ts:1` dice que las reglas que no cambian en toda la sesión
  van una sola vez cuando el arnés tiene append nativo
  (`t3:packages/provider-core/src/server/runtimeInstructions.ts`). Si la sesión ya no
  existe, lo dice y continúa con el historial del hilo (`ClaudeAdapterV2.ts:776`).
- **Historial cuando no hay sesión nativa**: tiene un presupuesto de 16 000 tokens
  (`t3:packages/provider-core/src/server/handoffBudget.ts:14`, `selectHistory` en `:223`).
  Lista los IDs de lo que omite. Para un chat lateral, el límite es 8 000 caracteres
  (`ContextHandoffService.ts:249`).

### Streaming incremental

- **Deltas de texto y razonamiento de Claude**: se agrupan cada 50 ms
  (`ProviderTextDeltaCoalescer.ts:40`, usado en `ClaudeAdapterV2.ts:9085` y `:9173`).
- **Modo «paragraph»**: entrega el texto en fronteras de Markdown, como mucho cada
  400 ms. Si el mensaje se queda quieto 800 ms, lo entrega entero
  (`assistantStreaming.ts:114`, `:155-243`).
- **Salida de comandos**: va por un canal propio, fuera de los eventos
  (`CommandOutputHub.ts:28-40`). Se manda como mucho cada 100 ms y, a partir de
  16 KB acumulados, se cambia por la cola. Solo la reciben las filas abiertas
  (`t3:apps/web/src/components/chat/CommandOutputPanel.tsx:25`). La cola es de
  64 K caracteres (`t3:packages/shared/src/terminalOutput.ts:41`).
- **Límites del stream en vivo**: 1 000 elementos y 8 MB por suscriptor. Si se
  llenan, el cliente retoma desde la última secuencia (`LiveStreamBudget.ts:18-19`).

### Persistencia

- **Escrituras de eventos**: van en lotes de 64 eventos o 1 MB, activados por
  defecto (`ProviderCommitBatch.ts:12-21`).
- **Recibos de órdenes**: `accepted` o `rejected` (`CommandReceiptStore.ts:40-52`).
  No se revisó cómo deduplica eventos repetidos del proveedor.

### Render

- **Salida de comandos**: las filas cerradas no se suscriben a ella (ver arriba).
- **Subagentes**: tienen una barra propia y un diálogo de trabajo hijo
  (`apps/web/src/components/chat/ProviderSubagentBar.tsx`, `NativeChildWorkDialog.tsx`).
  La salida de una tarea de Claude se lee del archivo cada 250 ms, como mucho
  256 KB (`Adapters/ClaudeTaskOutputTail.ts:10-14`).
- **Contexto que se le dice al modelo**: una línea `<runtime_info>` con el arnés,
  el modelo y el esfuerzo (`runtimeInstructions.ts`, `buildRuntimeInstructions`).

## Qué hace monoBOT y qué ya estaba bien

- **Resume de Codex**: también usa `excludeTurns: true`
  (`crates/monocode-core/src/provider.rs`, rama `thread/resume`). Lo comprueba
  `codex_handshake_stream_snapshot_and_resume`.
- **Contexto fijo**: Claude lo recibe en `--append-system-prompt`; el resto de
  proveedores, una vez por conversación, con el recibo de `introduction`
  (ver [code-harness.md](../done/code-harness.md)). Es el mismo criterio que T3.
- **Traspaso a otro proveedor**: `response::handoff` lo limita a 1 800 caracteres.
  Es más estricto que los 16 000 tokens de T3, y pierde más detalle.
- **Mensajes en cola**: tras recuperarse, no reenvía los que ya entregó
  (`follow_up::tests::recovery_preserves_attachments_and_edits_but_never_replays_delivered_messages`).
- **Salida de herramientas**: se guarda una cola de 64 KB (`ToolCall::bound_output`).
- **Historial**: el transcript lee las páginas de turnos fuera del hilo de la
  interfaz, y solo la ventana visible construye Markdown
  (`crates/monocode-gpui/src/transcript/history.rs`).
- **Lotes de eventos**: el bucle de cada turno ya los emitía cada 16 ms
  (`TICK`) o al llegar a 64 KB (`BATCH_BYTES`).

El plegado de eventos y el render de salidas no cambian lo que recibe el
proveedor: **su delta de payload es 0 bytes**. La delegación añade una
instrucción a la oferta existente, que usa el recibo de `introduction`; no es
una optimización de tokens.

## Qué se implementó

### 1. Eventos plegados dentro de cada lote (render)

`crates/monocode-core/src/provider/batch.rs`. Antes, el lote de 16 ms guardaba
todos los eventos y la app los aplicaba uno a uno. Codex manda un
`item/commandExecution/outputDelta` por cada trozo de salida, y por cada uno la
app recibía una copia entera de la herramienta, con hasta 64 KB de salida. Por
cada copia, el runtime reescribía la fila como JSON y buscaba contribuciones.
Además, el límite de 64 KB del lote contaba todas las copias, así que una salida
larga vaciaba el lote en cada delta.

Ahora, dentro de un lote:

- el texto seguido se une, y el razonamiento seguido también; los demás
  eventos conservan su posición entre ambos;
- una línea de estado repetida, con solo texto entre medias, se descarta;
- una actualización de herramienta en curso sustituye a la anterior si es
  la misma llamada, no cambia su metadata y no hay nada entre las dos;
- `bytes` mide lo que el lote guarda ahora. Una sola actualización de una
  herramienta en curso puede esperar al tick; el límite sigue aplicando al
  texto y a los demás eventos (`Batch::full`).

Nunca se pliegan los cambios de estado (en curso → hecho o fallido), las
aprobaciones, las preguntas ni los errores. Tampoco cambia el orden entre tipos
de evento.

**Medido** (`provider::batch::tests::codex_stream_measured_through_the_parser`):
frames reales de Codex pasados por el parser, 32 frames por tick. Son 200 deltas
de razonamiento y un comando que saca 2 000 líneas y termina con error.

| | Antes | Después |
| --- | --- | --- |
| Eventos que aplica la app | 2 402 | 85 |
| JSON de herramienta serializado | 87 532 040 B | 2 847 716 B |

El estado final es idéntico: el mismo razonamiento, el estado `Failed` y la línea
de error al final. El reparto de 32 frames por tick es una suposición del fixture,
no un tiempo medido en un CLI real. Con menos frames por tick, el plegado es menor.
Si el razonamiento llega intercalado con la salida del comando, solo se pliegan
las actualizaciones seguidas. Es un ahorro de render y escritura; **no de tokens**.

### 2. Salida larga de un fallo, plegada sin ocultar errores (render)

`crates/monocode-gpui/src/transcript/tools.rs`. El detalle de una herramienta
fallida clonaba en cada render toda su salida (hasta 64 KB), aunque estuviera
cerrado. Al abrirlo la pintaba entera. Ahora:

- Cerrado, no la lee.
- Abierto, pinta:
  - las primeras 20 líneas y las últimas 60;
  - entre medias, las líneas que nombran un error (`error`, `panicked`, `fatal`,
    `exception`, `traceback`, `fail`), hasta 40;
  - una nota «… N lines omitted …» por cada tramo omitido;
  - **Show all N lines**, que la muestra entera.
- La salida guardada no cambia.

**Medido** (`transcript::tools::tests`): 1 500 líneas con errores en las líneas
401, 701 y 1 101, y al final.

| | Antes | Después |
| --- | --- | --- |
| Líneas pintadas al abrir | 1 500 | 87 (20 + 3 errores + 4 notas + 60) |
| Alto del bloque | ≥ 30 000 px | ≤ 1 748 px |
| Copias de la salida con el detalle cerrado | 1 por render | 0 |

Las líneas con error y la última (`error: could not compile`) siguen visibles.

## Límites y pendientes

- **Codex y `developerInstructions`**: monoBOT las envía también en
  `thread/resume`. No se ha comprobado si Codex las añade otra vez al contexto
  cuando no cambian. T3 manda las suyas como `additionalContext`, que según su
  comentario Codex solo reenvía si cambian. Medirlo exige un Codex real; no se cambió.
- **Coste por delta en el parser de Codex**: sigue copiando la herramienta en
  cada delta (`provider.rs`, `outputDelta`). El lote ya pliega esas copias, pero
  la copia ocurre igual. Es memoria y CPU, no tokens.
- **Cola de 64 KB de la salida** (`ToolCall::bound_output`): guarda la cola y
  pierde el principio. Un error que salga al principio de una salida muy larga se
  pierde en lo guardado. Para guardar principio y final hay que tocar
  `crates/monocode-core/src/tool.rs`, que está fuera de este encargo.
- **Dónde vive el plegado**: es una regla de presentación en `transcript/tools.rs`,
  igual que `DIFF_LINES`. Si otra vista lo necesita (el móvil), debería subirse
  a `monocode_core::tool`.
- **Presupuesto del traspaso**: no se comparó con T3 en una conversación real.
  T3 da hasta 16 000 tokens; monoBOT, 1 800 caracteres.
- **No revisado en T3**: la deduplicación de eventos repetidos del proveedor
  (`ProviderEventIngestor.ts`), ni los subagentes más allá de los archivos citados.
