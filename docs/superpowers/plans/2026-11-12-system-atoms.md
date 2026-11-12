# System Atoms Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extract DevAssist's feature-agnostic system code (base64, path helpers, clipboard, open, process running, HTTP) into `src/atoms/sys/` with no change in behaviour.

**Architecture:** This is step 1 of 4 in `docs/superpowers/specs/2026-11-12-atom-module-split-design.md`. Each atom is a small module that names no feature type (`LocalRepo`, `RemoteRepo`, …). Feature code (`gitops.rs`, `github.rs`, `update.rs`, `app.rs`) keeps its own error wording and calls the atoms. Steps 2–4 of the spec (task atom, UI atoms, shell) get their own plans.

**Tech Stack:** Rust 2024 (MSRV 1.88), ratatui 0.30, ureq 3.5, std only for process handling.

---

## Execution notes (2026-11-12)

Executed on branch `refactor/sys-atoms`; final result `143 passed; 0 failed; 1 ignored`. Two things differed from the steps below:

- **Task 5, `gitops.rs`:** two internal calls to `was_cancelled()` also had to become `process::was_cancelled()`.
- **Task 5, tests:** on Windows `cmd`'s `echo` ends lines with `\r\n`, and the line reader treats a line ended by `\r` as a progress line. `run_hands_over_every_line` therefore checks only the text, and the failure test prints without a line ending (`<nul set /p=boom` on Windows, `printf` elsewhere). This reader behaviour is unchanged from 0.3.1; it matters for any future program that prints CRLF.
- **Not done:** the manual checks in Task 5 step 9 (cancel a running update) and Task 7 step 4 (smoke test in the TUI). `--doctor` was run against GitHub and exited 0.

## Ground rules for every task

- Baseline before starting: `cargo test --locked` → `129 passed; 0 failed; 1 ignored`.
- CI runs three commands; all must pass before each commit:
  - `cargo fmt --check`
  - `cargo clippy --all-targets --locked -- -D warnings`
  - `cargo test --locked`
- This is a binary crate, so an unused `pub fn` is a `dead_code` **error** under `-D warnings`. Every task therefore adds the atom **and** switches the callers in the same commit.
- Line numbers refer to the tree at commit `250e7e4` (Release 0.3.1). They drift as tasks land; locate code by function name when they no longer match.
- Not in this plan: `first_line` exists in both `gitops.rs` and `job.rs` but the two bodies differ, so they are not duplicates. `contains` (in `app.rs` and `ui.rs`) is a UI atom and belongs to the UI-atoms plan.

## File map

| File | Responsibility | Task |
|---|---|---|
| `src/atoms/mod.rs` (new) | declares `sys` | 1 |
| `src/atoms/sys/mod.rs` (new) | declares the six atoms | 1–6 |
| `src/atoms/sys/encode.rs` (new) | `base64` | 1 |
| `src/atoms/sys/paths.rs` (new) | `env_dir`, `home_dir`, `same_path`, `push_path` | 2 |
| `src/atoms/sys/clipboard.rs` (moved from `src/clipboard.rs`) | `set_text` | 3 |
| `src/atoms/sys/open.rs` (new) | `url`, `folder` | 4 |
| `src/atoms/sys/process.rs` (new) | `run`, `quiet`, cancel flag, `kill_active` | 5 |
| `src/atoms/sys/http.rs` (new) | `get`, `Profile`, `Response` | 6 |
| `src/gitops.rs`, `src/github.rs`, `src/update.rs`, `src/config.rs`, `src/app.rs`, `src/ui.rs`, `src/pathprompt.rs`, `src/main.rs` | callers switched to the atoms | 1–6 |

---

### Task 0: Branch

- [ ] **Step 1: Create the work branch**

```bash
git switch -c refactor/sys-atoms
```

- [ ] **Step 2: Confirm the baseline**

Run: `cargo test --locked`
Expected: `test result: ok. 129 passed; 0 failed; 1 ignored`

