//! Run a program, hear its output line by line, and stop it on request.

use std::io::{BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

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
        run(shell("echo one&& echo two"), |line, _| {
            lines.push(line.to_string());
        })
        .unwrap();
        assert_eq!(lines, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn a_failure_reports_the_last_line_it_printed() {
        // No line ending: cmd would end an echo with a carriage return,
        // which reads as a progress line.
        let script = if cfg!(windows) {
            "<nul set /p=boom 1>&2& exit 3"
        } else {
            "printf boom 1>&2; exit 3"
        };
        let err = run(shell(script), |_, _| {}).unwrap_err();
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
