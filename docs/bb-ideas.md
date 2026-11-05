# Qué aprovechar de bb

[bb](https://github.com/get-bb/bb) es un IDE de agentes (Electron, servidor Node y daemon por máquina) en el que casi todo es un plugin: proveedores, barra lateral, composer, git, etc. Revisado el 4 de noviembre de 2026.

MonoBOT ya cubre buena parte de lo que bb ofrece: automatizaciones, memoria compartida, side chat, preguntas al usuario, Inbox de GitHub con PRs y checks, artefactos y la reanudación automática cuando se acaba el límite de uso (`src/features/sessions/model/usageLimit.ts`). Este documento recoge lo que todavía no tenemos y merece la pena.

## Hecho

**provider-retry** (5 de noviembre de 2026). Cuando el proveedor está saturado (529, 429, "overloaded"), da un 5xx o se cae la conexión, la sesión vuelve a enviar el turno sola. Espera 1, 2, 4 y 8 minutos (15 s, 30 s, 1 y 2 min si es la conexión), cada vez con hasta un 25 % más al azar, y se rinde tras 4 reintentos. Un mensaje escrito por el usuario reinicia la cuenta. No reintenta límites de uso, crédito o gasto (tienen su propia tarjeta), ni las sesiones de automatizaciones o de hábitos de Mono. Encima del composer aparece una tarjeta con la cuenta atrás, "Retry now" y una X para no reintentar.

- Lógica: `src/features/sessions/model/providerRetry.ts`. Se activa en `session.error` (`src/integrations/harness/core/apply.ts`), y el temporizador vive en `App.tsx`, junto al de usage limit.
- Tarjeta: `src/features/sessions/ui/ProviderRetryNotice.tsx`, con el mismo estilo que `UsageLimitNotice`.
- Diferencia con bb: no hay ajuste de espera máxima. Con 4 reintentos la espera total no pasa de unos 19 minutos, así que ese ajuste no cambiaría nada. Las sesiones remotas y los chats de Mono todavía no reintentan solos.

## Ideas, por prioridad

| Idea de bb | Qué aporta a MonoBOT | Esfuerzo |
|---|---|---|
| **secrets** | El agente pide una API key y el usuario la escribe en un formulario oculto del hilo. El valor se guarda directo en un `.env` con permisos `0600`, actualizando las líneas existentes y añadiendo las nuevas; nunca aparece en el transcript. El agente solo recibe la ruta y qué nombres se añadieron o cambiaron. | Medio |
| **concurrency-limit** | Limita los agentes que corren a la vez: por defecto uno por CPU, con un límite global opcional. Los turnos que superan el límite esperan en cola y el hilo dice por qué. Un mensaje a un hilo que ya está corriendo no se retiene. | Medio |
| **scheduled-send** | "Enviar más tarde" desde el composer, conservando adjuntos, menciones, modelo y modo de permisos. | Bajo |
| **prompt-library** | Historial de los prompts enviados, con búsqueda y favoritos, para reinsertarlos en el composer. | Bajo |
| **keep-awake** | Impide que el Mac entre en reposo mientras hay agentes trabajando. | Bajo |
| **agent-annotations** | En el navegador integrado, haces clic en un elemento, escribes una nota y se añade al prompt con la URL, el selector, el texto, los estilos clave y los nombres de componentes de React. Muy útil para iterar UI. | Medio-alto |
| **account-pool** | Rota entre varias cuentas de Claude o Codex cuando una llega a su límite, y se combina con provider-retry para saber cuándo vuelve a haber una cuenta disponible. | Alto |

Siguiente recomendación: **scheduled-send** o **keep-awake**, por su bajo esfuerzo. **account-pool** ya puede apoyarse en provider-retry.

## Prácticas de su repo que nos sirven

- **Todo lo que hace la UI se puede hacer desde la CLI.** bb lo exige para cada feature. Nosotros ya tenemos `src-tauri/src/control_cli.rs` y `/operator` (ver `docs/agent-access.md`); podríamos pedirlo también para cada feature nueva.
- **Tests de fallos realistas**, no de getters ni de wiring.
- **Cada feature con su descripción para el usuario** (`PLUGIN_OVERVIEW.md` en bb): qué obtienes, cómo funciona, ajustes y comandos de CLI.

## Lo que no conviene copiar

- Su arquitectura de servidor HTTP sin autenticación más daemon: para MonoBOT, Tauri con comandos locales es más simple y más seguro.
- Los servicios alojados (bb account, bb connect, bb cloud): dependen de su infraestructura.
- Su estilo visual: como dice `CLAUDE.md`, adaptamos la idea al lenguaje visual de MonoBOT.