---

### Task 1: `encode` atom (base64)

**Files:**
- Create: `src/atoms/mod.rs`, `src/atoms/sys/mod.rs`, `src/atoms/sys/encode.rs`
- Modify: `src/main.rs:1` (module list), `src/gitops.rs:1313-1346`, `src/clipboard.rs:80-117`

- [ ] **Step 1: Create the module files with a failing test**

`src/atoms/mod.rs`:

```rust
//! Reusable building blocks. Nothing in here names a feature type.

pub mod sys;
```

`src/atoms/sys/mod.rs`:

```rust
//! Atoms that talk to the operating system or the network.

pub mod encode;
```

`src/atoms/sys/encode.rs`:

```rust
//! Text encodings.

#[cfg(test)]
mod tests {
    use super::base64;

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foob"), "Zm9vYg==");
        assert_eq!(base64(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
```

In `src/main.rs`, add as the first `mod` line (keeps the list alphabetical):

```rust
mod atoms;
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --locked encode`
Expected: compile error `unresolved import super::base64`

- [ ] **Step 3: Move the function in**

Cut `fn base64` from `src/gitops.rs` (the whole function, lines 1316–1346) and paste it into `src/atoms/sys/encode.rs` above the `tests` module. Change only its first line and add a doc comment:

```rust
/// Standard base64 with padding (RFC 4648).
pub fn base64(data: &[u8]) -> String {
```

- [ ] **Step 4: Switch the two callers**

`src/gitops.rs`, in `auth_header`:

```rust
    Some(format!(
        "AUTHORIZATION: Basic {}",
        crate::atoms::sys::encode::base64(&raw)
    ))
```

`src/clipboard.rs`: delete its own `#[cfg(not(windows))] fn base64` (lines 86–117) and change `set_osc52`:

```rust
    let encoded = crate::atoms::sys::encode::base64(text.as_bytes());
```

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `130 passed; 0 failed; 1 ignored` (the existing `auth_header` test in `gitops` still passes).

- [ ] **Step 6: Commit**

```bash
git add src/atoms src/main.rs src/gitops.rs src/clipboard.rs
git commit -m "refactor: add atoms/sys/encode and remove duplicate base64"
```

---

### Task 2: `paths` atom

**Files:**
- Create: `src/atoms/sys/paths.rs`
- Modify: `src/atoms/sys/mod.rs`, `src/config.rs:525-556`, `src/app.rs`, `src/ui.rs:214`, `src/gitops.rs:791`, `src/pathprompt.rs:8,139,233,296`

- [ ] **Step 1: Write the atom's tests**

Add `pub mod paths;` to `src/atoms/sys/mod.rs`.

`src/atoms/sys/paths.rs`:

```rust
//! Helpers for folders and for comparing paths.

use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_path_ignores_slash_style_and_a_trailing_slash() {
        assert!(same_path(Path::new("a\\b"), Path::new("a/b/")));
        assert!(!same_path(Path::new("a/b"), Path::new("a/c")));
    }

    #[test]
    fn same_path_ignores_case_only_on_windows() {
        assert_eq!(
            same_path(Path::new("/A/b"), Path::new("/a/b")),
            cfg!(windows)
        );
    }

    #[test]
    fn push_path_adds_a_path_once() {
        let mut list = vec![PathBuf::from("/srv/code/api")];
        assert!(!push_path(&mut list, PathBuf::from("/srv/code/api/")));
        assert!(push_path(&mut list, PathBuf::from("/srv/code/web")));
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn env_dir_rejects_a_missing_variable() {
        assert!(env_dir("DEVASSIST_NO_SUCH_VARIABLE").is_none());
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked paths`
Expected: compile errors `cannot find function same_path`, `push_path`, `env_dir`

- [ ] **Step 3: Move the four functions**

Cut `env_dir`, `home_dir`, `same_path` and `push_path` (with their doc comments) from `src/config.rs` (lines 525–556) and paste them into `src/atoms/sys/paths.rs` between the `use` line and the `tests` module. Make `env_dir` public; the bodies do not change:

