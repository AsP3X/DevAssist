use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

static PIDS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
static CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    Auto,
    Https,
    Ssh,
}

impl Protocol {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "https" | "http" => Ok(Self::Https),
            "ssh" => Ok(Self::Ssh),
            other => Err(format!(
                "protocol must be auto, https, or ssh (got {other})"
            )),
        }
    }
}

impl fmt::Display for Protocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::Https => "https",
            Self::Ssh => "ssh",
        })
    }
}

pub struct RepoCheck {
    pub info: Result<StatusInfo, String>,
    pub warning: Option<String>,
    pub fetched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusInfo {
    pub branch: String,
    pub dirty: bool,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream_gone: bool,
}

#[derive(Debug, Clone)]
pub struct LocalRepo {
    pub folder: String,
    pub owner: String,
    pub repo: String,
    pub origin: String,
    pub path: PathBuf,
    pub branch: String,
    pub dirty: bool,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream_gone: bool,
    pub ready: bool,
    pub fetched: bool,
    pub error: Option<String>,
    pub warning: Option<String>,
}

impl LocalRepo {
    pub fn state_label(&self) -> &str {
        if !self.ready {
            "…"
        } else if self.error.is_some() {
            "error"
        } else if self.dirty {
            "dirty"
        } else {
            "clean"
        }
    }

    pub fn sync_label(&self) -> String {
        if !self.ready {
            return "…".to_string();
        }
        if let Some(err) = &self.error {
            return err.clone();
        }
        if self.warning.is_some() {
            return "fetch failed".to_string();
        }
        if self.branch == "detached" {
            return "detached".to_string();
        }
        if self.upstream_gone {
            return "upstream gone".to_string();
        }
        match (self.ahead, self.behind) {
            (None, None) => "no upstream".to_string(),
            (Some(0), Some(0)) => "up to date".to_string(),
            (Some(a), Some(0)) => format!("ahead {a}"),
            (Some(0), Some(b)) => format!("behind {b}"),
            (Some(a), Some(b)) => format!("ahead {a}, behind {b}"),
            (Some(a), None) => format!("ahead {a}"),
            (None, Some(b)) => format!("behind {b}"),
        }
    }
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name == "."
        || name == ".."
        || name.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\0'])
    {
        return Err("invalid repository name".into());
    }
    Ok(())
}

/// `folder` must be a single path segment inside `root`.
pub fn safe_child(root: &Path, folder: &str) -> Result<PathBuf, String> {
    validate_name(folder)?;
    let root = root
        .canonicalize()
        .map_err(|err| format!("workspace: {err}"))?;
    let dest = root.join(folder);
    if !dest.exists() {
        return Err(format!("{folder} is not in this folder"));
    }
    let dest = dest.canonicalize().map_err(|err| err.to_string())?;
    if dest.parent() != Some(root.as_path()) {
        return Err("refusing to touch a path outside the workspace".into());
    }
    Ok(dest)
}

pub fn scan(root: &Path) -> Vec<LocalRepo> {
    let mut found = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let path = entry.path();
        let Some(git_dir) = git_dir(&path) else {
            continue;
        };
        let Ok(config) = fs::read_to_string(git_dir.join("config")) else {
            continue;
        };
        let Some(url) = origin_url(&config) else {
            continue;
        };
        let Some((owner, repo)) = github_owner_repo(&url) else {
            continue;
        };
        let folder = entry.file_name().to_string_lossy().to_string();
        found.push(LocalRepo {
            folder,
            owner,
            repo,
            origin: url,
            path,
            branch: "…".to_string(),
            dirty: false,
            ahead: None,
            behind: None,
            upstream_gone: false,
            ready: false,
            fetched: false,
            error: None,
            warning: None,
        });
    }
    found.sort_by(|a, b| a.folder.to_lowercase().cmp(&b.folder.to_lowercase()));
    found
}

pub fn default_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if !scan(&cwd).is_empty() {
        return cwd;
    }
    if let Some(parent) = cwd.parent()
        && !scan(parent).is_empty()
    {
        return parent.to_path_buf();
    }
    cwd
}

/// Higher means the row should sort above a calm, up-to-date checkout.
pub fn attention_rank(repo: &LocalRepo) -> u32 {
    if !repo.ready {
        return 0;
    }
    let mut rank = 0;
    if repo.error.is_some() {
        rank += 2_000;
    }
    if repo.warning.is_some() {
        rank += 1_600;
    }
    if repo.upstream_gone {
        rank += 1_500;
    }
    if let Some(behind) = repo.behind.filter(|count| *count > 0) {
        rank += 1_000 + behind.min(100);
    }
    if repo.dirty {
        rank += 200;
    }
    if repo.ahead.unwrap_or(0) > 0 {
        rank += 50;
    }
    rank
}

