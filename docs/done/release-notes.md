# monoBOT 0.0.3

## What's new

- **macOS is back.** This release ships for macOS and Windows at the same
  version, signed and notarized on macOS, and both update themselves from
  now on.
- **Build iOS apps with your agents.** Switch on **iOS Simulator** in
  Settings › MCP (macOS, needs Xcode). Agents can then boot a simulator, build
  your Xcode project, install and launch it, take screenshots, read its log,
  open links, and use the app: tap, swipe, type, press keys and Home, rotate,
  and read every element on screen. The first touch builds a small test
  runner on your Mac, which takes about a minute.
- **The simulator in monoBOT.** With the connector on, the right panel gets a
  **Simulator** tab that shows the booted simulator. Click to tap, drag to
  swipe and type to type. It refreshes a few times a second; **Open in
  Simulator.app** shows the full-speed window.
- **Swarm:** `/swarm` launches parallel agents on a live board, with Board,
  Members and Graph tabs; agents can start swarms, groups get a Board, and
  the graph is clickable.
- **A calmer transcript** in Focused: only the phase at work stays in view,
  each group of steps reads as one sentence ("Ran 3 commands and edited 2
  files"), a finished turn lists the files it changed, Settings › Chat › Tool
  visibility hides steps, and subagents gather under their faces.
- **The notch** shows two players at once, live agent states and when your
  limits reset.
- **Moving project backgrounds** (GIF and video), zoomable Mermaid diagrams,
  a dictation shortcut, and a lighter app at rest.
- **Windows:** the installer opens monoBOT through Explorer, so endpoint tools
  such as Sophos no longer stop it; tray and floating chat fixes.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
