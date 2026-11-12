# Changelog

What changed in each version of DevAssist. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[semantic versioning](https://semver.org/).

## [Unreleased]

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

[Unreleased]: https://github.com/AsP3X/DevAssist/compare/v0.3.4...HEAD
[0.3.4]: https://github.com/AsP3X/DevAssist/compare/v0.3.3...v0.3.4
[0.3.3]: https://github.com/AsP3X/DevAssist/compare/v0.3.2...v0.3.3
[0.3.2]: https://github.com/AsP3X/DevAssist/compare/v0.3.1...v0.3.2
[0.3.1]: https://github.com/AsP3X/DevAssist/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/AsP3X/DevAssist/releases/tag/v0.3.0