pub fn clone_url(
    owner: &str,
    name: &str,
    protocol: Protocol,
    locals: &[LocalRepo],
    api_url: &str,
) -> String {
    let ssh = match protocol {
        Protocol::Https => false,
        Protocol::Ssh => true,
        Protocol::Auto => prefer_ssh(locals),
    };
    if ssh {
        format!("git@github.com:{owner}/{name}.git")
    } else if api_url.starts_with("https://github.com/")
        || api_url.starts_with("https://www.github.com/")
    {
        api_url.to_string()
    } else {
        format!("https://github.com/{owner}/{name}.git")
    }
}

pub fn clone_destination(root: &Path, owner: &str, name: &str) -> Result<PathBuf, String> {
    validate_name(name)?;
    let primary = root.join(name);
    if !primary.exists() {
        return Ok(primary);
    }
    if same_repo(&primary, owner, name) {
        return Err(format!("{owner}/{name} is already in this folder"));
    }
    let alt_name = format!("{owner}-{name}");
    validate_name(&alt_name)?;
    let alt = root.join(&alt_name);
    if alt.exists() {
        return Err(format!(
            "a folder named {name} already exists, and {alt_name} is taken too"
        ));
    }
    Ok(alt)
}

pub fn status(path: &Path) -> Result<StatusInfo, String> {
    let output = git_command()
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(path)
        .arg("status")
        .arg("-sb")
        .arg("-uno")
        .output()
        .map_err(|err| format!("git status: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let err = err.trim();
        if err.is_empty() {
            return Err(format!("git status exited {}", output.status));
        }
        return Err(first_line(err));
    }
    Ok(parse_status(&String::from_utf8_lossy(&output.stdout)))
}

pub fn check(path: &Path, fetch_remote: bool, token: Option<&str>) -> RepoCheck {
    let mut warning = None;
    let mut fetched = false;
    if fetch_remote {
        match fetch(path, token) {
            Ok(()) => fetched = true,
            Err(_) if was_cancelled() => {}
            Err(err) => warning = Some(err),
        }
    }
    RepoCheck {
        info: status(path),
        warning,
        fetched,
    }
}

pub fn fetch(path: &Path, token: Option<&str>) -> Result<(), String> {
    let mut args = auth_args(token);
    args.extend([
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("fetch"),
        OsString::from("--prune"),
        OsString::from("--recurse-submodules=no"),
    ]);
    run_git(args, |_, _| {})
}

pub fn pull(
    path: &Path,
    token: Option<&str>,
    on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    let mut args = auth_args(token);
    args.extend([
        OsString::from("-c"),
        OsString::from("pull.rebase=false"),
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("pull"),
        OsString::from("--ff-only"),
        OsString::from("--recurse-submodules=no"),
    ]);
    run_git(args, on_line)
}

pub fn clone_repo(
    url: &str,
    dest: &Path,
    token: Option<&str>,
    on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    let mut args = auth_args(token);
    args.extend([
        OsString::from("clone"),
        OsString::from("--progress"),
        OsString::from("--recurse-submodules=no"),
        OsString::from("--"),
        OsString::from(url),
        dest.as_os_str().to_os_string(),
    ]);
    let result = run_git(args, on_line);
    if result.is_err() && dest.exists() {
        let _ = remove_tree(dest);
    }
    result
}

pub fn remove_repo(root: &Path, folder: &str) -> Result<(), String> {
    let dest = safe_child(root, folder)?;
    if git_dir(&dest).is_none() {
        return Err(format!("{folder} is not a git checkout"));
    }
    remove_tree(&dest)
}

pub fn open_folder(path: &Path) -> Result<(), String> {
    if !path.is_dir() {
        return Err(format!("{} is not a folder", path.display()));
    }
    let mut cmd = open_command(path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.spawn()
        .map_err(|err| format!("could not open {}: {err}", path.display()))?;
    Ok(())
}

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

pub fn kill_active() {
    let pids: Vec<u32> = pids().clone();
    for pid in pids {
        kill_pid(pid);
    }
}

pub fn parse_status(text: &str) -> StatusInfo {
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim();
    let dirty = lines.any(|line| !line.trim().is_empty());
    let mut branch = "?".to_string();
    let mut ahead = None;
    let mut behind = None;
    let mut upstream_gone = false;

    if let Some(rest) = first.strip_prefix("## ") {
        if rest.starts_with("HEAD (no branch)") {
            branch = "detached".to_string();
        } else {
            let head = rest.split("...").next().unwrap_or(rest);
            branch = head.split_whitespace().next().unwrap_or("?").to_string();
        }
        if rest.contains("[gone]") {
            upstream_gone = true;
        } else if let Some(brackets) = rest.split('[').nth(1) {
            let inside = brackets.split(']').next().unwrap_or("");
            for part in inside.split(',') {
                let part = part.trim();
                if let Some(value) = part.strip_prefix("ahead ") {
                    ahead = value.trim().parse().ok();
                } else if let Some(value) = part.strip_prefix("behind ") {
                    behind = value.trim().parse().ok();
                }
            }
            if rest.contains("...") {
                ahead = ahead.or(Some(0));
                behind = behind.or(Some(0));
            }
        } else if rest.contains("...") {
            ahead = Some(0);
            behind = Some(0);
        }
    }

    StatusInfo {
        branch,
        dirty,
        ahead,
        behind,
        upstream_gone,
    }
}

pub fn github_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = url.strip_prefix("ssh://git@github.com/") {
        rest
    } else if let Some(index) = url.find("github.com/") {
        &url[index + "github.com/".len()..]
    } else {
        return None;
    };
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = rest.split('/');
    let owner = parts.next().filter(|part| !part.is_empty())?;
    let repo = parts.next().filter(|part| !part.is_empty())?;
    if parts.next().is_some() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

pub fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed.eq_ignore_ascii_case("[remote \"origin\"]");
            continue;
        }
        if !in_origin {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() == "url" {
            let value = value.trim();
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

fn git_dir(repo: &Path) -> Option<PathBuf> {
    let git = repo.join(".git");
    if git.is_dir() {
        return Some(git);
    }
    if !git.is_file() {
        return None;
    }
    let text = fs::read_to_string(&git).ok()?;
    let dir = text
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?;
    let dir = dir.trim();
    let path = PathBuf::from(dir);
    if path.is_absolute() {
        Some(path)
    } else {
        Some(repo.join(path))
    }
}

fn prefer_ssh(locals: &[LocalRepo]) -> bool {
    let ssh = locals.iter().filter(|repo| is_ssh(&repo.origin)).count();
    let https = locals
        .iter()
        .filter(|repo| !repo.origin.is_empty() && !is_ssh(&repo.origin))
        .count();
    ssh > https
}

fn is_ssh(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("git@") || url.starts_with("ssh://")
}

fn same_repo(path: &Path, owner: &str, name: &str) -> bool {
    let Some(git_dir) = git_dir(path) else {
        return false;
    };
    let Ok(config) = fs::read_to_string(git_dir.join("config")) else {
        return false;
    };
    let Some(url) = origin_url(&config) else {
        return false;
    };
    let Some((found_owner, found_name)) = github_owner_repo(&url) else {
        return false;
    };
    found_owner.eq_ignore_ascii_case(owner) && found_name.eq_ignore_ascii_case(name)
}

fn auth_args(token: Option<&str>) -> Vec<OsString> {
    let mut args = vec![OsString::from("--no-optional-locks")];
    if let Some(header) = auth_header(token) {
        args.push(OsString::from("-c"));
        args.push(OsString::from(format!("http.extraHeader={header}")));
    }
    args
}

fn auth_header(token: Option<&str>) -> Option<String> {
    let token = token?.trim();
    if token.is_empty()
        || token
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return None;
    }
    Some(format!("AUTHORIZATION: bearer {token}"))
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
    hide_window(&mut cmd);
    let _ = cmd.status();
}

fn open_command(path: &Path) -> Command {
    #[cfg(windows)]
    {
        let mut cmd = Command::new("explorer");
        cmd.arg(path);
        cmd
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(path);
        cmd
    }
    #[cfg(all(not(windows), not(target_os = "macos")))]
    {
        let mut cmd = Command::new("xdg-open");
        cmd.arg(path);
        cmd
    }
}

fn git_command() -> Command {
    let mut cmd = Command::new("git");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never");
    hide_window(&mut cmd);
    cmd
}

fn hide_window(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let _ = cmd;
}

fn run_git(
    args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    mut on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    let mut child = git_command()
        .args(args)
        .spawn()
        .map_err(|err| format!("could not start git ({err}). Is Git installed?"))?;
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
                break child.wait().map_err(|err| err.to_string())?;
            }
        }
    };
    if status.success() {
        Ok(())
    } else if !failure_text.is_empty() {
        Err(failure_text)
    } else if let Some(line) = pending {
        Err(line)
    } else {
        Err(format!("git exited with {status}"))
    }
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

