# Equipo de agentes: especialistas y un orquestador que solo coordina

Plan del 2026-11-04. Toma las ideas del equipo de agentes de Echo
(`~/Developer/Echo/docs/on board/10-26-2026-equipos-de-agentes.md`) y las
construye sobre lo que monoBOT ya tiene: bots con rol, SOUL.md, MEMORY.md,
hábitos, `agents.message` y herramientas por bot.

## Por qué

- El Principal (el orquestador de la zona Agentes) arrancaba con Gmail, Drive,
  Notion y, como no tenía conectores propios, con **todos** los MCP activos y
  todas las skills instaladas, aunque su trabajo no los use.
- `agents.list` le daba al orquestador el nombre y el rol de cada agente, nada
  más. Repartía a ciegas: no sabía en qué es bueno cada uno ni qué herramientas
  tiene.
- Solo había una skill asignable (`monobot-finance`). Las skills que el usuario
  instala en `~/.claude/skills` y en plugins las veían todos los bots por igual.
- La memoria era de cada agente; no había forma de que todo el equipo supiera
  algo del usuario sin repetirlo en cada uno.

## Lo que se construye

### 1. El orquestador hace lo suyo y además coordina

- El Principal tiene su propia especialidad (asistente personal: preguntas del
  día a día, notas y tareas) y la hace él mismo, con sus cuentas (Gmail, Drive,
  Notion). Lo que es especialidad de otro agente, o necesita herramientas que
  solo otro tiene, lo reparte.
- Arranca sin conectores MCP ni skills instaladas: esas son de los
  especialistas que las usan. El usuario se las puede dar en sus ajustes.
- En sus ajustes y en `agents.list` aparece marcado como **Orchestrator**.
- Una migración, una sola vez, devuelve sus herramientas guardadas a las del
  rol. Las carpetas que el usuario le abrió se conservan.
- En los ajustes de cualquier bot, «Use role's» devuelve sus herramientas a las
  del rol.

### 2. Especialidades

- Cada agente tiene una **especialidad**: una línea que dice de qué se encarga.
  Viene de su rol y el usuario la cambia en sus ajustes; `bots.create` la acepta.
- Entra en cada turno del agente (`<specialty>`): toma lo suyo y, para lo que no
  es suyo, dice de quién es.
- `agents.list` devuelve por agente: especialidad, si es el orquestador, sus
  herramientas (cuentas, skills, conectores, acceso) y sus últimas 3 tareas
  delegadas. Así el orquestador elige bien y no repite trabajo.
- Roles nuevos, cada uno con sus herramientas por defecto: **Inbox** (Gmail),
  **Planner** (Notion), **Writer** (textos, Drive y Notion, puede escribir
  archivos) y **Analyst** (hojas y datos). Research gana los conectores de
  documentación (Context7, DeepWiki, Microsoft Learn, x.md); Documents, las
  skills de documentos si están instaladas.

### 3. Skills por agente

- Cada bot tiene una lista de skills instaladas que puede usar; si no la tiene,
  usa las sugeridas por su rol que estén instaladas.
- Al lanzar su sesión de Claude Code, monoBOT niega con `Skill(<nombre>)` cada
  skill instalada (usuario, proyecto y plugins) que no esté en su lista. Las
  skills propias de monoBOT (`monobot`) siempre quedan.
- `tools.read` y los ajustes del bot muestran las skills instaladas, marcadas.

### 4. Memoria del equipo

- Una memoria compartida (`team-memory`, en la misma carpeta de datos que los
  agentes): `app memory.add {"fact":"…","shared":true}` guarda algo que todo el
  equipo debe saber (preferencias del usuario, decisiones).
- Entra en cada turno de cada agente como `<team_memory>` (hasta 60 líneas) y
  vuelve a entrar cuando cambia. `memory.search` la incluye.

### 5. Aprobaciones de los agentes: «Needs you» y avisos nativos

- Causa: los avisos (notificación nativa, sonido y tarjeta dentro de la app)
  solo salían para sesiones de un proyecto conocido, y los agentes trabajan en
  la carpeta de inicio, que a propósito no cuenta como proyecto. Una aprobación
  pendiente de un agente no avisaba nunca.
- Ahora las sesiones de agentes avisan bajo una entrada propia, **Agents**, que
  se silencia como cualquier proyecto, y el aviso lleva el nombre del agente.
- El panel de novedades del orquestador tiene arriba una lista **Needs you**
  con cada agente que espera una aprobación, una respuesta o tiene el límite de
  uso agotado; un clic abre su chat.
- Las notificaciones nativas siguen siendo opt-in (Ajustes › Notificaciones),
  hasta que haya notch.

### 6. Delegar desde cualquier chat de código: monos y workers temporales

