# Using DevAssist

DevAssist runs full-screen in a terminal. Everything can be done with the
keyboard; the mouse works for selecting, clicking buttons, scrolling and
copying text. Press `?` in the app for the key list.

## Tools

DevAssist has two tools. The top line lists them:

```
  1 Repos   2 Docker                                  ctrl-p tools · ? help
```

- **Repos** is the overview of your local clones. It is what you see first.
- **Docker** lists containers, Compose projects, images and volumes.

Press the number of a tool to switch to it, or click its name. `Ctrl-P`
opens a list of the tools that narrows as you type; `Enter` goes to the
highlighted one. While you type into a field or a dialog is open, the
numbers are typed, not switched on.

A tool keeps working when it is not on screen: an update of your
repositories goes on while you look at Docker.

These keys work in every tool:

| Key | Does |
|---|---|
| `?` | The keys of the tool on screen |
| `v` | The version dialog: look for a newer DevAssist and install it |
| `Ctrl-C` | Copies selected text; quits when nothing is selected |
| `q` | Quits |

The right end of the top line says when a newer version is out and what a
copy did.

## The overview

Each row is one local clone.

| Column | Meaning |
|---|---|
| name | The folder's name. Two clones with the same folder name show as `work/api` and `home/api`. |
| branch | The branch that is checked out, or `detached`. |
| state | `clean`, or `dirty` when there are uncommitted changes. |
| sync | How the clone compares with GitHub. See below. |
| about | The repository's description from GitHub. |

The line under the list shows details for the highlighted row, such as its
full path.

### The sync column

| Text | Meaning |
|---|---|
| `up to date` | Every local branch matches the branch it follows. |
| `behind 4` | The remote has 4 commits you do not have. Counted over all local branches. |
| `ahead 2` | You have 2 commits the remote does not have, including on branches that were never pushed. |
| `3 new on dev` | The remote branch `dev` has 3 commits and you have no local branch for it. |
| `12 new on 3 branches` | The same, spread over several remote branches. |
| `no upstream` | A local branch follows no remote branch. |
| `upstream gone` | The branch it followed was deleted on the remote. |
| `detached` | No branch is checked out. |
| `fetch failed` | The last fetch did not work; the highlighted row's detail line says why. |
| `folder is missing` | The clone is on the list but its folder is gone. |

DevAssist fetches in the background when it starts, and again when you press
`r`. Between fetches the column shows what the last fetch found.

Rows that need attention are coloured: yellow for behind, dirty or a problem;
magenta when the only news is on other remote branches; cyan when you are
ahead. Press `s` to sort by attention instead of by name.

## Adding a repository

Press `a`. The dialog lists the repositories of your configured accounts.
Clones you already have stay visible, greyed out.

- Type to search the list.
- Type `owner/` or paste a link to an account to list all of its repositories.
- Type `owner/name` or paste a repository link to add exactly that one.
- `Space` ticks several repositories.
- `Enter` asks for the folder to clone into. `Tab` completes folder names, `~`
  is your home folder, and a folder that does not exist yet is created. The
  last folder you used is filled in next time.

After the clone, DevAssist creates local branches for the repository's other
remote branches, so the clone starts out `up to date` on all of them.

If the folder you chose already holds a clone of that repository, it is put on
the list instead of being cloned again.

## Importing clones you already have

Press `i` and name a folder. Every GitHub clone directly inside it joins the
list, and so does the folder itself if it is one. Nothing on disk is moved or
changed. You can also start DevAssist with `--import PATH`.

## Updating

- `u` updates the highlighted repository, or everything that is ticked.
- `U` updates every repository that has something new.

An update of one repository does this:

1. Fetches from the remote, once.
2. Fast-forwards the checked-out branch.
3. Fast-forwards every other local branch that is only behind.
4. Creates a local branch for each branch on `origin` that has new commits and
   no local branch yet.

Nothing is merged or rebased. A branch that has commits of its own is left
alone, and the progress dialog says so.

The progress dialog shows one row per repository. For an update, the bar counts
branches: how many are done out of how many had something new. For a clone, it
follows the whole transfer, including resolving deltas. `↑` `↓` or a click pick
a repository to see its own output; `h` hides the dialog while the run goes on,
and `l` shows it again.

## Selecting several

`Space` ticks the highlighted row and moves down. `Ctrl-A` ticks everything the
list shows, which respects the filter. `Esc` clears the ticks. With rows
ticked, update, remove, open and move-to-group act on all of them.

## The actions menu

`Enter` on a repository, or with several ticked, opens a menu:

- Update
- Open the folder
- Open on GitHub
- Move to a group
- Copy the path
- Remove

Choose with `↑` `↓` and `Enter`, press the letter shown on the right, or click.

## Groups

Groups sort the overview. They are not folders on disk and moving a repository
between groups changes nothing on disk.

