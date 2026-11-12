# DevAssist

[![CI](https://github.com/AsP3X/DevAssist/actions/workflows/ci.yml/badge.svg)](https://github.com/AsP3X/DevAssist/actions/workflows/ci.yml)
[![Latest release](https://img.shields.io/github/v/release/AsP3X/DevAssist)](https://github.com/AsP3X/DevAssist/releases/latest)
[![License: Apache-2.0 with Commons Clause](https://img.shields.io/badge/license-Apache--2.0%20%2B%20Commons%20Clause-blue)](LICENSE)

DevAssist is a terminal app that keeps all your local GitHub clones in one
overview. It shows which ones have something new, updates them on every
branch, clones new ones where you want them, and lets you sort them into
groups. A second tool manages Docker: containers, Compose projects, images
and volumes, with live logs. It works with the keyboard and with the mouse.

```
 DevAssist   in 3 folders
  7 installed · 2 behind · 1 new on other branches · 42 on GitHub · 35 not installed
  arrows move · enter actions · space selects · m group · / filter · s sort · ? help
  name                    branch            state   sync
  ▸ home  ·  2 repositories
▌ ▾ Work  ·  3 repositories  ·  2 to update
  api                     main              clean   behind 4
  portal                  feature/login     dirty   up to date
  web                     main              clean   3 new on dev
  ▾ No group  ·  2 repositories
  dotfiles                main              clean   up to date
  notes                   main              clean   ahead 1

   7 installed. 2 are behind. 1 has new commits on other branches.
   Group Work  ·  enter folds  ·  u updates it  ·  space selects it  ·  m renames
  Add a   Update u   Update all U   Remove d   Refresh r   Settings c   Quit q   Import i
```

## What it does

- **One overview for clones anywhere on disk.** DevAssist keeps a list of your
  checkouts. They do not have to share a folder.
- **Shows what is new.** For each clone: the branch, uncommitted changes, how
  far it is ahead or behind, and commits waiting on remote branches you have
  no local branch for.
- **Updates every branch.** An update fetches once, fast-forwards every local
  branch that is only behind, and creates local branches for new remote ones.
  It never merges or rebases, so your own commits are never touched.
- **Adds repositories.** Search your accounts, any other account, or paste a
  GitHub link, pick one or several, and choose the folder to clone into.
- **Imports what you already have.** Point it at a folder and its GitHub
  clones join the list. Nothing on disk changes.
- **Groups.** Sort repositories into named groups that fold. Groups only
  organise the list; they are not folders on disk.
- **Does several things in a row.** Actions you start while another is
  running wait in a queue and run one after another.
- **Manages Docker.** Open the Docker tool for containers grouped by
  Compose project, Compose projects, images and volumes. Start, stop and
  restart them, follow logs live, and remove or prune behind a dialog that
  lists exactly what will go. It needs the `docker` command and does
  nothing until you open it.
- **Updates itself.** DevAssist looks for a newer version and installs it in
  place.

## Install

DevAssist needs [Git](https://git-scm.com/) on your `PATH`. The Docker tool
needs `docker` there too, as Docker Desktop installs it; without it, only
that tool is unavailable. Releases are built for 64-bit Windows 10 and 11.

Download from the [latest release](https://github.com/AsP3X/DevAssist/releases/latest):

| File | Use it when |
|---|---|
| `devassist-<version>-setup.exe` | You want a setup wizard. It can install for you only, without administrator rights. **Recommended:** this is the install that can update itself. |
| `devassist-<version>-x64.msi` | You want a Windows Installer package for all users, for example to deploy it. |
| `devassist-<version>-x64.exe` | You want just the program. Put it anywhere and run it. |

Each release lists the SHA-256 of its files in `SHA256SUMS.txt`. The files are
not code-signed, so Windows SmartScreen may ask for confirmation on first run.

On Linux and macOS there are no downloads. DevAssist is written to build
there, but that is not tested; see [Building](docs/BUILDING.md).

## Quick start

1. Run `devassist` in Windows Terminal or PowerShell, or start **DevAssist**
   from the Start menu.
2. The first start asks which GitHub accounts to list. Your signed-in account
   is suggested.
3. Press `a` to add a repository. Pick one (or several with `Space`), press
   `Enter`, and choose the folder to clone into.
4. Already have clones? Press `i` and name the folder they are in.
5. Press `u` to update the highlighted repository, or `U` to update all.

For private repositories and higher rate limits, sign in with the
[GitHub CLI](https://cli.github.com/) (`gh auth login`) or set `GH_TOKEN`.
DevAssist then uses that sign-in. Without it, it sees public repositories only.

## Keys

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k` | Move |
| `Enter` | Open the actions menu for the selection. On a group: fold or unfold it |
| `Space` | Select several. `Ctrl-A` selects everything shown, `Esc` clears |
| `u` | Update the selection |
| `U` | Update every repository |
| `a` | Add a repository from GitHub |
| `i` | Import clones you already have |
| `m` | Put the selection into a group. On a group: rename it |
| `d`, `Delete` | Remove: delete from disk, or only take off the list. On a group: dissolve it |
| `o` | Open the folder in the file manager |
| `r`, `F5` | Fetch and refresh everything |
| `/` | Filter the list |
| `s` | Sort by name or by what needs attention |
| `l` | Show the last update or clone |
| `c` | Settings |
| `v` | Version: look for an update and install it |
| `?` | Help |
| `q` | Quit |

The mouse works too: click to select, double-click to update, click the
buttons, scroll with the wheel, and drag across any text to copy it.

## Updating DevAssist

DevAssist looks for a newer version when it starts. When there is one, the
header says so. Press `v`, then `Enter`: it downloads the new program, checks
it against the release's checksum, and puts it in place. `Enter` once more
restarts into the new version.

This works when DevAssist can write to its own folder, which is the case for
the setup wizard's "just for me" install and for the plain executable. An
install for all users in `Program Files` needs the installer again; DevAssist
tells you so and names the release page.

To switch the check off, put `update_check = off` in the settings file or
start with `--no-update-check`.

## Documentation

- [Using DevAssist](docs/USAGE.md): the overview, adding, updating, groups, the queue
- [Configuration](docs/CONFIGURATION.md): the settings file, command-line options, sign-in
- [Building](docs/BUILDING.md): building from source, tests, installers, project layout
- [Releasing](docs/RELEASING.md): how a release is made and how self-update works
- [Changelog](CHANGELOG.md)

## Contributing

Bug reports, ideas and pull requests are welcome. Start with
[CONTRIBUTING.md](CONTRIBUTING.md). Please report security problems privately,
as [SECURITY.md](SECURITY.md) describes.

## License

DevAssist is source-available under the
[Apache License 2.0 with the Commons Clause](LICENSE).

In short: you may use it, also at work, change it, and share it, for free.
You may not sell it, or sell a product or service whose value comes mainly
from it. Because of that condition it is not "open source" as the Open Source
Initiative defines the term. The [LICENSE](LICENSE) file is what counts; this
paragraph is only a summary.
