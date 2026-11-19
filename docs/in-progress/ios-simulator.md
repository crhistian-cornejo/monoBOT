# Simulador iOS dentro de monoBOT

Objetivo: que el agente cree, compile, lance y pruebe apps de iOS desde monoBOT,
con la pantalla del simulador en vivo dentro de la app (o en el Simulator.app
nativo), capturas, toques y lectura de la interfaz, como lo hace la pestaña Code
de la app de escritorio de Claude.

## Estado: fases 1, 2 y 3 hechas (10 nov 2026); panel nativo, bisel, historial, logs e inspector (18 nov 2026)

Lo del 18 de noviembre está en [El panel del simulador](#el-panel-del-simulador-18-nov-2026).
Lo del 19, en [Solo en proyectos iOS, y más rápido](#solo-en-proyectos-ios-y-más-rápido-19-nov-2026).

El conector **iOS Simulator** (`ios_simulator`, solo macOS, experimental, apagado
por defecto) se enciende en Ajustes › MCP. Con él, cada sesión de Claude, Codex o
ACP recibe un servidor MCP que es el propio ejecutable de monoBOT
(`monocode-gpui ios-simulator-mcp`, en `monocode-core::simulator`). No usa
Node, Poki ni APIs privadas: `xcrun simctl`, `xcodebuild` y, para tocar y leer la
interfaz, un runner de XCUITest que monoBOT compila en el Mac del usuario.

| Herramienta | Qué hace |
| --- | --- |
| `status` | Si Xcode está listo (y si no, el arreglo exacto), los simuladores iOS (los arrancados primero) y cuál se usa por defecto. |
| `boot` | Arranca un simulador, espera a `bootstatus` y lo muestra en Simulator.app (`open_simulator_app: false` lo deja sin ventana). |
| `find_projects`, `schemes` | Los `.xcworkspace`/`.xcodeproj` de una carpeta (sin entrar en `node_modules`, `Pods` ni en los propios bundles) y sus esquemas. |
| `build`, `build_status` | `xcodebuild build` en segundo plano, con DerivedData propio por proyecto en `~/Library/Caches/com.monocode.desktop/simulator-builds/`. `build_status` espera hasta 60 s y devuelve errores, avisos, log y, si sale bien, el `.app` y su bundle id (de `-showBuildSettings -json`). |
| `launch` | Instala el `.app` y lo abre (cierra la copia que ya estaba abierta), o reabre un bundle id. Arranca el dispositivo si hace falta. |
| `screenshot` | La pantalla como JPEG **al tamaño en puntos** (402×874 en un iPhone 18 Pro, con la escala de `simctl getenv SIMULATOR_MAINSCREEN_SCALE`), así que un sitio de la imagen es el punto que se toca; 1280 px como máximo. `save_to` guarda el PNG completo. |
| `logs` | `log show` del dispositivo de los últimos segundos, para un proceso si se nombra. |
| `open_url`, `appearance` | Enlaces profundos y modo claro u oscuro. |
| `describe_ui` | El árbol de accesibilidad en líneas cortas: tipo, etiqueta, identificador, valor, estado y el punto del centro de cada elemento. Omite lo que está fuera de pantalla (el teclado oculto) y los contenedores sin texto. |
| `tap` | Un punto (x, y en puntos) o el elemento con esa etiqueta o identificador; `duration` mantiene pulsado, `double` toca dos veces. |
| `swipe`, `type_text`, `press_key` | Arrastrar un dedo, escribir en el campo con el foco, y teclas (return, delete, flechas…; con modificadores, si hay teclado físico). |
| `press_button`, `rotate` | El botón de inicio y la orientación. |

### El runner de XCUITest (fase 2)

El plan era usar Indigo/DTUHID como el helper de Claude, pero el propio helper
de Claude dice que Indigo ya no funciona bien desde Xcode 27, y DTUHID es privado
y no está documentado. En su lugar, la entrada va por **XCUITest**, como en
Maestro o WebDriverAgent:

- El código está en `crates/monocode-core/src/simulator/runner/`: una app
  anfitriona vacía y un test, `RunnerUITests.testServe`. El proyecto se genera con
  `xcodegen generate` a partir de `project.yml`, y `simulator/input.rs` lo
  incrusta con `include_str!`.
- **Compilación**: la primera vez, `xcodebuild build-for-testing` en
  `~/Library/Caches/com.monocode.desktop/simulator-runner/builds/<hash>/` (unos
  40 s). El hash cubre el código y la versión de Xcode. Si cambia cualquiera de
  los dos, se recompila y se borran las versiones anteriores. Si otra sesión
  está compilando, espera a esa compilación.
- **Arranque**: `xcodebuild test-without-building -xctestrun … -destination
  id=<udid>` en su propio grupo de procesos (unos 15 s). Recibe el puerto, el
  token y el tiempo de inactividad por `TEST_RUNNER_MONOBOT_RUNNER_*`.
- **Servidor**: HTTP mínimo solo en loopback (`NWListener`). Cada acción es un
  `POST`, con el token en `X-Monobot-Token`, y se ejecuta en el hilo principal,
  como exige XCUITest.
- **Arranca en su propia sesión** (`setsid`). En un grupo de procesos propio
  moría con el proceso que lo había arrancado.
- **Un runner por dispositivo** sirve a todas las sesiones. El puerto y el token
  quedan en `simulator-runner/devices/<udid>.json` (modo 600). Se cierra solo
  tras 30 min sin peticiones o cuando el simulador se apaga. Si ya no responde,
  la siguiente llamada arranca otro.
- **Coordenadas**: los toques usan la coordenada de SpringBoard, es decir, la
  pantalla completa. Por eso funcionan sea cual sea la app que está delante.
  Escribir, las teclas y `describe_ui` se dirigen a la última app abierta con
  `launch`, o a la que nombre `bundle_id`.
- Escribir, las teclas y `describe_ui` usan la app nombrada; si no se nombra,
  la última abierta con `launch`, y la pantalla de inicio si esa ya no está
  delante. Nombrar una app que no está delante falla en el acto, porque
  consultarla bloquearía XCUITest.
- Medido en un iPhone 18 Pro con iOS 27: un toque, 0,8 s; tocar un elemento por
  nombre, unos 2 s; una tecla, 0,7 s; escribir 5 letras, 1,6 s; `describe_ui`,
  0,3 s. Lo primero en una sesión tarda unos 25 s si el runner ya está
  compilado, o algo más de un minuto si hay que compilarlo.
- El estado guarda qué compilación sirve. Un runner de un código o un Xcode
  anteriores se cierra y se sustituye en la siguiente llamada.

Las herramientas no piden permiso (actúan solo sobre el simulador y la carpeta
de builds de monoBOT), así que la sesión conserva su modo de permisos.
**Pendiente:**
- `build` ejecuta las fases de script del proyecto, como haría el agente con
  Bash. Una sesión de solo lectura o de plan todavía puede llamarla.
- No hay forma pública de saber qué app está delante. Si el usuario abre otra
  app a mano, el agente tiene que pasar su `bundle_id`.
- En horizontal, SpringBoard sigue dando el marco en vertical. Hay que comprobar
  cómo se traducen ahí los toques por coordenada.

## Cómo lo hace Claude (observado en Claude.app, 9 nov 2026)

Lo de abajo sale de inspeccionar el bundle instalado (`otool`, `strings`, el JS de
`app.asar`). No se decompiló código ni se observó en ejecución; donde algo es
inferencia, se dice.

### Tres piezas

1. **Un helper nativo aparte**: `Claude.app/Contents/Helpers/Claude iOS Sim.app`
   (`com.anthropic.claude.ios-sim`, Swift, `LSBackgroundOnly`, macOS 15+).
   - Lleva dentro `FBSimulatorControl`, `FBSimulatorAX` y `FBSimulatorVideo` de
     **idb de Meta** (licencia MIT, incluida como `LICENSE-facebook-idb.txt`).
   - Enlaza con *weak* `CoreSimulator.framework` (privado, en
     `/Library/Developer/PrivateFrameworks`) y
     `AccessibilityPlatformTranslation.framework` (privado). Abre
     `SimulatorKit.framework` en tiempo de ejecución desde el Xcode activo.
   - Usa IOSurface, Metal, VideoToolbox y AVFoundation para la imagen.
   - `Resources/SimulatorFrameworkBridge-iOS`: un binario **para el simulador**
     (plataforma 7) que se ejecuta dentro del dispositivo. Enlaza UIKit, Photos,
     Contacts y UserNotifications. Por inferencia: añade fotos y contactos,
     envía notificaciones, concede permisos y lee la interfaz desde dentro.
   - Corre fuera del proceso de la app, con el envoltorio `disclaimer` (para que
     no herede los permisos TCC de la app) y bajo un perfil Seatbelt que niega
     todo por defecto: sin red, y sin leer los archivos del usuario más allá del
     estado de CoreSimulator. El perfil va dentro del JS.

2. **El proceso principal de Electron** (`.vite/build/index.chunk-*.js`) arranca el
   helper con `spawn(..., {stdio: ["pipe","pipe","pipe"]})` y le habla por
   stdin/stdout.
   - Respuestas en tramas: `u32 LE longitud` + `u8 etiqueta` + cuerpo. Etiquetas:
     `J` (74) respuesta JSON de hasta 1 MiB, `B` (66) respuesta binaria (como las
     capturas), `F` (70) fotograma de un stream (`u32 streamId` + datos), y otras
     de fin de stream o de estado.
   - Si una trama supera su tope, mata el helper («desync»).
   - El formato exacto de las peticiones no se confirmó.
   - Operaciones: `listDevices`, `describeDevice` (nombre, estado, versión, ancho
     y alto en puntos), `screenshot`, `tap`, `swipe`, `touch`, `touchPath`,
     `touch2Path`, `button`, `key`, `text`, `rotate`, `setHardwareKeyboard`,
     `uiAvailable`, `uiDescribeAll`, `uiDescribeApplication`, `uiDescribePoint`,
     `uiDescribe`, `uiSetValue`, `startVideo` (H.264, fps, escala, «solo
     cambios»), `stopVideo`, `startSurfaceShare`, `stopSurfaceShare`,
     `startScreenFeeds`, `startScreenRecording`, `stopScreenRecording`,
     `interfaceOrientation`, `hideSimulatorApp`, `warmHid`, `inputReady`,
     `detach`, `helperPid` y los `hinge*` (plegables).
   - Lo que no necesita APIs privadas va directo a `xcrun simctl`: `boot`,
     `launch` (con `--console-pty` para leer la consola de la app),
     `install`, `list devices booted --json`, `io recordVideo`.
   - **Build**: lanza `xcodebuild -project|-workspace X -scheme S -configuration
     Debug -skipMacroValidation -destination D -derivedDataPath
     <userData>/simulator-builds/<hash del proyecto>/DerivedData build`. Después
     ejecuta `-showBuildSettings` para localizar el `.app` (prefiere
     `PRODUCT_TYPE = application` en `-iphonesimulator`). Cuenta errores,
     avisos y comandos fallidos, y deja el log en disco. La herramienta tiene
     `build` y `build_status`, y rechaza rutas relativas.
   - Mensajes propios para cada problema de Xcode: no instalado, solo Command
     Line Tools, componentes sin terminar, sin plataforma iOS
     (`xcodebuild -downloadPlatform iOS`), licencia sin aceptar.

3. **El panel en vivo es una NSView nativa, no un `<video>`**. El addon
   `@ant/claude-swift` (`swift_addon.node`) tiene `SimulatorSurfaceView`,
   `SimulatorScreenFeed`, `SimulatorPaneDisplay` y `SimulatorFrameGate`, y la
   coloca sobre la ventana de Electron con unos límites (`SimulatorViewBounds`).
   - `startSurfaceShare({udid, serviceName})` hace que el helper publique el
     IOSurface del framebuffer en un servicio mach. La vista lo pinta como
     contenido de una capa. Sin codificar ni decodificar: copia cero.
   - El H.264 por stream `F` se usa para grabar y cuando el Xcode está en otra
     máquina (sesiones SSH).

### Cómo llega a la pantalla y cómo se toca (dentro del helper)

- **Imagen**: `SimDevice` → pantallas de SimulatorKit (`SimScreen`, con
  «new-style frame callbacks» y el registro antiguo como reserva) → cada cambio
  de IOSurface (`FramebufferSurface`, `FollowingFramebufferSurface`) →
  VTCompression (H.264 o HEVC) o MJPEG, según el destino.
- **Entrada**: dos transportes.
  - **Indigo**: los mensajes HID clásicos (`IndigoDigitizerEvent`,
    `IndigoButtonEvent`, `IndigoKeyboardButtonEvent`), los que usa idb.
  - **DTUHID**: el servicio XPC `dtuhidd`, con los símbolos privados `_4sim`.
    El texto del binario dice que **CoreSimulator-1155.4 (Xcode 27) y
    posteriores ya no entregan bien Indigo**, y que ahí DTUHID es el transporte
    por defecto. Cualquier implementación nueva tiene que cubrir los dos.
- **Interfaz**: el árbol de accesibilidad con
  `SimDevice sendAccessibilityRequestAsync` y AccessibilityPlatformTranslation,
  más un recorrido basado en XCTest. Hay consultas por punto
  (`ui describe-point x y`) porque pedir «interactuable / tapado por» en todo
  el árbol es caro.

## Qué copiamos y qué no

- No se puede redistribuir `claude-ios-sim` ni `swift_addon`: son de Anthropic.
  Sí se puede usar **idb (MIT)**, que es lo mismo que usan ellos.
- La idea (un helper aparte, sandbox, stdio en tramas, IOSurface compartido para
  el panel y H.264 solo para grabar o para trabajo remoto) es un patrón, no código.
- Poki se queda como conector del iPhone real. El simulador no lo usa.

## Diseño para monoBOT

| Pieza | Dónde | Qué hace |
| --- | --- | --- |
| `monobot-ios-sim` | `crates/ios-sim-helper/` (paquete Swift que compila `cargo xtask`) | Helper aparte con FBSimulatorControl de idb. Habla por stdio en tramas `u32 len + u8 tag`. Ops: dispositivos, captura PNG, toques (Indigo y DTUHID), botones, teclas, texto, rotar, accesibilidad, IOSurface compartido, H.264. |
| `monocode-core::simulator` | core, sin GPUI | Arranca y vigila el helper (lo reinicia, pone tiempos límite, lo mata si se desincroniza). `simctl` para boot, install, launch, logs y openurl. `xcodebuild` para build y para encontrar el `.app`. Diagnóstico de Xcode. |
| MCP `monocode_simulator` | core, inyectado por sesión como `visual_bridge` | Herramientas para el agente: `build`, `build_status`, `launch`, `attach`, `screenshot`, `tap`, `swipe`, `touch_path`, `text`, `press_key`, `button`, `open_url`, `set_appearance`, `rotate`, `ui_describe`, `ui_describe_point`, `logs`. Coordenadas en puntos. |
| Panel `simulator` | `crates/monocode-gpui/src/simulator/` | `gpui::surface()` con el `CVPixelBuffer` hecho a partir del IOSurface que publica el helper. Los clics y arrastres se convierten en `touch`, y las teclas en `key`/`text`. Barra con Inicio, bloqueo, rotar, claro/oscuro, captura y abrir en Simulator.app. |
| Simulator.app nativo | igual | `open -a Simulator --args -CurrentDeviceUDID <udid>`. La entrada y las capturas van al dispositivo, no a la ventana, así que el agente puede controlarlo aunque esté abierto en Simulator.app. |

### Límites de GPUI que hay que resolver

- El `surface()` de GPUI (`vendor/gpui-pre/src/elements/surface.rs`) solo
  acepta `kCVPixelFormatType_420YpCbCr8BiPlanarFullRange`: hay un `assert_eq!`
  en `vendor/gpui-pre-apple/src/metal_renderer.rs` (`draw_surfaces`). El
  framebuffer del simulador es BGRA. Hay dos salidas:
  1. Convertir a NV12 en el helper con `VTPixelTransferSession` (GPU, barato).
  2. Parchear GPUI para que acepte BGRA (una textura y un shader). Es más
     limpio, pero toca el GPUI fijado: leer antes `docs/done/architecture.md`.
- Pasar el IOSurface de un proceso a otro: `IOSurfaceCreateMachPort` en el
  helper y `IOSurfaceLookupFromMachPort` en la app, por un servicio mach o
  enviando el puerto. Después, `CVPixelBufferCreateWithIOSurface`. Todo eso
  fuera del hilo de la interfaz; la vista solo recibe el buffer listo.
- El movimiento reducido no aplica a la imagen en vivo, pero sí a las
  transiciones del panel.

### Fases

1. ~~**Agente sin panel**~~ **Hecha** (ver «Estado» arriba).
2. ~~**Helper con entrada**~~ **Hecha, con XCUITest en lugar de idb** (ver «El
   runner de XCUITest»).
3. ~~**Panel en vivo en GPUI**~~ **Hecha, con fotogramas del runner.** Hay una
   pestaña **Simulator** en el panel derecho (`simulator_pane.rs`), solo en Mac
   y con el conector encendido. Muestra el simulador arrancado, o un botón
   «Boot a simulator» si no hay ninguno.
   - **Imagen**: la acción `screen` del runner (`XCUIScreen` a 2× los puntos,
     JPEG). El core la decodifica a BGRA y la vista la pinta con `img`,
     liberando el fotograma anterior. Solo pide fotogramas mientras la pestaña
     está a la vista.
   - **Entrada**: un clic toca, arrastrar desliza, teclear escribe (Intro,
     borrar, tabulador, Esc y flechas como teclas). Todo pasa por una cola en
     orden, y lo que se teclea mientras tanto se junta en un solo texto.
   - La cabecera tiene Inicio y «Open in Simulator.app».
   - **Medido**: el runner tarda unos 270 ms por captura, y `simctl io
     screenshot` en JPEG tarda 440 ms. En la app optimizada salen 3–4 imágenes
     por segundo. Para vídeo de verdad sigue haciendo falta el IOSurface del
     framebuffer (SimulatorKit, como el helper de Claude) o ScreenCaptureKit
     sobre Simulator.app; esa ventana es la vista rápida mientras tanto.
   - **Sin probar a mano en la app**: se comprobaron por separado la conversión
     de clics a puntos (tests), los fotogramas desde el core y que el runner
     sigue vivo tras cerrar su proceso.
4. **Extras**: ~~grabar vídeo~~ (hecho: ⌘R, ver abajo), fotos y contactos con el bridge que
   corre dentro del simulador, ubicación, notificaciones push
   (`simctl push`), y un Xcode en otra máquina por SSH con H.264.

## El panel del simulador (18 nov 2026)

Lo que pedían los puntos 1 a 4 de la lista de
[claude-code-review.md](claude-code-review.md). Código en
`crates/monocode-core/src/simulator/{native,controls,history,logs,inspect}.rs`
y `crates/monocode-gpui/src/simulator_pane.rs` (+ `simulator_pane/`).

### Vista nativa sobre un hueco medido

Con **iOS Simulator** activado en Ajustes › MCP, abre el panel derecho de
un proyecto iOS o Swift (ver abajo) en la vista **Focused** (⌘⇧B). La pestaña **Simulator** tiene
un icono de teléfono. Si no hay un dispositivo arrancado, elige uno de la
lista y pulsa **Boot simulator**. Si el arranque falla, puedes reintentarlo o
actualizar la lista sin cerrar la pestaña.

- **Core** (`simulator/native.rs`): carga `CoreSimulator` y `SimulatorKit` del
  Xcode activo con `dlopen`, busca el `SimDevice` por UDID, recorre sus
  `ioPorts` y se queda con el puerto `SimScreen` que tiene
  `framebufferSurface` (los otros son pantallas externas o CarPlay). Registra
  `registerScreenCallbacksWithUUID:callbackQueue:frameCallback:surfacesChangedCallback:propertiesChangedCallback:`
  en la cola principal: una llamada por fotograma y otra cuando cambia el
  IOSurface. `ScreenFeed` se desregistra al soltarse (fuera del hilo de la
  interfaz).
- **Medido con Xcode 27.2** en un iPhone 18 Pro con iOS 27.0: un IOSurface BGRA
  de 1206 × 2622 y un fotograma por cada cambio de pantalla. Hay un test que lo
  comprueba contra el dispositivo arrancado (y pasa sin hacer nada si no hay
  ninguno): `simulator::native::tests`.
- **Lo que hizo falta saber**:
  - Los descriptores son proxies remotos (`ROCKRemoteProxy-…`) sin métodos
    propios. La comprobación de `msg_send!` de objc2 los rechaza, así que esos
    mensajes van por `objc_msgSend` directo.
  - Antes de enviar cada selector, se comprueba que el protocolo de este Xcode
    lo declara (`protocol_getMethodDescription`).
  - Los bloques necesitan su firma (`v8@?0`): sin ella, el proxy lanza una
    excepción al pasarlos por XPC.
  - Toda llamada va dentro de `objc2::exception::catch`, porque una excepción
    de Objective-C que cruza Rust aborta la app. Si algo falla, el panel pasa a
    los fotogramas del runner y dice por qué («Showing snapshots: …»).
- **Corre dentro del proceso de monoBOT**, no en un helper aparte como en
  Claude. Si una versión de Xcode rompe esto de una forma que no lance
  excepción (un cuelgue en XPC, por ejemplo), lo notará la app. El siguiente
  paso sería un helper (el propio ejecutable con un subcomando) que publique el
  IOSurface por un puerto mach.
- **GPUI** (`simulator_pane/native.rs`): un `canvas` vacío en el hueco. En cada
  paint, coloca encima una NSView *layer-hosting* con los límites del hueco
  recortados a lo visible. Su capa tiene el IOSurface como `contents`. Cada
  fotograma solo llama a `setContentsChanged`, dentro de un `CATransaction`
  sin animaciones implícitas.
  - La vista devuelve `nil` en `hitTest:`, así que los clics llegan a GPUI y
    siguen siendo toques.
  - Se oculta cuando el hueco no se pinta: otra pestaña, el panel cerrado, el
    inspector abierto o un estado sin dispositivo.
  - Las esquinas redondeadas salen de la capa. El bisel se dibuja en GPUI
    alrededor.
- **Girar**: el framebuffer mantiene la disposición vertical aunque el
  dispositivo gire. Comprobado con Safari en `landscape_right`: la interfaz
  está dibujada girada un cuarto a la izquierda dentro del framebuffer, así que
  la capa la gira un cuarto a la derecha. La pantalla de inicio del iPhone no
  gira, así que en horizontal se ve de lado, igual que en Simulator.app.
- **Toques en horizontal (medido)**: los toques del runner por coordenada usan
  los puntos de la pantalla en **vertical**. El toque en (32, 94) abre las
  pestañas de Safari en `landscape_right`, y en (780, 32) no hace nada. En
  cambio, `describe_ui` da los marcos de la app en sus puntos **girados**
  (Safari mide 874 × 402). El panel convierte cada clic a puntos verticales y el
  inspector usa el espacio del árbol (`geometry::Space`).
  **Pendiente:** un agente que toca por coordenada lo que leyó en `describe_ui`
  falla en horizontal. Tocar por etiqueta sí funciona. El arreglo va en el
  runner (`point()` en `Runner.swift`): girar el punto cuando el marco de la
  app es horizontal.
- **Reserva**: los fotogramas JPEG del runner, como antes. Se dibujan tal
  cual, sin girar.

### Bisel y atajos

Los botones están bajo el dispositivo, con su atajo en el tooltip
(`chrome_tip_keys` en `theme.rs`). Los atajos funcionan mientras el panel
tiene el foco (contexto `SimulatorPane`):

| Atajo | Qué hace |
| --- | --- |
| ⌘⇧H | Inicio. |
| ⌘→ / ⌘← | Girar a la derecha / a la izquierda, con la acción `rotate` del runner. |
| ⌘⇧A | Cambia entre claro y oscuro (`simctl ui appearance`). |
| ⌘K | Conecta o desconecta el teclado del Mac (`SimDevice setHardwareKeyboardEnabled:keyboardType:error:`). Sin él, iOS muestra su propio teclado. Se supone conectado al abrir el panel. |
| ⌘S | Guarda una captura PNG en el Escritorio, con el nombre que usa Simulator.app. |
| ⌘R | Graba la pantalla (`simctl io recordVideo --codec=h264`) a un `.mov` en el Escritorio; ⌘R otra vez la termina con SIGINT. Mientras graba, se ve el tiempo. |
| ⌘0 / ⌘1 | Encajar en el hueco / tamaño real: un punto del dispositivo por píxel del panel, centrado y recortado. |

Teclear sin ⌘ sigue escribiendo en el dispositivo, solo si el foco está en el
panel. Si está en un campo de los cajones, escribe en el campo.

### Historial de acciones

- El servidor MCP (un proceso por sesión de agente) apunta cada `tap`,
  `swipe`, `type_text`, `press_key`, `press_button`, `rotate`, `launch`,
  `open_url` y `appearance` que sale bien en
  `~/Library/Caches/com.monocode.desktop/simulator-history/<udid>/steps.jsonl`.
- Medio segundo después guarda la pantalla como JPEG (acción `screen` del
  runner, escala 1).
- `launch` se apunta con el bundle id y sin `app_path`, así que repetirlo
  reabre la app instalada.
- Se quedan los últimos 120 pasos.
- El cajón **Actions** muestra cada paso con su miniatura y cuándo fue.
  Comprueba cada segundo si hay pasos nuevos, mirando el tamaño y la fecha del
  archivo.
- **«Again · N actions»** repite el último tramo: desde el último `launch`,
  sin cruzar una pausa de más de 5 minutos. Llama al mismo código que usan las
  herramientas (`mcp::perform`), con 350 ms entre pasos, y se para en el
  primer fallo.
- **Pendiente:**
  - Si el proceso MCP termina en ese medio segundo, el último paso se queda sin
    captura.
  - Las acciones no aparecen todavía como filas en el transcript, como en
    Claude.

### Cajón de logs

- `simctl spawn <udid> log stream --style ndjson --level info`, leído en un
  hilo y entregado en tandas cada 150 ms. Con el dispositivo en uso salen unas
  650 líneas por segundo; el cajón guarda las últimas 2000.
- Filtros:
  - **nivel**: All, Notice, Errors o Faults;
  - **alcance**: Device, o App (el subsistema con el bundle id de la última app
    lanzada);
  - **texto**: busca en el mensaje, el proceso, el subsistema y la categoría.
- Las más nuevas, arriba.
- **Pendiente:**
  - La salida de `print()` de la app no pasa por `os_log`. Para verla hay que
    lanzarla con `simctl launch --console-pty` o `--stdout`/`--stderr` a un
    archivo, y seguir ese archivo.
  - «Go to crash».

### Inspector → chat

- **Inspect** (cabecera) congela la pantalla: una copia del IOSurface, o un
  fotograma del runner en la reserva. La vista nativa se oculta, porque
  taparía el contorno. A la vez, lee el árbol con la acción `describe` del
  runner (la app lanzada la última, si está delante).
- Un clic elige la pila de elementos bajo el punto, con el de delante primero
  (`inspect::stack_at`). **In front** y **Behind** la recorren.
- Se puede escribir una nota.
- **Add to chat** escribe en el borrador del chat activo el tipo, la etiqueta,
  el identificador, el marco en puntos y la nota. Adjunta además un PNG
  recortado alrededor del elemento, con 8 pt de margen.
  - Camino: `SimulatorEvent::AddToChat` → `PanelEvent::AddToChat` →
    `Shell::add_to_chat` + `Conversation::quick_attach`.
  - Los recortes quedan en `~/Library/Caches/com.monocode.desktop/simulator-inspector/`.
- **Pendiente:**
  - Ver el elemento al pasar el ratón, antes de hacer clic.
  - Un modo que no congele la pantalla.

### Probado y sin probar

- **Tests**: 34 en el core (`simulator`) y 12 en el panel (`simulator_pane`).
  Entre ellos, el test de banco de GPUI con eventos reales: atajos, botones,
  teclear, clic y arrastre, inspector con su pila y «Add to chat», historial
  y filtro de logs.
- **Comprobado contra el simulador**:
  - el feed y sus fotogramas;
  - el teclado;
  - el giro del framebuffer;
  - los toques en horizontal;
  - el historial escrito por el MCP.
- **Sin probar a mano dentro de la app**: no se abrió la ventana para no
  quitarle el foco al usuario. Falta comprobar:
  - que la NSView queda alineada con el hueco (también con el zoom de
    interfaz);
  - el sentido del giro de la capa;
  - que no parpadea al cambiar de pestaña.
- **Límite conocido**: lo que GPUI dibuja encima del hueco, como un menú o un
  popover del panel derecho, queda debajo de la vista nativa. Pasa igual con el
  vídeo y los artefactos HTML.

## Solo en proyectos iOS, y más rápido (19 nov 2026)

### La pestaña solo aparece en proyectos iOS

La pestaña **Simulator** necesita ahora dos cosas: el conector encendido y que
el proyecto activo sea un proyecto iOS o Swift. En cualquier otro proyecto no
hay pestaña, y del simulador no existe nada: ni el `SimulatorPane`, ni
`simctl`, ni el sondeo de dispositivos, ni el framebuffer. No había otros
botones que abrieran el simulador: solo esta pestaña.

- **Qué cuenta** (`simulator/mcp/project.rs`, `is_ios_project`): hasta tres
  carpetas por debajo del proyecto (la misma profundidad que `find_projects`),
  un `.xcodeproj`, un `.xcworkspace`, un `Package.swift`, o una app Expo /
  React Native con `package.json` y, además, una carpeta `ios/`, un `app.json`
  cuyo `expo.platforms` incluye iOS (o no dice nada, que en Expo es iOS y
  Android), o un `app.config.js|ts` con `expo` entre las dependencias.
  - No entra en `node_modules`, `target`, `Pods`, `build`, `DerivedData`,
    `vendor`, `Carthage`, `dist` ni en carpetas ocultas (`.git`, `.build`…), y
    no sigue enlaces.
  - Se rinde tras 5000 entradas, así que una carpeta personal o un monorepo
    enorme cuestan milisegundos.
  - Es una función pura sobre una carpeta, con tests sobre carpetas temporales.
  - Está declarado desde `mcp.rs` porque `simulator.rs` quedaba fuera de esta
    tarea.
- **Cuándo se mira** (`right_panel.rs`):
  - Se mira en el ejecutor de fondo, nunca en el hilo de la interfaz.
  - Se guarda la respuesta por carpeta: volver a un proyecto ya visto
    responde en el acto.
  - Se vuelve a mirar al cambiar de proyecto, al encender el conector y con
    cada `GitChanged` del proyecto. Ese aviso sale del vigilante de archivos
    que ya tenía `ProjectGit`: no hay vigilante nuevo.
  - Con el conector apagado, no se mira.
- **La pestaña guardada**: si era Simulator, el panel abre en Explorer y vuelve
  a Simulator cuando llega la respuesta, si el proyecto es iOS y el usuario no
  ha elegido otra pestaña. Si el proyecto deja de serlo con la pestaña abierta,
  pasa a Explorer, como al apagar el conector.
- **El pane se crea la primera vez que se muestra** (antes se creaba con el
  panel, en todos los proyectos).
- `set_simulator_tab` pasa a ser `set_simulator_connector` (Ajustes › MCP lo
  llama al cambiar el interruptor).

### Medido antes de tocar nada

Se midió en un simulador propio, creado para esto y borrado al terminar
(iPhone 18 Pro, iOS 27.0, Xcode 27.2), nunca en el del usuario. Se usó un
`HOME` temporal, así que el runner se compiló y guardó su estado aparte.

- **Cómo medirlo de nuevo**:
  `MONOBOT_SIM_PERF_UDID=<udid> cargo test -p monocode-core
  simulator::native::tests::measure -- --ignored --nocapture`.
- **Carga del Mac**: entre 6 y 400 durante las cuatro pasadas, por las
  compilaciones de otras sesiones. Las cifras son las de la pasada más
  tranquila (carga entre 22 y 6). Sin carga, `simctl list` tarda unos 90 ms y
  gasta 70 ms de CPU.
- **CPU**: es la del proceso y la de sus hijos (`getrusage`). Con `ps -M` sobre
  el proceso, en la fase de reposo ningún hilo sumó tiempo en 20 s: todo el
  gasto estaba en los `simctl` que arranca, y `ps -M` no los cuenta. Tampoco
  está contado el trabajo que cada `simctl list` le pide a
  `CoreSimulatorService`.

Lo que se encontró:

1. **Cada toque, tecla o texto del panel listaba los dispositivos antes de
   llegar al runner.** `simulator::act` llama a `simctl list` para encontrar el
   dispositivo, aunque el panel ya lo sabe. Eso son 200 ms de los 625 ms de un
   toque, y le pasa igual a girar, a `describe` del inspector, a la captura
   del historial y a cada fotograma de reserva.
2. **El panel en vivo arrancaba `simctl list` cada 3 s** para saber si el
   dispositivo seguía arrancado: un 4,4 % de un núcleo sin que pase nada en
   pantalla.
3. **El toque de XCUITest se lleva casi todo lo que queda.** Un toque por
   coordenada calcula el marco de SpringBoard y espera a que la pantalla se
   quede quieta: unos 400 ms de los 458. El viaje al runner y vuelta son
   28 ms.
4. **Al abrir la pestaña**, `simctl getenv` (la escala de la pantalla, unos
   200 ms) y dos `xcode-select` (30 ms cada uno), cada vez.
5. **La imagen nativa ya iba bien**:
   - 219 fotogramas durante dos deslizamientos entre páginas de la pantalla
     de inicio, con un intervalo de 16,1 ms de mediana (60 por segundo) y
     33 ms en el percentil 90;
   - ninguna copia por fotograma;
   - GPUI no se repinta por fotograma: la capa solo se marca como cambiada.

   No se tocó. Con la ventana detrás de otra, el feed sigue marcando la capa,
   porque el usuario puede estar mirando lo que hace el agente. Con otra
   pestaña o el panel cerrado, el feed se cierra (como antes).

### Lo que cambió

| | Antes | Después |
| --- | --- | --- |
| Toque desde el panel | 625 ms, 111 ms de CPU | **298 ms**, 0,6 ms de CPU |
| Viaje al runner (`status`) | 265 ms, 163 ms de CPU | 28 ms, 0,4 ms de CPU |
| ¿Sigue arrancado? (cada 3 s) | 244 ms, 205 ms de CPU | 6,6 µs |
| Panel en vivo en reposo, 30 s | 4,4 % de un núcleo | 0,01 % |
| Fotograma de reserva del runner (`live_frame`) | 356 ms, 119 ms de CPU | 185 ms, 8 ms de CPU: de unos 2,8 a unos 5 por segundo |
| Volver a abrir la pestaña | `getenv` + 2 × `xcode-select` | ninguno |

- **`controls::act(udid, …)`** (con `input::act_known`): va directo al runner
  del dispositivo que el llamador ya sigue. Solo si no contesta ningún runner
  comprueba que el dispositivo está arrancado antes de arrancar otro.
  - Lo usan las entradas del panel, girar, `inspect::describe`, la captura
    del historial y los fotogramas de reserva (`live_frame`).
  - Los agentes siguen con `simulator::act`, porque nombran el dispositivo por
    nombre o no lo nombran.
- **`ScreenFeed::booted()`**: lee el estado que CoreSimulator guarda del
  `SimDevice` dentro del proceso. El panel lo consulta cada 3 s en lugar de
  `simctl list`. Un test apaga el dispositivo propio con el feed abierto: el
  estado cambió 20 µs después de que `simctl shutdown` volviera
  (`booted_notices_a_shutdown`, ignorado por defecto).
- **Toque rápido** (`"fast": true` en `tap`, solo desde el panel). El runner
  sintetiza el toque con las clases de XCTest que usa XCUITest por debajo
  (`XCPointerEventPath`, `XCSynthesizedEventRecord` y el `eventSynthesizer`
  del dispositivo, como WebDriverAgent). Así se salta el marco de SpringBoard
  y la espera.
  - Esas clases no son públicas. Se buscan en tiempo de ejecución y, si falta
    alguna, el toque va por `XCUICoordinate` como antes.
  - Medido: 298 ms frente a 458. Un toque rápido sobre «Ajustes» abrió Ajustes.
  - Los agentes no lo usan: su `tap` sigue volviendo con la pantalla quieta,
    que es lo que esperan para la siguiente captura.
  - Cambiar `Runner.swift` cambia la clave de compilación, así que la primera
    acción tras actualizar recompila el runner (unos 40 s).
- **Al abrir el feed**:
  - la escala de cada dispositivo se pide una vez y se guarda;
  - `xcode-select` solo corre al cargar los frameworks;
  - `xcode_problem` solo se mira la primera vez, y otra vez después de un
    fallo.

### Pendiente

- **Sin dispositivo arrancado**, la pestaña visible sigue buscando uno con
  `simctl list` cada 3 s.
- **Sin probar**:
  - el toque rápido en horizontal: el registro se crea en vertical, como las
    coordenadas de SpringBoard, pero no se ha medido;
  - la app entera abierta a mano. Se midieron el core y el runner sobre el
    simulador, y el panel con el banco de pruebas de GPUI.

### Riesgos

- APIs privadas que cambian con cada Xcode, como el paso de Indigo a DTUHID en
  Xcode 27. Hay que fijar la versión de idb y probar con el Xcode estable y con
  la beta.
- Solo macOS y solo con Xcode completo. En Windows y Linux el panel no aparece.
- Seguridad: el agente controla el simulador, no el Mac. El helper corre sin
  red y sin acceso a los archivos del usuario. El iPhone real sigue pidiendo
  permiso para cada acción, con Poki.
