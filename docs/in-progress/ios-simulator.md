# Simulador iOS dentro de monoBOT

Objetivo: que el agente cree, compile, lance y pruebe apps de iOS desde monoBOT,
con la pantalla del simulador en vivo dentro de la app (o en el Simulator.app
nativo), capturas, toques y lectura de la interfaz, como lo hace la pestaña Code
de la app de escritorio de Claude.

## Estado: fases 1, 2 y 3 hechas (10 nov 2026)

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
4. **Extras**: grabar vídeo (H.264 → MP4), fotos y contactos con el bridge que
   corre dentro del simulador, ubicación, notificaciones push
   (`simctl push`), y un Xcode en otra máquina por SSH con H.264.

### Riesgos

- APIs privadas que cambian con cada Xcode, como el paso de Indigo a DTUHID en
  Xcode 27. Hay que fijar la versión de idb y probar con el Xcode estable y con
  la beta.
- Solo macOS y solo con Xcode completo. En Windows y Linux el panel no aparece.
- Seguridad: el agente controla el simulador, no el Mac. El helper corre sin
  red y sin acceso a los archivos del usuario. El iPhone real sigue pidiendo
  permiso para cada acción, con Poki.
