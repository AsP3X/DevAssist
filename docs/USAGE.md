# Using DevAssist

DevAssist runs full-screen in a terminal. Everything can be done with the
keyboard; the mouse works for selecting, clicking buttons, scrolling and
copying text. Press `?` in the app for the key list.

## The welcome screen and the tools

DevAssist starts on its welcome screen: the name, and a card for every
tool. A card says what the tool is for and, where the tool has something
to report, how things stand ("56 clones · 3 behind").

- **Repos** is the overview of your local clones.
- **Docker** lists containers, Compose projects, images and volumes. Its
  card has no status until you have opened it once: DevAssist asks Docker
  nothing before that.

| Key | Does |
|---|---|
| `←` `→` `↑` `↓`, `Tab` | Choose a card |
| `Enter` | Open that tool. A click on a card does the same |
| a letter | Search the tools: the palette opens with the letter typed |

## Going from one tool to another

`Ctrl-P` opens the palette: Home and every tool in one list, each with what
it is for. Type to narrow the list, by name or by what a tool does
("containers" finds Docker). `↑` `↓` move, `Enter` goes there, `Esc` or
`Ctrl-P` again closes it. A click on a line goes there too.

The palette works on the welcome screen and in any tool. While you type
into a field or a dialog is open, the key belongs to that field or dialog;
close it first.

"Home" in the palette is the welcome screen. So is a click on `DevAssist`
at the left of the top line, which says where you are:

```
 DevAssist › Docker                                  ctrl-p tools · ? help
```

You can choose another key for the palette, and have DevAssist start in a
tool instead of on the welcome screen: see `palette_key` and `start` in
[Configuration](CONFIGURATION.md).

A tool keeps working when it is not on screen: an update of your
repositories goes on while you look at Docker.

These keys work on the welcome screen and in every tool:

| Key | Does |
|---|---|
| `Ctrl-P` | The palette |
| `?` | The keys of what is on screen |
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
| pulls | The repository's pull requests on GitHub, by state. See below. |
| path | Where the checkout is on disk. A path that does not fit loses its middle, as in `C:\…\work\api`. |
| about | The repository's description from GitHub. |

The line under the list shows details for the highlighted row, such as its
full path. Every column is only as wide as its longest entry, and the room
that frees goes to the path first, so it is shown in full where the window
allows, and then to the description. Where the window is too narrow for the
whole path it is shortened in the middle, and with fewer than 16 columns left
for it the path column is left out.

### The pulls column

How many pull requests the repository has on GitHub, in three short pieces:

| Piece | Meaning |
| --- | --- |
| `○2` | 2 are open (green). |
| `✓14` | 14 were merged (purple). |
| `✕3` | 3 were closed without being merged (red). |
| `–` | The repository has no pull requests. |
| `+2` | 2 pull requests are new: the clone has not saved them (yellow). |
| `~1` | 1 saved pull request got other commits since (yellow). |

A state with none is left out, and large numbers are shortened, so `✓1.2k` is
about 1,200. The line under the list spells the highlighted row out:
`pull requests: 2 open, 14 merged, 3 closed`.

