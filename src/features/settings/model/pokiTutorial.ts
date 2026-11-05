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
    body: "Poki is a separate open-source project that runs on your Mac and drives the iPhone through Xcode's developer services. MonoBOT starts it for your Phone bot; you install it once.",
    points: [
      "A Mac with macOS 15 or later",
      "Full Xcode, not only the command line tools",
      "Node.js 24 or later, Rust and ffmpeg",
      "An iPhone and its USB cable, or the iOS Simulator",
      "About ten minutes for the first build",
    ],
  },
  {
    scene: "clone",
    title: "Get Poki",
    body: "Clone it anywhere you like, then let its setup check what is missing and build the native bridge. The first build takes a few minutes.",
    commands: [`git clone ${POKI_REPO}.git ~/poki`, "cd ~/poki", "./bin/setup"],
    points: [
      "On Apple Silicon you can download the prebuilt package from Poki's releases instead and run npm ci in it: no Rust needed.",
    ],
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
      "No Developer Mode in Settings? Open Xcode › Window › Devices and Simulators once with the phone connected.",
      "prepare-device mounts the developer services when they are missing. It changes no security setting and downloads nothing.",
    ],
  },
  {
    scene: "start",
    title: "Start Poki and leave it running",
    body: "Poki's service has to run while a bot uses the phone. Start it in a terminal and keep that window open; MonoBOT tells you when it stops.",
    commands: ["cd ~/poki", "npm start"],
    points: [
      "Never start it with POKI_MCP_APPROVAL_MODE=allow: that approves sensitive actions without asking anyone.",
      "After updating Poki, stop it and run npm start again.",
    ],
  },
  {
    scene: "folder",
    title: "Point MonoBOT to it",
    body: "In Settings › Connectors › iPhone (Poki), choose the folder you cloned and turn the switch on. The status says Ready when the folder is right and Poki is running.",
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
