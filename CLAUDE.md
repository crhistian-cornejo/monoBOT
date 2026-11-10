# monoBOT

App de escritorio nativa en Rust con GPUI, más una app móvil Expo. Ya no hay React ni Tauri.

| Carpeta | Qué hay |
| --- | --- |
| `crates/monocode-gpui/` | App de escritorio (interfaz GPUI). Miembro del workspace de Cargo de la raíz (`Cargo.toml`, `Cargo.lock`, salida en `target/`). |
| `crates/monocode-core/` | Lógica sin interfaz: proveedores, persistencia, Git/archivos, bots, correo, enlace móvil. No depende de GPUI. |
| `crates/{mobile-link,persona,email,http,dictation,voice,visual}` | Crates compartidos que enlaza el core o la app. `persona` dibuja las caras 3D de los bots en el escritorio y en el móvil. |
| `mobile/` | App Expo (iOS/Android). Proyecto pnpm propio con su lockfile; tiene su propio `AGENTS.md`. |
| `host/` | Host remoto en Node (`monocode-host`) que instala la app por SSH. |
| `deploy/cloudflare-mail/` | Worker opcional de correo entrante en Rust → WebAssembly (Cloudflare Email Routing → R2 + D1). Crate aparte, no está en el workspace. |
| `vendor/gpui-pre*/` | GPUI fijado y parcheado. No lo actualices sin leer `docs/done/architecture.md`. |
| `xtask/` | Herramientas del escritorio en Rust (`cargo xtask`): runner de desarrollo, build, `.app`, instaladores, firma y publicación de actualizaciones. |
| `scripts/` | Solo lo que no puede ser Rust: `generators/` (fuentes JS de los bundles que la app ejecuta en QuickJS) y `perf/` (el `claude` simulado para perfilar). |

## Comandos

```sh
cargo xtask dev             # app en desarrollo con datos aislados; r + Enter recompila
cargo xtask build           # build optimizado (y .app en macOS)
cargo xtask test            # tests de core y GPUI
cargo xtask check           # fmt + clippy -D warnings + tests de xtask
cargo xtask cargo test -p monocode-gpui --profile native-dev <filtro>
cargo xtask help            # el resto: package, release, bump-version, icons, ssh-smoke, perf
```

El escritorio no necesita Node ni Bun. Antes de hacer commit: `cargo fmt --all` y los tests de lo que tocaste con `cargo xtask cargo test … <filtro>` (si tocaste `xtask/`, `cargo test -p xtask`). Para el móvil, `pnpm --dir mobile exec tsc --noEmit` y `pnpm --dir mobile lint`. Para el host, `pnpm --dir host test` (incluye `tsc --noEmit`). Para el Worker de correo, `cargo test` dentro de `deploy/cloudflare-mail/`.

## Código

- La lógica va en `monocode-core`, sin GPUI. La app GPUI solo pinta y conecta. Nada de E/S, parseo ni disco en el hilo de la interfaz.
- Los tests van junto al código (`#[cfg(test)] mod tests` o `<módulo>/tests.rs`). Los tests de interfaz usan el banco de pruebas de GPUI con eventos reales, no copias de la máquina de estados.
- Los bundles `crates/monocode-gpui/assets/native-editor.js` y `settings-arcade.js` son generados. Su fuente está en `scripts/generators/native-editor/` y `scripts/generators/native-settings/`: cambia la fuente y regenera, no edites el bundle. Los iconos (`hugeicons/`, `material-icons.json`) salen de `cargo xtask icons`. El logo y todos los iconos de la app (escritorio y móvil) salen de `assets/logo/monobot.svg` con `cargo xtask logo`: cambia el SVG y regenera, no edites los PNG, el `.ico` ni el `.icns`. El icono Liquid Glass de macOS 26 lo compila `cargo xtask build` en un Mac con Xcode, desde `assets/macos/AppIcon.icon`.
- Las herramientas nuevas del escritorio van en `xtask/` (Rust, con tests junto al código), no en scripts sueltos. Llama a programas externos (codesign, hdiutil, rpmbuild…) solo cuando no hay otra forma.

## Interfaz: respeta lo que ya existe

- Antes de crear interfaz nueva, busca un componente que ya lo haga y reutilízalo: `theme.rs` (botones de icono, tooltips, `section_label`, `primary_action`, rellenos), `modal.rs`, `popover.rs`, `pickers.rs`, `settings/widgets.rs`. Si dos zonas necesitan lo mismo, súbelo a un módulo compartido. No lo dupliques.
- Los colores salen de `theme.rs` y de la paleta activa (`settings/palettes.rs`): `content(alpha)`, `background_base()`, `pane_fill()`, `accent_color()`, `text_selection()`… Nada de `rgb(0x…)` sueltos en las vistas. El acento sigue el color que el usuario elige en Ajustes y el tema activo.
- El movimiento respeta el movimiento reducido del sistema (`cx.reduce_motion()`: dibuja el estado final). No añadas animación a interacciones muy frecuentes.
- Las capturas de otros productos (Factory, Claude, etc.) muestran qué hacer, no cómo debe verse: adapta la idea al lenguaje visual de la app.

## Nombres y datos

- Lo que ve el usuario dice monoBOT.
- Los identificadores internos se quedan como están, porque cambiarlos borraría los datos del usuario: crates `monocode-*`, `com.monocode.desktop` (bundle y llavero), `MONOCODE_*` en variables de entorno y `~/.monocode-host`.
- Por lo mismo siguen escritos `MonoBOT` los nombres que ya están en disco o que leen otros programas: la carpeta `Documentos/MonoBOT`, los datos de desarrollo `MonoBOT Development`, los instaladores `MonoBOT_<versión>_…`, el `User-Agent`, el autor y el mensaje de los commits de checkpoint, la tarea de Windows `MonoBOT Host-<SID>` y el nombre de la app móvil en `mobile/app.json` (cambiarlo cambia la versión de runtime de las actualizaciones).
- Los secretos (tokens de correo, Mailflare, Cloudflare) van al llavero del sistema, nunca a archivos, argumentos de proceso ni al chat.

## Documentación

La documentación está en `docs/`, ordenada por estado (índice en `docs/README.md`): `docs/done/` describe lo que ya funciona (empieza por `docs/done/desktop.md` y `docs/done/architecture.md`), `docs/in-progress/` lo que está en marcha o pendiente (`tauri-gap-audit.md`, `port-plan.md`, `limitations.md`, `mail-workspace.md`) y `docs/old/` el historial. Al terminar algo, mueve o actualiza su documento.
