# Lo que hace Claude Code (desktop) y monoBOT todavía no

Revisión del 2026-11-18. Se analizó en estático y en solo lectura la app de
escritorio de Claude, versión 5.11492.5, en su pestaña Code:

- el renderer está en `Contents/Resources/ion-dist.asar`;
- el proceso principal está en `app.asar` (`.vite/build/*.js`);
- el simulador lo maneja el helper `Contents/Helpers/Claude iOS Sim.app` (`claude-ios-sim`).

El análisis usó REA (`analyze_javascript_application`, evidencia `ev_bc8e9fc9…`)
y una búsqueda de texto en los asar extraídos. No se ejecutó nada ni se copió
código. Lo que ya cuenta [ios-simulator.md](ios-simulator.md) no se repite
aquí: el formato del helper, Indigo/DTUHID, Seatbelt e IOSurface.

## El simulador de iOS dentro del chat

- **Dónde va.** Es una baldosa más del `tileLayout` de la sesión, junto al
  chat. Hay una por dispositivo y puede haber varias. También sirve en una
  ventana aparte, si esa ventana admite vistas nativas. En el transcript, cada
  acción del agente sobre el dispositivo es una fila («Launched an app»,
  «Long-pressed»…) con su captura. Hay dos flags, `simulatorMiniView` y
  `simulatorScreenshotFlight`, que parecen una vista flotante pequeña y una
  captura que vuela al chat.
- **Cómo llegan los fotogramas.**
  1. El renderer deja un rectángulo vacío y lo mide sin cesar (ResizeObserver
     y un intervalo de 500 ms).
  2. Con `nativeViewMount`, `nativeViewSetBounds`, `nativeViewSetVisible` y
     `nativeViewUnmount` coloca una NSView nativa justo encima de ese hueco.
  3. Si no puede, cae a H.264 sobre un canvas, con FPS y fotogramas perdidos
     medidos.
- **Equivalente en monoBOT.** La pestaña Simulator del panel derecho dibuja
  JPEG a unos 3–4 fps. El camino de Claude encaja con GPUI: un hueco medido y
  una vista nativa encima, como ya hacen los demás embebidos de macOS en la
  app.
- **Chasis y controles.** Bisel con botones de hardware y sus atajos:
  - Home ⌘⇧H, girar ⌘→, apariencia ⌘⇧A, teclado ⌘K;
  - captura ⌘S, grabar vídeo ⌘R;
  - zoom para encajar ⌘0 y a tamaño real ⌘1.

  Hay anillos donde tocó el agente y rótulos de actividad («Claude is using
  this device»). Además: portapapeles en ambos sentidos, medios añadidos con
  `simctl addmedia`, status bar a las 9:41, animaciones lentas y agitar.
- **Inspector → chat.** Se elige un elemento de la pantalla, se puede pasar al
  de detrás o al de delante, se escribe una nota y «Add to chat» lo adjunta con
  su marco, su etiqueta y un recorte.
- **Historial de acciones.** Cada toque, texto o lanzamiento del agente, con
  la pantalla de después y «Again · N actions» para repetirlas.
- **Logs.** Un cajón bajo el dispositivo con `log stream`, filtros por nivel
  y alcance y «Go to crash».
- **Arranque.** Primero pide consentimiento («Let Claude use this
  simulator?»). Luego revisa Xcode paso a paso, cada paso con su arreglo.
  Lanza la app con `simctl launch --console-pty`, que alimenta el cajón de logs.

## Diez cosas que monoBOT no tiene, por valor

1. **Inspector del simulador → chat.** Elegir un elemento, añadir una nota y
   adjuntarlo con su recorte.
2. **Historial de acciones del simulador**, con la miniatura de cada paso y
   «repetir N».
3. **Simulador en vivo con vista nativa** sobre un hueco medido, con H.264
   como reserva, los botones del bisel y sus atajos.
4. **Cajón de logs** bajo el dispositivo, desde el lanzamiento.
5. **Caja de «siguiente paso»** bajo cada turno terminado: un resumen, una
   decisión y uno o dos botones que la responden.
6. **Lista de tareas fijada** bajo el último mensaje: el plan vivo del agente,
   que se va marcando.
7. **Capítulos**: «Pin as chapter», un separador en el transcript y un índice
   flotante.
8. **Tarjetas de sesión aparte**: el agente propone una tarea relacionada
   («Do it here» o «Start session»). Un side chat se puede separar a sesión
   propia; monoBOT tiene `btw.rs`, pero no ofrece ni separa.
9. **Ventanas sueltas** para una sesión o un panel, con el compositor puenteado
   a la ventana principal.
10. **Tres vistas del transcript**:
    - Normal, con las herramientas agrupadas («Read 5 files · Ran 3 commands»);
    - Thinking;
    - Verbose.

Después vienen:

- un modo Bypass que espera a que acabe el turno para activarse;
- un embellecedor de capturas (marco, inclinación y fondo);
- secciones de la barra lateral con su propio agrupado y orden;
- una vista en rejilla de varias sesiones a la vez.
