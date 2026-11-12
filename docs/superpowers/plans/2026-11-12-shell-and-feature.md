# Shell and Feature Trait Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give DevAssist the structure a second feature can plug into: a `shell` that owns the terminal and the main loop, a `Feature` trait, and today's app moved under `features/repos` as the first feature.

**Architecture:** This is the first half of step 4 in `docs/superpowers/specs/2026-11-12-atom-module-split-design.md`. The shell holds a list of features, draws the active one, sends it the events, and drains background work from all of them. `App` implements `Feature` whole. Nothing a user can see changes.

**Tech Stack:** Rust 2024 (MSRV 1.88), ratatui 0.30 (crossterm events).

---

## Execution notes (2026-11-12)

Executed on branch `refactor/shell-and-feature` as written; final result `180 passed; 0 failed; 1 ignored`, clippy and format clean at every commit.

- **Task 3 Step 9 was done for real:** the debug build was started in a terminal tab. The overview drew, the header filled in ("55 installed", "80 on GitHub") and the status spinner ran, which shows events, drawing and background work all pass through the shell. Ctrl-C quit it and the shell prompt came back.
- **Task 4:** the `docs/BUILDING.md` table also got a row for `src/features/repos/mod.rs`, and the note about layering went after the table's last row (`.github/workflows/`), not after the source rows.
- **Not done:** a restart after a self-update (`Request::Restart`). It needs a newer release to install; the shell test covers that a restart request wins, not the relaunch itself.

## Why step 4 is split in two

The spec's step 4 does two things. They have very different amounts of guesswork:

| Part | What it does | What it depends on |
|---|---|---|
| **4a (this plan)** | Adds `shell/` and the `Feature` trait; moves the repository code to `features/repos/`; `App` becomes the first feature. | Only on code that exists. |
| **4b (with the Docker spec)** | Takes what two features share out of `App` (header, footer, status line, help, mouse text selection, the self-update dialog) and adds a way to switch features. | On what the second feature actually needs. |

Deciding today which parts of the header or the status line are "shared" would mean designing for a feature that has no spec. After 4a a second feature can be added and run; 4b then moves exactly what both turn out to use.

Consequences for this plan, each kept deliberately small:

- **The trait has five methods**, the ones the main loop calls today. `name`, `hints` and `doctor` from the spec's sketch have no caller yet and would be dead code (an error under `-D warnings`). They arrive with their first caller.
- **No feature switcher.** With one feature there is nothing to switch to and no way to test a key for it. The shell already holds a list and an active index, so adding the key in 4b is small.
- **`update.rs` and `config.rs` stay at the top of `src/`.** Both are used by `App` and belong partly to the shell (`update` is the program updating itself; `config` finds and writes the settings file). 4b decides their home.
- **`--doctor` moves to `features/repos` as a plain function.** It lists repositories and nothing else.

## Ground rules for every task

- Baseline: `cargo test --locked` → `175 passed; 0 failed; 1 ignored` (master at v0.3.5).
- Before each commit all three must pass, each checked by its own exit code:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
- This is a binary crate: an unused `pub` item, and an unused trait method, is a `dead_code` error under `-D warnings`.
- Files are moved with `git mv` so history follows them.

## File map

| File | Responsibility | Task |
|---|---|---|
| `src/atoms/ui/select.rs` (moved from `src/select.rs`) | Mouse text selection | 1 |
| `src/features/mod.rs` (new) | declares `repos` | 2 |
| `src/features/repos/mod.rs` (new) | declares the feature's modules; `impl Feature for App`; `doctor` | 2, 3 |
| `src/features/repos/{app,ui,gitops,github,gitprogress,settings}.rs` (moved from `src/`) | unchanged content | 2 |
| `src/shell/mod.rs` (new) | `Feature`, `Request`, `Shell`, `run` | 3 |
| `src/shell/terminal.rs` (new) | terminal setup, restore, panic hook, UTF-8 | 3 |
| `src/main.rs` | start-up only | 2, 3 |
| `docs/BUILDING.md`, `CONTRIBUTING.md` | source layout | 4 |

