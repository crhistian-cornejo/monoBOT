# monoBOT 0.0.13

## New

- **A native-looking notch.** The notch now matches the rest of monoBOT on
  macOS. Its pages are a segmented control whose highlight glides to the
  page you pick. Buttons are rounded and react when pressed, settings use the
  system's switches, and Settings has a sidebar like macOS's own. Widgets,
  the clipboard preview and the player sit on glass cards. Everything
  follows **Look → Corners**, and motion stops when you reduce motion in
  System Settings.

## Improved

- **A faster iOS Simulator.** A tap from the Simulator tab reaches the
  device in about half the time, and an open tab no longer uses the
  processor while nothing changes.
- **The Simulator only where it helps.** The Simulator tab appears only in
  projects with an Xcode project or workspace, a Swift package, or an Expo
  or React Native app that builds for iOS. In other projects it starts
  nothing.

## Fixed

- **Live streams in the notch's player.** A live video no longer shows a
  length of millions of hours that pushed the player's buttons onto the
  widgets. Videos longer than an hour show their time as `1:17:55`.

## Updates

monoBOT checks for updates when it opens and every four hours. A new version
downloads by itself; restart to install it. On Windows it also installs when
you quit. If an update can't finish, the sidebar offers the installer to
download by hand.

## Before you install

- This release is for macOS and Windows. Linux builds are paused.
- macOS is the main verified platform for the app itself. Windows still needs
  wider runtime testing; see [known limitations](../in-progress/limitations.md).
