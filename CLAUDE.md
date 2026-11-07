# MonoBOT

App de escritorio Tauri 2: React en `src/`, Rust de la app en `src-tauri/src`, crates reutilizables en `crates/`. Gestor de paquetes: pnpm.

## UI: respeta lo que ya existe

- Antes de crear UI nueva, busca en el repo un componente o patrón que ya lo haga y úsalo: `src/shared/ui` (Modal, SecondaryButton, SearchableSelect, Popover, PaneHeader, BetaBadge…) y los diálogos que ya existen (por ejemplo `src/features/source-control/ui/CreateWorktreeDialog.tsx`).
- Estila con Tailwind y los tokens de `src/styles/index.css`: `content`, `background-base`, `selection`, `accent`… Nada de colores fijos (`#hex`, `orange-500`, `text-white` sobre un color propio). El acento es `accent`: sigue el color que el usuario elige en Ajustes y el tema activo.
- Botón principal: clase `primary-action`. Botón secundario: `SecondaryButton`, o el botón plano de los diálogos (`rounded-md px-3 py-1.5 text-[12px] hover:bg-content/8`).
- El cuerpo de un `Modal` no trae padding: ponlo tú (`p-4`, o `px-5 py-4` en diálogos grandes). Etiquetas `text-[12px] text-content/70`; campos `h-9 rounded-md border border-content/10 bg-background-base px-2.5 text-[13px] outline-none focus:border-content/25`.
- Las capturas de otros productos (Factory, Claude, etc.) muestran qué hacer, no cómo debe verse: adapta la idea al lenguaje visual del repo y no copies su estilo.
- No dupliques UI: si dos features necesitan lo mismo, reutiliza el componente de la otra o súbelo a `src/shared/ui`.

## Código

- Cada feature vive en `src/features/<feature>/{model,ui}`; la lógica va en `model`, sin React.
- Los tests van junto al archivo (`*.test.ts`, vitest).
- Antes de hacer commit: `pnpm exec tsc --noEmit` y `pnpm exec vitest run <ruta tocada>`.
- Los nombres visibles dicen MonoBOT. Los identificadores internos (`monocode.*` en localStorage, `com.monocode.desktop`, nombres de crates) se quedan como están, porque cambiarlos borraría los datos del usuario.
