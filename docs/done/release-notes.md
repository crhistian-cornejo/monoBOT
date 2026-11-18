# monoBOT 0.0.8

## What's new

- **Captains.** Your personal agents are now Captains, each with its own chat,
  listed above your threads in one sidebar for code and personal agents. A
  New Captain dialog picks a role and a name, and a Captain's chat opens on
  its portrait with **View Captain**.
- **Monos as subagents, from any provider.** Claude, Codex, OpenCode, Cursor
  and the rest can hand a large task to Chief and your monos, or start a
  swarm. **Hand to your crew** in a thread's menu does it for the thread's
  latest request, and the right panel's **Crew** tab follows the run, keeping
  its names and faces after it ends.
- **A compact model picker.** Favorites first, then every model grouped by
  who makes it, or by provider. Hover a model to set its effort on a dithered
  slider, turn on fast mode or thinking, choose the account it runs on, or
  make it a favorite (⌥1–⌥9).
- **Live dictation.** Words show as you say them. The speech model loads when
  monoBOT opens and stays ready, so stopping no longer sits on "Finishing…";
  if the model is still loading the bar says so, and Esc gets you out.
- **Corners your way.** Choose square to soft corners in Settings ›
  Appearance, and separately for the notch, whose outline opens to match.
  Context menus are native on macOS, the interface uses the system font, and
  the accent follows the system's.
- **Threads that show their time.** A working thread shows a live timer in
  the sidebar. Workstreams have their own page, from the bar at the foot of
  the sidebar.
- **What's new** shows each release's highlights as cards.

## Fixed

- Codex chats can reach the app from their sandbox when they hand work to
  Chief or a swarm, and OpenCode 2 chats get their own server with the app's
  access, instead of the shared background service.
- A new request is no longer mistaken for an older Chief run of the same
  chat; it is told to wait for it or stop it. Deleting Chief's chat ends its
  run, and Chief's own first turn is no longer offered Chief.
- The Environment frame stays above the chat when the right panel is open.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
