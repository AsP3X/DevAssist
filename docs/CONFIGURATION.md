# Configuration

## Where the settings are

DevAssist keeps its settings and its list of clones in one file per user:

| System | File |
|---|---|
| Windows | `%APPDATA%\DevAssist\devassist.toml` |
| Linux, macOS | `~/.config/devassist/devassist.toml` (or under `$XDG_CONFIG_HOME`) |

Because the file is per user, DevAssist can be started from any folder and
installed anywhere. `--config PATH` uses another file.

The app writes this file itself: when you save the settings dialog (`c`), and
whenever the list of clones or the groups change. Comments you add by hand are
lost when it is rewritten.

## The settings file

One `key = value` per line. Lines that start with `#` are comments.

```
title = My Tools
owner = my-user
owner = my-org
repo = someone/tool
protocol = ssh
update_check = off
palette_key = ctrl-k
start = repos
clone_dir = C:\dev
checkout = C:\dev\tool
group = Client work
checkout = D:\work\client\api
checkout = D:\work\client\web
collapsed = Client work
watch = C:\dev\tool
watch_interval = 15
pull_requests = on
```

| Key | Meaning |
|---|---|
| `title` | The heading shown in the app. Default: `DevAssist`. |
| `owner` | A GitHub user or organisation whose repositories the Add dialog lists. Repeat for several. `org` is the same key. |
| `owners = none` | List no account. Repositories are then added by `owner/name`. |
| `repo` | One repository to list in Add as well, as `owner/name`. |
| `protocol` | How clones are fetched: `auto`, `https` or `ssh`. `auto` follows what your existing clones use, and falls back to https. |
| `update_check` | `on` or `off`. Whether DevAssist looks for a newer version when it starts. Default: on. |
| `palette_key` | The key that opens the palette, the list that takes you to a tool. Default: `ctrl-p`. Write `ctrl-`, `alt-` or `shift-` and a letter or digit, or `f1` to `f12` alone or with one of those: `ctrl-k`, `alt-t`, `f2`, `ctrl-shift-p`. A letter or digit needs `ctrl` or `alt`, so that typing stays typing, and `ctrl-c` is taken. A key your terminal keeps for itself never reaches DevAssist. |
| `start` | Where DevAssist starts: `home` for the welcome screen, or a tool's name, `repos` or `docker`. Default: `home`. A name no tool has starts at home. |
| `theme` | `light`, `dark` or `system`. Whether the window is light or dark, or follows the system. Default: `system`. The terminal app keeps this setting and does not use it; it is for the window. |
| `backdrop` | `lake`, `midday`, `golden`, `dusk`, `night` or `forest`. The picture behind the window. Default: `lake`. The terminal app keeps this setting and does not use it; it is for the window. |
| `blur` | `on` or `off`. Whether the window blurs the picture behind it. Default: on. The terminal app keeps this setting and does not use it; it is for the window. |
| `clone_dir` | The folder the Add dialog offers for a new clone. It follows the folder you used last. |
| `checkout` | The path of a clone on the list. One line per clone. |
| `group` | Starts a group. The `checkout` lines after it are in that group, up to the next `group` line. Checkouts before the first `group` line are in no group. |
| `collapsed` | A group that is folded shut. |
| `watch` | The path of a clone that is pulled automatically while DevAssist runs. One line per clone; it has to be on the list as a `checkout` too. `w` in the overview sets it. |
| `watch_interval` | Minutes between those pulls, from 1 to 1440. Default: 5. |
| `pull_requests` | `on` or `off`. Whether every fetch also saves the commits of all pull requests in the clone, as `refs/pull/<number>/head`. Default: off. |

If no `owner` is set, DevAssist uses the account your clones belong to. When
they belong to several, it uses the account you are signed in with.

## Command-line options

```
devassist [--owner NAME]... [--repo OWNER/NAME]... [--import PATH]...
devassist --doctor
devassist --help
```

| Option | Meaning |
|---|---|
| `--import PATH` | Add every GitHub clone directly inside `PATH` to the list. `--root` is the same option. |
| `--owner NAME` | List this GitHub user or organisation. Repeat for several. `--org` is the same. |
| `--no-owner` | List no account. |
| `--repo OWNER/NAME` | Also list this one repository. A GitHub link works too. |
| `--protocol MODE` | `auto`, `https` or `ssh`. |
| `--title TEXT` | The heading shown in the app. |
| `--config PATH` | Use this settings file. |
| `--no-update-check` | Do not look for a newer version at this start. |
| `--doctor` | Print the version, the settings and the state of every clone, then exit. Useful in a bug report. |
| `--help` | Print the help. |

Options apply to that start. `--import` is the exception: the clones it finds
are saved to the list.

## Signing in to GitHub

DevAssist does not ask for a password and stores no credentials. It uses a
token from the first of these that has one:

1. The `GH_TOKEN` environment variable
2. The `GITHUB_TOKEN` environment variable
3. The [GitHub CLI](https://cli.github.com/), by running `gh auth token`

With a token it can list and clone your private repositories, and GitHub's
rate limits are much higher. Without one it sees public repositories only.

For `git` itself, DevAssist passes the token to GitHub over https for that one
command. Clones that use ssh rely on your ssh keys as usual.

## Settings from older versions

Before version 0.3.0 the app was called artcraft-repos and managed a single
folder. On its first start, DevAssist takes over:

- the settings in `%APPDATA%\artcraft-repos`, and
- an `artcraft-repos.toml` in the folder it is started in or beside the
  executable, together with the clones in that folder.

The old files are left where they are.
