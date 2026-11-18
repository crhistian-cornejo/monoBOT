# monoBOT 0.0.9

## Fixed

- **Captain chats read in order.** A message whose turn reached the agent and
  then failed (for example, a model the account can't use) no longer shows
  twice, once with its error and again at the bottom as "Not sent". Copies
  saved that way are cleaned up when the chat opens.
- **The Max effect wherever a model has it.** The model card's corner moves for
  Max and every Ultra effort (Ultrathink, Ultracode), wherever they sit in a
  model's list, and for a provider's own effort control.
- A bot the Principal creates with its own instructions keeps them; a new
  bot's settings open with its soul.
- A chat whose Chief run ended still shows that run's board, and can start a
  swarm again.
- Deleting Chief's chat forgets its run for good.
- Chief and the crew's coders show under Monos in every project, and only
  there.
- Grouped by maker, the model picker still shows a provider that is loading,
  or whose models failed, with Retry.
- Dragging the effort knob outside the card no longer closes the card or
  leaves the drag stuck.
- OpenCode chats that call the app start without waiting when their private
  server prints no password.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
