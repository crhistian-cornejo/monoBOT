# Correo, bots y agenda con Cloudflare y Mailflare

## Estado de esta entrega

La implementación es opcional. Las cuentas IMAP y el envío SMTP existentes
siguen funcionando sin Cloudflare ni Mailflare.

### Desplegado en la cuenta de Cloudflare (8 de noviembre de 2026)

La cuenta se limpió: se borraron los Workers y buckets de Buddy y Echo. Solo
quedan recursos de monoBOT:

| Recurso | Nombre | Uso |
| --- | --- | --- |
| Worker | `monobot-inbound-mail` | API de correo entrante, en `https://monobot-inbound-mail.getecho.workers.dev` |
| D1 | `monobot-inbound-mail` | Índice de mensajes (migración `0001_init.sql` aplicada) |
| R2 | `monobot-inbound-mail-raw` | MIME original; lifecycle `expire-raw-mail` a 37 días |
| R2 | `monobot-releases` | Feed del actualizador y paquetes, público en `https://pub-5244a0b3b0814931924338438f119f34.r2.dev` |

`INBOUND_TOKEN` es un valor aleatorio de 64 caracteres. Está guardado como
secreto del Worker y en el llavero de macOS con el nombre «monoBOT inbound mail
token». Cópialo de Acceso a Llaveros al campo protegido *Inbound token* de
**Inbox → Agenda**, junto con la URL del Worker.

La cuenta todavía **no tiene dominio**. Mientras no lo tenga:

- la API se publica en `workers.dev` (`workers_dev = true`) y sigue exigiendo el
  token, que falla cerrado sin él;
- `ALLOWED_RECIPIENTS` vale `bot@pending-domain.invalid`, así que no se acepta
  correo real;
- Email Routing, Email Sending y Mailflare no se pueden configurar, porque los
  tres necesitan un dominio con DNS en esta cuenta.

Mailflare tampoco está desplegado. Hay que construir su código (AGPL, externo)
y el modo automático de esta sesión lo bloqueó. Ver los pasos abajo.

Repositorio de referencia: **https://github.com/hieunc229/mailflare**

Se usa el protocolo MCP público de Mailflare. No se ha copiado su código AGPL
a los crates MIT de monoBOT.

| Bloque | Implementación |
| --- | --- |
| Mailflare MCP | Conector nativo, desactivado inicialmente y concedido explícitamente a cada bot. Proxy local con lista de herramientas permitidas y token en el llavero del sistema. |
| Envío HTTP | Resend y Cloudflare Email Sending, además de SMTP. Lo usa el botón Enviar existente; los agentes solo guardan borradores. |
| Recepción | Worker opcional en Rust (WebAssembly), Email Routing → R2 + D1. El escritorio descarga páginas por HTTPS, sin abrir puertos públicos ni mantener IMAP para esos alias. |
| Bandejas de bots | Alias → ID de bot y remitentes permitidos. Cola persistente; se crea o reanuda la conversación del bot. |
| Agenda | Eventos locales, importación/exportación ICS y lectura del calendario de Mailflare, con zonas IANA y fechas con offset. |

### Desplegar Mailflare en la cuenta

Mailflare es un proyecto externo (AGPL). No se copia a este repositorio: se
construye desde su repositorio y se despliega en la cuenta como Worker
`mailflare`. Necesita un dominio en la cuenta para recibir y enviar.

```sh
git clone --depth 1 https://github.com/hieunc229/mailflare && cd mailflare
npm ci
npx wrangler d1 create mailflare          # pega el database_id en wrangler.jsonc (sin commitearlo)
npx wrangler r2 bucket create mailflare-raw
npx wrangler queues create mailflare-inbound
npx wrangler queues create mailflare-outbound
npx wrangler queues create mailflare-agent
npm run deploy
npx wrangler secret put CF_TOKEN          # token de Cloudflare con permisos sobre el dominio
```

`CF_TOKEN` se crea en **Manage Account → Account API Tokens** con DNS Edit,
Zone Read, Zone Settings Edit y Email Routing Rules Edit sobre el dominio, más
Email Sending Edit si va a enviar (requiere el plan Workers Paid). Después abre
`https://mailflare.getecho.workers.dev/setup`, crea el administrador, conecta
el dominio y genera la clave MCP (**Settings → API keys → Allow MCP access**).
No ejecutes las migraciones de D1 a mano: `/setup` las aplica.