```rust
pub fn env_dir(key: &str) -> Option<PathBuf> {
```

In `src/config.rs`, add below the existing `use crate::…` lines:

```rust
use crate::atoms::sys::paths::{env_dir, home_dir, push_path, same_path};
```

The test `a_checkout_is_listed_once_and_import_is_a_flag` in `config.rs` stays where it is; it reaches the functions through `use super::*`.

- [ ] **Step 4: Switch the other callers**

Run (Git Bash):

```bash
sed -i -E 's/\b(crate::)?config::(same_path|push_path|home_dir)\b/paths::\2/g' src/app.rs src/ui.rs src/gitops.rs src/pathprompt.rs
```

Then add the import to each of the four files, next to their other `use crate::…` lines:

```rust
use crate::atoms::sys::paths;
```

In `src/pathprompt.rs`, delete `use crate::config;` (line 8): nothing else in that file uses it.

- [ ] **Step 5: Check nothing still goes through `config`**

Run: `grep -nE 'config::(same_path|push_path|home_dir)' src/*.rs`
Expected: no output

- [ ] **Step 6: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `134 passed; 0 failed; 1 ignored`

- [ ] **Step 7: Commit**

```bash
git add src
git commit -m "refactor: move path helpers to atoms/sys/paths"
```

---

### Task 3: `clipboard` atom

**Files:**
- Move: `src/clipboard.rs` → `src/atoms/sys/clipboard.rs`
- Modify: `src/main.rs`, `src/atoms/sys/mod.rs`, `src/app.rs:1211,1838`

- [ ] **Step 1: Move the file**

```bash
git mv src/clipboard.rs src/atoms/sys/clipboard.rs
```

Delete `mod clipboard;` from `src/main.rs`. Add `pub mod clipboard;` to `src/atoms/sys/mod.rs` (keep the list alphabetical: `clipboard`, `encode`, `paths`).

In `src/atoms/sys/clipboard.rs`, shorten the call in `set_osc52`:

```rust
    let encoded = super::encode::base64(text.as_bytes());
```

- [ ] **Step 2: Switch the callers**

```bash
sed -i 's/crate::clipboard::set_text/crate::atoms::sys::clipboard::set_text/g' src/app.rs
```

Run: `grep -n 'crate::clipboard' src/*.rs`
Expected: no output

- [ ] **Step 3: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `134 passed; 0 failed; 1 ignored` (the Windows clipboard round-trip test moved with the file).

- [ ] **Step 4: Commit**

```bash
git add -A src
git commit -m "refactor: move clipboard to atoms/sys"
```

---

### Task 4: `open` atom

**Files:**
- Create: `src/atoms/sys/open.rs`
- Modify: `src/atoms/sys/mod.rs`, `src/gitops.rs:801-826,1389-1408`, `src/app.rs:1191,2242`

- [ ] **Step 1: Write the atom with failing tests**

Add `pub mod open;` to `src/atoms/sys/mod.rs`.

`src/atoms/sys/open.rs`:

```rust
//! Hand a web page or a folder to the desktop.

use std::path::Path;
use std::process::{Command, Stdio};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_refuses_anything_but_https() {
        let err = url("http://example.com").unwrap_err();
        assert_eq!(err, "http://example.com is not a web address");
        assert!(url("file:///etc/passwd").is_err());
    }

    #[test]
    fn folder_refuses_what_is_not_a_folder() {
        let missing = std::env::temp_dir().join("devassist-no-such-folder");
        let err = folder(&missing).unwrap_err();
        assert_eq!(err, format!("{} is not a folder", missing.display()));
    }
}
```

