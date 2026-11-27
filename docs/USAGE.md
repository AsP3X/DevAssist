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
something else is left alone: it is not taken for the forks, and cloning the
forks into it is refused with "D:\code\api-fork is there already and does not
hold the forks of ada/api". Only that repository's part of the run fails;
the folder is not touched. Only commits are saved: titles and comments of
pull requests are not. GitHub lists at most 10,000 forks of one repository
here.

### Updating them

Once the forks of a repository are cloned, its line in the actions menu
reads **Update the forks** instead of **Clone the forks**, on the same key
(`Enter`, then `n`). It brings the forks of the highlighted repository up to
date, or of every ticked one, and also brings in forks made since. `u` in
the fork overview does the same for the repository shown there.

**Update the forks of all repositories** in the actions menu (`Enter`, then
`a`) brings every `-fork` folder up to date in one go: the forks of each
repository on the overview that has them cloned. What is highlighted or
ticked does not matter, and a repository whose forks were never cloned is
left out.

An update is much quicker than the first run. GitHub says for each fork when
it was last pushed to and how many pull requests it has open, and a fork
where both are as they were is not fetched again: of hundreds of forks only
the few that changed are. Those are fetched eight at a time, as they are the
first time. The result says how many forks there are and how many were
fetched now.

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
folder again by itself. When a read fails, the forks that were listed go, so
nothing old is acted on: the list says "The forks could not be read. r tries
again." and the line under the status gives Git's own words. An overview that
is open while its repository moves reads the forks at the new place from then
on. The window has this overview too (see "The forks" under "The window").

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
- Clone the forks, or Update the forks once they are cloned
- Update the forks of all repositories
- Migrate, which opens a submenu
- Remove

Choose with `↑` `↓` and `Enter`, press the letter shown on the right, or click.

### Migrate: moving a repository to another folder

**Migrate** (`t`) opens a submenu; `Esc` goes back to the menu. Its line
**Move to another folder…** (`f`) asks for a folder and moves the highlighted
repository, or every ticked one, into it. Each keeps its folder name, so
`D:\code\api` moved to `E:\work` becomes `E:\work\api`, and the question says
"each repository keeps its folder name and its forks inside". The folder is
created when it is not there yet.

The repository stays on the list with everything it had: its group, the
automatic pull and the saved pull requests. On the same drive the move is
instant. Onto another drive everything is copied first and the original is
deleted only after the copy is complete, so a failure halfway leaves the
original as it was. A move is refused when the target folder exists already.
Close programs that have files of the repository open first: Windows does not
move a folder that is in use.

The forks go along. When the repository has a forks folder beside it
(`D:\code\api-fork`, see "Cloning the forks"), it moves after the repository
to the same place under the new folder (`E:\work\api-fork`), the same way:
renamed on the same drive, copied and then deleted onto another. Its fork
folders are then repaired with `git worktree repair`, so they work at the new
place, and the next fetch of the forks takes up where the last one stopped.
The progress dialog says "Its forks folder went along to E:\work\api-fork."

Only a forks folder DevAssist made goes along: one whose Git configuration
says whose forks it holds (`devassist.forksof`, which cloning the forks sets).
Any other folder called `api-fork`, such as a clone of your own, stays where
it is, untouched, and the move says nothing about it. So does a folder that is
on the list as a repository of its own: it moves only when you move it. The
menu and `f` count the forks as cloned only for a forks folder DevAssist
made. A folder whose `.git` is there but whose configuration cannot be read
stays where it is too, and the move names it: "The forks folder of api
stayed at D:\code\api-fork: its Git configuration could not be read (…)."

