# Docker tool and shared shell chrome

Date: 2026-11-12
Status: approved design, not yet implemented

## Goal

DevAssist gets a second tool next to the repository overview: Docker. It lists containers, Compose projects, images and volumes, and starts, stops, removes and prunes them, with live logs.

This is step 5 ("proof") of `2026-11-12-atom-module-split-design.md`, and it carries step 4b with it: what two tools share leaves the repos `App` and moves to the shell, and the shell learns to switch tools.

In the app and its docs a feature is called a **tool**. In the code it stays `Feature`.

## Out of scope

- A shell into a container (`docker exec`).
- Pulling or building images.
- Networks.
- Resource statistics (`docker stats`).
- Settings for the Docker tool. It gets a `[docker]` section in `devassist.toml` when it has its first setting.
- Orbit. It gets its own spec after this is released.

## How DevAssist talks to Docker

It runs the `docker` command with `--format '{{json .}}'` through `atoms/sys/process` and parses the lines with `serde_json`.

Reasons: it works with Docker Desktop, contexts, remote hosts and Compose as the user has them set up, and it adds no dependency. The Engine API over the named pipe would need an async runtime and a large crate, and would not handle Compose.

## Shell changes (step 4b)

### Switching tools

- The first header line belongs to the shell. It shows the program title, the tool strip (`1 Repos  2 Docker`, the active one highlighted) and the update notice.
- `1`–`9` switch to that tool. A click on a name in the strip does too.
- `Ctrl-P` opens the tool menu: the tools as a list with a text filter, `↑` `↓` and `Enter`. It is built from the `menu` and `input` atoms.
- The shell handles these keys only while the active tool reports that it is not capturing input (see `Feature::modal`). With a text field or a dialog open, a digit is typed and `Ctrl-P` goes to the tool.

### What moves out of the repos `App`

| Today in `App` / `ui.rs` | Goes to | Why it is shared |
|---|---|---|
| `mouse`, `pointer`, `screen`, `copy_note`, `finish_selection`, `capture_screen`, `paint_selection`, Ctrl-C copy | `shell/select.rs` | Text can be dragged and copied on every screen, logs most of all. |
| `update`, `update_quiet`, `update_check`, `draw_update`, the `v` key, `restart` | `shell/update.rs` (dialog and state); `src/update.rs` moves to `shell/update/` beside it | The program updates itself, whichever tool is on screen. |
| `Mode::Help`, `draw_help` | `shell/help.rs` | One dialog; each tool supplies its key list. |
| the title part of `draw_header` | `shell/chrome.rs` | The strip lives there. |
| `config.rs` | `shell/config.rs` | The shell locates, reads and writes the file. |

What stays in each tool: its own header line below the strip, its footer hints and its status line.

### The `Feature` trait after this change

```rust
pub trait Feature {
    /// The name in the strip and the tool menu.
    fn name(&self) -> &'static str;
    /// True while a text field or a dialog has the keyboard, so the
    /// shell leaves its own keys to the tool.
    fn modal(&self) -> bool;
    /// The tool's keys, for the help dialog: sections of (key, what it does).
    fn help(&self) -> Vec<HelpSection>;
    /// Called when the tool comes on screen. The Docker tool starts its
    /// first load here.
    fn shown(&mut self);
    fn on_event(&mut self, event: Event);
    fn drain(&mut self) -> bool;
    fn waiting(&self) -> bool;
    /// Draws into `area`, which is the screen below the shell's line.
    fn draw(&mut self, frame: &mut Frame, area: Rect);
    fn request(&self) -> Option<Request>;
}
```

`Request::Restart` is no longer asked for by a tool: the shell's update dialog sets it. `Request` keeps `Quit` only; the shell holds its own restart flag.

Order of keys in the shell: an open shell dialog (help, update, tool menu) gets the key first; then, if the tool is not modal, the shell's own keys (`1`–`9`, `Ctrl-P`, `?`, `v`); everything else goes to the tool. Ctrl-C copies a text selection if there is one, and otherwise goes to the tool, which quits.

