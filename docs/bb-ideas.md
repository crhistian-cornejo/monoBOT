# Qué aprovechar de bb

[bb](https://github.com/get-bb/bb) es un IDE de agentes (Electron, servidor Node y daemon por máquina) en el que casi todo es un plugin: proveedores, barra lateral, composer, git, etc. Revisado el 4 de noviembre de 2026.

MonoBOT ya cubre buena parte de lo que bb ofrece: automatizaciones, memoria compartida, side chat, preguntas al usuario, Inbox de GitHub con PRs y checks, artefactos y la reanudación automática cuando se acaba el límite de uso (`src/features/sessions/model/usageLimit.ts`). Este documento recoge lo que todavía no tenemos y merece la pena.

## Hecho

**provider-retry** (5 de noviembre de 2026). Cuando el proveedor está saturado (529, 429, "overloaded"), da un 5xx o se cae la conexión, la sesión vuelve a enviar el turno sola. Espera 1, 2, 4 y 8 minutos (15 s, 30 s, 1 y 2 min si es la conexión), cada vez con hasta un 25 % más al azar, y se rinde tras 4 reintentos. Un mensaje escrito por el usuario reinicia la cuenta. No reintenta límites de uso, crédito o gasto (tienen su propia tarjeta), ni las sesiones de automatizaciones o de hábitos de Mono. Encima del composer aparece una tarjeta con la cuenta atrás, "Retry now" y una X para no reintentar.

- Lógica: `src/features/sessions/model/providerRetry.ts`. Se activa en `session.error` (`src/integrations/harness/core/apply.ts`), y el temporizador vive en `App.tsx`, junto al de usage limit.
- Tarjeta: `src/features/sessions/ui/ProviderRetryNotice.tsx`, con el mismo estilo que `UsageLimitNotice`.
- Diferencia con bb: no hay ajuste de espera máxima. Con 4 reintentos la espera total no pasa de unos 19 minutos, así que ese ajuste no cambiaría nada. Las sesiones remotas y los chats de Mono todavía no reintentan solos.

**scheduled-send** (5 de noviembre de 2026). Junto al botón de enviar hay un reloj con las mismas horas que "Remind me" (en 1 h, en 3 h, esta tarde 18:00, mañana 9:00, la semana que viene lunes 9:00, de `sessionReminderPresets`) y "Elegir fecha y hora". El mensaje queda en la cola de la sesión con un reloj y su hora, y se puede editar, borrar o enviar al momento ("Send now"). Cuando llega la hora pasa a la cola normal: se envía si el agente está libre, o espera o guía el turno en curso como cualquier mensaje en cola. Conserva texto, adjuntos, menciones, modo (plan, orquestar) y tarjetas de nota o handoff.

- Lógica: `src/features/sessions/model/scheduledSend.ts`. La cola (`queuedHead` en `messageQueue.ts`) se salta los mensajes que esperan. El temporizador está en `App.tsx`.
- Se guarda con la sesión (`sendAt` en cada mensaje de la cola), así que sobrevive a reinicios. Tras un reinicio o un error, la cola solo se pausa si había mensajes para enviar ya.
- UI: `src/features/sessions/ui/SendLaterButton.tsx`. El formulario de fecha y hora está en `src/shared/ui/DateTimeForm.tsx`, compartido con el silencio de notificaciones.
- Diferencias con bb: se envía con el modelo y los permisos que tenga la sesión al llegar la hora, no con los del momento de programarlo, porque cambiar de modelo tiene su propio flujo de handoff. Solo se ofrece en sesiones que se guardan: no en una sesión vacía (para empezar algo a una hora, usa Automatizaciones), ni en remotas, ni en chats de Mono. Los mensajes programados solo salen con la app abierta.

**keep-awake** (5 de noviembre de 2026). Mientras hay trabajo en marcha, el equipo no entra en reposo por inactividad: un agente trabajando, un reintento de provider-retry o una reanudación de límite de uso esperando, o un mensaje de Send later pendiente. La pantalla sí puede apagarse, y cerrar la tapa o suspender a mano duermen el equipo igual. Se desactiva en Ajustes → General → "Keep computer awake" (activado por defecto).

- Qué cuenta como trabajo en marcha: `src/features/sessions/model/keepAwake.ts`. Las sesiones remotas no cuentan, porque su trabajo corre en el host. `App.tsx` llama a `keep_awake_set` solo cuando eso cambia.
- Sistema: `src-tauri/src/keep_awake.rs`. En Windows, `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` desde un hilo propio, porque la petición pertenece al hilo que la hace. En macOS, una aserción de IOKit `PreventUserIdleSystemSleep`, como `caffeinate -i`. En Linux todavía no hace nada.
- Diferencia con bb: también cubre lo que espera su hora (reintentos, reanudaciones y Send later), no solo los agentes trabajando.

## Ideas, por prioridad

| Idea de bb | Qué aporta a MonoBOT | Esfuerzo |
|---|---|---|
| **secrets** | El agente pide una API key y el usuario la escribe en un formulario oculto del hilo. El valor se guarda directo en un `.env` con permisos `0600`, actualizando las líneas existentes y añadiendo las nuevas; nunca aparece en el transcript. El agente solo recibe la ruta y qué nombres se añadieron o cambiaron. | Medio |
| **concurrency-limit** | Limita los agentes que corren a la vez: por defecto uno por CPU, con un límite global opcional. Los turnos que superan el límite esperan en cola y el hilo dice por qué. Un mensaje a un hilo que ya está corriendo no se retiene. | Medio |
| **prompt-library** | Historial de los prompts enviados, con búsqueda y favoritos, para reinsertarlos en el composer. | Bajo |
| **agent-annotations** | En el navegador integrado, haces clic en un elemento, escribes una nota y se añade al prompt con la URL, el selector, el texto, los estilos clave y los nombres de componentes de React. Muy útil para iterar UI. | Medio-alto |
| **account-pool** | Rota entre varias cuentas de Claude o Codex cuando una llega a su límite, y se combina con provider-retry para saber cuándo vuelve a haber una cuenta disponible. | Alto |

Siguiente recomendación: **prompt-library**, por su bajo esfuerzo. **account-pool** ya puede apoyarse en provider-retry.

## Prácticas de su repo que nos sirven

- **Todo lo que hace la UI se puede hacer desde la CLI.** bb lo exige para cada feature. Nosotros ya tenemos `src-tauri/src/control_cli.rs` y `/operator` (ver `docs/agent-access.md`); podríamos pedirlo también para cada feature nueva.
- **Tests de fallos realistas**, no de getters ni de wiring.
- **Cada feature con su descripción para el usuario** (`PLUGIN_OVERVIEW.md` en bb): qué obtienes, cómo funciona, ajustes y comandos de CLI.

## Lo que no conviene copiar

- Su arquitectura de servidor HTTP sin autenticación más daemon: para MonoBOT, Tauri con comandos locales es más simple y más seguro.
- Los servicios alojados (bb account, bb connect, bb cloud): dependen de su infraestructura.
- Su estilo visual: como dice `CLAUDE.md`, adaptamos la idea al lenguaje visual de MonoBOT.
