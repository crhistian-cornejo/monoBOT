/** The scene each step of the iPhone tutorial animates. */
export type PokiScene =
  "needs" | "clone" | "cable" | "developer" | "start" | "folder" | "bot";

export type PokiStep = {
  scene: PokiScene;
  title: string;
  body: string;
  /** Terminal lines to copy, run in order from the Poki folder. */
  commands?: readonly string[];
  /** Short points under the body. */
  points?: readonly string[];
};

export const POKI_REPO = "https://github.com/JosssphZhou/poki";

export const POKI_STEPS: readonly PokiStep[] = [
  {
    scene: "needs",
    title: "What you need",
    body: "Poki is a separate open-source project (AGPL-3.0) that runs on your Mac and drives the iPhone through Xcode's developer services, over MCP. MonoBOT can install it for you and starts it while its connector is on.",
    points: [
      "A Mac with Apple Silicon and macOS 15 or later",
      "Full Xcode, not only the command line tools",
      "Node.js 24 or later, Python 3 and ffmpeg (brew install node ffmpeg)",
      "An iPhone and its USB cable, or the iOS Simulator",
    ],
  },
  {
    scene: "clone",
    title: "Get Poki",
    body: "Turn on Settings › Connectors › iPhone (Poki). MonoBOT asks, then downloads Poki's published release (about 11 MB), refuses it unless its SHA-256 matches the one GitHub lists, and unpacks it inside MonoBOT's own data folder. No admin rights, no install scripts, nothing else on your Mac changes.",
    points: [
      "Intel Mac, or you prefer your own copy? Clone and build it yourself (needs Rust), then choose that folder:",
    ],
    commands: [`git clone ${POKI_REPO}.git ~/poki`, "cd ~/poki", "./bin/setup"],
  },
  {
    scene: "cable",
    title: "Connect the iPhone with its cable",
    body: "The first pairing needs the cable. Plug the iPhone into the Mac, unlock it, tap Trust and type your passcode on the phone yourself.",
    points: [
      "After this first pairing it can also work over the same Wi-Fi, while the phone stays unlocked.",
      "Only using the Simulator? Skip to step 5.",
    ],
  },
  {
    scene: "developer",
    title: "Turn on Developer Mode and UI Automation",
    body: "On the iPhone, open Settings › Privacy & Security › Developer Mode, turn it on and let the phone restart. Then turn on Settings › Developer › UI Automation.",
    commands: ["node bin/prepare-device.mjs"],
    points: [
      "Run it from the Poki folder that Settings shows.",
      "No Developer Mode in Settings? Open Xcode › Window › Devices and Simulators once with the phone connected.",
      "prepare-device mounts the developer services when they are missing. It changes no security setting and downloads nothing.",
    ],
  },
  {
    scene: "start",
    title: "Poki runs while the connector is on",
    body: "MonoBOT starts Poki's service (its npm start) when you turn the connector on or a Phone bot needs it, and stops it when you turn it off or quit. Its address and token stay on this Mac.",
    points: [
      "MonoBOT never starts it with POKI_MCP_APPROVAL_MODE=allow, which would approve sensitive actions without asking anyone.",
      "Running your own copy in a terminal with npm start works too; MonoBOT uses it and leaves it alone.",
    ],
  },
  {
    scene: "folder",
    title: "Check that it says Ready",
    body: "After installing, the Poki folder fills in by itself; with your own copy, choose its folder. The status says Ready when the folder is right and Poki is running.",
  },
  {
    scene: "bot",
    title: "Make a Phone bot",
    body: "In Bots, use + › Phone, then check in its settings that iPhone (Poki) is on. It reads the screen on its own; each tap, swipe or typed text shows a card in its chat that you allow or deny.",
    points: [
      "Keep the phone unlocked with the app in front; set Auto-Lock to a longer time while you use it.",
      "It stops and asks before sending, paying, buying, changing settings or deleting, and never enters codes or unlocks the phone.",
      "What is on the screen goes to the model you use, like any other message.",
    ],
  },
];
