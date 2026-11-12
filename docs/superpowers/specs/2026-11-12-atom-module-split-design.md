# Atom-based module split

Date: 2026-11-12
Status: approved design, not yet implemented

## Goal

DevAssist 0.3.1 is a single-purpose app: one `App` struct (about 60 fields, `src/app.rs`, 4,300 lines) and one drawing module (`src/ui.rs`, 3,000 lines), both built around the repository list. We plan to add the functionality of the orbit TUI tool and Docker container management.

This design restructures the code so that a new feature is assembled from reusable **atoms** instead of re-implementing inputs, lists, dialogs, process handling and background jobs each time.

The restructuring changes no behaviour. Every step keeps the existing tests passing.

## Out of scope

- Any new user-facing feature. Docker and orbit get their own specs.
- A cargo workspace. One crate with three directories is enough at this size; the layout can be promoted to crates later without redesign.
- A plugin system or runtime loading. Features are compiled in.

## Layers

```
shell      main loop, terminal, feature switcher, global keys, config file
   │
features   repos · update · (later) orbit · docker
   │
atoms      ui · sys · task
```

Rules:

1. Dependencies point downward only.
2. Features never import each other.
3. An atom never names a feature type (`LocalRepo`, `RemoteRepo`, a container, …). It takes plain data and returns plain data.

Rule 3 is the test for whether something is an atom. If it needs a feature type, it belongs in that feature.

## Target layout

```
src/
  main.rs
  shell/
    mod.rs        Shell: owns features, routes events and messages
    terminal.rs   setup, restore, panic hook, enable_utf8
    chrome.rs     header, footer, feature switcher
    config.rs     file location, parsing, per-feature sections
    feature.rs    the Feature trait and Ctx
  atoms/
    ui/
      input.rs    single-line text input
      list.rs     selection, scroll, marks, filter
      dialog.rs   centered frame, button row
      menu.rs     keyed action menu
      confirm.rs  yes/no dialog
      text.rs     fit / fit_end, padded rows
      progress.rs bar, spinner, item icons
      hits.rs     hit map, contains
      select.rs   mouse text selection (today's select.rs)
      path.rs     folder prompt with completion (today's pathprompt.rs)
    sys/
      process.rs  spawn, stream output, track PIDs, cancel, kill
      http.rs     shared agent, GET with token, error mapping
      clipboard.rs
      open.rs     open a URL or folder
      encode.rs   base64
      paths.rs    home_dir, same_path, push_path
    task/
      job.rs      Job, Item, JobEvent (today's job.rs)
      worker.rs   background thread + message channel
  features/
    repos/
      mod.rs      implements Feature
      state.rs    the repo-specific fields of today's App
      view.rs     the overview and its dialogs
      git.rs      today's gitops.rs minus process handling
      github.rs   today's github.rs minus HTTP plumbing
      settings.rs today's settings.rs
    update/
      mod.rs      self-update: check, install, restart
```

## Atoms

### UI atoms

Each UI atom is a struct that owns its state and exposes three things:

- `on_key` / `on_mouse` → returns an `Outcome` enum (`Pending`, `Done(value)`, `Cancelled`)
- `draw(frame, area)`
- plain accessors for its value

This is the pattern `pathprompt::Outcome` and `settings::Outcome` already use; the atoms generalise it.

| Atom | Comes from | Used today by |
|---|---|---|
| `input` | per-dialog string editing, `delete_word` in `app.rs` | filter, Add search, group prompt, path prompt, settings |
| `list` | `selected` / `*_scroll` / `marked` fields, `align_scroll` | installed list, Add list |
| `dialog` | `centered`, `draw_dialog_buttons` | every dialog |
| `menu` | `Menu`, `MenuItem`, `draw_menu` | actions menu |
| `confirm` | `Mode::ConfirmRemove`, `draw_confirm` | remove |
| `text` | `padded_row`, `fit`, `fit_end`, `join_facts` (`column_widths` stays with the repository table) | every screen |
| `progress` | `bar_spans`, `item_icon`, `SPINNER` | progress dialog, update dialog |
| `hits` | `Hits`, `contains` (defined twice) | all mouse handling |
| `select` | `select.rs` unchanged | whole screen |
| `path` | `pathprompt.rs` with the `RemoteRepo` dependency removed | clone target, import |

`menu` and `confirm` become generic: `menu` takes a list of `(label, key, id)`, `confirm` takes a title, body lines and button labels. The repos feature supplies the content.

### System atoms

| Atom | Comes from | Notes |
|---|---|---|
| `process` | `run_git`, `pump`, PID tracking, `kill_active`, `hide_window`, cancel flags in `gitops.rs` | Takes a program and arguments; `git.rs` adds git-specific arguments and auth. Docker will call the same atom with `docker`. |
| `http` | `agent()` and GET helpers duplicated in `github.rs` and `update.rs` | One agent, one GET, one error type. |
| `clipboard` | `clipboard.rs` unchanged | |
| `open` | `open_url`, `open_folder`, `open_command` in `gitops.rs` | |
| `encode` | `base64` in `clipboard.rs` and `gitops.rs` | One copy. |
| `paths` | `home_dir`, `same_path`, `push_path` in `config.rs` | |