## Cuentas IMAP, llavero y quién lee cada buzón

Esto funciona igual en macOS y Windows, sin Cloudflare.

- **Conectar:** en **Inbox → Email → Cuentas**, o desde el chat de un bot
  (`mail.connect`). Los dos usan el mismo formulario (`inbox/mail/connect.rs`):
  proveedor, dirección, contraseña de aplicación, nombre visible y **quién lo lee**.
- **La contraseña:** el servidor IMAP tiene que aceptarla (`LOGIN` + `SELECT
  INBOX`). Después se guarda en el llavero del sistema (Keychain en macOS,
  Credential Manager en Windows) como `com.monocode.desktop` / `email-imap-<id>`.
  El campo se vacía al pulsar Conectar. Nunca va a un archivo, al chat, al modelo
  ni a la nube.
- **Desde un bot:** `mail.connect` abre el diálogo de la app con la dirección y el
  nombre que pidió el bot. Por defecto la cuenta queda solo para ese bot. El
  agente nunca ve el diálogo ni la clave. Al conectarse, el bot recibe un aviso
  interno con la dirección y quién la lee, y sigue con lo que hacía.
- **Quién lo lee:** `mail/accounts.json` guarda `readers` (dirección → ids de
  agentes); una dirección sin entrada la leen todos. Solo el usuario lo cambia:
  en el diálogo o en cada cuenta de **Inbox → Email → Cuentas** («Read by…»).
  La primera vez se importa el `monocode.mail.private` de la app de escritorio
  antigua.

## Gmail sin dominio: la pestaña Email completa

Todo esto funciona con un Gmail normal (IMAP + contraseña de aplicación), sin
Cloudflare ni dominio, en macOS y Windows:

| Función | En monoBOT |
| --- | --- |
| Redactar y responder | **New message (C)** y **Forward (F)** en la pestaña Email (`inbox/mail/compose.rs`). Sale por SMTP con la misma contraseña de aplicación; Gmail guarda su copia en Enviados. *Save as draft* lo deja en Borradores del servidor. Un reenvío lleva el texto, no los adjuntos. |
| El agente redacta respuestas | `mail.propose`: el agente propone una respuesta o un correo nuevo y aparece como tarjeta en su chat, con remitente, destinatarios y asunto. Solo el usuario pulsa *Send*, *Save as draft* o *Discard* (`monos/mail_card.rs`). |
| Reglas de enrutado | **Rules** en Inbox › Email › Cuentas (`mail/rules.json`). Por remitente o palabras del asunto, solo correo nuevo: etiqueta de Gmail (`X-GM-LABELS`), estrella, leído, archivar o pasárselo a un agente. Lo pasado a un agente entra en su chat por la misma cola que el correo de Cloudflare. Un correo retenido como posible estafa nunca se pasa a un agente, y un agente solo recibe correo de cuentas que puede leer. |
| Registro de auditoría | **Agent activity** en la misma página (`mail/activity.jsonl`): quién listó, buscó, leyó, redactó o propuso, sobre qué, y si funcionó. Nunca guarda el texto del correo. |
| Conversaciones | La lista agrupa por hilo (`X-GM-THRID` en Gmail; asunto sin «Re:/Fwd:» en otros) y el lector muestra los demás mensajes del hilo. |
| Etiquetas | Las etiquetas propias de Gmail se ven en la lista y en el lector. |
| Darse de baja | Botón **Unsubscribe** cuando el correo trae `List-Unsubscribe`: una petición si admite `One-Click` (RFC 8058), si no abre la página o prepara el correo al `mailto`. Nunca en correo retenido, y nunca hacia `localhost` ni la red local. |
| Posponer | **Snooze** (3 horas, mañana 8:00, lunes 8:00). Es local: el correo sale de las vistas hasta su hora y espera en la vista **Snoozed**. |

## Configuración en monoBOT (solo con dominio propio)

Con Gmail o iCloud no hace falta nada de esto: la pestaña Email envía por SMTP
con la contraseña de aplicación, y **Email › Cuentas** lo dice
(«Sending from your own account»).

Abre **Inbox → Agenda → Advanced**. La página explica para qué sirve y deja
los ajustes plegados salvo que ya haya alguno en uso (`uses_own_domain`):

