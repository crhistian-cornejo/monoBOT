> **monobot-email**: portado de `crates/email` de Echo (MIT, Echo contributors). En MonoBOT lo usan `src-tauri/src/mail.rs` (comandos) y `src/features/mail` (la pestaña Email del Inbox). Lo que sigue describe el crate tal como era en Echo.

# echo-email

El correo del dueño dentro de Echo, por IMAP con una contraseña de aplicación: Gmail, iCloud o cualquier servidor. Lee las cuentas juntas, ordena cada mensaje por reglas (prioridad, rutina, en espera), hace lo que el dueño pide sobre uno (leer, destacar, archivar) y guarda una respuesta como borrador. **Nunca envía, nunca borra y nunca ejecuta lo que dice un mensaje**: su texto es información, no instrucciones. Al abrir uno, su HTML se muestra limpio (`sanitize.rs`) en un marco que no ejecuta nada y no carga nada por su cuenta.

## Qué hay aquí

| Archivo | Qué hace |
| --- | --- |
| `account.rs` | La cuenta (dirección, servidor, proveedor) y su validación: nada que pueda ir a un comando con comillas o saltos de línea |
| `secrets.rs` | La contraseña, en Credential Manager o Keychain (`echo-base`), nunca en un archivo ni en la base de datos |
| `imap.rs` | Cliente IMAP sobre TLS, escrito para lo que el correo necesita: entrar, listar carpetas, leer lo más reciente con `BODY.PEEK` y, al abrir uno, el mensaje entero con `BODY.PEEK[]` (nada se marca como leído), poner una bandera, mover un mensaje y guardar un borrador |
| `clean.rs` | Lo que se limpia del texto antes de leerlo: entidades sin decodificar (`&zwnj;`), caracteres invisibles de relleno, las notas `[image: …]` y los enlaces (los de entre `<…>` se quitan y los demás dicen a dónde apuntan; nada se puede pulsar) |
| `mime.rs` | Cabeceras y cuerpo como texto para la lista: quoted-printable, base64, juegos de caracteres, HTML sin etiquetas. Al abrir, el mensaje entero: su HTML, su texto, sus imágenes incrustadas (`cid:`) y la lista de sus adjuntos (nombre seguro, tipo, tamaño: sin los bytes). Un adjunto solo se baja del servidor cuando el usuario lo abre (`Mail::attachment`) y nunca se guarda en memoria |
| `sanitize.rs` | La lista blanca del HTML: etiquetas, atributos y estilos que se conservan (tablas, colores, tamaños, enlaces). Los enlaces solo van a la web (`https`) o a un correo; las imágenes incrustadas pasan a `data:` (solo PNG, JPEG, GIF o WebP); las de la web se quedan sin cargar y se listan. Quita scripts, manejadores de eventos, formularios, `position` y todo `url(…)` |
| `cache.rs` | La memoria de lo que se lee otra vez, acotada en bytes: la última lectura de las cuentas, los últimos mensajes abiertos y las últimas imágenes de la web. Solo vive mientras la app está abierta; el menos usado sale primero |
| `remote.rs` | Trae una imagen de la web cuando el dueño lo pide: solo `https` a una dirección pública (nunca esta máquina ni su red), sin cookies ni referrer, de un tipo de imagen conocido, hasta 5 MB y sin redirecciones |
| `triage.rs` | Las reglas: remitente que el dueño marcó como interesante primero; verificación del remitente fallida, estafa de un remitente automático, aviso de envío y boletín quedan **en espera**; un recibo o una factura es rutina aunque lo mande un robot. En espera nada se borra: espera la decisión del dueño |
| `search.rs` | La búsqueda de solo lectura de una cuenta con las palabras del buscador de Gmail (`from:`, `subject:`, `after:`, `before:`, `{…}`): en Gmail va tal cual (`X-GM-RAW`, sobre «Todos»); en otro servidor se convierte en criterios IMAP y lo que no se puede preguntar (palabras con tildes, el segundo exacto) se comprueba aquí. Nada se marca como leído |
| `service.rs` | `Mail`: lectura de las cuentas en paralelo (el Inbox primero; las carpetas especiales se buscan una vez), acciones, borradores y la búsqueda (`Mail::search`); recuerda qué mensaje del servidor es cada id de la última lectura, la restaura de lo que el núcleo guardó (`restore`) y suma lo que llega (`arrived`) |
| `watch.rs` | La escucha de cada bandeja de entrada: una conexión `IDLE` por cuenta que dice qué llegó en cuanto el servidor lo avisa, de solo lectura (`EXAMINE`), con espera creciente si se corta |
| `model.rs` | Lo que recibe la página (`apps/desktop/src/features/mail/model/types.ts`) |

