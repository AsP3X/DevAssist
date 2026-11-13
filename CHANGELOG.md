# Changelog

What changed in each version of DevAssist. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/).

## [Unreleased]

## [0.5.1] - 2026-11-13

An update no longer leaves a force-pushed branch behind.

### Fixed

- **An update follows a branch that was force-pushed on the remote.** A
  local branch nobody committed to stayed "ahead 1, behind 20" for good
  after a bot rebased its pull request, because an update only
  fast-forwards. It now moves such a branch to the rewritten one. A branch
  with commits of your own is still left alone.

## [0.5.0] - 2026-11-12

A welcome screen, and one way to go between tools: the palette.

### Added

- **A welcome screen.** DevAssist starts on it: the name in large letters
  and a card for every tool, with what the tool is for and how things stand.
  Arrows and `Enter`, or a click, open a tool.
- **The palette lists Home and every tool with what it is for.** Typing
  narrows it by name or by description, so "containers" finds Docker.
  Typing a letter on the welcome screen opens it.
- Two settings: `palette_key` chooses the key that opens the palette, and
  `start` makes DevAssist open a tool directly instead of the welcome
  screen.

### Changed

- **The palette is the one way to go from tool to tool.** The number keys
  `1` and `2` and the clickable list of tools in the top line are gone; the
  number keys now belong to the tool on screen.
- The top line says where you are, as `DevAssist › Docker`. A click on
  `DevAssist` goes to the welcome screen.
- DevAssist no longer starts in the repository overview. Set `start = repos`
  to have that back.

## [0.4.0] - 2026-11-12

DevAssist gets a second tool: Docker.

### Added

- **A Docker tool.** Press `2`, or click Docker in the new top line. It
  lists containers (grouped by Compose project), Compose projects, images
  and volumes on four tabs, and follows Docker by itself when something
  changes.
  - `s` starts and stops, `R` restarts, `l` opens the logs and follows
    them, `Enter` shows the actions for the selection.
  - `d` removes and `p` prunes, both behind a dialog that lists by name
    what will go. Running containers and images in use stay unless you
    press `f` or click the second button, which Enter never does; a
    volume in use always stays. Taking a Compose project down keeps its
    volumes unless you tick the box.
  - It needs the `docker` command. If Docker is missing or stopped, the
    tool says so and the rest of DevAssist works as before. Nothing runs
    `docker` until you open the tool.
- **A top line with the tools.** `1` and `2` switch between Repos and
  Docker, a click on a name does too, and `Ctrl-P` opens a list of tools
  that narrows as you type.
- `--doctor` prints a line about Docker.

### Changed

- The screen has one more line at the top, so the smallest terminal
  DevAssist draws on is now 48 by 13.
- The note about a newer version, "release to copy" and "Copied to the
  clipboard" moved from the repository overview to the right end of the
  top line, where every tool shows them.
- Help (`?`) lists the keys of the tool on screen, then the keys that work
  everywhere. Help, the version dialog (`v`) and copying text by dragging
  work the same in both tools.

## [0.3.6] - 2026-11-12

A maintenance release that finishes preparing the code for new features.

### Changed

- Reorganised the internals: the program now runs its repository overview
  as one feature inside a shell that can hold more. Nothing changes in how
  the app looks or behaves.

### Fixed

- When the app cannot start, a window opened from the Start menu or by a
  double click now stays open until you press Enter, so the error can be
  read. Before, the window closed at once.

## [0.3.5] - 2026-11-12

A maintenance release that continues preparing the code for new features.

### Changed

- In the group dialog, Ctrl-W deletes the last word, as it does in the other
  text fields. Ctrl-Shift-U and Ctrl-Shift-W now work wherever Ctrl-U and
  Ctrl-W do.
- Reorganised the internals further: text fields, lists, the actions menu
  and the folder dialog are now shared building blocks. Apart from the keys
  above, nothing changes in how the app looks or behaves.

## [0.3.4] - 2026-11-12

Fixes a settings file that stopped the app from starting, and continues
preparing the code for new features.

### Changed

