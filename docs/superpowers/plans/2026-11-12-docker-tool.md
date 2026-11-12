# Docker Tool and Shared Shell Chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Docker tool next to the repository overview, and move what both tools share (tool switching, text selection, help, self-update) into the shell.

**Architecture:** Implements `docs/superpowers/specs/2026-11-12-docker-tool-design.md`. The shell draws one line with the tool strip and gives each tool the rest of the screen. The Docker tool runs the `docker` CLI with JSON output on a worker thread and is built from atoms, three of them new (`tabs`, `log`, `process::Stream`).

**Tech Stack:** Rust 2024 (MSRV 1.88), ratatui 0.30 (crossterm events), serde_json. No new dependencies.

---

## How this plan is written

This plan was written and executed in the same session by the same author, so it fixes the files, the interfaces, the tests and the checks of each task, and leaves the function bodies to the task. The earlier plans in this folder spell out every line; this one does not.

## Execution notes (2026-11-12)

Executed on branch `feat/docker`; final result `263 passed; 0 failed; 2 ignored`, clippy and format clean at every commit.

- **Tasks 1 to 4 are one commit.** The strip, selection, help, the updater and the two file moves all meet in `shell/mod.rs` and in the repos `App`, and no state between them would have compiled without code that the next task deletes.
- **Task 5 was done in a separate worktree** while tasks 1 to 4 were under way, and cherry-picked: one commit per atom. Its test for "`kill_active` leaves a stream alone" checks that the stream's process is not in the list `kill_active` works from, instead of calling it: a real call would stop the programs other tests run at the same moment.
- **Tasks 6 to 8 are one commit.** The four tabs share the row, selection, confirmation and drawing code, so the Compose, Images and Volumes tabs were a few lines each on top of Containers.
- **Task 10 is a test, not a session at the keyboard:** `live_docker_lists_logs_stops_and_removes`, marked `#[ignore]`, drives the tool with key events against the real engine on a container pair, an image tag and a volume it creates. It passed against Docker 29.7.2: first load, live logs, stop and start, a change made outside the tool arriving through `docker events`, remove, forced remove, image, volume, and a refused command ending in the error dialog. Afterwards no `devassist-test-*` object and no stray `docker` process was left, and the two containers that were there before were untouched.
- **Prune was not run against the real engine.** It would have removed the machine's own stopped containers and unused images. It runs the same three commands as remove, on the items the dialog lists; the lists are covered by unit tests.
- **The unreachable engine** was checked with `DOCKER_HOST` pointed at a closed port: `--doctor` prints "not reachable" with Docker's message in under a second.
- **Not checked by hand:** the Docker tool inside the real terminal with real key presses. The dev build was started and the top line and the overview drew; nothing in this session could type into it.

## Ground rules for every task

- Branch: `feat/docker`.
- Baseline: `cargo test --locked` → `180 passed; 0 failed; 1 ignored` (master at v0.3.6).
- Before each commit all three must pass, each checked by its own exit code:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
- This is a binary crate: an unused `pub` item or trait method is a `dead_code` error under `-D warnings`. Something arrives with its first caller.
- Files are moved with `git mv` so history follows them.
- An atom names no feature type. Features do not import each other. The shell's new code does not import a feature; `main.rs` wires them. `shell/config.rs` is the exception: it still parses the repos tool's settings, as it did before the move.
- No test runs `docker` or `git` against the network.

## File map

| File | Change |
|---|---|
| `src/shell/mod.rs` | The trait's new methods, key and mouse routing, the layout with the strip line. |
| `src/shell/chrome.rs` | New. Draws the strip line; returns the click rectangles. |
| `src/shell/select.rs` | New. Text selection state, screen capture, painting, copying. From `features/repos/app.rs` and `ui.rs`. |
| `src/shell/help.rs` | New. The help dialog: the tool's text plus the shell's own keys. |
| `src/shell/tools.rs` | New. The Ctrl-P tool menu. |
| `src/shell/updater.rs` | New. `UpdateState`, the look and install threads, the version dialog. From `features/repos/app.rs` and `ui.rs`. |
| `src/shell/update.rs` | Moved from `src/update.rs`, unchanged. |
| `src/shell/config.rs` | Moved from `src/config.rs`, unchanged. |
| `src/features/repos/app.rs`, `ui.rs`, `mod.rs` | Lose selection, help, update, restart; `draw` takes an area; `modal`, `help`, `name`, `shown`. |
| `src/atoms/ui/tabs.rs` | New atom. |
| `src/atoms/ui/log.rs` | New atom. |
| `src/atoms/sys/process.rs` | Gains `Stream`. |
| `src/features/docker/{mod,model,cli,state,actions,view}.rs` | New. The Docker tool. |
| `src/main.rs` | Builds both tools; passes the GitHub token lookup to the shell. |
| `docs/USAGE.md`, `docs/BUILDING.md`, `README.md`, `CHANGELOG.md`, `CONTRIBUTING.md` | The Docker tool, switching, the new layout. |