Mouse: the shell handles drags for text selection and clicks on the strip; other mouse events go to the tool unchanged.

## New atoms

| Atom | What it is | Shape |
|---|---|---|
| `atoms/ui/tabs` | A tab bar. | `on_key(&mut usize, key, count) -> bool` for `Tab` / `Shift-Tab`; `draw(frame, area, labels, selected) -> Vec<Rect>` returning each label's rectangle for clicks. |
| `atoms/ui/log` | A log viewer. | A struct `LogView`: a capped buffer of lines (5,000), `push(line)`, follow on or off, scroll, a text filter, `on_key -> Outcome { Stay, Close }`, `draw(frame, area, title)`. Names no feature type. |
| `atoms/sys/process::Stream` | A long-running process whose lines arrive as they are printed. | `Stream::start(cmd, on_line)` runs it on its own thread; `stop()` and `Drop` kill it. It does not use the single "active process" slot that `run` and `kill_active` share, so a log can follow while an action runs. |

None of these names a container. Orbit can use all three.

## The Docker tool

### Layout

```
src/features/docker/
  mod.rs      implements Feature
  model.rs    Container, Project, Image, Volume; parsing docker's JSON
  cli.rs      builds each docker command
  state.rs    the lists, the active tab, selection, ticks, filter, dialogs
  actions.rs  what each action runs and what its confirmation lists
  view.rs     the tabs, the rows, the dialogs
```

### Screen

```
 DevAssist   1 Repos  [2 Docker]                              v0.4.0
 3 running · 5 stopped · 12 images · 5 volumes
 [Containers]  Compose   Images   Volumes               filter: _
 ▾ shroud
 >  ● shroud-postgres   postgres:17    Up 3 hours     5432→5432
    ○ shroud-api        shroud/api     Exited (0) 2 days ago
 ▾ (no project)
    ○ backups           restic:latest  Exited (0) 5 days ago
 shroud-postgres · 3f2a9c1b04d2 · created 2026-10-30
 enter actions · l logs · s start/stop · tab next · ? help
```

### Tabs

| Tab | Rows | Source | Actions |
|---|---|---|---|
| Containers | name, image, status, ports; grouped under the Compose project from the label `com.docker.compose.project`, groups fold | `docker ps -a` | start, stop, restart, remove, logs, copy the name |
| Compose | project, running / total, compose file | `docker compose ls -a` | up, stop, restart, down, logs |
| Images | repository:tag, short ID, size, age, in use or not | `docker image ls`, "in use" from the containers | remove, prune |
| Volumes | name, driver, the containers that mount it | `docker volume ls`, mounts from the containers | remove, prune |

### Keys

They follow the repos list wherever the meaning is the same.

| Key | Does |
|---|---|
| `Tab` / `Shift-Tab` | next / previous tab |
| `↑` `↓` `j` `k`, `Home` `End`, `PgUp` `PgDn` | move |
| `/` | filter the tab's list; `Esc` clears |
| `Space` | tick the row; `Ctrl-A` ticks all shown; `Esc` clears |
| `Enter` | actions menu for the row or the ticked rows; on a group header, fold |
| `s` | start what is stopped, stop what is running (containers, projects) |
| `R` | restart |
| `l` | logs |
| `d` / `Delete` | remove (asks first) |
| `p` | prune this tab (asks first) |
| `r` / `F5` | reload |
| `q` | quit |

### Loading and refreshing

- Nothing runs until the tool is first shown. Then it loads all four lists on a worker thread and starts `docker events --format '{{json .}}'` as a `Stream`.
- An event marks the lists stale; the tool reloads at most once every 500 ms.
- `r` reloads at once.
- After an action finishes, the tool reloads.

### When Docker is not there

| Situation | How it is recognised | What the tool shows |
|---|---|---|
| `docker` is not installed | the process cannot be started | "Docker is not installed." and where to get it |
| The engine is not running | `docker ps` fails | "Docker is not running. Start Docker Desktop, then press r." with the first line of docker's error |

