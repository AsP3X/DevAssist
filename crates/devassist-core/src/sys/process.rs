//! Run a program, hear its output line by line, and stop it on request.

use std::io::{BufReader, Read};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
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

/// Runs `cmd` to the end and returns what it printed. A failure carries
/// the last line of its error output. `kill_active` and `request_cancel`
/// leave it alone.
pub fn capture(mut cmd: Command) -> Result<String, RunError> {
    cmd.stdin(Stdio::null());
    let output = cmd.output().map_err(RunError::Start)?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let errors = String::from_utf8_lossy(&output.stderr);
    match errors.lines().map(str::trim).rfind(|line| !line.is_empty()) {
        Some(line) => Err(RunError::Failed(line.to_string())),
        None => Err(RunError::Exit(output.status)),
    }
}

/// A long-running program whose lines arrive as they are printed.
pub struct Stream {
    pid: u32,
    /// Set by `stop`, and when the program has ended by itself.
    stopped: Arc<AtomicBool>,
}

impl Stream {
    /// Starts `cmd` and returns at once. `on_line` hears every line the
    /// program prints, as an error or not, on a thread of its own.
    /// `on_end` runs once when the program ends by itself: with nothing
    /// when it succeeded, otherwise with the last line it printed or,
    /// when it printed none, its exit status. It does not run for a
    /// stream that was stopped.
    ///
    /// `kill_active` and `request_cancel` leave a stream alone.
    pub fn start(
        mut cmd: Command,
        mut on_line: impl FnMut(&str) + Send + 'static,
        on_end: impl FnOnce(Result<(), String>) + Send + 'static,
    ) -> Result<Stream, RunError> {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(RunError::Start)?;
        let pid = child.id();
        let stopped = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel::<(bool, String)>();
        if let Some(pipe) = child.stdout.take() {
            pump_whole(pipe, tx.clone());
        }
        if let Some(pipe) = child.stderr.take() {
            pump_whole(pipe, tx.clone());
        }
        drop(tx);

        let flag = Arc::clone(&stopped);
        thread::spawn(move || {
            let mut last = String::new();
            let mut hear = |text: String| {
                if !flag.load(Ordering::SeqCst) {
                    on_line(&text);
                    if !text.trim().is_empty() {
                        last = text.trim().to_string();
                    }
                }
            };
            let status = loop {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok((_, text)) => hear(text),
                    // The program can be gone while something it started
                    // still holds its output open.
                    Err(RecvTimeoutError::Timeout) => {
                        if let Ok(Some(status)) = child.try_wait() {
                            while let Ok((_, text)) = rx.try_recv() {
                                hear(text);
                            }
                            break Ok(status);
                        }
                    }
                    Err(RecvTimeoutError::Disconnected) => break child.wait(),
                }
            };
            // From here on `stop` has nothing left to stop.
            if flag.swap(true, Ordering::SeqCst) {
                return;
            }
            on_end(match status {
                Ok(status) if status.success() => Ok(()),
                Ok(status) if last.is_empty() => Err(status.to_string()),
                Ok(_) => Err(last),
                Err(err) => Err(err.to_string()),
            });
        });
        Ok(Stream { pid, stopped })
    }

    /// Stops the program and what it started. Does nothing the second
    /// time, or when the program has already ended.
    pub fn stop(&mut self) {
        if !self.stopped.swap(true, Ordering::SeqCst) {
            kill_pid(self.pid);
        }
    }
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.stop();
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
    #[cfg(feature = "testing")]
    super::note(super::Reach::Program);
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

