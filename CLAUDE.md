# MonoBOT

App de escritorio nativa en Rust con GPUI, más una app móvil Expo. Ya no hay React ni Tauri.

| Carpeta | Qué hay |
| --- | --- |
| `crates/monocode-gpui/` | App de escritorio (interfaz GPUI). Miembro del workspace de Cargo de la raíz (`Cargo.toml`, `Cargo.lock`, salida en `target/`). |
| `crates/monocode-core/` | Lógica sin interfaz: proveedores, persistencia, Git/archivos, bots, correo, enlace móvil. No depende de GPUI. |
| `crates/{mobile-link,email,http,dictation,voice,visual}` | Crates compartidos que enlaza el core o la app. |
| `mobile/` | App Expo (iOS/Android). Proyecto pnpm propio con su lockfile; tiene su propio `AGENTS.md`. |
| `host/` | Host remoto en Node (`monocode-host`) que instala la app por SSH. |
| `deploy/cloudflare-mail/` | Worker opcional de correo entrante (Cloudflare Email Routing → R2 + D1). |
| `vendor/gpui-pre*/` | GPUI fijado y parcheado. No lo actualices sin leer `docs/native/migration.md`. |
| `scripts/` | Runner de desarrollo, empaquetado, publicación y generadores de JS embebido. |

## Comandos

```sh
bun run dev                 # app en desarrollo con datos aislados; r + Enter recompila
bun run build               # build optimizado (y .app en macOS)
bun run test                # tests de core y GPUI
bun run check               # tests de scripts + fmt + clippy -D warnings
node scripts/native-cargo.mjs test -p monocode-gpui --profile native-dev <filtro>
```

Antes de hacer commit: `cargo fmt --manifest-path crates/monocode-gpui/Cargo.toml --all` y los tests de lo que tocaste con `node scripts/native-cargo.mjs test … <filtro>`. Para el móvil, `pnpm exec tsc --noEmit` y `pnpm lint` dentro de `mobile/`.

## Código

- La lógica va en `monocode-core`, sin GPUI. La app GPUI solo pinta y conecta. Nada de E/S, parseo ni disco en el hilo de la interfaz.
- Los tests van junto al código (`#[cfg(test)] mod tests` o `<módulo>/tests.rs`). Los tests de interfaz usan el banco de pruebas de GPUI con eventos reales, no copias de la máquina de estados.
- Los bundles `crates/monocode-gpui/assets/native-editor.js` y `settings-arcade.js` son generados. Su fuente está en `scripts/native-editor/` y `scripts/native-settings/`: cambia la fuente y regenera, no edites el bundle.

## Interfaz: respeta lo que ya existe

- Antes de crear interfaz nueva, busca un componente que ya lo haga y reutilízalo: `theme.rs` (botones de icono, tooltips, `section_label`, `primary_action`, rellenos), `modal.rs`, `popover.rs`, `pickers.rs`, `settings/widgets.rs`. Si dos zonas necesitan lo mismo, súbelo a un módulo compartido. No lo dupliques.
- Los colores salen de `theme.rs` y de la paleta activa (`settings/palettes.rs`): `content(alpha)`, `background_base()`, `pane_fill()`, `accent_color()`, `text_selection()`… Nada de `rgb(0x…)` sueltos en las vistas. El acento sigue el color que el usuario elige en Ajustes y el tema activo.
- El movimiento respeta el movimiento reducido del sistema (`cx.reduce_motion()`: dibuja el estado final). No añadas animación a interacciones muy frecuentes.
- Las capturas de otros productos (Factory, Claude, etc.) muestran qué hacer, no cómo debe verse: adapta la idea al lenguaje visual de la app.

## Nombres y datos

- Lo que ve el usuario dice MonoBOT.
- Los identificadores internos se quedan como están, porque cambiarlos borraría los datos del usuario: crates `monocode-*`, `com.monocode.desktop` (bundle y llavero), `MONOCODE_*` en variables de entorno y `~/.monocode-host`.
- Los secretos (tokens de correo, Mailflare, Cloudflare) van al llavero del sistema, nunca a archivos, argumentos de proceso ni al chat.

## Documentación

Empieza por `docs/native/README.md`. El estado y lo pendiente están en `docs/native/parity.md`, `docs/native/monobot-port.md` y `docs/native/tauri-gap-audit.md`. El correo y Cloudflare, en `docs/native/mail-workspace.md`.