Con Ajustes › Chat › «hand work to monos» (`chief_offer`), un chat de código
local aprende una vez por conversación del proveedor (`control::chief_offer`)
que puede repartir trabajo por el CLI `monocode-gpui control`:

- **`control mono {"mono","task"}`**: la tarea va al chat propio de un mono de
  código del usuario, que trabaja con su memoria (`Monos::hand_task`). Un mono
  ocupado (mensaje en cola o en curso, chat trabajando, o con una tarea de una
  run de Chief) la rechaza; la tarea no se encola detrás. Sin monos, el error
  lo dice y nombra `control worker`: nunca se inventa un mono. La respuesta
  vuelve una sola vez al chat que la dio (`mono_task_done`, guardada hasta que
  el chat está libre).
- **`control worker {"title","task","files"}`**: si ningún mono encaja, el
  chat crea explícitamente un worker temporal. Es un `delegate` en la run del
  propio chat (se abre una si no la tiene, como `control swarm`): mismo
  harness y modelo que inició el turno, su propio worktree y recibos por
  request-id. Requiere un checkout Git local con al menos un commit; no
  comparte la carpeta del lead cuando no se puede aislar. Las aprobaciones
  van al chat. El chat pasa a ser su lead: recibe el resultado una
  vez, lo revisa con `review` y cierra con `finish`. `files` vale `["."]` si
  no se da.
- El worker temporal vive en la run y en su sesión («Agent · título»). No se
  añade al roster de monos ni de Captains, no recibe identidad de mono ni
  escribe memoria. Se ve y se selecciona en la pestaña Swarm del panel derecho
  **de ese chat** (en Classic, Swarm vive en la barra lateral); otro chat no
  lo ve.

Reglas comunes (`Shell::delegation_refusal`):

- Solo delega el chat local que ve el usuario: no un worker de una run, ni un
  mono, ni Chief (que delega con `delegate` dentro de su run), ni un chat
  remoto, ni uno gestionado por otra run. Así no hay ciclos: lo delegado no
  puede volver a delegar.
- Request-id obligatorio y válido. Un reintento con el mismo id devuelve la
  primera respuesta (`control::Handoffs` para `mono`, recibos de la run para
  `worker`); el mismo id con otra entrada se rechaza.
- Los campos desconocidos se rechazan; el mono se valida contra el roster.

**Proveedores.** La oferta es texto en el turno y el CLI; el entorno del
bridge llega a todo harness local (`run_process` pasa
`settings.environment`; OpenCode arranca un servidor privado con esas
credenciales). Lo que hay comprobado con tests:

| Proveedor | Comprobado |
| --- | --- |
| Codex | El sandbox abre la red local cuando el chat llama a la app (`codex_chats_that_call_back_into_the_app_can_reach_its_bridge`). |
| OpenCode | Servidor privado con las credenciales de la sesión (`reaches_app` en `provider/opencode.rs`). |
| Claude, Pi y los ACP (Cursor, Grok, Antigravity, Devin, Droid, Kimi, Hermes, Fx, Omp) | Reciben la oferta y el entorno, pero ningún test prueba que su herramienta de shell llegue a 127.0.0.1. Si su sandbox corta la red, la llamada falla con el error de conexión del CLI. En modo supervisado, el usuario aprueba el comando. |
| Chats remotos | No reciben la oferta. |

El flujo del shell (`control mono`, `control worker`, rechazos, reintentos,
cancelación, un único resultado y la visibilidad por chat) se prueba en
`shell/orchestration/tests.rs` con el harness `fixture`, sin servicios reales.
Límite de ese harness: no completa ningún turno con éxito. El worker temporal
termina con el error del proveedor como resultado, y es ese resultado el que
el test comprueba que llega al chat una sola vez y con contenido (por
`continue_lead`, contando el turno en el transcript del chat). El fin del turno
del chat que llamó a `control worker` se modela poniendo su lead en reposo,
como hacen los tests de Chief. Que un worker con éxito entregue su informe lo
cubre core (`interrupted_runs_pause_resume_from_saved_state_and_deliver_results_once`);
de punta a punta con un proveedor real no hay test.

## No objetivos

- No se cambia cómo se aprueban permisos ni lo que siempre está bloqueado
  (enviar, compartir, borrar).
- No se copian el tablero durable, los perfiles cognitivos ni los mensajes entre
  pares de Echo en esta entrega (ver «Siguiente»).
- Las herramientas por bot siguen aplicándose solo en Claude Code, como hoy.

## Siguiente

- Tablero de tareas durable con asignados (Echo, fase 5).
- Perfiles con nombre (rápido, profundo, investigador) que fijen modelo y
  esfuerzo por agente (Echo, fase 2).
- Una página del equipo con todos los agentes, su estado y sus tareas.
