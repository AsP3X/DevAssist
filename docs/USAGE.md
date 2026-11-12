# Using DevAssist

DevAssist runs full-screen in a terminal. Everything can be done with the
keyboard; the mouse works for selecting, clicking buttons, scrolling and
copying text. Press `?` in the app for the key list.

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

## Copying text

Drag across any text and release the mouse to copy it. `Ctrl-C` copies a
selection too, and quits when nothing is selected.