/// Sends every line as it was printed: with its indentation, and empty
/// ones too. Only a line feed ends a line; the carriage return before it
/// and spaces at the end are left out.
fn pump_whole(reader: impl Read + Send + 'static, tx: mpsc::Sender<(bool, String)>) {
    thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut buf = Vec::new();
        let mut chunk = [0u8; 2048];
        let whole = |line: &[u8]| String::from_utf8_lossy(line).trim_end().to_string();
        loop {
            match reader.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(size) => {
                    buf.extend_from_slice(&chunk[..size]);
                    while let Some(pos) = buf.iter().position(|byte| *byte == b'\n') {
                        let line: Vec<u8> = buf.drain(..=pos).collect();
                        if tx.send((false, whole(&line[..line.len() - 1]))).is_err() {
                            return;
                        }
                    }
                }
            }
        }
        if !buf.is_empty() {
            let _ = tx.send((false, whole(&buf)));
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
    fn capture_returns_the_output_or_the_last_line_of_the_error() {
        let out = capture(shell("echo one&& echo two")).unwrap();
        let lines: Vec<&str> = out.lines().map(str::trim).collect();
        assert_eq!(lines, ["one", "two"]);

        let script = if cfg!(windows) {
            "echo first 1>&2& echo boom 1>&2& exit 3"
        } else {
            "echo first 1>&2; echo boom 1>&2; exit 3"
        };
        let err = capture(shell(script)).unwrap_err();
        assert!(matches!(err, RunError::Failed(text) if text == "boom"));

        let err = capture(shell("exit 3")).unwrap_err();
        assert!(matches!(err, RunError::Exit(status) if status.code() == Some(3)));

        let err = capture(Command::new("devassist-no-such-program")).unwrap_err();
        assert!(matches!(err, RunError::Start(_)));
    }

    #[test]
    fn a_program_that_does_not_exist_cannot_start() {
        let err = run(Command::new("devassist-no-such-program"), |_, _| {}).unwrap_err();
        assert!(matches!(err, RunError::Start(_)));
    }

    /// Long enough for a slow machine, short enough to end a stuck test.
    const WAIT: Duration = Duration::from_secs(20);

    /// A command that prints `up` and then runs for half a minute.
    fn long_running() -> Command {
        shell(if cfg!(windows) {
            "echo up&& ping -n 30 127.0.0.1 >nul"
        } else {
            "echo up; exec sleep 30"
        })
    }

    type Lines = mpsc::Receiver<String>;
    type End = mpsc::Receiver<Result<(), String>>;

    /// Starts a stream that passes its lines and its end on.
    fn stream(cmd: Command) -> Result<(Stream, Lines, End), RunError> {
        let (line_tx, line_rx) = mpsc::channel();
        let (end_tx, end_rx) = mpsc::channel();
        let stream = Stream::start(
            cmd,
            move |line| {
                let _ = line_tx.send(line.to_string());
            },
            move |result| {
                let _ = end_tx.send(result);
            },
        )?;
        Ok((stream, line_rx, end_rx))
    }

    #[test]
    fn a_stream_hands_over_every_line_and_then_its_end() {
        let (_stream, lines, end) = stream(shell("echo one&& echo two")).unwrap();
        assert_eq!(end.recv_timeout(WAIT), Ok(Ok(())));
        let lines: Vec<String> = lines.try_iter().collect();
        assert_eq!(lines, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn a_stream_hears_what_is_printed_as_an_error_too() {
        let (_stream, lines, end) = stream(shell("echo out&& echo err 1>&2")).unwrap();
        assert_eq!(end.recv_timeout(WAIT), Ok(Ok(())));
        let mut lines: Vec<String> = lines.try_iter().collect();
        lines.sort();
        assert_eq!(lines, vec!["err".to_string(), "out".to_string()]);
    }

    #[test]
    fn a_stream_that_fails_ends_with_the_last_line_or_the_exit_status() {
        let script = if cfg!(windows) {
            "echo boom& exit 3"
        } else {
            "echo boom; exit 3"
        };
        let (_stream, _lines, end) = stream(shell(script)).unwrap();
        assert_eq!(end.recv_timeout(WAIT), Ok(Err("boom".to_string())));

        let (_stream, _lines, end) = stream(shell("exit 3")).unwrap();
        let result = end.recv_timeout(WAIT).unwrap();
        assert!(matches!(result, Err(text) if text.contains('3')));
    }

    #[test]
    fn stopping_a_stream_ends_its_program_without_calling_on_end() {
        let (mut stream, lines, end) = stream(long_running()).unwrap();
        assert_eq!(lines.recv_timeout(WAIT), Ok("up".to_string()));
        let started = Instant::now();
        stream.stop();
        // The end is not reported; its channel closes once the program is
        // gone and the stream has let go of it.
        assert_eq!(end.recv_timeout(WAIT), Err(RecvTimeoutError::Disconnected));
        assert!(started.elapsed() < Duration::from_secs(10));
        // Stopping again does nothing.
        stream.stop();
    }

    #[test]
    fn dropping_a_stream_stops_it() {
        let (stream, lines, end) = stream(long_running()).unwrap();
        assert_eq!(lines.recv_timeout(WAIT), Ok("up".to_string()));
        let started = Instant::now();
        drop(stream);
        assert_eq!(end.recv_timeout(WAIT), Err(RecvTimeoutError::Disconnected));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_stream_of_a_program_that_does_not_exist_cannot_start() {
        let result = stream(Command::new("devassist-no-such-program"));
        assert!(matches!(result, Err(RunError::Start(_))));
    }

    #[test]
    fn kill_active_does_not_stop_a_stream() {
        // Calling `kill_active` here would also stop what other tests are
        // running, so this looks at the list it works from.
        let (stream, lines, end) = stream(long_running()).unwrap();
        assert_eq!(lines.recv_timeout(WAIT), Ok("up".to_string()));
        assert!(!pids().contains(&stream.pid));
        assert!(!stream.stopped.load(Ordering::SeqCst));
        assert_eq!(
            end.recv_timeout(Duration::from_millis(300)),
            Err(RecvTimeoutError::Timeout)
        );
    }
}