- `m` puts the highlighted or ticked repositories into a group. Type a name: a
  new name makes a new group. `↑` `↓` in the dialog walk through the groups
  that exist. An empty name takes the repositories out of their group.
- `Enter`, `←` `→` or a click on a group's header fold and unfold it. A folded
  group still shows how many repositories it holds and how many have updates.
- With a group's header highlighted, `u` updates the whole group, `Space`
  ticks it, and `o` opens its folders.
- `m` on a header renames the group.
- `d` on a header dissolves the group. Its repositories stay on the list.

A group exists as long as something is in it.

## Removing

`d` asks first, and offers two things:

- **Delete** removes the folder from disk.
- **Forget** only takes the repository off the list; the folder stays.

DevAssist only ever deletes a folder that is a git checkout. The repository on
GitHub is never touched.

## Doing several things at once

Git commands run one at a time. If you start something while another task is
running, it is queued and runs afterwards; the status line shows what is next.
`Esc` cancels the running task and drops the queue. When a task fails, the
queue waits until you close its dialog, so the error is not lost.

## Filtering

`/` filters the list by name, owner, branch or path. `Esc` clears the filter.
The filter also looks inside folded groups.

## Docker

Press `2`. DevAssist asks Docker only from that moment on; if you never
open the tool, it never runs `docker`.

The tool needs the `docker` command, as Docker Desktop installs it. If
Docker is not installed or not running, the tool says so and `r` tries
again. The rest of DevAssist is not affected.

### The four lists

`Tab` and `Shift-Tab` move between them, and so does a click on a name.

| List | Shows |
|---|---|
| Containers | Every container, running or not: name, image, status and published ports as `8081→80`. Containers of a Compose project sit under the project's header. |
| Compose | Every Compose project: how many of its containers run, and its compose files. |
| Images | Every image: name, ID, size, age, and the containers made from it. |
| Volumes | Every volume: name, driver, and the containers that mount it. A volume Docker named itself shows the first 12 characters. |

`●` marks a running container, `○` one that is not. The lists follow
Docker by themselves: something started or stopped elsewhere shows up
within a second. `r` asks again by hand.

### Keys

| Key | Does |
|---|---|
| `↑` `↓`, `j` `k`, `PgUp` `PgDn`, `Home` `End` | Move |
| `Space` | Tick the row; `Ctrl-A` ticks everything shown; `Esc` clears |
| `Enter` | The actions for the row or the ticked rows. On a project's header: fold it |
| `s` | Start what is stopped, stop what runs |
| `R` | Restart |
| `l` | Logs |
| `d`, `Delete` | Remove. In Compose: take the project down |
| `p` | Prune: remove everything unused in this list |
| `/` | Filter the list; `Esc` clears the filter |
| `r`, `F5` | Ask Docker again |

A project's header stands for all of its containers: with it highlighted,
`s`, `R` and `d` act on the whole project's containers and `Space` ticks
them.

In the Compose list, `s` brings a project up from its compose files when
some of it is not running, and stops it when all of it runs. Up, stop,
restart and down show their output as it comes; `Esc` hides the output
and the command goes on.

Actions run one at a time. One started while another runs waits and runs
after it; the status line shows what is next.

### Logs

`l` on a container, or on a project in the Compose list, opens its log
over the whole tool and follows it.

| Key | Does |
|---|---|
| `↑` `↓`, `PgUp` `PgDn`, `Home` | Scroll. Scrolling up stops following |
| `End`, `f` | Follow the newest line again |
| `/` | Show only lines that contain the text you type |
| `Esc`, `q` | Close, which stops reading the log |

The view shows the last 500 lines and what comes after, and keeps at most
5,000. Drag to copy, as everywhere.

### Removing and pruning

Nothing is removed by one key. `d` and `p` open a dialog that lists by
name what will go.

| You remove | The dialog |
|---|---|
| Containers | Lists the stopped ones, which `Enter` removes. Running ones are listed apart and stay, unless you press `f`: "Stop and remove all". |
| Images | Lists the unused ones, which `Enter` removes. Images a container was made from are listed with that container and stay, unless you press `f`: "Remove anyway". |
| Volumes | Lists the unused ones, which `Enter` removes with all data in them. A volume a container mounts is listed and always stays. |
| A Compose project (down) | Names the project's containers. Its volumes stay unless you tick "also remove its volumes" with `Space`. |

Prune looks at the whole list instead of your selection:

| In | `p` removes |
|---|---|
| Containers | Every stopped container |
| Images | Every image without a name that no container uses, and how much space that frees. Tick the box to include named images no container uses |
| Volumes | Every unnamed volume no container mounts. Tick the box to include named ones |

The dialog shows exactly what will be removed, and only that is removed.

## Copying text

Drag across any text and release the mouse to copy it. `Ctrl-C` copies a
selection too, and quits when nothing is selected. This works in every
tool and every dialog.
