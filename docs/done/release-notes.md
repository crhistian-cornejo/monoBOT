# monoBOT 0.0.12

## New

- **Search your clipboard history.** The notch's Clipboard page now keeps
  your last 200 copies (up from 30) and up to 8 pictures. Search them by
  their words, or by tag: `#link`, `#code`, `#rust`, `#github.com`, `#md`.
  The tags your copies share most sit under the search, one click from
  filtering. The history still lives in memory only.
- **Native menus in the notch.** On macOS, right-click a copy to copy it
  again (or a color as hex, `rgb()` or `hsl()`), open a link, show a file in
  Finder, find copies with the same tag or remove it. Right-click a widget to
  make it narrower or wider, move it or remove it without the pencil.
- **Any code chat can hand work off.** With **Settings → Chat → hand work to
  monos** on, a local code chat can give a task to one of your code monos, or
  start a temporary worker in its own worktree when no mono fits. The result
  comes back to that chat once, and the worker shows in that chat's Swarm
  tab.
- **Environment in the composer.** The context ring opens one panel with the
  workspace (folder or worktree), the branch and the turn's changes, and how
  full the context is, with Compact. Click it or press Cmd/Ctrl+I.
- **Background commands show their output.** A command running in the
  background shows its last line, and its last 8 lines when you open it, for
  Claude and Antigravity.

## Improved

- **Lighter streaming.** The app takes a turn's events in batches and folds
  repeated tool updates together, so long command output costs far less to
  draw. A failed command shows its first and last lines and the lines that
  name an error, with **Show all** for the rest.

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
