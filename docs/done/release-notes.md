# monoBOT 0.0.12

## Fixed

- **iOS Simulator in the right panel.** The Simulator tab now appears in
  the default layout on macOS when **iOS Simulator** is enabled in
  **Settings → MCP**. If no device is running, choose one and boot it without
  leaving monoBOT. If the boot fails, retry it or refresh the device list
  from the same tab.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