Los datos que Echo guarda (qué cuentas están conectadas, qué remitentes interesan y en qué mensajes se permitieron las imágenes) son del núcleo, no de este crate. Este crate no escribe mensajes a disco: lo que se reutiliza (la última lectura, los mensajes abiertos, las imágenes) vive en `cache.rs`, en memoria, y se pierde al cerrar la app. El núcleo guarda aparte un resumen ligero de la última lectura (remitente, asunto, vista previa, fecha, estado y las primeras líneas del texto; nunca el HTML ni las imágenes) para que Correo se pinte al abrir; se borra al quitar la cuenta. Abrir un mensaje o cambiar de ventana no vuelve a conectarse al servidor mientras la memoria lo tenga.

## Cómo se une al resto

Solo hay un punto de unión: `crates/core/src/surfaces/mail.rs`, con los comandos `mail_overview`, `mail_add_account`, `mail_remove_account`, `mail_act`, `mail_open`, `mail_image`, `mail_draft`, `mail_check` (qué tan seguro parece un correo, por reglas de `echo_rules::mailbrief`) y `mail_summary` (su resumen, que escribe el modelo más barato al pulsar «Resumir»; ver [el documento](../../docs/on%20board/10-27-2026-correo-resumen-y-verificacion.md)), los hábitos de correos recurrentes (`mail_habits`, `mail_habit_decide`, `mail_habit_remove` y `mail_reply_suggestion`, con las reglas de `echo_rules::mailhabits`; lo aprendido es el ajuste `email.habits` y se borra con la cuenta), y los de Ajustes › Finanzas `finance_correo_accounts` y `finance_set_correo` (qué cuentas puede leer el agente de finanzas, que las busca con `Mail::search`). La página que los usa es `apps/desktop/src/features/mail`.

## Cómo quitarlo

Es un crate aparte para poder quitarlo sin rastro:

1. Borrar la carpeta `crates/email`.
2. En la raíz: quitar `"crates/email"` de `members` y `echo-email` de `[workspace.dependencies]` en `Cargo.toml`.
3. En `crates/core`: quitar `echo-email.workspace = true` de `Cargo.toml`, `pub mod mail;` de `src/lib.rs`, los comandos `mail_*`, `finance_correo_accounts` y `finance_set_correo` de `src/door.rs`, borrar `src/surfaces/mail.rs` y, en `src/integrations/finance.rs`, las dos llamadas a `surfaces::mail` (`inboxes` y `LiveInboxes::find`: las cuentas de Correo dejan de ser bandejas de finanzas; las vigiladas siguen).
4. En `assets/page-scopes.json`: quitar `"mail_*"` de la ventana `code`.
5. Opcional: la página (`apps/desktop/src/features/mail`) y su fila «Correos» en la barra lateral de Code; sin los comandos muestra «Aún no hay cuentas conectadas».
6. Las contraseñas que guardó siguen en el almacén del sistema con el nombre `email-imap-…`: se quitan desde la página con «Quitar» antes de borrar el crate, o a mano desde Credential Manager / Keychain.

## Lo que no hace (todavía)

- Outlook y Microsoft 365: bloquean la contraseña de aplicación; harían falta OAuth y Microsoft Graph.
- OAuth de Google y la API de Gmail (hilos y etiquetas completos): hoy se usan las etiquetas que Gmail da por IMAP.
- Enviar por su cuenta y borrar. Una respuesta sale solo cuando el dueño pulsa «Enviar» (`smtp.rs`), al remitente del mensaje; un borrador se guarda en la carpeta de borradores de la cuenta.
- Avisos en el notch y herramientas para los agentes.
- Hojas de estilo (`<style>`) de un correo: no se conservan; sí los estilos en línea, que es lo que usan la mayoría de los correos de empresas.
- Imágenes de `http://` y fondos con `url(…)`: no se traen. Una imagen de la web solo llega si el dueño la pide, y su servidor ve la dirección de este equipo.
- Correos de más de 25 MB: no se abren entero (su texto sigue en la lista).
- Guardar en disco el HTML, las imágenes o el texto entero de los mensajes: no se hace; solo el resumen ligero del núcleo (véase arriba).

```bash
cargo test -p echo-email
```