When something is at that place already, or another repository of the same
move goes there, the forks folder cannot follow. The progress dialog says so
before the repository moves ("The forks folder of api cannot follow:
something is at E:\work\api-fork already."), and the repository moves all the
same. The forks folder stays where it was, whole, as it does when its move
fails or is cancelled. The run then ends with a fault that names it, which the
progress dialog and the status line keep: "Moved api to E:\work. The forks
folder of api stayed at D:\code\api-fork: something is at E:\work\api-fork
already." If the forks folder moved but its fork folders could not be
repaired, the run ends with a fault that says so: "… Fetch the forks again to
repair them." Every fetch of the forks repairs the fork folders of a forks
folder that was moved before it does anything else with them. The repair
needs Git 2.30 or newer: with an older Git it fails at the move and at each
fetch, which then says "The fork folders could not be repaired: …" and
removes nothing, so they can still be repaired once Git is updated.

### Migrate: pushing a repository to another service

**Push to another service…** (`s`) in the same submenu puts a complete copy
of one repository on another Git service, such as GitLab, Codeberg, Bitbucket
or a server of your own:

1. Make an empty repository on the other service, without a README.
2. Highlight the repository, press `Enter`, `t`, `s`, and type or paste the
   address of the new repository: `https://gitlab.com/you/name.git` or
   `git@codeberg.org:you/name.git`.
3. `Enter` pushes. The progress dialog shows what Git sends.

Every branch, every tag and every pull request goes over. DevAssist fetches
from GitHub first, the pull requests too, whatever the **Pull requests**
setting says, so the copy has what GitHub has now. A branch is taken from
GitHub when the local one is missing or only behind. The default branch is
pushed first, which makes it the default one over there on most services.

A pull request arrives as a branch named `pull/<number>`: open, merged and
closed ones alike, also those from forks and those whose branch is deleted.
Git cannot create a pull request on another service, so the title, the
description and the comments do not go over; from the branch you can open a
merge request there. When a branch of your own is called `pull` or starts
with `pull/`, the branches are named `pull-request/<number>` instead. Doing
it again moves each of these branches to where its pull request is now.

When GitHub cannot be reached, or the repository is gone there, the push
still happens with what is saved in the checkout, and the last line says so.

The repository itself does not change: it keeps GitHub as its origin and stays
on the list as before. The address is kept as one more remote, named after the
host (`gitlab`, `codeberg`; `mirror` for a local folder or an address without
a host), so doing it again later brings the copy up to date, and
`git push gitlab` works in a terminal.

A local folder works as an address too, for a bare repository on a disk: it
must be given by its full path, and it cannot be the repository itself or a
folder inside it. A path that is not a full one is refused.

If you cancel a push, the status line says that what was already sent stays
on the other service and that the address was not kept.

What it does not do:

- It never forces a branch or a tag. If the other repository holds commits
  that the branch being pushed lacks, the push stops with an error, and what
  was sent before it stays. The branches of pull requests are the one thing
  that is replaced there, since they follow what the pull request is now.
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

From the window you can update, watch, open, group and remove the repositories
that are on the list, add repositories from GitHub, import clones you already
have, move repositories to another folder and push one to another Git
service. It also shows the forks of a repository, fetches them and brings them
up to date (see "The forks" below). Settings are still done in the terminal
app. Folding a group is saved to the settings file, as in
the terminal app, and so are clones it takes over from an older place at
start. What the program does by itself it also does here: it fetches when it
starts, and it pulls the repositories you watch when their interval is over.
With no repositories it says "No repositories yet" and names Add repository
and Import. With a
settings file that has a mistake it shows the mistake and loads nothing.

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k` | Move the highlight |
| `PgUp` `PgDn` | Move it a page |
| `Home` `End` | Highlight the first or the last row |
| `←` `→` | Fold or unfold the highlighted group |
| `Space` | Tick or untick the highlighted row and move down |
| `Ctrl-A` | Tick every row shown |
| `Enter` | Open the menu of the highlighted row, as its "…" does; on a group's header, fold the group (with rows ticked, open the menu for them) |
| `a` | Open Add, to find repositories on GitHub and clone them |
| `i`, `I` | Open Import, to add the clones in a folder |
| `f`, `F` | Open the forks of the highlighted repository, or of the one ticked, in the table's place |
| `u` | Update the ticked rows, or the highlighted row (all of a highlighted group) |
| `U` | Update every repository |
| `w` | Turn the automatic pull on or off for the ticked rows, or the highlighted row |
| `o` | Open the folders of the ticked rows, or the highlighted row |
| `m` | Move the ticked rows, or the highlighted row, into a group; on a group's header, rename the group |
| `d`, `Delete` | Ask before removing the ticked rows, or the highlighted row |
| `l` | Show the progress of the run again after it was hidden |
| `s` | Change the sort |
| `/` | Type in the filter |
| `r`, `F5` | Fetch and read again |
| `Esc` | Clear the filter when it lets nothing through; else stop work that has no dialog, such as a deletion from disk; else clear the ticks |

`U` is a capital letter. With Caps Lock on, `u` types `U` and updates every
repository, as it does in the terminal app. A key that acts, such as `u`, `w`
or `d`, acts once when it is held down. `w`, `o`, `m`, `l` and `s` also take
their capital letter; `i` takes `I` and `f` takes `F`; `a` and `d` only the
small one.

A click on a row highlights it, on its box ticks it, on a group's header
highlights and folds it, and on the box in the head ticks every row shown. A
row's "…" opens its menu. If the row is not ticked, the menu is for that row
alone and the other ticks are cleared; if it is ticked, the menu is for every
ticked row. In the top row, beside the window's own buttons, "Import" opens
Import, "Update all" updates every repository, "Update all forks" brings the
forks of every repository that has them cloned up to date, and "Add
repository" opens Add; they are there with no repositories too. "Update all
forks" does what "Update the forks of all repositories" does in the row's
menu, whatever is ticked or highlighted; with no forks cloned anywhere it says
"No forks are cloned yet. Clone the forks of a repository first." on the
status line. In a narrow window the buttons show their icons only. Once
something is ticked, a bar above the table says how many, with Update, Move to
group, Open folder and Remove, and a button that clears the ticks. A double
click on a button acts once. A double click on a row does not update, as it
does in the terminal app.

While the filter has the keyboard, `↑` `↓` and `PgUp` `PgDn` still move the
highlight. `Home` and `End` belong to the text. `Tab` keeps the keyboard in the
filter. `Esc` clears the filter. The other keys of the table, such as `r`,
`u` and `d`, act only when the filter does not have the keyboard: there they
are typed into the filter, and `F5` does nothing. When a dialog comes up by
itself while you type in the filter, the filter has the keyboard again once
the dialog goes.

### The menu

The menu has these lines, in the words and with the keys of the terminal app's
menu:

| Line | Key |
|---|---|
| Update | `u` |
| Open the folder (the folders) | `o` |
| Open on GitHub | `g` |
| Move to a group… | `m` |
| Copy the path (the paths) | `c` |
| Auto-pull on or off | `w` |
| Fetch the pull requests | `p` |
| Show the forks | `f` |
| Clone the forks, or Update the forks | `n` |
| Update the forks of all repositories | `a` |
| Migrate | `t` |
| Remove… | `d` |

`↑` `↓` (or `k` `j`) move the highlight and stop at the ends. `Home` and `End`
go to the first and the last line. `Enter` runs the highlighted line, a line's
key runs that line, and `Esc` or `q` closes the menu. A click on a line runs
it; a click outside the menu closes it and goes no further. Migrate opens a
side menu (see below). The second of the forks' lines reads "Update the forks"
once every repository the menu is for has its forks cloned, and "Clone the
forks" otherwise. A rule sets the forks' three lines apart. "Show the forks"
does what `f` does on the list (see "The forks" below), and "Update the forks
of all repositories" does what "Update all forks" in the top row does.

### The dialogs

While a menu or a dialog is open, it takes every key and click, and the table
under it takes none. `Esc` closes it. `Tab` and `Shift-Tab` walk a dialog's
buttons, and `Enter` presses the main one.

- **Progress.** A run, such as an update, shows its tasks, each with a bar,
  and the output of the one you pick. While the run goes on,
  Cancel stops it and Hide closes the dialog and lets the run go on; `Esc`
  hides it too, it does not cancel. A hidden run is said in the status bar.
  `l`, or a click on the status bar, brings it back. When the run has ended
  the button is Close. A run that went well closes the dialog by itself a
  moment after it ended; one that failed stays until you close it.
- **Remove.** It names the repositories, up to three, then counts them, and
  says for each folder whether it is clean or has uncommitted changes, or
  "not read yet" while its state is not known (it is still being read, or
  could not be read); a folder that is no longer on the list shows only its
  name. Cancel
  leaves everything. Forget takes the repositories off the list and leaves
  their folders. Delete from disk deletes the folders, and the status line
  says how it went; `Esc` on the list stops it. The repositories on GitHub are
  never touched. No button is the main one: `Enter` does nothing
  until `Tab` has picked a button, and `Esc` cancels. On a group's header with
  nothing ticked, `d` does not dissolve the group as in the terminal app. It
  says "Pick repositories to remove."
- **Group.** It asks for a group name, with the group of the first repository
  in the field. `Up` and `Down` in the field walk through the groups there
  are, and a click on a group puts its name in the field. A new name makes a
  new group, and an empty name takes the repositories out of their group. A
  name is 40 characters at most. On a group's header (`m`) the same dialog
  renames the group.

`Esc` does not cancel a run that has a progress dialog, shown or hidden; its
Cancel button does (`l` brings a hidden one back). Work without a dialog, a
deletion from disk or the automatic pull, stops with `Esc` on the list. A
second `Esc` after closing a dialog stops a running deletion or automatic
pull; a held `Esc` does nothing more than its first press.

### Adding repositories

`a`, or "Add repository" in the top row, opens "Add a repository". Its search
field has the keyboard. Under it a line says how things stand: how many
repositories are listed and how many of them you have cloned, "Loading
repositories from GitHub…" while GitHub is asked, or GitHub's own error.
With an error, a Retry button beside the line asks GitHub again.

- Type to search the list. A search has no spaces, so `Space` is never typed
  there.
- Type `owner/` or paste a link to an account to list all of its
  repositories. Type `owner/name` or paste a repository link to add exactly
  that one. When nothing in your list matches a plain word, the box offers
  "List github.com/" and the word: a click on it, or `Enter`, lists that
  account.
- `↑` `↓` move the highlight. `Space` ticks the highlighted row, so you can
  tick several; so does a click on the row's box. A click elsewhere on a row
  highlights it.
- A repository you have cloned already is greyed out, marked "Installed" and
  cannot be ticked. Others are marked "archived" or "private" where they are.
- "Clone into" holds the folder. It starts with the folder you used last.
  `~` is your home folder, and a folder that does not exist yet is created.
  In that field `Space` types, and the folder field's own keys work (see
  below).
- The main button says what it will clone: "Clone 2 repositories" for what is
  ticked, else "Clone" and the name of the highlighted repository. It is off
  while there is nothing to clone. `Enter` presses it, in the search and in
  the folder field.

A repository that is ticked stays ticked when you change the search so that
the list no longer shows it, and it is cloned all the same. A line above the
buttons names those ("Also ticked: owner/a, owner/b and 3 more"), and the
button counts them.

When the folder cannot be used, the dialog stays open and says why under the
field. Otherwise it closes and the progress dialog shows the clones. Each
repository goes into a folder of its own, named after it, inside the one you
chose. If that name is taken by a different repository, the folder is named
`owner-name` instead; a clone of the same repository that is there already is
put on the list instead of being cloned again. `Esc`, Cancel and a click outside the dialog close it.

### Importing clones

`i`, or "Import" in the top row, opens "Import clones you already have". Its
field "Folder" starts with the folder you cloned into last. Every GitHub
clone directly inside the folder you name is put on the list, and so is the
folder itself if it is one. Nothing on disk is changed. The status line says
how many were added, that everything there was on the list already, or that
the folder holds no GitHub clones. When the folder cannot be used, the dialog
stays open and says why under the field.

### The folder field

Add's "Clone into", Import's "Folder" and Move's "Move into" are one kind of
field. A folder is given by its full path, or from `~`.

While the field has the keyboard, the folders that match what you typed show
under it: those whose names start with the last part of the text, whatever
the case, up to forty. Six rows show at a time; the wheel scrolls the rest.

| Key | Does |
|---|---|
| `↓` | Light the first suggestion, then the next; it stops at the last |
| `↑` | Light the one above; from the first, back into the field |
| `Enter` | With a suggestion lit, take it: the text becomes that folder with a separator at its end, so you can go on into it. With none lit, `Enter` is the dialog's |
| `Esc` | Close the suggestions; they come back when the text changes. A second `Esc` closes the dialog |

A click on a suggestion takes it. A long folder is cut from the left so its
last name shows, and all of it shows when you point at it. There is no `Tab`
completion in the window (`Tab` moves between a dialog's controls) and no
folder picker of the system.

### Moving repositories

**Migrate**, then **Move to another folder…**, asks for a folder and moves the
chosen repositories into it. The dialog's title names them ("Move api and web
to another folder", "Move 5 repositories to another folder"), and its field
"Move into" starts at the folder the first one is in. Each repository keeps
its folder name, its group, the automatic pull, the saved pull requests and
its forks: its forks folder goes along and its fork folders are repaired, as
in the terminal app (see "Migrate: moving a repository to another folder").

Under the field a box has a row for each repository: where it
is now, an arrow and where it goes. A row can say:

- "is there already": it is in that folder already, and stays where it is.
- "cannot move there": something is at its place already, an earlier row of
  this move goes to the same place, or the folder lies inside the repository
  itself. The other rows still move.

While the folder you typed cannot be used, the rows show only the folder
names, and the line under the field gives the reason. When it can be used,
that line is the hint "A folder that does not exist yet is created." When the
window can tell that the move takes repositories to another drive, a note
says so ("E: is another drive."; only on Windows is the drive named, elsewhere
it reads "That folder is on another drive."), that everything is copied first and the
original is deleted only after the copy is complete, and that programs with
files of the repositories open should be closed first. When something is at
the place a repository's forks folder would go, or another row of the move
goes there, or its Git configuration cannot be read, a note under the rows
says that it stays and why: "The forks folder of api stays where it is:
something is at E:\work\api-fork already." Several for reasons of their own
take a line each, three at most, then "And 2 more."; in a window too short for
that and three rows, the note names fewer, down to one sentence that names
them all. The repository itself still moves.

The main button counts only the rows that move: "Move" for one, "Move 3
repositories" for three. It is off while none can move. `Enter` in the field
presses it. Cancel, `Esc` and a click outside close the dialog. The move
itself is a run with a progress dialog. Its Cancel is looked at between
repositories and between the files of a copy, so the file being copied is
finished first. What was moved before stays moved. The original of a
repository is not touched until its copy is complete, so it is never lost;
a copy that was cancelled or failed is removed again. If that removal fails,
for instance because a file is held open, the partial copy stays at the
target and the run says so: "The partial copy at E:\work\api could not be
removed: …", in the progress dialog and on the status line. Once a copy is
complete, deleting the original is not interrupted. If the original cannot be
deleted completely, the copy is the repository from then on (the list and the
settings follow it), and the run ends with a fault that names the folder left
behind: "Moved api to E:\work. The old folder D:\code\api could not be
deleted: …". The progress dialog then stays open until you close it, and the
status line keeps the sentence. Delete what is left of the old folder
yourself. The repositories stay on the list. A forks folder that could not
follow ends the run the same way: "Moved api to E:\work. The forks folder of
api stayed at D:\code\api-fork: something is at E:\work\api-fork already."

### Pushing a repository to another service

**Migrate**, then **Push to another service…**, opens "Push NAME to another
service" for one repository. Push takes one repository at a time: with
several chosen, the status line says "Select one repository: each needs an
address of its own."

Make an empty repository on the other service first, without a README. Then
type or paste its address in "Address of the new repository":

- `https://gitlab.com/you/name.git`, or another `https://`, `http://`,
  `ssh://` or `file://` address.
- `git@codeberg.org:you/name.git`.
- A local folder, for a bare repository on a disk: give its full path. The
  folder must exist, and it cannot be the repository itself or a folder inside
  it. A path that is not a full one is refused.

DevAssist asks for no password. The push signs in with what Git already has
for that host, a saved login or an SSH key. If Git cannot sign in, push to
that address once in a terminal so Git can ask and remember.

The box under the field lists what a push does, each line with a check, and
what it leaves as it is or does not do, each with a cross. The list is the
same for every repository, except that the fourth line names the remote once
an address is typed, and that its sentence about a remote of that name being
pointed at the new address shows only when this repository already has such a
remote with another address.

- Every branch and tag goes over. A branch goes as origin has it where the one
  here is behind it or missing, otherwise as it is here.
- Origin, if there is one, is fetched first. When it cannot be reached, what
  is saved here goes.
- Every GitHub pull request, open, merged or closed, goes over as a branch
  `pull/<number>` (under another name if `pull` is taken here), replaced on
  every push.
- The repository here keeps its files and its origin, and keeps the address as
  a remote named after its host, such as `gitlab`; for a local folder, or an
  address without a host, the remote is named `mirror`. A remote of that name
  that it has already is pointed at the new address.
- With a cross: apart from those pull-request branches, nothing there is
  overwritten or deleted. A branch there only moves forward, so no commit
  there is lost.
- With a cross: when a branch there has commits that the pushed one lacks, or
  a tag there differs from the pushed one, the push stops with an error part
  of the way, before the address is kept.
- With a cross: no pull request is opened there, and no issue, comment or
  release goes over.

"Push" is off while the address is empty. `Enter` in the field, or "Push",
pushes. A push cannot be undone on the other service, so only a fresh press
made inside the dialog pushes: not the `Enter` or the click that opened it,
and not a held key. When the address is refused, the dialog stays open and
says why under the field until the text changes. Otherwise it closes and the
progress dialog shows what Git sends. If you cancel the push, the status line
says that what was already sent stays on the other service and that the
address was not kept.

### Migrate in the menu

In the row's menu, **Migrate** (`t`) opens a side menu beside it with two
lines: **Move to another folder…** (`f`) and **Push to another service…**
(`s`). `Enter`, `→`, `t` or a click on Migrate opens it. In it `↑` `↓` (or
`k` `j`) move the highlight and stop at the ends. `Enter`, a line's letter or
a click runs the line. `←`, `Esc` or `q` close it and leave the menu open; a
second `Esc` closes the menu. While the side menu is open, the menu's own
letters do nothing, and the pointer moving over another line of the menu
closes it.

Migrate acts on the ticked rows when the menu was opened on a ticked row, and
otherwise on that row alone. Move takes all of them. Push takes one.

### The forks

`f` on the list opens the forks of the highlighted repository, or of the one
ticked, and so does **Show the forks** in the row's menu. They take the table's
place, read from the `-fork` folder without asking GitHub (see "Cloning the
forks"). The title says "Forks of" and the repository's name. The sidebar keeps
Repositories lit, and the round button that hides or shows the sidebar is a
back button ("Back to the repositories") while the forks are open.

When the forks of the repository are not cloned, nothing opens: the status line
says "The forks of homelab are not cloned yet. Clone the forks does it." With
several repositories ticked, or none highlighted, it says "Highlight one
repository to see its forks."

In the top row, **Read again** and **Open the folder** share a capsule, and
the main button is **Fetch the forks**. **Open the folder** there opens the
folder of the highlighted fork, as `Enter` does. In a narrow window the
capsule shows its icons only, and in a narrower one the main button too. Under
it, the toolbar has a field "Filter by owner or name", the segments **All**
and **With changes**, each with how many forks it holds, and a button with the
sort at its right end. A click on the button changes the sort, in this order:
"Most changes first" (forks with commits of their own first, the one furthest
ahead on top), "Newest commit first" and "By name".

Each fork is a row, in the order of the sort:

| Column | What it says |
|---|---|
| Fork | The owner and name of the fork, as `bob/api` |
| Branches | How many branches were fetched from it |
| Pull requests | How many pull requests were fetched from it, or "None" |
| Ahead of ada/main | How many commits its furthest branch is ahead ("3 ahead", with a rule that grows with the number), or "Nothing"; a dash for a fork with a folder whose commits ahead could not be counted (Git older than 2.41, or the original's main branch could not be read). The head names what the forks are compared with: the owner of the original and its default branch. When that cannot be read it says "Ahead of the original" |
| Newest commit | The date of its newest commit, as "23 Nov 2026"; empty when nothing was fetched from it |
| State | "Changes, folder", "No changes", "Not fetched" or "Gone from GitHub", as in the terminal app with a capital letter |

The column of the sort has an arrow in its head. At the end of a row are two
round buttons: **Open the folder**, for a fork that has a folder, and **Open
on GitHub**, for one that is still there. In a narrow window columns are left
out: first Newest commit, then Branches, then Pull requests.

One fork is highlighted, with a tinted row. There are no ticks: the forks are
only looked at. A click on a row highlights it, and a double click opens its
folder. Where the panel has no row it says why, in these words:

- "Reading the forks…" while the folder is read.
- "The forks could not be read. r tries again." when it could not, with Git's
  own words under it. The rows that were there go with it, so no old row is
  acted on. **Read again** is the way on.
- "No fork is in the folder yet. u fetches them." when the folder holds none.
- "No fork matches. esc clears the filter, c shows the forks without changes."
  when the filter or the segment hides every fork. Enter, `o` and `g` do
  nothing then.

The keys are the terminal app's, except where it says so:

| Key | What it does |
|---|---|
| `↑` `↓`, `j` `k`, `PgUp` `PgDn`, `Home` `End` | Move the highlight |
| `Enter`, `o` | Open the folder of the highlighted fork |
| `g` | Open the highlighted fork on GitHub |
| `u` | Fetch the forks of this repository |
| `r`, `F5` | Read the folder again |
| `s` | Change the sort |
| `c` | Switch between All and With changes |
| `/` | Put the keyboard in the filter |
| `l` | Show the progress of a run again after it was hidden |
| `Esc` | Clear the filter when it has text; else go back to the repositories |
| `Backspace` | Go back to the repositories |

`o`, `g`, `u`, `l`, `s` and `c` also take their capital letter. A key that acts
acts once when it is held down; only the moving keys go on. `q` and `←` do not
go back, as they do in the terminal app; `Esc`, `Backspace` and the back
button do. A fork without a folder opens no folder, and one that is gone from
GitHub opens no page; the status line says so.

While the filter has the keyboard, letters, `Backspace` and `/` only type.
`/` keeps what is in the filter. `Esc` clears its text, and with none left,
gives the keyboard back to the list without going back. `Enter` and `↓` give
it back too. The filter and the segment start the rows again at the top.

The status bar says, at its left, what the program said since you last did
something here, such as that a folder was opened or could not be. After a read
that failed it says the read's error, in the danger colour, until a read goes
well. Otherwise it says the forks in one sentence, as "38 forks. 6 have
changes and a folder. 1 is gone from GitHub. 61 branches and 9 pull requests
are saved.", leaving out the parts that are none. At its right it names the
forks' folder and when they were last fetched, as "Fetched 25 Nov 2026" (the
day the collection's file of stamps was last written, in UTC). In a narrow
window the day gives way first, then the folder is cut from its left, and then
it goes too; the words at the left stay whole. While a run exists, a click on
the status bar shows its dialog again, as `l` does.

**Fetch the forks**, `u`, and the menu's lines that clone or update forks
start a run, whose progress dialog has a count of the forks. Under its title,
once the run has said where the forks go, it says so ("Into D:\code\api-fork.
What the checkout has already is taken from disk, so only what the forks added
is downloaded."), and above its output it counts: "24 of 38 forks, 8 at a
time", with one bar. A fetch of one repository's forks has no list of tasks,
as the count stands in its place; a run for several repositories keeps the
list, and the count is of the repository being fetched now. When the fetch
pauses, a line with a pause icon under the bar says why and for how long,
counting down: "The fetch timed out. Paused, going on in 0:12." or "GitHub's
limit is reached. Paused, going on in 4:10." The line goes when the fetch goes
on. Cancel, Hide, `Esc` and the closing of the dialog are as for any run.
Between two repositories of a run, and once the run has ended, the dialog
keeps the last count, without a pause, also when it was hidden meanwhile; the
next run starts afresh. When the run ends, the forks are read again by
themselves.

### The demo

`--demo` holds all work. Every menu and dialog opens, but nothing is run,
opened, copied or written, and the status line says "This is a demo. Nothing
is run or changed." With `--demo --show NAME` the window opens with one of
them already open: `progress`, `remove`, `group`, `menu`, `migrate` (the menu
with its side menu), `add`, `add-folder` (Add with the folder field's
suggestions showing), `import`, `move`, `push`, `forks` (the forks of `api`,
with `bob` highlighted) or `forks-fetching` (those forks with a fetch of them
held in the progress dialog). The demo reads no folder and runs no Git, so its
folders are made up and its move changes nothing. A folder or an address you
type into one of its dialogs is checked by its text alone: the demo does not
look whether it is there.

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