After this plan `src/` looks like this:

```
src/
  main.rs       start-up: command line, then shell::run
  config.rs     settings file and command line
  update.rs     self-update
  shell/        mod.rs  terminal.rs
  features/     mod.rs  repos/{mod,app,ui,gitops,github,gitprogress,settings}.rs
  atoms/        sys/  task/  ui/
```

---

### Task 0: Branch

- [ ] **Step 1: Create the work branch**

```bash
git switch master
git switch -c refactor/shell-and-feature
```

- [ ] **Step 2: Confirm the baseline**

Run: `cargo test --locked`
Expected: `test result: ok. 175 passed; 0 failed; 1 ignored`

---

### Task 1: Move text selection to the atoms

`select.rs` names no feature type and imports nothing from the crate. It is the one UI atom still sitting at the top level.

**Files:**
- Move: `src/select.rs` → `src/atoms/ui/select.rs`
- Modify: `src/main.rs`, `src/atoms/ui/mod.rs`, `src/app.rs`, `src/ui.rs`

- [ ] **Step 1: Move and rewire**

```bash
git mv src/select.rs src/atoms/ui/select.rs
sed -i '/^mod select;$/d' src/main.rs
sed -i -E 's/crate::select\b/crate::atoms::ui::select/g' src/app.rs src/ui.rs
```

In `src/atoms/ui/mod.rs`, add `pub mod select;` after `pub mod progress;`.

- [ ] **Step 2: Check for leftovers**

Run: `grep -rn 'crate::select' src`
Expected: no output

- [ ] **Step 3: Verify**

Run: `cargo fmt; cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `175 passed; 0 failed; 1 ignored`

- [ ] **Step 4: Commit**

```bash
git add -A src
git commit -m "refactor: move text selection to atoms/ui/select"
```

---

### Task 2: Move the repository code under `features/repos`

A pure move. File names stay the same so only module paths change.

**Files:**
- Create: `src/features/mod.rs`, `src/features/repos/mod.rs`
- Move: `src/app.rs`, `src/ui.rs`, `src/gitops.rs`, `src/github.rs`, `src/gitprogress.rs`, `src/settings.rs` → `src/features/repos/`
- Modify: `src/main.rs`, `src/config.rs`, `src/update.rs`, and the moved files (paths only)

- [ ] **Step 1: Move the files**

```bash
mkdir -p src/features/repos
git mv src/app.rs src/ui.rs src/gitops.rs src/github.rs src/gitprogress.rs src/settings.rs src/features/repos/
```

- [ ] **Step 2: Declare the modules**

`src/features/mod.rs`:

```rust
//! What the program can do. Each feature is built from atoms and knows
//! nothing about the other features.

pub mod repos;
```

`src/features/repos/mod.rs`:

```rust
//! Local clones of GitHub repositories: the overview, and adding,
//! updating, grouping and removing them.