Both tests return before anything is spawned, so no window opens during `cargo test`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked open::`
Expected: compile errors `cannot find function url`, `folder`

- [ ] **Step 3: Move the code**

Cut `open_url`, `open_folder` (`src/gitops.rs:801-826`) and `open_command` (`src/gitops.rs:1389-1408`) and paste them into `src/atoms/sys/open.rs` above the `tests` module. Rename them; bodies stay the same apart from the inner call:

```rust
/// Opens a web page in the default browser.
pub fn url(url: &str) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("{url} is not a web address"));
    }
    let mut cmd = command(Path::new(url));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|err| format!("could not open {url}: {err}"))?;
    Ok(())
}

/// Opens a folder in the file manager.
pub fn folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let mut cmd = command(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|err| format!("could not open {}: {err}", path.display()))?;
    Ok(())
}

fn command(target: &Path) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("explorer");
        cmd.arg(target);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(target);
        cmd
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(target);
        cmd
    }
}
```

- [ ] **Step 4: Switch the callers**

In `src/app.rs`, add `use crate::atoms::sys::open;` and change:

- line 1191: `gitops::open_url(page)` → `open::url(page)`
- line 2242: `gitops::open_folder(&repo.path)` → `open::folder(&repo.path)`

Run: `grep -nE 'open_url|open_folder|open_command' src/*.rs`
Expected: no output

- [ ] **Step 5: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `136 passed; 0 failed; 1 ignored`

- [ ] **Step 6: Commit**

```bash
git add src
git commit -m "refactor: move open url/folder to atoms/sys/open"
```

---

### Task 5: `process` atom

This is the atom Docker will reuse: it runs any program, streams its output, and can stop it.

**Files:**
- Create: `src/atoms/sys/process.rs`
- Modify: `src/atoms/sys/mod.rs`, `src/gitops.rs:1-14,828-846,1348-1387,1410-1554`, `src/github.rs:577-596`, `src/app.rs` (15 call sites), `src/main.rs:143`

- [ ] **Step 1: Write the atom's tests**

Add `pub mod process;` to `src/atoms/sys/mod.rs`.

`src/atoms/sys/process.rs`:

```rust
//! Run a program, hear its output line by line, and stop it on request.

use std::io::{BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests {
    use super::*;

    /// A command line the platform's shell runs.
    fn shell(script: &str) -> Command {
        #[cfg(windows)]
        {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", script]);
            cmd
        }
        #[cfg(not(windows))]
        {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", script]);
            cmd
        }
    }

    #[test]
    fn run_hands_over_every_line() {
        let mut lines = Vec::new();
        run(shell("echo one&& echo two"), |line, progress| {
            lines.push((line.to_string(), progress));
        })
        .unwrap();
        assert_eq!(
            lines,
            vec![("one".to_string(), false), ("two".to_string(), false)]
        );
    }

    #[test]
    fn a_failure_reports_the_last_line_it_printed() {
        let err = run(shell("echo boom 1>&2&& exit 3"), |_, _| {}).unwrap_err();
        assert!(matches!(err, RunError::Failed(text) if text == "boom"));
    }

    #[test]
    fn a_silent_failure_reports_the_exit_status() {
        let err = run(shell("exit 3"), |_, _| {}).unwrap_err();
        assert!(matches!(err, RunError::Exit(status) if status.code() == Some(3)));
    }

    #[test]
    fn a_program_that_does_not_exist_cannot_start() {
        let err = run(Command::new("devassist-no-such-program"), |_, _| {}).unwrap_err();
        assert!(matches!(err, RunError::Start(_)));
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked process::`
Expected: compile errors `cannot find function run`, `cannot find type RunError`

- [ ] **Step 3: Write the atom**

Insert between the `use` lines and the `tests` module of `src/atoms/sys/process.rs`. `pump`, `pids`, `track_start`, `track_end`, `PidGuard` and `kill_pid` are cut from `src/gitops.rs` unchanged apart from `hide_window` → `quiet`; `run` is `run_git` with the git wording taken out.

```rust
static PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static CANCELLED: AtomicBool = AtomicBool::new(false);

/// Why a run did not succeed. The caller words the message.
#[derive(Debug)]
pub enum RunError {
    /// The program could not be started.
    Start(std::io::Error),
    /// Waiting for the program failed.
    Wait(String),
    /// The program failed; this is the last line it printed.
    Failed(String),
    /// The program failed without printing anything.
    Exit(ExitStatus),
}

/// Runs `cmd` to the end. `on_line` hears every line of output; the flag is
/// true for a progress line (one ended by a carriage return), which is
/// passed on at most every 80 ms.
pub fn run(mut cmd: Command, mut on_line: impl FnMut(&str, bool)) -> Result<(), RunError> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(RunError::Start)?;
    let pid_guard = PidGuard(child.id());
    track_start(pid_guard.0);
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    if let Some(pipe) = stdout {
        pump(pipe, tx.clone());
    }
    if let Some(pipe) = stderr {
        pump(pipe, tx.clone());
    }
    drop(tx);

    let mut pending: Option<String> = None;
    let mut failure_text = String::new();
    let mut last_progress = Instant::now() - Duration::from_secs(1);
    let flush_progress = |pending: &mut Option<String>,
                          last_progress: &mut Instant,
                          on_line: &mut dyn FnMut(&str, bool)| {
        if last_progress.elapsed() >= Duration::from_millis(80)
            && let Some(line) = pending.take()
        {
            on_line(&line, true);
            *last_progress = Instant::now();
        }
    };

    let status = loop {
        match rx.recv_timeout(Duration::from_millis(40)) {
            Ok((true, text)) => {
                pending = Some(text);
                flush_progress(&mut pending, &mut last_progress, &mut on_line);
            }
            Ok((false, text)) => {
                if let Some(line) = pending.take() {
                    on_line(&line, true);
                }
                failure_text = text.clone();
                on_line(&text, false);
            }
            Err(RecvTimeoutError::Timeout) => {
                flush_progress(&mut pending, &mut last_progress, &mut on_line);
                if let Ok(Some(status)) = child.try_wait() {
                    while let Ok(item) = rx.try_recv() {
                        match item {
                            (true, text) => pending = Some(text),
                            (false, text) => {
                                if let Some(line) = pending.take() {
                                    on_line(&line, true);
                                }
                                failure_text = text.clone();
                                on_line(&text, false);
                            }
                        }
                    }
                    if let Some(line) = pending.take() {
                        on_line(&line, true);
                    }
                    break status;
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                if let Some(line) = pending.take() {
                    on_line(&line, true);
                }
                break child
                    .wait()
                    .map_err(|err| RunError::Wait(err.to_string()))?;
            }
        }
    };
    if status.success() {
        Ok(())
    } else if !failure_text.is_empty() {
        Err(RunError::Failed(failure_text))
    } else if let Some(line) = pending {
        Err(RunError::Failed(line))
    } else {
        Err(RunError::Exit(status))
    }
}

/// Keeps a console window from flashing up on Windows.
pub fn quiet(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

/// Asks every running program to stop and remembers that it was asked.
pub fn request_cancel() {
    CANCELLED.store(true, Ordering::SeqCst);
    kill_active();
}

pub fn clear_cancel() {
    CANCELLED.store(false, Ordering::SeqCst);
}

pub fn was_cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// Stops every program `run` is waiting for.
pub fn kill_active() {
    let pids: Vec<u32> = pids().clone();
    for pid in pids {
        kill_pid(pid);
    }
}

fn pids() -> std::sync::MutexGuard<'static, Vec<u32>> {
    PIDS.lock().unwrap_or_else(|err| err.into_inner())
}

fn track_start(pid: u32) {
    pids().push(pid);
}

fn track_end(pid: u32) {
    pids().retain(|item| *item != pid);
}

struct PidGuard(u32);

impl Drop for PidGuard {
    fn drop(&mut self) {
        track_end(self.0);
    }
}

fn kill_pid(pid: u32) {
    let pid = pid.to_string();
    #[cfg(windows)]
    let mut cmd = Command::new("taskkill");
    #[cfg(windows)]
    {
        cmd.args(["/PID", &pid, "/T", "/F"]);
    }
    #[cfg(not(windows))]
    let mut cmd = Command::new("kill");
    #[cfg(not(windows))]
    {
        cmd.args(["-TERM", &pid]);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    quiet(&mut cmd);
    let _ = cmd.status();
}

fn pump(reader: impl Read + Send + 'static, tx: mpsc::Sender<(bool, String)>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    buf.extend_from_slice(&chunk[..size]);
                    while let Some(pos) =
                        buf.iter().position(|byte| *byte == b'\n' || *byte == b'\r')
                    {
                        let line: Vec<u8> = buf.drain(..=pos).collect();
                        let separator = *line.last().unwrap_or(&b'\n');
                        let text = String::from_utf8_lossy(&line[..line.len() - 1])
                            .trim()
                            .to_string();
                        if text.is_empty() {
                            continue;
                        }
                        let progress = separator == b'\r';
                        if tx.send((progress, text)).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        if !buf.is_empty() {
            let text = String::from_utf8_lossy(&buf).trim().to_string();
            if !text.is_empty() {
                let _ = tx.send((false, text));
            }
        }
    });
}
```

- [ ] **Step 4: Run the atom's tests**

Run: `cargo test --locked process::`
Expected: 4 passed. (Clippy still fails at this point because `gitops.rs` has its own copies; the next step removes them.)

- [ ] **Step 5: Reduce `gitops.rs` to its git-specific part**

Delete from `src/gitops.rs`:

- the statics `PIDS` and `CANCELLED` (lines 13–14)
- `request_cancel`, `clear_cancel`, `was_cancelled`, `kill_active` (lines 828–846)
- `pids`, `track_start`, `track_end`, `PidGuard`, `kill_pid` (lines 1348–1387)
- `hide_window` (lines 1421–1428)
- `pump` (lines 1518–1554)

Replace the import block at the top (lines 1–11) with:

```rust
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::atoms::sys::paths;
use crate::atoms::sys::process::{self, RunError};
```

(`use crate::atoms::sys::paths;` is already there from Task 2; keep one copy.)

Change `git_command` to call the atom:

```rust
fn git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never");
    process::quiet(&mut cmd);
    cmd
}
```

Replace the whole of `run_git` with a wrapper that keeps the existing messages word for word:

```rust
fn run_git(
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    let mut cmd = git_command();
    cmd.args(args);
    process::run(cmd, on_line).map_err(|err| match err {
        RunError::Start(err) => format!("could not start git ({err}). Is Git installed?"),
        RunError::Wait(text) | RunError::Failed(text) => text,
        RunError::Exit(status) => format!("git exited with {status}"),
    })
}
```

- [ ] **Step 6: Switch the callers outside `gitops.rs`**

```bash
sed -i -E 's/\bgitops::(request_cancel|clear_cancel|was_cancelled|kill_active)\b/process::\1/g' src/app.rs
```

Add to `src/app.rs`, next to its other `use crate::…` lines:

```rust
use crate::atoms::sys::process;
```

In `src/main.rs` the one call (`gitops::kill_active();`, line 143) is inside the panic hook; replace it with the full path instead of adding an import:

```rust
        atoms::sys::process::kill_active();
```

In `src/github.rs`, `gh_cli_token` has its own copy of the window flag. Replace

```rust
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
```

with

```rust
    crate::atoms::sys::process::quiet(&mut cmd);
```

- [ ] **Step 7: Check for leftovers**

Run: `grep -nE 'gitops::(request_cancel|clear_cancel|was_cancelled|kill_active)|hide_window|creation_flags' src/*.rs`
Expected: no output (the only `creation_flags` left is in `src/atoms/sys/process.rs`, which this glob does not cover)

- [ ] **Step 8: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `140 passed; 0 failed; 1 ignored`. The `gitops` tests that clone, fetch and update real temporary repositories exercise `process::run` through `run_git`.

- [ ] **Step 9: Check cancelling by hand**

Run: `cargo run`. Start an update of all repositories and cancel it from the progress dialog while it runs (keys are listed in `docs/USAGE.md`).
Expected: the run stops and reports the cancel exactly as 0.3.1 does.

- [ ] **Step 10: Commit**

```bash
git add src
git commit -m "refactor: extract process running and cancelling to atoms/sys/process"
```

---

### Task 6: `http` atom

The two existing agents have different timeouts on purpose: API calls give up after 20 s in total, while a download may run long but must keep moving. The atom keeps both as named profiles so nothing changes.

**Files:**
- Create: `src/atoms/sys/http.rs`
- Modify: `src/atoms/sys/mod.rs`, `src/github.rs:1-3,506-530`, `src/update.rs:13-14,255-287`

- [ ] **Step 1: Write the atom's tests**

Add `pub mod http;` to `src/atoms/sys/mod.rs`.

`src/atoms/sys/http.rs`:

```rust
//! HTTP GET under the program's name, with an optional sign-in.

use std::sync::OnceLock;
use std::time::Duration;

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::thread::{self, JoinHandle};

    /// Answers one request with `reply` and hands back what was asked,
    /// in lowercase.
    fn serve_once(reply: &'static str) -> (String, JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/thing", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut seen = Vec::new();
            let mut chunk = [0u8; 1024];
            while !seen.windows(4).any(|window| window == b"\r\n\r\n") {
                let size = stream.read(&mut chunk).unwrap();
                if size == 0 {
                    break;
                }
                seen.extend_from_slice(&chunk[..size]);
            }
            stream.write_all(reply.as_bytes()).unwrap();
            String::from_utf8_lossy(&seen).to_lowercase()
        });
        (url, server)
    }

    #[test]
    fn get_sends_the_name_the_headers_and_the_sign_in() {
        let (url, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        let mut response = get(
            &url,
            Profile::Quick,
            &[("Accept", "text/plain")],
            Some("secret"),
        )
        .unwrap();
        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.body_mut().read_to_string().unwrap(), "ok");
        let asked = server.join().unwrap();
        assert!(asked.contains("user-agent: devassist"));
        assert!(asked.contains("accept: text/plain"));
        assert!(asked.contains("authorization: bearer secret"));
    }

    #[test]
    fn get_without_a_sign_in_sends_no_authorization() {
        let (url, server) =
            serve_once("HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        get(&url, Profile::Steady, &[], None).unwrap();
        assert!(!server.join().unwrap().contains("authorization"));
    }

    #[test]
    fn a_refusal_comes_back_as_an_answer_not_an_error() {
        let (url, server) = serve_once(
            "HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\nConnection: close\r\n\r\nnope",
        );
        let response = get(&url, Profile::Quick, &[], None).unwrap();
        assert_eq!(response.status().as_u16(), 404);
        server.join().unwrap();
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --locked http::`
Expected: compile errors `cannot find function get`, `cannot find type Profile`

- [ ] **Step 3: Write the atom**

Insert between the `use` lines and the `tests` module:

```rust
/// What a GET answers with, whatever its status.
pub type Response = ureq::http::Response<ureq::Body>;

/// How patient a request is.
#[derive(Clone, Copy)]
pub enum Profile {
    /// Give up after 20 seconds in total. For small API answers.
    Quick,
    /// May take long as a whole, but must keep moving. For downloads.
    Steady,
}

fn agent(profile: Profile) -> &'static ureq::Agent {
    static QUICK: OnceLock<ureq::Agent> = OnceLock::new();
    static STEADY: OnceLock<ureq::Agent> = OnceLock::new();
    match profile {
        Profile::Quick => QUICK.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(20)))
                // The caller judges the status: the body often says why.
                .http_status_as_error(false)
                .build()
                .into()
        }),
        Profile::Steady => STEADY.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(15)))
                .timeout_recv_response(Some(Duration::from_secs(30)))
                .timeout_per_read(Some(Duration::from_secs(30)))
                .http_status_as_error(false)
                .build()
                .into()
        }),
    }
}

/// Asks for `url`. An answer with any status comes back; an error means the
/// server could not be reached. `bearer` is sent as the sign-in.
pub fn get(
    url: &str,
    profile: Profile,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
) -> Result<Response, String> {
    let mut request = agent(profile).get(url).header("User-Agent", "devassist");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if let Some(token) = bearer {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    request.call().map_err(|err| err.to_string())
}
```

- [ ] **Step 4: Run the atom's tests**

Run: `cargo test --locked http::`
Expected: 3 passed

- [ ] **Step 5: Switch `github.rs`**

Delete `fn agent` (lines 506–517) and the imports `use std::sync::OnceLock;` and `use std::time::Duration;` (lines 2–3). Add:

```rust
use crate::atoms::sys::http::{self, Profile};
```

In `gh_get`, replace everything from `let mut request = agent()` down to and including the `.map_err(|err| GhError::Other(err.to_string()))?;` that follows `.call()` with:

```rust
    let mut response = http::get(
        url,
        Profile::Quick,
        &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ],
        token,
    )
    .map_err(GhError::Other)?;
```

The rest of `gh_get` (status, `link`, rate-limit handling) stays as it is.

- [ ] **Step 6: Switch `update.rs`**

Delete `fn agent` (lines 255–268) and the imports `use std::sync::OnceLock;` and `use std::time::Duration;` (lines 13–14). Add:

```rust
use crate::atoms::sys::http::{self, Profile};
```

Replace the `Answer` alias and `get`:

```rust
type Answer = http::Response;

/// Asks GitHub for `url`. An answer with any status comes back; an error
/// means GitHub could not be reached. The sign-in is not passed on when
/// GitHub redirects a download to its file storage.
fn get(url: &str, token: Option<&str>, accept: &str) -> Result<Answer, String> {
    http::get(
        url,
        Profile::Steady,
        &[("Accept", accept), ("X-GitHub-Api-Version", "2022-11-28")],
        token,
    )
    .map_err(|err| format!("could not reach GitHub: {err}"))
}
```

- [ ] **Step 7: Check for leftovers**

Run: `grep -n 'ureq::' src/*.rs`
Expected: no output (all `ureq` use is now in `src/atoms/sys/http.rs`)

- [ ] **Step 8: Verify**

Run: `cargo fmt && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: clippy clean; `143 passed; 0 failed; 1 ignored`

- [ ] **Step 9: Check against the real GitHub by hand**

Run: `cargo run -- --doctor`
Expected: the `update` line reports either "none, this is the latest" or a version, and the `github` line reports a repository count with no `github error:` lines. This needs network access.

- [ ] **Step 10: Commit**

```bash
git add src
git commit -m "refactor: share one HTTP atom between GitHub API and updater"
```

---

### Task 7: Final check

- [ ] **Step 1: Confirm the atoms name no feature**

Run: `grep -nE 'gitops|github|config::|LocalRepo|RemoteRepo|crate::app|crate::ui' src/atoms -r`
Expected: no output

- [ ] **Step 2: Confirm the layout**

Run: `ls src/atoms/sys`
Expected: `clipboard.rs  encode.rs  http.rs  mod.rs  open.rs  paths.rs  process.rs`

- [ ] **Step 3: Full CI run**

Run: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`
Expected: all three pass; `143 passed; 0 failed; 1 ignored`

- [ ] **Step 4: Smoke test the app**

Run: `cargo run`. Check each of these against `docs/USAGE.md`: the list loads and shows statuses; the Add dialog lists remote repositories; the actions menu opens a repository's folder and copies its path; quitting leaves the terminal clean.

No changelog entry: nothing a user can see has changed.
