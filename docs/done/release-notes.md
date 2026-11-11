# monoBOT 0.0.5

## What's new

- **Hand big tasks to your monos.** A Claude chat can pass a large task that
  splits into parallel parts to **Chief**, a mono that coordinates your code
  monos in its own chat. Each part goes to a mono with its own chat and
  worktree, where you see its changes; Chief reviews their work into your
  checkout and sends the chat one summary when it is done. The monos stay
  with you as agents and are reused for later tasks. Chief and the monos run
  Claude Opus 5.5 at medium effort. The Swarm tab shows Chief and every mono
  with its face. Turn it off in Settings › Chat › **Let Claude hand work to
  your monos**.
- **Subagents read at a glance.** Each subagent in a reply shows the working
  spinner, a check once it is done or a red cross if it failed, and under its
  title its latest step or the first line of its report. Subagents no longer
  wear the faces that belong to your own agents.
- **Background work sits in the chat.** Commands and subagents still running
  after a reply show in a card under it, instead of over the message box.
- **Stay signed in after an update.** On Windows, updating or quitting
  monoBOT no longer signs Claude Code or Codex out: the app lets each CLI
  finish saving its renewed sign-in before it closes it.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