The numbers come from GitHub when DevAssist starts and when you press `r`, in
one request for all repositories. That needs a sign-in (`GH_TOKEN`, or
`gh auth login`); without one, and for a repository that is not on GitHub, the
column stays empty. It is left out in a window narrower than 100 columns. The
states count what is on GitHub, not what is saved in the clone: see
[Saving pull requests](#saving-pull-requests) for that.

The yellow pieces `+` and `~` compare GitHub with the clone. Every fetch, at
start and on `r`, looks at the pull requests of the remote without
downloading them and counts the ones the clone has not saved and the ones
whose commits changed. The line under the list says it in words, such as
`2 new, 1 changed to fetch: enter, p`. **Fetch the pull requests** in the
actions menu brings them and clears the mark. A repository whose pull
requests were never saved shows all of them as new, and with **Pull requests**
on in the settings the mark does not appear, because every fetch saves them
already. Only commits are compared: a new comment or review is not a change.

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

A branch that was force-pushed on the remote, as a bot does when it rebases
its pull request, cannot be fast-forwarded. If it is not checked out and holds
only what the remote had before, the update moves it to the rewritten branch.
If you committed to it, it is left alone like any other branch with commits of
its own.

The progress dialog shows one row per repository. For an update, the bar counts
branches: how many are done out of how many had something new. For a clone, it
follows the whole transfer, including resolving deltas. `↑` `↓` or a click pick
a repository to see its own output; `h` hides the dialog while the run goes on,
and `l` shows it again.

## Pulling a repository automatically

`w` watches the highlighted repository, or everything that is ticked. A
watched repository carries a `↻` after its name and is updated on a timer for
as long as DevAssist runs, also while another tool is on screen. `w` on a
repository that is watched already stops it.

An automatic pull does what `u` does: it fetches, fast-forwards every branch
that is only behind and adds new remote branches. Your own commits and
uncommitted changes are left alone, so a branch that cannot be fast-forwarded
simply stays behind.

It opens no dialog. The status line says when something came in or when a pull
failed, and says nothing new when there was nothing to bring in. A pull waits
while another task runs, and a task you start during a pull waits for it. `Esc`
cancels a running pull.

The first pull runs right after you start DevAssist or turn watching on, then
every 5 minutes. The settings (`c`) change the interval, from 1 minute to a
day. Which repositories are watched is remembered between starts.

## Saving pull requests

Switch **Pull requests** to `on` in the settings (`c`) and every fetch also
saves the commits of all pull requests of a repository in its clone: open,
merged and closed ones, also those that come from a fork. That covers the
fetch at start, `r`, an update and an automatic pull. It is off by default,
because a repository with thousands of pull requests takes a while the first
time.

To fetch them once, without the setting, choose **Fetch the pull requests** in
the actions menu (`Enter`, then `p`). It works on the highlighted repository
or on all ticked ones, brings the pull requests that are new and the ones
that got more commits, and says how many that were.

They are kept as `refs/pull/<number>/head`, the name GitHub gives them, apart
from your branches: no local branch is made for them and they never count as
something new. With a repository highlighted, the line under the status says
how many are saved. To look at one:

```
git log refs/pull/12/head
git switch -c pr-12 refs/pull/12/head
```

It is a backup, so nothing is removed again: a pull request that disappears
from GitHub stays in the clone. Only the commits are saved. Titles,
descriptions, comments and reviews are not.

## Cloning the forks

**Clone the forks** in the actions menu (`Enter`, then `n`) asks GitHub for
every fork of the highlighted repository, and for the forks of those forks,
and fetches them all into a folder beside the checkout: its name with `-fork`
added. The forks of `D:\code\api` go into `D:\code\api-fork`. With several
repositories ticked, each gets a folder of its own.

Every fork that changed something gets a folder of its own in there, named
after the account it belongs to: `D:\code\api-fork\bob` holds the files of
`bob`'s fork. A fork counts as changed when one of its branches has commits
the original does not have. Most forks never do, and get no folder: their
files would be the original's. The folder shows the fork's default branch
when the changes are there, and otherwise the branch with its newest commit
among those that have changes.

Behind the folders is one Git repository, in `api-fork\.git`, not one clone
per fork. Forks share nearly all their commits with the original, so they are
stored once, and hundreds of forks take about the room of one clone plus the
files of each folder. The original and every fork, changed or not, are
remotes of that repository, each named after its account:

| What | Where it is |
| --- | --- |
| The branches of `bob`'s fork | `bob/main`, `bob/feature`, … |
| The pull requests of `bob`'s fork | `refs/pull/bob/<number>/head` |
| The branches and pull requests of the original, owned by `ada` | `ada/main`, `refs/pull/ada/<number>/head` |

A fork's folder is a Git worktree, so Git works in it as in any checkout. To
look at more than its files:

```
cd api-fork\bob
git branch -r --list "bob/*"
git log ada/main..bob/feature
git switch --detach bob/feature
```

What the checkout has already is taken from disk, so only what the forks
added is downloaded. The progress dialog counts the forks. A fork that cannot
be fetched, because it is private or was deleted, is named there and the rest
goes on.

Choose the line again later to bring in new forks and new commits. The
folders follow their forks, and a fork that has changes by then gets its
folder. Files you changed in a folder are kept. Nothing is removed: a branch
or a fork that disappears from GitHub stays. A `-fork` folder from before
this layout, which had the original's files at the top, is brought over the
same way: those files go, unless you changed one, and the forks' folders
appear.
The folder is not put on the overview, and a folder of that name that is
something else is left alone. Only commits are saved: titles and comments of
pull requests are not. GitHub lists at most 10,000 forks of one repository
here.

### Updating them all

**Update all cloned forks** in the actions menu (`Enter`, then `a`) brings
every `-fork` folder up to date in one go: the forks of each repository on
the overview that has them cloned. What is highlighted or ticked does not
matter, and a repository whose forks were never cloned is left out.

An update is much quicker than the first run. GitHub says for each fork when
it was last pushed to and how many pull requests it has open, and a fork
where both are as they were is not fetched again: of hundreds of forks only
the few that changed are. Those are fetched eight at a time, as they are the
first time. The result says how many forks there are and how many were
fetched now. **Clone the forks** on a repository that has its folder already
does the same for that one, and also brings in forks made since.

A fork that could not be fetched is tried again every time. A pull request
opened on a fork from a third repository is seen with the fork's next push
or pull request.

A fork that GitHub still lists but no longer has, because it was deleted or
made private, is left out from then on: the run says so once and does not
try it again. What was fetched from it stays, its branches and its folder
too. Should it come back and change, it is fetched again.

### Looking at the forks

`f` on a repository opens the overview of its forks, once they are cloned.
**Show the forks** in the actions menu does the same. It takes the place of
the list of repositories and is read from the `-fork` folder, without asking
GitHub.

Each fork has a line: its owner and name, how many branches and pull requests
were fetched from it, how many commits its furthest branch is ahead of the
original's main branch, the date of its newest commit, and its state. The
state is `changes, folder` for a fork with commits of its own, `no changes`
for one that has none, and `gone from GitHub` for one that is no longer
there. The heading counts them.

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k`, `PgUp` `PgDn`, `Home` `End` | move |
| `Enter`, `o` | open the folder with the fork's files |
| `g` | open the fork on GitHub |
| `u` | fetch the forks of this repository |
| `s` | sort: most changes first, newest commit first, by name |
| `c` | show only the forks with changes, or all again |
| `/` | filter by owner or name |
| `r` | read the folder again |
| `Esc`, `←` | back to the repositories |

A double-click opens the folder too. After a fetch the overview reads the
folder again by itself.

A limit or a timeout does not end the run. Without a sign-in GitHub answers
only 60 requests an hour, enough to list about 6,000 forks. When that limit
is reached, the run pauses until GitHub resets it, at most an hour later, and
then goes on where it was. When GitHub does not answer, or the fetch of a
fork times out, it pauses and tries again, three times with longer pauses.
The progress dialog says why it pauses and counts down to when it goes on.
`Esc` cancels during a pause too.

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
- Auto-pull on or off
- Fetch the pull requests
- Show the forks
- Clone the forks
- Update all cloned forks
- Migrate, which opens a submenu
- Remove

Choose with `↑` `↓` and `Enter`, press the letter shown on the right, or click.

### Migrate: moving a repository to another folder

**Migrate** (`t`) opens a submenu; `Esc` goes back to the menu. Its line
**Move to another folder…** (`f`) asks for a folder and moves the highlighted
repository, or every ticked one, into it. Each keeps its folder name, so
`D:\code\api` moved to `E:\work` becomes `E:\work\api`. The folder is created
when it is not there yet.

The repository stays on the list with everything it had: its group, the
automatic pull and the saved pull requests. On the same drive the move is
instant. Onto another drive everything is copied first and the original is
deleted only after the copy is complete, so a failure halfway leaves the
original as it was. A move is refused when the target folder exists already.
Close programs that have files of the repository open first: Windows does not
move a folder that is in use.

### Migrate: pushing a repository to another service

**Push to another service…** (`s`) in the same submenu puts a complete copy
of one repository on another Git service, such as GitLab, Codeberg, Bitbucket
or a server of your own:

1. Make an empty repository on the other service, without a README.
2. Highlight the repository, press `Enter`, `t`, `s`, and type or paste the
   address of the new repository: `https://gitlab.com/you/name.git` or
   `git@codeberg.org:you/name.git`.
3. `Enter` pushes. The progress dialog shows what Git sends.

Every branch and every tag goes over. A branch is taken from what was last
fetched from GitHub when the local one is missing or only behind, so press `r`
first to have the newest state. The default branch is pushed first, which makes
it the default one over there on most services.

The repository itself does not change: it keeps GitHub as its origin and stays
on the list as before. The address is kept as one more remote, named after the
host (`gitlab`, `codeberg`), so doing it again later brings the copy up to
date, and `git push gitlab` works in a terminal.

What it does not do:

- It never forces. If the other repository already holds different commits,
  the push is refused and nothing there is overwritten.
- It does not create the repository on the other service, and it does not
  delete or archive the one on GitHub.
- Pull requests, issues, releases and wikis are not part of a Git repository
  and stay on GitHub. The saved pull request commits (`refs/pull`) are not
  pushed either: most services refuse them.
- It signs in with what Git already has for the other host, a saved login or
  an SSH key. DevAssist does not ask for a password and does not send your
  GitHub sign-in there. If the push is refused for the sign-in, run
  `git push` to that address once in a terminal so Git can ask and remember.

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

## The window (being built)

`devassist-gui` is a second program: a window that shows the same list of
repositories. It is not in the downloads. Build it from the source (see
[BUILDING.md](BUILDING.md)) and start it:

```
cargo run -p devassist-gui
```

It reads the same settings file as the terminal app. `--config PATH` reads
another one. `--demo` shows twelve made-up repositories instead of yours and
reads no settings file.

The window shows the repositories in their groups. Each row has its name,
branch, changes (`Clean` or `Uncommitted`), pull requests, and how far it is
behind and ahead. Above the table are a filter, the sort and, once something
is ticked, how many are ticked, with a button that clears the ticks.

You cannot change a repository from the window yet. Folding a group is saved
to the settings file, as in the terminal app, and so are clones it takes over
from an older place at start. What the program does by itself it also does
here: it fetches when it starts, and it pulls the repositories you watch when
their interval is over, as in the terminal app. With no repositories it says so
and points to the terminal app. With a settings file that has a mistake it
shows the mistake and loads nothing. In `--demo`, refresh does nothing.

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k` | Move the highlight |
| `PgUp` `PgDn` | Move it a page |
| `Home` `End` | Highlight the first or the last row |
| `←` `→` | Fold or unfold the highlighted group |
| `Space` | Tick or untick the highlighted row and move down |
| `Ctrl-A` | Tick every row shown |
| `s` | Change the sort |
| `/` | Type in the filter |
| `r`, `F5` | Fetch and read again |
| `Esc` | Clear the filter when it lets nothing through, otherwise clear the ticks |

A click on a row highlights it, on its box ticks it, on a group's header
highlights and folds it, and on the box in the head ticks every row shown.

While the filter has the keyboard, `↑` `↓` and `PgUp` `PgDn` still move the
highlight. `Home` and `End` belong to the text. `Tab` keeps the keyboard in the
filter. `Esc` clears the filter. The other keys of the table, such as `r` and
`F5`, act only when the filter does not have the keyboard: there `r` is typed
into the filter and `F5` does nothing.

Everything else, such as adding, updating, moving and removing, is still done
in the terminal app.

## Docker

Open it from the welcome screen or the palette. DevAssist asks Docker only
from that moment on; if you never open the tool, it never runs `docker`.

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

A project's header stands for its containers: with it highlighted, `s`,
`R` and `d` act on the project's containers and `Space` ticks them.

Keys only act on what the list shows. With a filter on, a project's header
stands for the containers the filter lets through, ticked rows the filter
hides are left alone, and nothing happens when the filter matches nothing.

In the Compose list, `s` stops a project when any of it runs and starts
its containers when none does. Start, stop, restart and down show their
output as it comes; `Esc` hides the output and the command goes on.
Starting uses the containers as they are. To create a project from its
compose files, or to apply a changed file, run `docker compose up`
yourself; the project then appears in the list.

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
| Containers | Lists the stopped ones, which `Enter` removes. Running ones are listed apart and stay, unless you press `f`: "Stop and remove all". `Enter` never does that, also when every container you chose is running. |
| Images | Lists the unused ones, which `Enter` removes. Images a container was made from are listed with that container and stay, unless you press `f`: "Remove anyway". A second name for an image that is in use under another name is removed as a name only. |
| Volumes | Lists the unused ones, which `Enter` removes with all data in them. A volume a container mounts is listed and always stays. |
| A Compose project (down) | Names the project's containers. Its volumes stay unless you tick "also remove its volumes" with `Space`. |

Prune looks at the whole list instead of your selection:

| In | `p` removes |
|---|---|
| Containers | Every stopped container |
| Images | Every image without a name that no container uses, and how much space that frees. Tick the box to include named images no container uses |
| Volumes | Every unnamed volume no container mounts. Tick the box to include named ones |

The dialog shows exactly what will be removed, and only that is removed.
When the list is longer than the window, the dialog says how many names it
could not show; make the terminal taller to read them all before you
confirm.

Projects are taken down one at a time.

## Copying text

Drag across any text and release the mouse to copy it. `Ctrl-C` copies a
selection too, and quits when nothing is selected. This works in every
tool and every dialog.