1. **Remote mail server URL** (servidor MCP, p. ej. una instancia de Mailflare): el origen HTTPS o su URL `/mcp`.
2. **Access token:** la clave que ese servidor espera, no el token de Cloudflare.
3. **Transporte de envío:** SMTP (predeterminado), Resend o Cloudflare.
4. **Cloudflare account ID:** necesario únicamente para enviar con Cloudflare.
5. **Inbound Worker URL:** el origen HTTPS del Worker de este repositorio.
6. **Inbound token:** el mismo secreto `INBOUND_TOKEN` configurado en el Worker.
7. **Rutas:** dirección del alias, ID del bot y lista de remitentes completos.
   El usuario debe activar cada ruta expresamente.

Los tokens se escriben en los campos protegidos, nunca en el chat. Los ajustes
sin secretos se guardan en `<data-dir>/mail/workspace.json`; las credenciales
se guardan en Keychain/Credential Manager o el almacén equivalente de Linux.
Los campos vacíos de credenciales no deben sobrescribir los valores guardados.

Después de configurar el servidor, activa «Let bots use this server» y concédelo al bot desde
sus herramientas. Los agentes no pueden concedérselo a sí mismos. Las sesiones
sin bot no reciben este conector privado automáticamente.

### Mailflare MCP

En tu instancia: **Settings → API keys → Allow MCP access**.

- Selecciona únicamente las bandejas necesarias.
- Concede lectura y borradores para trabajar con correo.
- Concede `Read calendar` para mostrar la agenda remota.
- Si quieres solicitudes de envío, concede el permiso de revisión
  correspondiente. `request_send` devuelve un enlace: **no envía**.
- No hace falta dar permisos de administración o gestión del calendario.

monoBOT lanza su propio proxy stdio (`--mailflare-mcp <data-dir>`).
El token no aparece en argumentos de proceso, archivos MCP ni variables de
entorno del agente. El proxy usa Streamable HTTP hacia `/mcp` y bloquea llamadas
no revisadas aunque el cliente ignore la lista de herramientas.

Se permiten lectura de correo/calendario, borradores y solicitudes de revisión.
No se permiten borrado, organización destructiva, envío directo, administración
ni escrituras en el calendario remoto. Los eventos que crea `calendar.create`
son **locales**, no invitaciones externas.

### Envío por API

**Resend**

- Verifica el dominio de la dirección remitente.
- Crea una API key limitada a envío y guárdala en el campo Resend.
- Endpoint: `POST https://api.resend.com/emails`.

**Cloudflare Email Sending**

- Activa Email Sending para la cuenta y verifica el dominio remitente.
- Crea un token con `Email Sending: Edit`, limitado a la cuenta necesaria.
- Guarda su account ID (32 caracteres hexadecimales) y el token en monoBOT.
- Endpoint:
  `POST https://api.cloudflare.com/client/v4/accounts/{account_id}/email/sending/send`.

La dirección From sigue siendo la cuenta o alias que recibió el mensaje. Un
proveedor HTTP no autoriza a suplantar una cuenta Gmail/iCloud; el dominio
remitente debe estar verificado por el proveedor elegido.

No hay reintentos automáticos ni cambio silencioso a SMTP. Un timeout puede
ocurrir después de que el proveedor haya aceptado el correo: comprueba su panel
antes de volver a pulsar Enviar. Las entregas parciales de Cloudflare se muestran
como incidencia, no como éxito completo.

### Recepción y bots

El Worker está en [`deploy/cloudflare-mail`](../../deploy/cloudflare-mail/README.md).
Ese README contiene los comandos de creación, migración, secretos y despliegue.

El escritorio consulta una página de hasta 20 mensajes cada 30 segundos,
aproximadamente, mientras está abierto. Una actualización manual de Email
también sincroniza. El Worker recibe y almacena correo aunque el escritorio
esté suspendido; el procesamiento de bots espera a que monoBOT vuelva a estar
abierto y tenga conexión.

El cursor solo avanza después de guardar toda la página y su cola de entregas.
Un fallo de red o escritura permite repetir la descarga sin duplicar la cola.
Una entrega se confirma después de persistir el outbox del bot, antes de
ejecutarla. Los mensajes mantienen IDs estables.

Para ejecutar un bot deben cumplirse todas estas condiciones:

- alias configurado y activado expresamente;
- bot existente, no eliminado y con conversación disponible;
- remitente SMTP en la lista exacta de direcciones permitidas;
- comprobación conservadora SPF/DMARC del receptor;
- cabecera From coherente con el remitente y mensaje no retenido por triage.