## Interfaces fixed up front

```rust
// src/shell/mod.rs
pub trait Feature {
    fn name(&self) -> &'static str;
    fn modal(&self) -> bool;
    fn help(&self) -> &'static str;
    fn shown(&mut self);
    fn on_event(&mut self, event: Event);
    fn drain(&mut self) -> bool;
    fn waiting(&self) -> bool;
    fn draw(&mut self, frame: &mut Frame, area: Rect);
    fn request(&self) -> Option<Request>;   // Request::Quit only
}
pub struct Options {
    pub token: fn() -> Option<String>,  // for the update look; main.rs passes github::token
    pub look_for_update: bool,          // settings.update_check_now
}
pub fn run(options: Options, make: impl FnOnce() -> Vec<Box<dyn Feature>>) -> Result<(), Box<dyn Error>>;
```

`help` returns text, not the sections the spec sketched: every tool's help is a block of `key   what it does` lines today, and the shell only needs to print it above its own keys.

Mouse contract between shell and tool: the shell owns left-button drags (text selection). A tool receives `Down(Left)` and `Up(Left)` only for a click without a drag, and treats `Up(Left)` as the click. Moves, the wheel and the right button are passed on unchanged.

```rust
// src/atoms/ui/tabs.rs
pub fn on_key(selected: &mut usize, key: KeyEvent, count: usize) -> bool;
pub fn draw(frame: &mut Frame, area: Rect, labels: &[&str], selected: usize, mouse: Option<(u16, u16)>) -> Vec<Rect>;

// src/atoms/ui/log.rs
pub enum Outcome { Stay, Close }
pub struct LogView { /* title, lines (cap 5,000), follow, scroll, filter, typing */ }
impl LogView {
    pub fn new(title: impl Into<String>) -> Self;
    pub fn push(&mut self, line: &str);          // strips ANSI escapes and control characters
    pub fn end(&mut self, note: impl Into<String>); // the stream is over: shown in the title bar
    pub fn typing(&self) -> bool;                // the filter field has the keyboard
    pub fn on_key(&mut self, key: KeyEvent) -> Outcome;
    pub fn paste(&mut self, text: &str);
    pub fn scroll_by(&mut self, delta: isize);   // the wheel
    pub fn draw(&mut self, frame: &mut Frame, area: Rect);
}

// src/atoms/sys/process.rs
pub struct Stream { /* child pid, stop flag */ }
impl Stream {
    pub fn start(
        cmd: Command,
        on_line: impl FnMut(&str) + Send + 'static,
        on_end: impl FnOnce(Result<(), String>) + Send + 'static,
    ) -> Result<Stream, RunError>;
    pub fn stop(&mut self);
}
impl Drop for Stream { /* stop */ }
```

---

### Task 1: Shell owns the top line and switches tools

**Files:** `src/shell/mod.rs`, `src/shell/chrome.rs` (new), `src/shell/tools.rs` (new), `src/features/repos/mod.rs`, `src/features/repos/ui.rs`, `src/main.rs`

- [ ] Add `name`, `modal`, `shown` and the `area` parameter of `draw` to the trait. Repos: `name` is "Repos", `modal` is `mode != Mode::List`, `shown` does nothing, `ui::draw(frame, app, area)` lays out inside `area` and centres its dialogs in it.
- [ ] `chrome::draw(frame, area, names, active, note, mouse) -> Vec<Rect>`: ` 1 Repos  2 Docker ` on the left, the active one black on cyan; `note` right-aligned.
- [ ] The shell draws the strip on row 0 and the tool below it. The "Make the terminal a bit larger." check (now 48 × 13) moves to the shell.
- [ ] Keys while the tool is not modal: `1`–`9` switch (calling `shown`), `Ctrl-P` opens the tool menu. Click on a strip name switches.
- [ ] `tools.rs`: the menu state (filter text, selected index), `on_key -> Outcome { Stay, Close, Pick(usize) }`, `draw`. Built from `input::edit`, `menu::draw` and `dialog::window`.
- [ ] Tests (shell, with stub tools): a digit switches and calls `shown`; a digit does not switch while the tool is modal; a click on the strip switches; Ctrl-P, typing, Enter picks; the tool is drawn below the strip; quitting from an inactive tool still quits.
- [ ] Repos drawing tests: `render` draws into the full test screen as `area`; expectations unchanged.
- [ ] Verify, commit: `feat: switch between tools with number keys, the strip and a menu`.

