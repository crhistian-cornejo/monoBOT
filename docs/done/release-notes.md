# monoBOT 0.0.6

## What's new

- **Environment beside the chat.** Press Ctrl+I (Cmd+I on a Mac), click the
  new button at the end of the composer's bar or the context ring: a floating
  frame shows where the chat works (local folder or worktree), its branch,
  which opens the branch picker, the turn's changes, which open the diff, and
  how full the context window is, with a bar that turns amber, then red, and
  a button to compact. It sits in the chat's margin when there is room, moves
  the chat left when there isn't, and Esc closes it.
- **Sessions sorted by what they are doing.** The code sidebar now lists
  sessions whose agent is working, or waiting on you, under **Working**, then
  the ones that finished while you were away under **Done**, then the rest.
- **Swarm boards open when you ask.** A swarm's card says **Swarm** and
  explains that the coordinator is planning; **Open board** opens the Swarm
  tab even before any agent starts and after you closed the panel, and a new
  swarm opens it by itself. Coordinators (and Chief) now split the work and
  delegate after a short look at the code, instead of reading the whole
  project first.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