In both cases no events stream runs, `r` tries again, and the other tools are unaffected.

### Actions

- An action runs on the worker thread, one at a time; a second one waits in a queue, as in the repos tool.
- While it runs, the status line shows a spinner and what is happening ("Stopping shroud-postgres…").
- A failure opens a dialog with docker's error text.
- Actions with long output (`compose up`, `compose down`) open the log viewer with their output streaming in. `Esc` hides it; the action goes on.

### Logs

`l` opens the log viewer over the list and starts `docker logs --follow --tail 500 <container>` (for a project: `docker compose -p <project> logs --follow --tail 500`) as a `Stream`.

| Key | Does |
|---|---|
| `↑` `↓` `PgUp` `PgDn` `Home` | scroll; scrolling up stops following |
| `End` or `f` | follow again |
| `/` | show only lines containing the text |
| `Esc` / `q` | close and stop the stream |

Text is copied by dragging, as everywhere.

### Safety

Every removal asks first, in a dialog that lists by name what will go.

| Action | Dialog |
|---|---|
| Remove containers | Lists them. If any is running, the default button removes only the stopped ones and a second button, "Stop and remove all", uses `--force`. |
| Remove images | Lists them. An image in use by a container is listed with that container; removing it needs the second button, "Remove anyway" (`--force`). |
| Remove volumes | Lists them with the containers that mount them. A volume in use cannot be removed; the dialog says so and leaves it out. |
| Prune (containers / images / volumes) | Loads what would go first (stopped containers; dangling images, with a tick box for "all unused"; unused volumes), and shows the count, the names and, for images, the total size. |
| Compose down | Names the project and its containers. A tick box "also remove its volumes", off by default. |

Nothing is removed by a single key press.

### `--doctor`

Gains one line: the Docker version and how many containers there are, or why Docker could not be reached. A missing Docker does not make `--doctor` fail.

## Testing

- `model.rs`: parsing tests against JSON lines captured from a real `docker ps -a`, `image ls`, `volume ls`, `compose ls` and `events`.
- `cli.rs`: each command is built as a `Command` and its arguments are asserted, so no test runs docker.
- `state.rs` / `actions.rs`: key events in, state and the queued action out. The confirmation contents are tested as data.
- `view.rs`: drawing into a ratatui `TestBackend` for the four tabs, the not-running screen and the remove dialog.
- Atoms: `tabs`, `log` and `Stream` each have their own tests with plain data. `Stream` is tested with a short-lived system command.
- Shell: switching by key and click, digits not switching while the tool is modal, help and update reachable from a stub tool, text selection.
- By hand, against the real engine: all tabs, logs of a running container, and each destructive action on containers, images and volumes created for the test. Existing containers are not touched.

## Order of work

Each step compiles, passes the tests and leaves the app usable.

1. Shell: the trait's new methods, the strip, switching, the tool menu.
2. Shell: text selection, help, the update dialog and `config.rs` move out of the repos tool.
3. Atoms: `tabs`, `log`, `Stream`.
4. Docker: model, cli, the Containers tab, actions, logs.
5. Docker: Compose tab.
6. Docker: Images and Volumes tabs, prune.
7. `--doctor`, the docs (`USAGE.md`, `BUILDING.md`, `README.md`), the changelog.
8. Release 0.4.0.

## Risks

- **Step 2 moves code out of a 4,200-line `App` with 180 tests around it.** Tests that press `v` or `?` on an `App` move to the shell with the code they test.
- **Docker's JSON differs between versions** (for example `Labels` as one comma-separated string, `Mounts` as names only). The model parses only the fields it needs and treats a missing one as empty.
- **`docker events` can end** when the engine restarts. When the stream ends, the tool falls back to the not-running screen and `r` starts it again.
- **Windows console windows.** Every docker process is started hidden through `process::quiet`, as git is.