### Task 2: Text selection moves to the shell

**Files:** `src/shell/select.rs` (new), `src/shell/mod.rs`, `src/features/repos/app.rs`, `src/features/repos/ui.rs`

- [ ] `select.rs`: `Selection { pointer, screen, note, failed }` with `on_mouse(kind, at) -> Pass` (`Pass::On` to hand the event to the tool, `Pass::Swallow`), `on_key(key) -> Pass` (Ctrl-C copies when something is selected; Esc dismisses; any other key clears and passes), `capture(buf)`, `paint(buf)`, `note() -> Option<(&str, bool)>`. `capture_screen` and `paint_selection` move here from `ui.rs`.
- [ ] The shell calls `capture` and `paint` after drawing everything, and shows the note ("release to copy", "Copied to the clipboard", or the error) as the strip's right-hand note.
- [ ] Repos `App` loses `pointer`, `screen`, `copy_note`, `copy_failed`, `dismiss_selection`, `copy_selection` and the selection lines at the top of `on_key`; `on_event` treats `Up(Left)` as a click. `footer_hint` keeps only its last line.
- [ ] Tests: selection tests move to `shell/select.rs` (drag then release copies; a click without a drag passes `Up`; Esc dismisses; Ctrl-C passes when nothing is selected).
- [ ] Verify, commit: `refactor: select and copy text in the shell`.

### Task 3: Help and self-update move to the shell

**Files:** `src/shell/help.rs` (new), `src/shell/updater.rs` (new), `src/update.rs` → `src/shell/update.rs`, `src/shell/mod.rs`, `src/features/repos/{app,ui,mod}.rs`, `src/main.rs`

- [ ] `git mv src/update.rs src/shell/update.rs`; rewrite `crate::update` → `crate::shell::update`.
- [ ] `updater.rs`: `UpdateState` (moved), `Updater { state, quiet, worker, open }` with `look(quiet)`, `open()`, `on_key -> Outcome { Stay, Close, Restart }`, `drain() -> bool`, `waiting()`, `notice() -> Option<String>` (the "version X is out, press v" text), `draw(frame, area, mouse, tick)`. The token comes from `Options::token`.
- [ ] `help.rs`: `draw(frame, area, tool_help)`; the shell's own lines: `1-9` and `ctrl-p` for tools, `v` version, drag copies, `ctrl-c` copies or quits. Any of Esc, `?`, Enter, `q` closes.
- [ ] Shell: `?` and `v` open the dialogs while the tool is not modal; an open shell dialog takes all keys and clicks; `Restart` sets the shell's restart flag and quits. `Request::Restart` is removed. `update::clean_up()` and the start-up look run in `shell::run`.
- [ ] Repos loses `Mode::Help`, `Mode::Update`, `update`, `update_quiet`, `restart`, `Msg::Update*`, the `v` and `?` keys, `draw_help`, `draw_update`, and the notice in its header. Its help text becomes `Feature::help`. `update_check` stays: it is the setting the settings form saves.
- [ ] Tests: the two update tests from `app.rs` and the drawing test `the_version_dialog_walks_through_an_update` move to `updater.rs`; a shell test opens help and the version dialog from a stub tool and checks a modal tool keeps `?` and `v`.
- [ ] Verify, commit: `refactor: show help and install updates from the shell`.

### Task 4: `config.rs` moves to the shell

- [ ] `git mv src/config.rs src/shell/config.rs`; rewrite `crate::config` → `crate::shell::config`; `main.rs` uses `shell::config`.
- [ ] Verify, commit: `refactor: move the settings file code to shell/config`.

### Task 5: The three atoms

**Files:** `src/atoms/ui/tabs.rs`, `src/atoms/ui/log.rs`, `src/atoms/sys/process.rs`, the two `mod.rs`