- Reorganised the internals further: dialog frames, buttons, progress bars
  and text fitting are now shared building blocks. Nothing changes in how
  the app looks or behaves.

### Fixed

- Folding the repositories that are in no group no longer stops the app from
  starting. It wrote a line to the settings file that the app then refused
  to read ("collapsed needs a value"). Settings files that already have
  that line are read again.

## [0.3.3] - 2026-11-12

A maintenance release that continues preparing the code for new features.

### Changed

- Reorganised the internals further: progress tracking and background work
  are now shared building blocks that no longer assume git. Nothing changes
  in how the app looks or behaves.

## [0.3.2] - 2026-11-12

A maintenance release that prepares the code for new features.

### Changed

- Reorganised the internals: running programs, HTTP requests, the clipboard
  and path handling are now shared building blocks. Nothing changes in how
  the app looks or behaves.

## [0.3.1] - 2026-11-12

A maintenance release. It is also the first version that an installed 0.3.0
can update itself to.

### Changed

- Updated the libraries for HTTP (ureq 3), the terminal (ratatui 0.30) and
  hashing (sha2 0.11). Nothing changes in how the app looks or behaves.
- A self-update download that stops moving for 30 seconds is ended with a
  message, instead of waiting indefinitely.

## [0.3.0] - 2026-11-12

The app is now called DevAssist, and it no longer manages one folder: it keeps
a list of clones that can be anywhere on disk.

### Added

- Clones can be anywhere. Adding a repository asks for the folder to clone
  into, with Tab completion, and remembers it for next time.
- Import (`i`) adds clones you already have, from any folder.
- Groups: sort repositories into named groups that fold (`m`, `Enter`, `←` `→`).
- An actions menu on `Enter`: update, open the folder, open on GitHub, move to
  a group, copy the path, remove.
- `Space` selects several repositories, in the overview and in Add. Update,
  remove, open and move-to-group act on the selection.
- The sync column shows commits on remote branches that have no local branch,
  such as `3 new on dev`.
- Search any account or repository in Add; clones you already have stay
  visible, greyed out.
- A progress dialog for updates and clones, with a row and output per
  repository.
- A queue: actions started while another runs wait and run in order.
- Remove can forget a repository without deleting its folder.
- A first-run setup and a settings dialog (`c`) for the accounts to list.
- Self-update: DevAssist looks for a newer version at start-up and installs it
  in place (`v`).
- Installers: an MSI and a setup wizard.

### Changed

- The app was renamed from artcraft-repos to DevAssist. The command is now
  `devassist`.
- Settings moved to `%APPDATA%\DevAssist\devassist.toml`. Settings from the old
  name and location are taken over on the first start.
- Update fetches once, fast-forwards every local branch, and creates local
  branches for new remote ones, instead of pulling the checked-out branch only.
- A fresh clone gets local branches for its other remote branches.
- Clone progress covers the whole transfer, including resolving deltas.
- DevAssist fetches in the background at start-up, so the sync column is
  current without pressing `r`.
- The default heading is `DevAssist` instead of following the account name.
- `--root` is now `--import` and adds that folder's clones to the list. The
  old name still works.

### Fixed

- Local branches that were never pushed now count as ahead.

## [0.2.0] - 2026-10-21

The first version, as artcraft-repos: a terminal app that adds, updates and
removes GitHub clones in one folder.

[Unreleased]: https://github.com/AsP3X/DevAssist/compare/v0.5.1...HEAD
[0.5.1]: https://github.com/AsP3X/DevAssist/compare/v0.5.0...v0.5.1
[0.5.0]: https://github.com/AsP3X/DevAssist/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/AsP3X/DevAssist/compare/v0.3.6...v0.4.0
[0.3.6]: https://github.com/AsP3X/DevAssist/compare/v0.3.5...v0.3.6
[0.3.5]: https://github.com/AsP3X/DevAssist/compare/v0.3.4...v0.3.5
[0.3.4]: https://github.com/AsP3X/DevAssist/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/AsP3X/DevAssist/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/AsP3X/DevAssist/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/AsP3X/DevAssist/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/AsP3X/DevAssist/releases/tag/v0.3.0