### Task atom

*Revised 2026-11-12 while planning this step; see `docs/superpowers/plans/2026-11-12-task-atom.md`.*

- `job` holds `Job`, `Item` and `JobEvent`, made generic: a job reads progress through a `Reader` function it is given. Git's phase table and wording move out to `src/gitprogress.rs` (later `features/repos`).
- `worker` owns the channel that is `tx` / `rx` in `App` today: `spawn` runs work on a thread, `drain()` returns what the threads reported.

There is no `queue` atom. The queue is a plain `VecDeque` of a feature's own request enum, and a wrapper would add nothing. The rule for *when* the next request starts depends on shell state (`Mode`, the hidden-dialog flag), so it is separated in step 4, when `App` is split.

A feature that needs background work with progress gets it by owning a `Worker<Msg>` and building `Job`s.

## Feature contract

```rust
pub trait Feature {
    fn name(&self) -> &'static str;
    fn on_event(&mut self, event: &Event, ctx: &mut Ctx) -> bool; // handled?
    fn drain(&mut self, ctx: &mut Ctx) -> bool;                   // changed?
    fn draw(&mut self, frame: &mut Frame, area: Rect, ctx: &mut Ctx); // records hits
    fn hints(&self, width: usize) -> Vec<Hint>;                   // footer
    fn waiting(&self) -> bool;                                    // fast redraw
    fn doctor(&self, out: &mut dyn Write) -> bool;                // ok?
}
```

`Ctx` is what the shell lends a feature: the status line, the tick counter, the hit map, the mouse position, and requests to the shell (`quit`, `restart`, `switch_to`).

The shell holds `Vec<Box<dyn Feature>>` and an active index. Event flow:

1. Shell handles global keys (quit, help, switch feature).
2. Otherwise the active feature's `on_event` gets it.
3. Every loop turn, the shell calls `drain` on **all** features, so background work continues while another feature is on screen.

With one feature registered, the switcher is hidden and the app looks exactly as it does today.

## Config

`devassist.toml` keeps its current top-level keys, which the repos feature reads, so existing files keep working. A new feature reads its own `[section]`. `shell/config.rs` owns locating, reading and writing the file; each feature parses and renders only its own keys.

## What happens to `App`

| Today in `App` | Goes to |
|---|---|
| `mode` for global dialogs (Help), `should_quit`, `restart`, `tick`, `mouse`, `pointer`, `screen`, `copy_note`, `hits`, `status` | shell / `Ctx` |
| `update`, `update_quiet`, `update_check` | `features/update` |
| `job`, `tx`, `rx` | `atoms/task` (`Job`, `Worker`), owned by the repos feature |
| `queue`, `busy` | `features/repos/state.rs` |
| everything else (checkouts, groups, remotes, filters, selection, lookups, …) | `features/repos/state.rs` |

## Order of work

Each step is a separate change that compiles and passes `cargo test` with no behaviour change.

1. **System atoms.** Create `atoms/sys`; move `process`, `http`, `open`, `encode`, `paths`, `clipboard`; delete the duplicates.
2. **Task atom.** Move `job.rs` and make it generic; extract `worker` from `App`.
3. **UI atoms.** In two parts, each with its own plan:
   - *Drawing* (`text`, `hits`, `progress`, `dialog`): pure functions leave `ui.rs`; no state moves. Plan: `docs/superpowers/plans/2026-11-12-ui-atoms-drawing.md`.
   - *Stateful* (`input`, `list`, `menu`, `confirm`, `path`): fields and key handling leave `App`, one dialog at a time.
4. **Shell and feature trait.** Add `shell/`; move terminal setup out of `main.rs`; split `App` per the table above; `repos` and `update` implement `Feature`.
5. **Proof.** The Docker spec is written against this structure. If it needs something no atom provides, that becomes a new atom, not feature-local code.

Steps 1–3 can ship independently; the app is better after each even if step 4 is delayed.

## Testing

- Existing unit tests move with their code.
- Each atom gets its own tests against plain data, with no feature types involved.
- UI atoms are tested by feeding key events and asserting on `Outcome` and state, and by drawing into a ratatui `TestBackend` buffer where layout matters.
- After step 4: a shell test with a stub feature checks routing (global keys, `drain` on inactive features).

## Risks

- **`ui.rs` function bodies were outlined, not read line by line.** Some atoms may split differently once the code is moved. The atom list is a starting point; rule 3 decides the boundaries.
- **Orbit has not been reviewed.** The atoms are derived from DevAssist alone. Orbit may need atoms not listed here (for example a multi-pane layout or a log viewer).
- **Borrowing.** `draw` currently takes `&mut App` because it records hit rectangles while drawing. Splitting state across shell and feature needs care so `Ctx` and the feature are not borrowed mutably at once; the hit map lives in `Ctx` for that reason.
