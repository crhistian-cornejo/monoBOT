# monoBOT 0.0.2

## What's new

- **Updates work on company networks.** Networks that inspect HTTPS with
  their own certificate made the update check fail with "UnknownIssuer". The
  updater now trusts the Windows certificate store, and so do Jira, Linear,
  GitLab, Azure DevOps, inbox media and the other HTTPS connections.
  0.0.1 can't update itself on those networks: install 0.0.2 once from the
  download page.
- **monoBOT lives in the Windows tray.** Click its logo for the same menu as
  the macOS menu bar: chat with a Mono in its floating window, open the quick
  composer, the notch or the app, or quit. Windows hides new tray icons behind
  the arrow: drag it onto the taskbar, or use Settings › Monos › Tray icon ›
  Taskbar settings.
- **Quick composer anywhere on Windows:** Ctrl+Shift+Space.
- **Notifications show monoBOT's logo** on Windows.
- **The notch opens the right chat:** a click on an agent opens exactly that
  conversation, and Claude Code sessions in a terminal bring their own window
  forward.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for Windows. macOS builds return once they can be signed and
  notarized; Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
