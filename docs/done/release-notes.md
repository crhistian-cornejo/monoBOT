# monoBOT 0.0.4

## What's new

- **An Agents tab beside Swarm** shows what a Claude chat's subagents and
  workflow runs are doing, live: each run's phases in the script's order and,
  under them, every agent's state, time and latest step or report.
- **Swarm** sits next to Orchestrator in the + menu with its own lime
  `/swarm` chip, and modes no longer stack on top of a swarm.
- **Dictate anywhere with Ctrl+Space.** The shortcut goes to the focused
  composer, or to the notch when nothing is focused: the words appear as
  they come and paste into the app in front, while monoBOT steps into the
  background so the notch and agents keep running. Dictation text is
  cleaner (fewer fillers, stutters and spoken commands), and downloads
  resume if they are interrupted.
- **Agents keep working.** The loop guard no longer stops an agent for
  running commands that only look alike: calls count once they finish,
  waiting starts the count over, and a stopped turn offers **Continue**.
- **Stronger reasoning efforts shimmer** in the model menu: Max, Ultra,
  Ultracode and Ultrathink rows fill with the effort's colour while
  highlighted.
- **Mail pictures on Windows** render again, including images pasted
  inside the message itself.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