fn remove_tree(path: &Path) -> Result<(), String> {
    fn clear_readonly(path: &Path) {
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe extern "system" {
                fn GetFileAttributesW(name: *const u16) -> u32;
                fn SetFileAttributesW(name: *const u16, attrs: u32) -> i32;
            }
            unsafe {
                const INVALID: u32 = 0xFFFF_FFFF;
                const READONLY: u32 = 0x1;
                let attrs = GetFileAttributesW(wide.as_ptr());
                if attrs != INVALID && attrs & READONLY != 0 {
                    let _ = SetFileAttributesW(wide.as_ptr(), attrs & !READONLY);
                }
            }
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = fs::symlink_metadata(path) {
                let mut perms = meta.permissions();
                perms.set_mode(perms.mode() | 0o200);
                let _ = fs::set_permissions(path, perms);
            }
        }
    }
    fn rec(path: &Path) -> std::io::Result<()> {
        let meta = fs::symlink_metadata(path)?;
        if meta.file_type().is_symlink() {
            clear_readonly(path);
            return fs::remove_file(path);
        }
        if meta.is_dir() {
            for entry in fs::read_dir(path)? {
                rec(&entry?.path())?;
            }
            clear_readonly(path);
            fs::remove_dir(path)
        } else {
            clear_readonly(path);
            fs::remove_file(path)
        }
    }
    rec(path).map_err(|err| format!("could not delete {}: {err}", path.display()))
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_owner_and_repo_from_common_urls() {
        assert_eq!(
            github_owner_repo("https://github.com/storytold/photocraft"),
            Some(("storytold".into(), "photocraft".into()))
        );
        assert_eq!(
            github_owner_repo("https://github.com/storytold/photocraft.git"),
            Some(("storytold".into(), "photocraft".into()))
        );
        assert_eq!(
            github_owner_repo("git@github.com:storytold/photocraft.git"),
            Some(("storytold".into(), "photocraft".into()))
        );
        assert_eq!(
            github_owner_repo("ssh://git@github.com/storytold/photocraft.git"),
            Some(("storytold".into(), "photocraft".into()))
        );
        assert_eq!(
            github_owner_repo("https://user:token@github.com/storytold/photocraft.git"),
            Some(("storytold".into(), "photocraft".into()))
        );
        assert!(github_owner_repo("https://gitlab.com/storytold/photocraft").is_none());
    }

    #[test]
    fn reads_origin_url_from_git_config() {
        let config = "\
[core]\n\trepositoryformatversion = 0\n\
[remote \"origin\"]\n\turl = https://github.com/storytold/photocraft\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n";
        assert_eq!(
            origin_url(config).as_deref(),
            Some("https://github.com/storytold/photocraft")
        );
    }

    #[test]
    fn parses_short_status() {
        let clean = parse_status("## main...origin/main\n");
        assert_eq!(clean.branch, "main");
        assert!(!clean.dirty);
        assert_eq!(clean.ahead, Some(0));
        assert_eq!(clean.behind, Some(0));

        let drifted = parse_status("## main...origin/main [ahead 1, behind 2]\n");
        assert_eq!(drifted.ahead, Some(1));
        assert_eq!(drifted.behind, Some(2));

        let behind = parse_status("## main...origin/main [behind 3]\n");
        assert_eq!(behind.ahead, Some(0));
        assert_eq!(behind.behind, Some(3));

        let dirty = parse_status("## main...origin/main\n M src/main.rs\n");
        assert!(dirty.dirty);

        let local = parse_status("## main\n");
        assert_eq!(local.branch, "main");
        assert_eq!(local.ahead, None);
        assert_eq!(local.behind, None);

        let detached = parse_status("## HEAD (no branch)\n");
        assert_eq!(detached.branch, "detached");

        let gone = parse_status("## main...origin/main [gone]\n");
        assert!(gone.upstream_gone);
    }

    #[test]
    fn rejects_names_that_escape_the_workspace() {
        let root = std::env::temp_dir().join("artcraft-repos-name-test");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        assert!(safe_child(&root, "..").is_err());
        assert!(safe_child(&root, r"foo\bar").is_err());
        assert!(safe_child(&root, "missing").is_err());
        let _ = fs::remove_dir_all(&root);
    }

    fn sample(origin: &str) -> LocalRepo {
        LocalRepo {
            folder: "demo".into(),
            owner: "acme".into(),
            repo: "demo".into(),
            origin: origin.into(),
            path: PathBuf::from("demo"),
            branch: "main".into(),
            dirty: false,
            ahead: Some(0),
            behind: Some(0),
            upstream_gone: false,
            ready: true,
            fetched: false,
            error: None,
            warning: None,
        }
    }

    fn write_origin(root: &Path, folder: &str, url: &str) {
        let git = root.join(folder).join(".git");
        fs::create_dir_all(&git).unwrap();
        fs::write(
            git.join("config"),
            format!("[remote \"origin\"]\n\turl = {url}\n"),
        )
        .unwrap();
    }

    #[test]
    fn scan_keeps_every_github_owner_and_skips_other_hosts() {
        let root = std::env::temp_dir().join(format!("artcraft-repos-scan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_origin(&root, "alpha", "https://github.com/storytold/alpha.git");
        write_origin(&root, "beta", "git@github.com:other/beta.git");
        write_origin(&root, "gitlab", "https://gitlab.com/acme/gitlab.git");
        fs::create_dir_all(root.join("notes")).unwrap();
        let found = scan(&root);
        let _ = fs::remove_dir_all(&root);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].folder, "alpha");
        assert_eq!(found[0].owner, "storytold");
        assert_eq!(found[1].owner, "other");
        assert!(found[1].origin.starts_with("git@"));
    }

    #[test]
    fn clone_url_follows_the_majority_remote_unless_forced() {
        let https = sample("https://github.com/acme/demo.git");
        let ssh = sample("git@github.com:acme/demo.git");
        assert_eq!(
            clone_url(
                "acme",
                "new",
                Protocol::Auto,
                &[https.clone(), ssh.clone()],
                ""
            ),
            "https://github.com/acme/new.git"
        );
        assert_eq!(
            clone_url(
                "acme",
                "new",
                Protocol::Auto,
                &[ssh.clone(), ssh.clone()],
                ""
            ),
            "git@github.com:acme/new.git"
        );
        assert_eq!(
            clone_url("acme", "new", Protocol::Https, &[ssh], ""),
            "https://github.com/acme/new.git"
        );
        assert_eq!(
            clone_url(
                "acme",
                "new",
                Protocol::Https,
                &[],
                "https://github.com/acme/new.git"
            ),
            "https://github.com/acme/new.git"
        );
        assert_eq!(
            clone_url("acme", "new", Protocol::Ssh, &[https], ""),
            "git@github.com:acme/new.git"
        );
    }

    #[test]
    fn clone_destination_avoids_a_folder_used_by_another_repo() {
        let root = std::env::temp_dir().join(format!("artcraft-repos-dest-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_origin(&root, "demo", "https://github.com/other/demo.git");
        let fresh = clone_destination(&root, "acme", "fresh").unwrap();
        assert_eq!(fresh, root.join("fresh"));
        let alt = clone_destination(&root, "acme", "demo").unwrap();
        assert_eq!(alt, root.join("acme-demo"));
        let err = clone_destination(&root, "other", "demo").unwrap_err();
        assert!(err.contains("already"));
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ranks_repositories_that_need_attention_first() {
        let clean = sample("");
        let mut ahead = sample("");
        ahead.ahead = Some(3);
        let mut dirty = sample("");
        dirty.dirty = true;
        let mut behind = sample("");
        behind.behind = Some(2);
        let mut gone = sample("");
        gone.upstream_gone = true;
        let mut broken = sample("");
        broken.error = Some("nope".into());
        assert!(attention_rank(&broken) > attention_rank(&gone));
        assert!(attention_rank(&gone) > attention_rank(&behind));
        assert!(attention_rank(&behind) > attention_rank(&dirty));
        assert!(attention_rank(&dirty) > attention_rank(&ahead));
        assert!(attention_rank(&ahead) > attention_rank(&clean));
    }

    #[test]
    fn sync_label_says_when_the_fetch_failed() {
        let mut repo = sample("");
        repo.warning = Some("network down".into());
        assert_eq!(repo.sync_label(), "fetch failed");
    }
}