El contenido llega como JSON de terceros, no como instrucciones del usuario.
No se abren enlaces ni se ejecutan adjuntos. Los alias asignados a un bot no
quedan disponibles para otros agentes a través de `mail.*`.

Los mensajes aparecen también en Email. Sus borradores se guardan localmente,
para que el usuario los revise y envíe mediante el transporte HTTP elegido.
Marcar leído, destacar o archivar un mensaje cloud cambia su vista local, no
el archivo original de R2.

### Agenda e ICS

La Agenda es un calendario: vista **Month** (seis semanas) o **Week**, con
‹ Today › para moverse, y debajo los eventos del día elegido (los locales se
pueden borrar). **New event** propone el día elegido.

**Google Calendar** sigue un calendario por su «Secret address in iCal
format» (Google Calendar › Configuración › tu calendario › Integrar
calendario). La dirección funciona como una contraseña: va al llavero
(`mail-workspace-calendar-…`), el campo se vacía al guardar y nunca se
escribe en disco. monoBOT la descarga por HTTPS (8 MiB máx., sin red local),
expande las recurrencias (`DAILY`, `WEEKLY` con `BYDAY`, `MONTHLY` con
`BYDAY`/`BYMONTHDAY`, `YEARLY`, `INTERVAL`, `COUNT`, `UNTIL`, `EXDATE`,
`RECURRENCE-ID` y eventos cancelados) de 62 días atrás a 400 adelante, y la
guarda en `mail/calendar-feed.json`. Se actualiza al abrir la Agenda si tiene
más de 15 minutos. Una regla que no sabe expandir (`BYSETPOS`…) se muestra
solo en su primera fecha y la Agenda lo dice. Es de solo lectura: monoBOT no
crea ni cambia eventos en Google, y los agentes no leen este calendario
(`agenda_for_user` es solo para la vista del usuario).

Los eventos locales se guardan en `<data-dir>/mail/agenda.json`. Los tiempos
de entrada deben llevar offset ISO 8601 y la zona debe ser IANA, por ejemplo
`Europe/Madrid`. La agenda remota usa rangos positivos de hasta 90 días.
Una respuesta remota truncada no se presenta como agenda completa.

Acciones disponibles para los Monos:

`calendar.list` solo incluye el servidor remoto si ese bot recibió el conector
explícitamente y sigue habilitado. Los demás agentes solo leen la agenda local.

```text
calendar.list {"start":"2026-11-07T00:00:00+01:00","end":"2026-11-14T00:00:00+01:00"}
calendar.create {"title":"Revisar lanzamiento","start":"2026-11-09T10:00:00+01:00","end":"2026-11-09T10:30:00+01:00","timezone":"Europe/Madrid"}
```

La importación ICS es atómica y conserva UID para no duplicar una segunda
importación del mismo archivo. Admite eventos únicos UTC, `TZID` y días
completos con `DTEND`, conservando `VALUE=DATE` al volver a exportar.
Los bloques con hora no se convierten en días completos por empezar a medianoche.
Rechaza recurrencias (para eso está seguir el calendario por su dirección
iCal), horas flotantes y horas ambiguas durante cambios de horario, en lugar
de interpretarlas incorrectamente. Las recurrencias del servidor remoto se
leen ya expandidas en su agenda.

La exportación incluye los eventos locales/importados, no copia el calendario
remoto. Usa CRLF, escape de texto y plegado de líneas UTF-8.

## Pendientes de cuenta y despliegue

Nada de esta sección se ha ejecutado contra una cuenta real.

- [x] Cuenta: la personal; recursos de otras apps borrados.
- [ ] Añadir un dominio a Cloudflare y elegir el subdominio HTTPS de la API y los alias definitivos.
- [ ] Revisar costes/planes de Workers, D1, R2 y Email Sending. No se ha activado
  facturación ni se garantiza que todo entre en cuotas gratuitas.
- [x] Crear D1 `monobot-inbound-mail` y R2 `monobot-inbound-mail-raw`.
- [ ] Copiar `wrangler.example.toml` a `wrangler.toml` (ignorado por Git),
  completar bindings, destinatarios permitidos y ruta HTTPS.
- [x] Aplicar la migración del Worker y desplegarlo con Wrangler autenticado (en workers.dev).
- [ ] Con dominio: volver a `workers_dev = false`, descomentar `routes` y poner los destinatarios reales en `ALLOWED_RECIPIENTS`.
- [x] Crear `INBOUND_TOKEN` (64 caracteres) y guardarlo como secreto del Worker
  y en el llavero de macOS. Falta pegarlo en monoBOT (Inbox → Agenda).
