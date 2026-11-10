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