pub mod app;
pub mod github;
pub mod gitops;
pub mod gitprogress;
pub mod settings;
pub mod ui;
```

- [ ] **Step 3: Rewrite the module paths**

```bash
sed -i -E 's/crate::(app|ui|github|gitops|gitprogress|settings)\b/crate::features::repos::\1/g' src/config.rs src/update.rs src/features/repos/*.rs
```

`crate::atoms::ui::…` is not touched: the pattern needs `crate::` directly before the name.

In `src/main.rs`, replace the `mod` lines at the top

```rust
mod app;
mod atoms;
mod config;
mod github;
mod gitops;
mod gitprogress;
mod settings;
mod ui;
mod update;
```

with

```rust
mod atoms;
mod config;
mod features;
mod update;
```

and replace `use app::App;` with:

```rust
use features::repos::app::App;
use features::repos::{github, gitops, ui};
```

The rest of `main.rs` (which writes `ui::draw`, `gitops::load`, `github::token` without `crate::`) then compiles as it is.

- [ ] **Step 4: Check for leftovers**

Run: `grep -rnE 'crate::(app|ui|github|gitops|gitprogress|settings)\b' src`
Expected: no output

- [ ] **Step 5: Verify**

Run: `cargo fmt; cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `175 passed; 0 failed; 1 ignored`. `cargo fmt` re-wraps a few `use` lines that became longer; that is expected.

- [ ] **Step 6: Commit**

```bash
git add -A src
git commit -m "refactor: move the repository feature to features/repos"
```

---

### Task 3: The shell and the `Feature` trait

**Files:**
- Create: `src/shell/mod.rs`, `src/shell/terminal.rs`
- Modify: `src/main.rs` (everything below `main`), `src/features/repos/mod.rs`

- [ ] **Step 1: Write the shell's tests**

`src/shell/mod.rs`:

```rust
//! The frame every feature runs in: the terminal, the main loop, and the
//! list of features.

mod terminal;

use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event};

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::widgets::Paragraph;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<String>>>;

    struct Stub {
        name: &'static str,
        log: Log,
        changed: bool,
        waiting: bool,
        request: Option<Request>,
    }

    impl Feature for Stub {
        fn on_event(&mut self, _: Event) {
            self.log.borrow_mut().push(format!("{} event", self.name));
        }
        fn drain(&mut self) -> bool {
            self.log.borrow_mut().push(format!("{} drain", self.name));
            self.changed
        }
        fn waiting(&self) -> bool {
            self.waiting
        }
        fn draw(&mut self, frame: &mut Frame) {
            frame.render_widget(Paragraph::new(self.name), frame.area());
        }
        fn request(&self) -> Option<Request> {
            self.request
        }
    }

    fn stub(name: &'static str, log: &Log) -> Stub {
        Stub {
            name,
            log: log.clone(),
            changed: false,
            waiting: false,
            request: None,
        }
    }

    fn shell_of(stubs: Vec<Stub>) -> Shell {
        Shell::new(
            stubs
                .into_iter()
                .map(|stub| Box::new(stub) as Box<dyn Feature>)
                .collect(),
        )
    }

    fn a_key() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
    }

    #[test]
    fn an_event_goes_to_the_active_feature_only() {
        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log), stub("two", &log)]);
        shell.on_event(a_key());
        shell.active = 1;
        shell.on_event(a_key());
        assert_eq!(*log.borrow(), ["one event", "two event"]);
    }

    #[test]
    fn every_feature_is_drained_even_when_it_is_not_on_screen() {
        let log = Log::default();
        let mut second = stub("two", &log);
        second.changed = true;
        let mut shell = shell_of(vec![stub("one", &log), second]);
        assert!(shell.drain());
        assert_eq!(*log.borrow(), ["one drain", "two drain"]);

        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log)]);
        assert!(!shell.drain());
    }

    #[test]
    fn the_shell_waits_while_any_feature_does() {
        let log = Log::default();
        let mut second = stub("two", &log);
        assert!(!shell_of(vec![stub("one", &log), stub("two", &log)]).waiting());
        second.waiting = true;
        assert!(shell_of(vec![stub("one", &log), second]).waiting());
    }

    #[test]
    fn a_restart_asked_by_any_feature_wins_over_a_quit() {
        let log = Log::default();
        assert_eq!(shell_of(vec![stub("one", &log)]).request(), None);
        let mut quits = stub("one", &log);
        quits.request = Some(Request::Quit);
        let mut restarts = stub("two", &log);
        restarts.request = Some(Request::Restart);
        assert_eq!(
            shell_of(vec![quits, restarts]).request(),
            Some(Request::Restart)
        );
        let mut quits = stub("one", &log);
        quits.request = Some(Request::Quit);
        assert_eq!(
            shell_of(vec![quits, stub("two", &log)]).request(),
            Some(Request::Quit)
        );
    }

    #[test]
    fn only_the_active_feature_is_drawn() {
        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log), stub("two", &log)]);
        shell.active = 1;
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).unwrap();
        terminal.draw(|frame| shell.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..8)
            .map(|x| buffer.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert_eq!(row.trim_end(), "two");
    }
}
```

`src/shell/terminal.rs` (empty for now, so the module resolves):

```rust
//! Putting the terminal into full-screen mode and back.
```

Add `mod shell;` to `src/main.rs` (after `mod features;`).

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked shell::`
Expected: compile errors `cannot find trait Feature`, `cannot find type Shell`, `Request`

- [ ] **Step 3: Write the trait and the shell**

Insert in `src/shell/mod.rs` between the `use` lines and the `tests` module:

```rust
/// One thing the program can do, as the shell sees it.
pub trait Feature {
    /// A key, a mouse action, a paste or a resize.
    fn on_event(&mut self, event: Event);
    /// Takes in what background work reported. True when something
    /// changed and the screen should be drawn again.
    fn drain(&mut self) -> bool;
    /// True while something is moving, so the screen is drawn at a
    /// steady pace without waiting for a key.
    fn waiting(&self) -> bool;
    fn draw(&mut self, frame: &mut Frame);
    /// What the feature asks of the program, if anything.
    fn request(&self) -> Option<Request>;
}

/// What a feature can ask of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Quit,
    /// Quit and start the program again: an update was installed.
    Restart,
}

/// The features, and which one is on screen.
struct Shell {
    features: Vec<Box<dyn Feature>>,
    active: usize,
}

impl Shell {
    fn new(features: Vec<Box<dyn Feature>>) -> Self {
        Self {
            features,
            active: 0,
        }
    }

    fn on_event(&mut self, event: Event) {
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.on_event(event);
        }
    }

    /// Background work goes on in every feature, on screen or not.
    fn drain(&mut self) -> bool {
        let mut changed = false;
        for feature in &mut self.features {
            changed |= feature.drain();
        }
        changed
    }

    fn waiting(&self) -> bool {
        self.features.iter().any(|feature| feature.waiting())
    }

    fn draw(&mut self, frame: &mut Frame) {
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.draw(frame);
        }
    }

    fn request(&self) -> Option<Request> {
        let mut asked = None;
        for feature in &self.features {
            match feature.request() {
                Some(Request::Restart) => return Some(Request::Restart),
                Some(Request::Quit) => asked = Some(Request::Quit),
                None => {}
            }
        }
        asked
    }
}

/// Runs the program until a feature asks to quit. `make` builds the
/// features once the console is ready for them.
pub fn run(
    make: impl FnOnce() -> Vec<Box<dyn Feature>>,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal::enable_utf8();
    let mut shell = Shell::new(make());
    let mut screen = terminal::setup()?;
    let mut dirty = true;
    let result = loop {
        if dirty || shell.waiting() {
            screen.draw(|frame| shell.draw(frame))?;
            dirty = false;
        }
        if shell.request().is_some() {
            break Ok(());
        }
        let wait = if shell.waiting() { 40 } else { 200 };
        if event::poll(Duration::from_millis(wait))? {
            while event::poll(Duration::from_millis(0))? {
                shell.on_event(event::read()?);
            }
            dirty = true;
        }
        if shell.drain() {
            dirty = true;
        }
    };
    terminal::restore(&mut screen)?;
    if shell.request() == Some(Request::Restart) {
        // An update was installed: hand the terminal to the new version.
        let exe = std::env::current_exe()?;
        let status = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .status()?;
        std::process::exit(status.code().unwrap_or(0));
    }
    result
}
```

This is `run_tui` from `main.rs` with `app` replaced by `shell`. The order is kept exactly: UTF-8 first, then the features are built (which starts their first `git` processes), then the terminal is switched.

- [ ] **Step 4: Move the terminal code**

Cut `setup_terminal`, `restore_terminal` and `enable_utf8` from `src/main.rs` into `src/shell/terminal.rs`, renamed `setup`, `restore` and `enable_utf8`, each `pub`. The bodies do not change:

```rust
//! Putting the terminal into full-screen mode and back.

use std::io;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub type Screen = Terminal<CrosstermBackend<io::Stdout>>;

/// Switches to the full-screen view. A panic switches back first, and
/// stops the programs that were started.
pub fn setup() -> Result<Screen, Box<dyn std::error::Error>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::atoms::sys::process::kill_active();
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        hook(info);
    }));
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore(terminal: &mut Screen) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Makes the Windows console read and write UTF-8.
pub fn enable_utf8() {
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
            fn SetConsoleCP(code_page: u32) -> i32;
        }
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
    }
}
```

- [ ] **Step 5: Make `App` a feature and move `--doctor`**

Append to `src/features/repos/mod.rs`:

```rust
use ratatui::Frame;
use ratatui::crossterm::event::Event;

use crate::config;
use crate::shell::{Feature, Request};
use crate::update;

impl Feature for app::App {
    fn on_event(&mut self, event: Event) {
        app::App::on_event(self, event);
    }

    fn drain(&mut self) -> bool {
        app::App::drain(self)
    }

    fn waiting(&self) -> bool {
        app::App::waiting(self)
    }

    fn draw(&mut self, frame: &mut Frame) {
        ui::draw(frame, self);
    }

    fn request(&self) -> Option<Request> {
        if !self.should_quit {
            None
        } else if self.restart {
            Some(Request::Restart)
        } else {
            Some(Request::Quit)
        }
    }
}
```

Then cut the whole `fn doctor` from `src/main.rs` and paste it below, made `pub`, with a doc comment. Its body does not change:

```rust
/// `--doctor`: prints the version, the settings and every listed
/// repository's state, without the full-screen view.
pub fn doctor(settings: &config::Settings) -> Result<(), Box<dyn std::error::Error>> {
```

- [ ] **Step 6: Reduce `main.rs` to start-up**

Replace the whole of `src/main.rs` with:

```rust
mod atoms;
mod config;
mod features;
mod shell;
mod update;

use std::io::{self, IsTerminal};

use features::repos::app::App;
use shell::Feature;

fn main() {
    if let Err(err) = run() {
        eprintln!("devassist: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match config::startup()? {
        config::Startup::Help => {
            config::print_help();
            Ok(())
        }
        config::Startup::Run(settings) if settings.doctor => features::repos::doctor(&settings),
        config::Startup::Run(settings) => {
            if !io::stdout().is_terminal() {
                eprintln!("devassist is a full-screen terminal app.");
                eprintln!("Open Windows Terminal or PowerShell and run devassist");
                std::process::exit(1);
            }
            shell::run(move || vec![Box::new(App::new(*settings)) as Box<dyn Feature>])
        }
    }
}
```

- [ ] **Step 7: Verify**

Run: `cargo fmt; cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"` then `cargo test --locked`
Expected: `clippy=0`; `180 passed; 0 failed; 1 ignored`

- [ ] **Step 8: Check the two paths that have no test**

Run: `cargo run -- --doctor; echo "exit=$?"`
Expected: the version line, the repository list, `exit=0`

Run: `cargo run -- --help; echo "exit=$?"`
Expected: the help text, `exit=0`

Run: `cargo run < /dev/null > out.txt 2>&1; echo "exit=$?"; cat out.txt; rm out.txt`
Expected: `devassist is a full-screen terminal app.` and `exit=1` (the output is not a terminal, so the full-screen view is refused before the shell starts)

- [ ] **Step 9: Start the app once**

No automated test runs `shell::run`: it needs a real terminal. Run `cargo run`, check that the overview appears and the status column fills in, then quit. The terminal should be back to normal, with the cursor showing.

- [ ] **Step 10: Commit**

```bash
git add -A src
git commit -m "refactor: run the app through a shell and a Feature trait"
```

---

### Task 4: Bring the docs up to date

`docs/BUILDING.md` has a table of source files. It is already out of date after the atom work (`job.rs`, `pathprompt.rs` and `clipboard.rs` have moved) and this plan moves the rest.

**Files:**
- Modify: `docs/BUILDING.md:87-97`, `CONTRIBUTING.md:52-54`

- [ ] **Step 1: Replace the table rows in `docs/BUILDING.md`**

Replace the eleven rows from `` | `src/main.rs` | `` to `` | `src/select.rs`, `src/clipboard.rs` | `` (lines 87–97) with:

```markdown
| `src/main.rs` | Start-up: the command line, then the shell |
| `src/config.rs` | The settings file and the command line |
| `src/update.rs` | Looking for a newer release and installing it |
| `src/shell/` | The terminal, the main loop, and the `Feature` trait every feature implements |
| `src/features/repos/app.rs` | The repository overview's state and everything the keys do |
| `src/features/repos/ui.rs` | Drawing: the overview and every dialog |
| `src/features/repos/gitops.rs` | Everything that runs `git`: status, fetch, update, clone, remove |
| `src/features/repos/github.rs` | The GitHub API: listing repositories, the sign-in token |
| `src/features/repos/gitprogress.rs` | Reading git's progress output |
| `src/features/repos/settings.rs` | The settings dialog's form |
| `src/atoms/sys/` | Building blocks for the system: running programs, HTTP, clipboard, paths |
| `src/atoms/task/` | Building blocks for background work: the job model, worker threads |
| `src/atoms/ui/` | Building blocks for the screen: text fields, lists, menus, dialogs, the folder prompt, text selection |

A feature is built from atoms and never imports another feature. An atom names no feature type. The design is in `docs/superpowers/specs/2026-11-12-atom-module-split-design.md`.
```

(The paragraph after the table is new; put a blank line before it.)

- [ ] **Step 2: Fix the two paths in `CONTRIBUTING.md`**

```bash
sed -i 's#`src/ui.rs`#`src/features/repos/ui.rs`#; s#`src/gitops.rs`#`src/features/repos/gitops.rs`#' CONTRIBUTING.md
```

- [ ] **Step 3: Check that no doc names a file that is gone**

Run: `grep -rnoE 'src/[a-z_/]+\.rs' README.md CONTRIBUTING.md docs/BUILDING.md docs/RELEASING.md docs/USAGE.md docs/CONFIGURATION.md | sort -u`
Then for each path printed: `ls <path>`
Expected: every path exists.

- [ ] **Step 4: Commit**

```bash
git add docs/BUILDING.md CONTRIBUTING.md
git commit -m "docs: describe the shell, features and atoms layout"
```

---

### Task 5: Final check

- [ ] **Step 1: Confirm the layering**

Run: `grep -rnE 'crate::(features|shell)' src/atoms`
Expected: no output (atoms know nothing above them)

Run: `grep -rnE 'crate::features' src/shell`
Expected: no output (the shell knows the trait, not the features)

- [ ] **Step 2: Confirm the layout**

Run: `ls src src/shell src/features src/features/repos`
Expected:

```
src:                 atoms  config.rs  features  main.rs  shell  update.rs
src/shell:           mod.rs  terminal.rs
src/features:        mod.rs  repos
src/features/repos:  app.rs  github.rs  gitops.rs  gitprogress.rs  mod.rs  settings.rs  ui.rs
```

- [ ] **Step 3: Full CI run**

Run: `cargo fmt --check; echo "fmt=$?"; cargo clippy --all-targets --locked -- -D warnings; echo "clippy=$?"; cargo test --locked`
Expected: `fmt=0`, `clippy=0`, `180 passed; 0 failed; 1 ignored`

No changelog entry: nothing a user can see has changed.

## What a second feature needs after this plan

For the Docker spec to build on:

1. A struct that implements `shell::Feature` (five methods), built from atoms: a `Worker<Msg>` for background work, `process::run` for the `docker` CLI, `Job` for progress, `dialog`, `menu`, `input` and `list` for the screen.
2. A line in `main.rs` adding it to the list given to `shell::run`.
3. Step 4b: a key in the shell to switch the active feature, and the shared chrome (header, footer, help, text selection, the update dialog) taken out of `App`.