- [ ] `tabs`: tests for Tab / Shift-Tab wrapping, other keys not handled, `draw` returning one rectangle per label and marking the selected one.
- [ ] `log`: tests for the cap, ANSI stripping, following (a push keeps the view at the end), scrolling up stops following and End resumes, the filter shows only matching lines, Esc closes the filter first and the view second, drawing into a `TestBackend`.
- [ ] `Stream`: tests with a short system command (`cmd /C echo` on Windows, `sh -c` elsewhere): lines arrive and `on_end` runs; `stop` ends a long-running command; a missing program is a `RunError`.
- [ ] Until the Docker tool uses them the modules carry `#[allow(dead_code)]` on their `mod` lines; Task 6 removes it.
- [ ] Verify, commit one atom at a time.

### Task 6: Docker model, commands and the Containers tab

**Files:** `src/features/docker/{mod,model,cli,state,actions,view}.rs`, `src/features/mod.rs`, `src/main.rs`

- [ ] `model.rs`: `Container { id, name, image, state, status, ports, project, mounts }`, `Project { name, status, running, total, files }`, `Image { id, repository, tag, size, age, containers }`, `Volume { name, driver }`; `parse_containers`, `parse_images`, `parse_volumes`, `parse_projects`. Tests use lines captured from Docker 29.7 (sanitised), including labels whose values contain commas, an image reference with a digest, and missing fields.
- [ ] `cli.rs`: one function per command returning a `Command` (`ps`, `images`, `volumes`, `compose_ls`, `events`, `logs`, `start`, `stop`, `restart`, `rm`, …). Tests assert the arguments.
- [ ] `state.rs`: `Docker` with the lists, `Engine { Unknown, Loading, Ready, Missing, Down(String) }`, the tab, per-tab selection / ticks / filter / scroll, the dialog (`Menu`, `Confirm`, `Error`, `Log`), the action queue, a `Worker<Msg>`, the events `Stream`.
- [ ] `actions.rs`: `Request` (what to run) and `Confirm` (what the dialog lists and which buttons it has), built from the selection as plain data.
- [ ] `view.rs`: the summary line, the tab bar, the Containers rows grouped by project, the detail line, the footer, the not-installed / not-running screens, the actions menu, the confirm dialog, the error dialog, the log view.
- [ ] Keys as in the spec. Logs open a `LogView` fed by a `Stream`.
- [ ] Tests: key events on a `Docker::for_test` with canned lists; confirmation contents; drawing of the Containers tab, the not-running screen and the remove dialog.
- [ ] `main.rs` adds the tool. Verify. Run against the real engine. Commit: `feat: add the Docker tool with containers and live logs`.

### Task 7: Compose tab

- [ ] Rows from `compose ls -a`; up, stop, restart, down (with the volumes tick box), logs. Long output goes to the log view.
- [ ] Tests: parsing `running(6)` / `exited(2)` / mixed statuses; the down dialog; drawing.
- [ ] Verify, commit: `feat: manage Compose projects in the Docker tool`.

### Task 8: Images and Volumes tabs, prune

- [ ] Images: rows, remove (with "Remove anyway"), prune (dangling, tick box for all unused, count and size shown first).
- [ ] Volumes: rows with the containers that mount them, remove (in-use ones left out and named), prune.
- [ ] Containers prune (stopped ones).
- [ ] Tests: in-use detection, the dialogs' contents, drawing.
- [ ] Verify, commit: `feat: list, remove and prune images and volumes`.

### Task 9: `--doctor`, docs, changelog

- [ ] `--doctor` prints the Docker line; a missing Docker does not fail it.
- [ ] `docs/USAGE.md`: switching tools, the Docker tool. `docs/BUILDING.md` and `CONTRIBUTING.md`: the layout. `README.md`: one paragraph. `CHANGELOG.md`: `[Unreleased]`.
- [ ] Mark both specs as implemented.
- [ ] Verify, commit.

### Task 10: By hand, against the real engine

- [ ] All four tabs show what `docker` shows.
- [ ] Logs of a running container follow; the filter and scrolling work; closing stops the `docker logs` process.
- [ ] On containers, an image and a volume created for the test: start, stop, restart, remove, force-remove, prune. Existing containers are not touched.
- [ ] With the engine unreachable (`DOCKER_HOST` pointed at nothing): the not-running screen, and Repos still works.

### Task 11: Release 0.4.0

- [ ] Follow `docs/RELEASING.md`. Merge `feat/docker` into `master` first.
- [ ] A 0.3.6 copy in a scratch folder updates itself to 0.4.0 and restarts: the check that was never run.
