# Investigar en la web: qué tienen los agentes

> Las rutas `src/` y `src-tauri/` de este documento son de la app Tauri, que ya no está en el repositorio (se conserva en la etiqueta local de git `archive/tauri-app`). Lo que falta portar está en [tauri-gap-audit.md](native/tauri-gap-audit.md).

Revisado el 5 de noviembre de 2026, comparando con Echo (`D:\DevPrograms\Echo`).

## Qué pueden hacer hoy

| Necesidad | Cómo | Dónde |
|---|---|---|
| Buscar en la web | La búsqueda propia de cada CLI: `WebSearch` en Claude, la búsqueda de OpenAI en Codex (`cached` por defecto), `search_web` en Gemini/Antigravity. Exa si el usuario la activa en Ajustes → Conectores | Proveedores en `src/integrations/harness/providers` |
| Leer una página con sus datos | `monobot app web.read {url, query?, maxChars?}`: texto principal en Markdown, título, sitio, autor, fechas, imagen, icono, índice, enlaces, DOI y PDF de una página académica | `src-tauri/src/web.rs` (descarga segura) y `src/features/web/model/webPage.ts` (lectura) |
| Descargar archivos y PDFs | `monobot app web.download {url, name?}`: guarda en `Documentos/MonoBOT/<agente>/`. Un PDF, o la página de un artículo que lleva a uno, también se lee como paper (`ref`, texto, índice) | `web.rs`, manejador `web.*` en `src/features/agent-app/model/agentApp.ts` |
| Buscar papers | `papers.find` consulta a la vez OpenAlex (o Crossref si falla), Semantic Scholar y arXiv; intercala resultados y quita duplicados | `src/features/papers/model/papers.ts` |
| Leer, citar y dibujar papers | `papers.read`, `papers.figure`, `papers.add` | `papers.ts`, `paperStore.ts`, `src-tauri/src/papers.rs` |

Reglas comunes: solo direcciones públicas (cada redirección se revisa; nada de `localhost` ni redes privadas), límites de tamaño y tiempo, y lo que dice una página es dato, nunca instrucción. Los sitios que bloquean programas se informan; nunca se intenta saltarlos.

Otros cambios de esta ronda:

- En modo plan, Claude ya puede usar `WebFetch` (antes se denegaba).
- El bot Research recibe Exa y Playwright cuando el usuario los activa.
- La skill `research` enseña `web.read` y `web.download`.

## Qué tiene Echo y nosotros no (pendiente)

1. **Navegador del agente** (`crates/core/src/code/browser.rs` en Echo): abrir, leer como esquema accesible con referencias numeradas, hacer clic, escribir (nunca en campos de contraseña o tarjeta), capturas y consola, con cursor visible. Nosotros solo tenemos el conector Playwright de solo lectura.
2. **Iconos y vistas previas en disco**, con protección SSRF y caché negativa de 7 días (`integrations/site_icons.rs`). Los nuestros viven en memoria y se pierden al reiniciar.
3. **Fuentes de todos los proveedores en un solo formato**, para mostrarlas igual vengan de Claude, Codex o Gemini (`providers/mod.rs` `found_pages`).
4. **Cuarentena de memoria**: un turno que leyó la web no escribe memoria ni cambia sus propias instrucciones sin el usuario (`agents/memory.rs`).
5. **Claves de API para conectores** guardadas en el llavero del sistema. Hoy no se puede configurar ninguna clave, así que no hay Brave, Tavily ni la API de Google con clave propia.

Google Académico no tiene API y bloquea a los programas: se cubre con OpenAlex, Semantic Scholar y arXiv.
