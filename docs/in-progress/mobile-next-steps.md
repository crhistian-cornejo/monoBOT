# Mobile app: next steps

Estado de la app Expo en `mobile/` y lo que falta para que se parezca al
escritorio en agentes personales (monos) y grupos. Pendiente, no terminado.

## Estado actual

- `pnpm ios` compila y arranca en el simulador iPhone 18 Pro (iOS 27.x).
  Muestra la pantalla de inicio con «Connect to desktop».
- Emparejamiento con el escritorio: hecho en un equipo de desarrollo.
- Ya existe: lista de monos, chat de mono, perfil del mono (documentos,
  archivo, modelo, proyectos, hábitos, subsesiones), crear, editar, reiniciar
  y borrar monos, y leer/guardar SOUL.md y MEMORY.md.
- Caras como en el escritorio: los bots llevan su persona 3D, que el móvil
  dibuja con el mismo crate (`crates/persona`, vía `crates/mobile-link`), y
  las mascotas se mueven en el hilo de la interfaz. Ver «Faces» en
  `docs/done/desktop.md`.
- Métodos que la app puede pedir al escritorio: `APP_METHODS` en
  `crates/monocode-core/src/mobile/host.rs`.

## Para arrancar en local

```bash
cd mobile
export LANG=en_US.UTF-8 LC_ALL=en_US.UTF-8   # CocoaPods falla sin UTF-8
pnpm install
pnpm ios
```

- Sin `LANG` UTF-8, `pod install` falla con `Encoding::CompatibilityError`.
- `ios/` se genera al compilar y está en `.gitignore`. No se edita a mano.
- Android: pendiente. Hacen falta Android Studio, un NDK y `ANDROID_NDK_HOME`.

## Próximos pasos, por orden

1. **Grupos en el catálogo.** Publicar los grupos en `workspace.list` con
   miembros y objetivo, y un flag `groupTools`, igual que `monoTools`.
   Fuente: `crates/monocode-core/src/mono/groups/group.rs`.
2. **Lectura del chat de grupo.** Reutilizar la conversación y el `watch` que
   ya usan los monos.
3. **Enviar al grupo y responder peticiones de acceso** (`group.access`).
   Antes confirmar cómo el escritorio enruta un mensaje de grupo
   (`crates/monocode-gpui/src/monos/groups.rs`). Añadir los métodos a
   `APP_METHODS`.
4. **Crear y editar grupos** (miembros, objetivo, carpetas). Referencia:
   `crates/monocode-gpui/src/monos/group_dialog.rs`.
5. **Principal y roles.** El Principal es el mono con `bot_role = "principal"`
   y va fijado arriba. Los 13 roles están en
   `crates/monocode-core/src/mono/groups/roster.rs`.
6. **Más adelante:** llamadas de grupo (`monos/group_call.rs`), scheduler,
   workstreams, memoria, noticias, investigación, papers y correo.

## Reglas del proyecto que aplican

- La lógica va en `monocode-core`. La UI del escritorio solo conecta.
- Los métodos nuevos del móvil se añaden a la lista de permitidos, no como
  comandos genéricos. Ver el comentario de `host.rs`.
- El móvil no guarda estado propio de los datos del escritorio.
- Antes de cualquier cambio en Expo, leer la documentación de la versión
  major de `expo` (ver `mobile/AGENTS.md`).