- [ ] Activar Email Routing y revisar los registros MX/SPF existentes antes de
  cambiarlos. No sustituir sin revisión el correo de un dominio en producción.
- [ ] Crear reglas de alias hacia el Worker. Mantener catch-all desactivado
  salvo necesidad expresa; preferir destinatarios exactos.
- [x] Ajustar retención y lifecycle de R2 (30 y 37 días). El lifecycle debe ser mayor que
  `RETENTION_DAYS` (por defecto 30 días, lifecycle sugerido 37).
- [ ] Comprobar con correo real que Cloudflare antepone exactamente su
  `Authentication-Results` y que el formato coincide con `auth_results.rs`.
  Sin esa evidencia, dejar las rutas de bots desactivadas. No rebajar la
  comprobación para forzar que funcione.
- [ ] Verificar que un fallo de D1/R2 durante `email()` no se presenta al
  remitente como entrega aceptada. Las pruebas locales no prueban el
  comportamiento SMTP de la plataforma.
- [ ] Verificar recepción durante suspensión y recuperación posterior,
  reintentos, alias desconocidos, remitentes no autorizados y SPF/DMARC fallido.
- [ ] Verificar dominio de envío, DKIM/SPF/DMARC y claves Resend/Cloudflare.
  Hacer el primer envío únicamente a un destinatario de prueba autorizado.
- [ ] Desplegar o elegir una instancia Mailflare, terminar `/setup`, crear
  usuario, dominios y bandejas, y generar una clave MCP de privilegios mínimos.
  Seguir su [guía oficial de despliegue](https://github.com/hieunc229/mailflare/blob/main/docs/deployment.md).
- [ ] Comprobar lectura, borradores, agenda y revisión humana con una instancia
  real de Mailflare y los CLIs de agentes instalados.
- [ ] Configurar alertas de consumo, backups, rotación de secretos y pruebas
  de restauración de D1/R2.

## Límites conocidos y siguientes extensiones

- Los adjuntos entrantes están limitados por el mensaje completo de 2 MiB.
- La vista local mantiene un caché acotado de 500 mensajes, conservando
  borradores y entregas pendientes. No es un archivo histórico completo.
- Un mensaje expirado en R2 se muestra como no disponible; su metadata no
  bloquea para siempre el resto de la sincronización.
- Permanecer desconectado más que la retención puede perder contenido antes
  de que el escritorio lo descargue. No se promete almacenamiento infinito.
- El Worker no envía push por sí mismo ni despierta el escritorio. La app móvil
  conserva el acceso existente a las conversaciones de bots por el enlace
  cifrado; una pantalla móvil de correo/agenda y push directo desde el Worker
  quedan pendientes.
- Los bloques de agenda no programan por sí solos una notificación o un
  despertar del agente. Para eso existen hábitos y `wakes.add`; un sistema de
  alarmas propio de calendario queda pendiente.
- Edición de eventos desde la agenda nativa, recurrencia ICS y
  sincronización bidireccional de calendario quedan pendientes.

Los eventos locales o importados sí se pueden borrar desde Agenda con
confirmación. Los eventos de Mailflare permanecen de solo lectura. Importar y
exportar ICS usa el portapapeles, no un selector de archivos.

## Validación

Sin credenciales ni envío de correo real:

```sh
cargo test -p monobot-email --lib
cargo test -p monocode-core --lib inbox::workspace
cargo test -p monocode-core --lib connectors::
cargo test -p monocode-gpui --profile native-dev cloud_mail_waits_for_a_separate_turn_and_failed_delivery_stays_blocked
cargo test -p monocode-gpui --profile native-dev inbox::workspace::tests
cargo check -p monocode-gpui --profile native-dev
cd deploy/cloudflare-mail
cargo test
cargo clippy --target wasm32-unknown-unknown -- -D warnings
```

Documentación de contratos públicos:

- [MCP de Mailflare](https://github.com/hieunc229/mailflare/blob/main/docs/email-assistant-and-mcp.md).
- [Cloudflare REST Sending](https://developers.cloudflare.com/email-service/api/send-emails/rest-api/).
- [Resend Send Email](https://resend.com/docs/api-reference/emails/send-email).
