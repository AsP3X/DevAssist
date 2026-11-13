use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::atoms::sys::paths;
use crate::atoms::sys::process::{self, RunError};

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
    /// Commits ahead of upstream, summed across every local branch.
    pub total_ahead: u32,
    /// Commits behind upstream, summed across every local branch.
    pub total_behind: u32,
    /// True only when every local branch has an upstream and matches it.
    pub all_branches_current: bool,
    /// Local branches that are behind the branch they follow.
    pub behind_branches: u32,
    /// Commits on remote branches that no local branch follows or contains.
    pub remote_new: u32,
    /// Those remote branches, the most recently committed first.
    pub remote_branches: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LocalRepo {
    pub folder: String,
    pub owner: String,
    pub repo: String,
    pub origin: String,
    pub path: PathBuf,
    /// The group of the overview this checkout is in. Empty for none.
    pub group: String,
    pub branch: String,
    pub dirty: bool,
    pub ahead: Option<u32>,
    pub behind: Option<u32>,
    pub upstream_gone: bool,
    pub total_ahead: u32,
    pub total_behind: u32,
    pub all_branches_current: bool,
    pub behind_branches: u32,
    pub remote_new: u32,
    pub remote_branches: Vec<String>,
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

    /// Ahead and behind counts that cover every local branch.
    /// The checked-out branch still counts when the full scan did not run.
    pub fn sync_counts(&self) -> (u32, u32) {
        (
            self.total_ahead.max(self.ahead.unwrap_or(0)),
            self.total_behind.max(self.behind.unwrap_or(0)),
        )
    }

    /// True when a remote holds commits that an update could bring in.
    pub fn has_incoming(&self) -> bool {
        self.sync_counts().1 > 0 || self.remote_new > 0
    }

    /// How many branches an update would move or add, as far as the last
    /// check knows. The update itself counts again after it has fetched.
    pub fn incoming_branches(&self) -> u32 {
        let behind = self
            .behind_branches
            .max(u32::from(self.behind.unwrap_or(0) > 0));
        behind.saturating_add(u32::try_from(self.remote_branches.len()).unwrap_or(u32::MAX))
    }

    /// Copies a finished status check into the row.
    pub fn apply(&mut self, info: StatusInfo) {
        self.branch = info.branch;
        self.dirty = info.dirty;
        self.ahead = info.ahead;
        self.behind = info.behind;
        self.upstream_gone = info.upstream_gone;
        self.total_ahead = info.total_ahead;
        self.total_behind = info.total_behind;
        self.all_branches_current = info.all_branches_current;
        self.behind_branches = info.behind_branches;
        self.remote_new = info.remote_new;
        self.remote_branches = info.remote_branches;
        self.error = None;
        self.ready = true;
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
        let current = "up to date";
        let local = match self.sync_counts() {
            (0, 0) if self.branch == "detached" => "detached".to_string(),
            (0, 0) if self.upstream_gone => "upstream gone".to_string(),
            (0, 0) if self.all_branches_current => current.to_string(),
            (0, 0) => "no upstream".to_string(),
            (a, 0) => format!("ahead {a}"),
            (0, b) => format!("behind {b}"),
            (a, b) => format!("ahead {a}, behind {b}"),
        };
        match self.remote_label() {
            Some(remote) if local == current => remote,
            Some(remote) => format!("{local}, {remote}"),
            None => local,
        }
    }

    /// What waits on remote branches that have no local branch, such as
    /// "3 new on dev". None when there is nothing.
    pub fn remote_label(&self) -> Option<String> {
        match self.remote_branches.as_slice() {
            [] => None,
            [only] => Some(format!("{} new on {only}", self.remote_new)),
            many => Some(format!(
                "{} new on {} branches",
                self.remote_new,
                many.len()
            )),
        }
    }

    /// The same with the branch names spelled out, for the detail line.
    pub fn remote_detail(&self) -> Option<String> {
        let names = &self.remote_branches;
        if names.is_empty() {
            return None;
        }
        let commits = if self.remote_new == 1 {
            "1 new commit".to_string()
        } else {
            format!("{} new commits", self.remote_new)
        };
        let shown = names
            .iter()
            .take(3)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let word = if names.len() == 1 {
            "branch"
        } else {
            "branches"
        };
        Some(match names.len().saturating_sub(3) {
            0 => format!("{commits} on remote {word} {shown}"),
            rest => format!("{commits} on remote {word} {shown} and {rest} more"),
        })
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

/// Reads a folder as a GitHub checkout. None when it is not one.
pub fn read_checkout(path: &Path) -> Option<LocalRepo> {
    let git_dir = git_dir(path)?;
    let config = fs::read_to_string(git_dir.join("config")).ok()?;
    let url = origin_url(&config)?;
    let (owner, repo) = github_owner_repo(&url)?;
    Some(LocalRepo {
        folder: folder_name(path),
        owner,
        repo,
        origin: url,
        path: path.to_path_buf(),
        branch: "…".to_string(),
        ..LocalRepo::default()
    })
}

fn folder_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.display().to_string())
}

/// The GitHub checkouts directly inside `root`.
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
        found.extend(read_checkout(&entry.path()));
    }
    found.sort_by_key(|repo| repo.folder.to_lowercase());
    found
}

/// The rows of the overview: one per listed checkout, wherever it is.
/// A path that is gone or no longer a GitHub checkout stays as a row that
/// says so, so it can be taken off the list.
pub fn load(paths: &[PathBuf]) -> Vec<LocalRepo> {
    let mut found: Vec<LocalRepo> = paths
        .iter()
        .map(|path| {
            read_checkout(path).unwrap_or_else(|| LocalRepo {
                folder: folder_name(path),
                repo: folder_name(path),
                path: path.clone(),
                branch: "?".to_string(),
                ready: true,
                error: Some(
                    if path.is_dir() {
                        "not a GitHub checkout"
                    } else {
                        "folder is missing"
                    }
                    .to_string(),
                ),
                ..LocalRepo::default()
            })
        })
        .collect();
    label_checkouts(&mut found);
    found.sort_by_key(|repo| repo.folder.to_lowercase());
    found
}

/// Gives every row a name of its own. Two folders called `api` become
/// `work/api` and `home/api`, and the full path if that is still the same.
fn label_checkouts(repos: &mut [LocalRepo]) {
    for depth in 2..=4 {
        let labels: Vec<String> = repos
            .iter()
            .map(|repo| repo.folder.to_lowercase())
            .collect();
        for index in 0..repos.len() {
            let label = &labels[index];
            if labels.iter().filter(|other| *other == label).count() < 2 {
                continue;
            }
            let path = &repos[index].path;
            repos[index].folder = if depth == 4 {
                path.display().to_string()
            } else {
                let parts: Vec<String> = path
                    .components()
                    .filter_map(|part| match part {
                        std::path::Component::Normal(name) => {
                            Some(name.to_string_lossy().to_string())
                        }
                        _ => None,
                    })
                    .collect();
                parts[parts.len().saturating_sub(depth)..].join("/")
            };
        }
    }
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
    let (ahead, behind) = repo.sync_counts();
    if behind > 0 {
        rank += 1_000 + behind.min(100);
    }
    if repo.remote_new > 0 {
        rank += 400 + repo.remote_new.min(100);
    }
    if repo.dirty {
        rank += 200;
    }
    if ahead > 0 {
        rank += 50;
    }
    if repo.error.is_none() && !repo.all_branches_current && ahead == 0 && behind == 0 {
        rank += 30;
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

/// The checkout of this repository that already sits in `root`, if any.
pub fn existing_checkout(root: &Path, owner: &str, name: &str) -> Option<PathBuf> {
    validate_name(name).ok()?;
    let path = root.join(name);
    same_repo(&path, owner, name).then_some(path)
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
    let mut info = parse_status(&String::from_utf8_lossy(&output.stdout));
    if let Ok(branches) = branch_divergence(path) {
        info.total_ahead = branches.ahead;
        info.total_behind = branches.behind;
        info.all_branches_current = branches.all_current;
        info.behind_branches = branches.behind_branches;
        info.remote_new = branches.remote_new;
        info.remote_branches = branches.remote_branches;
    }
    Ok(info)
}

pub fn check(path: &Path, fetch_remote: bool, token: Option<&str>) -> RepoCheck {
    let mut warning = None;
    let mut fetched = false;
    if fetch_remote {
        match fetch(path, token) {
            Ok(()) => fetched = true,
            Err(_) if process::was_cancelled() => {}
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

/// What an update changed in one checkout.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Updated {
    /// The checked-out branch moved.
    pub current: bool,
    /// Other local branches that were fast-forwarded.
    pub forwarded: Vec<String>,
    /// Local branches created for remote branches that had none.
    pub created: Vec<String>,
    /// Branches left alone, each with the reason.
    pub left: Vec<(String, String)>,
}

impl Updated {
    pub fn changed(&self) -> bool {
        self.current || !self.forwarded.is_empty() || !self.created.is_empty()
    }
}

/// Brings one checkout up to date with its remote, on every branch.
///
/// Fetches once, fast-forwards the checked-out branch and every other
/// local branch that is only behind, and creates a local branch for each
/// branch of origin that has new commits and no local branch yet. Nothing
/// is merged or rebased: a branch with commits of its own is left alone.
/// A branch the remote rewrote follows it when it holds no local work.
/// An error means the fetch or the checked-out branch failed; the other
/// branches have had their turn by then.
///
/// `on_steps` hears how many branches are done out of how many have
/// something to bring in: once after the fetch, then after each branch.
pub fn update(
    path: &Path,
    token: Option<&str>,
    mut on_line: impl FnMut(&str, bool),
    on_steps: impl FnMut(u32, u32),
) -> Result<Updated, String> {
    let mut args = auth_args(token);
    args.extend([
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("fetch"),
        OsString::from("--prune"),
        OsString::from("--progress"),
        OsString::from("--recurse-submodules=no"),
    ]);
    run_git(args, &mut on_line)?;
    if process::was_cancelled() {
        return Err("cancelled".into());
    }
    bring_in(path, true, on_line, on_steps)
}

/// After a clone, which only makes a local branch for the default branch:
/// creates a local branch for every other branch of origin that holds
/// commits of its own, the same way an update does. `created` names them.
pub fn track_remote_branches(
    path: &Path,
    on_line: impl FnMut(&str, bool),
) -> Result<Updated, String> {
    bring_in(path, false, on_line, |_, _| {})
}

/// The part of an update that happens after the fetch: moves and creates
/// local branches from what the remote-tracking branches hold. Without
/// `with_current` the checked-out branch is left as it is.
fn bring_in(
    path: &Path,
    with_current: bool,
    mut on_line: impl FnMut(&str, bool),
    mut on_steps: impl FnMut(u32, u32),
) -> Result<Updated, String> {
    let refs = list_refs(path)?;
    let mut done = Updated::default();

    // What there is to bring in: branches that are behind, and branches of
    // origin with new commits that no local branch follows. Other remotes
    // may reuse a name, so only origin's branches are taken.
    let due =
        |branch: &&LocalBranch| !branch.upstream.is_empty() && !branch.gone && branch.behind > 0;
    let head_due = with_current
        && refs
            .locals
            .iter()
            .filter(|branch| branch.head)
            .any(|branch| due(&branch));
    let others: Vec<&LocalBranch> = refs
        .locals
        .iter()
        .filter(|branch| !branch.head)
        .filter(due)
        .collect();
    let unseen = unseen_remote_commits(path, &refs.tracked()).unwrap_or_default();
    let fresh: Vec<(String, String)> = refs
        .untracked()
        .into_iter()
        .filter(|(reference, tip)| {
            reference.starts_with("refs/remotes/origin/") && unseen.contains(tip)
        })
        .collect();
    let total =
        u32::try_from(usize::from(head_due) + others.len() + fresh.len()).unwrap_or(u32::MAX);
    let mut steps = 0u32;
    on_steps(steps, total);

    let mut current = Ok(());
    if with_current {
        let before = head_commit(path);
        current = run_git(
            [
                OsStr::new("--no-optional-locks"),
                OsStr::new("-C"),
                path.as_os_str(),
                OsStr::new("merge"),
                OsStr::new("--ff-only"),
                OsStr::new("@{upstream}"),
            ],
            &mut on_line,
        );
        done.current = current.is_ok() && before != head_commit(path);
        if done.current && head_due {
            steps += 1;
            on_steps(steps, total);
        }
    }

    for branch in others {
        let name = short_local(&branch.name);
        if branch.ahead > 0 {
            // The remote rewrote the branch, and the commits that look like
            // the branch's own are only what the remote held before.
            let moved = if upstream_was_rewritten(path, &branch.upstream, &branch.name) {
                move_branch(path, &branch.upstream, &branch.name, true)
            } else {
                Err("has commits of its own".to_string())
            };
            match moved {
                Ok(()) => {
                    let upstream = short_upstream(&branch.upstream);
                    on_line(&format!("{name}: moved to the rewritten {upstream}"), false);
                    done.forwarded.push(name.to_string());
                    steps += 1;
                    on_steps(steps, total);
                }
                Err(err) => leave(&mut done, &mut on_line, name, &err),
            }
            continue;
        }
        match fast_forward(path, &branch.upstream, &branch.name) {
            Ok(()) => {
                let commits = if branch.behind == 1 {
                    "1 commit".to_string()
                } else {
                    format!("{} commits", branch.behind)
                };
                on_line(&format!("{name}: fast-forwarded {commits}"), false);
                done.forwarded.push(name.to_string());
                steps += 1;
                on_steps(steps, total);
            }
            Err(err) => leave(&mut done, &mut on_line, name, &err),
        }
    }

    for (reference, _) in fresh {
        let name = reference
            .strip_prefix("refs/remotes/origin/")
            .unwrap_or(&reference);
        let local = format!("refs/heads/{name}");
        let moved = match refs.locals.iter().find(|branch| branch.name == local) {
            None => track_branch(path, name, &reference).map(|()| {
                on_line(
                    &format!("{name}: new local branch that follows origin/{name}"),
                    false,
                );
                done.created.push(name.to_string());
            }),
            Some(branch) if branch.head => Err("does not follow origin".to_string()),
            Some(_) if is_ancestor(path, &local, &reference) => {
                fast_forward(path, &reference, &local).map(|()| {
                    on_line(&format!("{name}: fast-forwarded to origin/{name}"), false);
                    done.forwarded.push(name.to_string());
                })
            }
            Some(_) => Err("has commits of its own".to_string()),
        };
        match moved {
            Ok(()) => {
                steps += 1;
                on_steps(steps, total);
            }
            Err(reason) => leave(&mut done, &mut on_line, name, &reason),
        }
    }

    current.map(|()| done)
}

fn leave(done: &mut Updated, on_line: &mut impl FnMut(&str, bool), name: &str, reason: &str) {
    on_line(&format!("{name}: left alone, {reason}"), false);
    done.left.push((name.to_string(), reason.to_string()));
}

fn short_local(reference: &str) -> &str {
    reference.strip_prefix("refs/heads/").unwrap_or(reference)
}

fn head_commit(path: &Path) -> Option<String> {
    let output = git_command()
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "-q", "--verify", "HEAD"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Moves a branch that is not checked out forward to `from`. Git refuses
/// anything that is not a fast-forward, and a branch another worktree has
/// checked out.
fn fast_forward(path: &Path, from: &str, to: &str) -> Result<(), String> {
    move_branch(path, from, to, false)
}

/// "refs/remotes/origin/dev" reads as "origin/dev".
fn short_upstream(reference: &str) -> &str {
    reference.strip_prefix("refs/remotes/").unwrap_or(reference)
}

/// True when a branch holds nothing but what its upstream held earlier:
/// the remote was force-pushed and nobody committed to the branch here.
/// Git reads that from the upstream's reflog, so a branch whose history
/// there has expired counts as having commits of its own.
fn upstream_was_rewritten(path: &Path, upstream: &str, local: &str) -> bool {
    let commit = |args: &[&str]| {
        let output = git_command().arg("-C").arg(path).args(args).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && !text.is_empty()).then_some(text)
    };
    let tip = commit(&["rev-parse", "-q", "--verify", local]);
    tip.is_some() && tip == commit(&["merge-base", "--fork-point", upstream, local])
}

/// Moves a branch that is not checked out to `from`. Without `force` git
/// refuses anything that is not a fast-forward. Either way it refuses a
/// branch another worktree has checked out.
fn move_branch(path: &Path, from: &str, to: &str, force: bool) -> Result<(), String> {
    let force = if force { "+" } else { "" };
    let output = git_command()
        .arg("-C")
        .arg(path)
        .args([
            "fetch",
            "--no-tags",
            "--no-prune",
            "--recurse-submodules=no",
            ".",
        ])
        .arg(format!("{force}{from}:{to}"))
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(last_line(&String::from_utf8_lossy(&output.stderr)))
    }
}

/// Creates a local branch that follows a remote one.
fn track_branch(path: &Path, name: &str, remote: &str) -> Result<(), String> {
    let output = git_command()
        .arg("-C")
        .arg(path)
        .args(["branch", "-q", "--track", "--"])
        .arg(name)
        .arg(remote)
        .output()
        .map_err(|err| err.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(last_line(&String::from_utf8_lossy(&output.stderr)))
    }
}

fn is_ancestor(path: &Path, older: &str, newer: &str) -> bool {
    git_command()
        .arg("-C")
        .arg(path)
        .args(["merge-base", "--is-ancestor"])
        .arg(older)
        .arg(newer)
        .output()
        .is_ok_and(|output| output.status.success())
}

fn last_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .to_string()
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

/// Deletes a checkout from disk. Only a git checkout below some other
/// folder is deleted: never a drive, a filesystem root or the home folder.
pub fn remove_checkout(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!("{} is not a full path", path.display()));
    }
    let dest = path
        .canonicalize()
        .map_err(|_| format!("{} is not there anymore", path.display()))?;
    let home = paths::home_dir().and_then(|home| home.canonicalize().ok());
    if dest.parent().is_none() || home.as_deref() == Some(dest.as_path()) {
        return Err(format!("refusing to delete {}", path.display()));
    }
    if git_dir(&dest).is_none() {
        return Err(format!("{} is not a git checkout", path.display()));
    }
    remove_tree(&dest)
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

    let total_ahead = ahead.unwrap_or(0);
    let total_behind = behind.unwrap_or(0);
    // Until the full branch scan lands, trust the checked-out branch only.
    let all_branches_current =
        ahead == Some(0) && behind == Some(0) && !upstream_gone && branch != "detached";
    StatusInfo {
        branch,
        dirty,
        ahead,
        behind,
        upstream_gone,
        total_ahead,
        total_behind,
        all_branches_current,
        behind_branches: u32::from(total_behind > 0),
        remote_new: 0,
        remote_branches: Vec::new(),
    }
}

/// What a walk over every local and remote branch found.
#[derive(Debug, Default)]
struct Branches {
    ahead: u32,
    behind: u32,
    all_current: bool,
    behind_branches: u32,
    remote_new: u32,
    remote_branches: Vec<String>,
}

struct ParsedRefs {
    ahead: u32,
    behind: u32,
    all_current: bool,
    /// Local branches that are behind the branch they follow.
    behind_branches: u32,
    /// Local branches that follow nothing.
    local_only: Vec<String>,
    /// The upstreams local branches follow, where they still exist.
    tracked: Vec<String>,
    /// Remote branches no local branch follows, as (ref, tip), in git's order.
    untracked: Vec<(String, String)>,
    has_remotes: bool,
}

/// One local branch as `for-each-ref` reports it.
struct LocalBranch {
    /// The full ref, such as "refs/heads/main".
    name: String,
    /// The full upstream ref. Empty when the branch follows nothing.
    upstream: String,
    ahead: u32,
    behind: u32,
    gone: bool,
    /// Checked out in this folder.
    head: bool,
}

struct RefList {
    locals: Vec<LocalBranch>,
    /// Remote branches as (ref, tip), the most recently committed first.
    remotes: Vec<(String, String)>,
    has_remotes: bool,
}

impl RefList {
    /// The upstreams local branches follow, where they still exist.
    fn tracked(&self) -> Vec<String> {
        self.locals
            .iter()
            .filter(|branch| !branch.upstream.is_empty() && !branch.gone)
            .map(|branch| branch.upstream.clone())
            .collect()
    }

    /// Remote branches no local branch follows.
    fn untracked(&self) -> Vec<(String, String)> {
        let tracked = self.tracked();
        self.remotes
            .iter()
            .filter(|(name, _)| !tracked.contains(name))
            .cloned()
            .collect()
    }
}

fn list_refs(path: &Path) -> Result<RefList, String> {
    let output = git_command()
        .arg("-C")
        .arg(path)
        .arg("for-each-ref")
        .arg("--sort=-committerdate")
        .arg("--format=%(refname)%09%(objectname)%09%(upstream)%09%(upstream:track)%09%(HEAD)")
        .arg("refs/heads")
        .arg("refs/remotes")
        .output()
        .map_err(|err| format!("git for-each-ref: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let err = err.trim();
        if err.is_empty() {
            return Err(format!("git for-each-ref exited {}", output.status));
        }
        return Err(first_line(err));
    }
    Ok(parse_ref_list(&String::from_utf8_lossy(&output.stdout)))
}

fn branch_divergence(path: &Path) -> Result<Branches, String> {
    let parsed = summarize(&list_refs(path)?);
    let mut branches = Branches {
        ahead: parsed.ahead,
        behind: parsed.behind,
        all_current: parsed.all_current,
        behind_branches: parsed.behind_branches,
        ..Branches::default()
    };
    if !parsed.local_only.is_empty()
        && parsed.has_remotes
        && let Ok(extra) = local_only_commits(path, &parsed.local_only)
    {
        branches.ahead = branches.ahead.saturating_add(extra);
    }
    if !parsed.untracked.is_empty()
        && let Ok(unseen) = unseen_remote_commits(path, &parsed.tracked)
    {
        (branches.remote_new, branches.remote_branches) = remote_only(&parsed.untracked, &unseen);
    }
    Ok(branches)
}

#[cfg(test)]
fn parse_refs(text: &str) -> ParsedRefs {
    summarize(&parse_ref_list(text))
}

/// Reads `for-each-ref` output over refs/heads and refs/remotes.
fn parse_ref_list(text: &str) -> RefList {
    let mut locals = Vec::new();
    let mut remotes = Vec::new();
    let mut has_remotes = false;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(5, '\t');
        let name = parts.next().unwrap_or("").trim();
        let tip = parts.next().unwrap_or("").trim();
        let upstream = parts.next().unwrap_or("").trim();
        let track = parts.next().unwrap_or("").trim();
        let head = parts.next().unwrap_or("").trim() == "*";
        if name.starts_with("refs/remotes/") {
            has_remotes = true;
            // origin/HEAD only points at another branch.
            if !name.ends_with("/HEAD") {
                remotes.push((name.to_string(), tip.to_string()));
            }
        } else if name.starts_with("refs/heads/") {
            let (ahead, behind, gone) = parse_track(track);
            locals.push(LocalBranch {
                name: name.to_string(),
                upstream: upstream.to_string(),
                ahead,
                behind,
                gone,
                head,
            });
        }
    }
    RefList {
        locals,
        remotes,
        has_remotes,
    }
}

/// Sums what the local branches are ahead and behind.
fn summarize(refs: &RefList) -> ParsedRefs {
    let mut ahead = 0u32;
    let mut behind = 0u32;
    let mut all_current = !refs.locals.is_empty();
    let mut behind_branches = 0u32;
    let mut local_only = Vec::new();
    for branch in &refs.locals {
        if branch.upstream.is_empty() {
            all_current = false;
            local_only.push(branch.name.clone());
            continue;
        }
        if branch.gone {
            all_current = false;
            continue;
        }
        ahead = ahead.saturating_add(branch.ahead);
        behind = behind.saturating_add(branch.behind);
        behind_branches += u32::from(branch.behind > 0);
        if branch.ahead > 0 || branch.behind > 0 {
            all_current = false;
        }
    }
    ParsedRefs {
        ahead,
        behind,
        all_current,
        behind_branches,
        local_only,
        tracked: refs.tracked(),
        untracked: refs.untracked(),
        has_remotes: refs.has_remotes,
    }
}

fn parse_track(track: &str) -> (u32, u32, bool) {
    let inside = track.trim().trim_start_matches('[').trim_end_matches(']');
    if inside == "gone" {
        return (0, 0, true);
    }
    let mut ahead = 0u32;
    let mut behind = 0u32;
    for part in inside.split(',') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix("ahead ") {
            ahead = value.trim().parse().unwrap_or(0);
        } else if let Some(value) = part.strip_prefix("behind ") {
            behind = value.trim().parse().unwrap_or(0);
        }
    }
    (ahead, behind, false)
}

fn local_only_commits(path: &Path, refs: &[String]) -> Result<u32, String> {
    if refs.is_empty() {
        return Ok(0);
    }
    let mut cmd = git_command();
    cmd.arg("-C").arg(path).arg("rev-list").arg("--count");
    for reference in refs {
        cmd.arg(reference);
    }
    cmd.arg("--not").arg("--remotes");
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&output.stderr)));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.trim()
        .parse()
        .map_err(|_| format!("unexpected rev-list count: {}", text.trim()))
}

/// The commits the remotes have that no local branch has or follows.
/// What a followed upstream holds already counts as "behind".
fn unseen_remote_commits(
    path: &Path,
    tracked: &[String],
) -> Result<std::collections::HashSet<String>, String> {
    let mut cmd = git_command();
    cmd.arg("-C")
        .arg(path)
        .arg("rev-list")
        .arg("--ignore-missing")
        .arg("--remotes")
        .arg("--not")
        .arg("--branches")
        .arg("HEAD");
    for reference in tracked {
        cmd.arg(reference);
    }
    let output = cmd.output().map_err(|err| err.to_string())?;
    if !output.status.success() {
        return Err(first_line(&String::from_utf8_lossy(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

/// Counts the unseen commits and names the remote branches that hold them.
/// A branch holds some exactly when its tip is one of them.
fn remote_only(
    untracked: &[(String, String)],
    unseen: &std::collections::HashSet<String>,
) -> (u32, Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    for (reference, tip) in untracked {
        if !unseen.contains(tip) {
            continue;
        }
        let name = short_remote(reference);
        if !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        return (0, names);
    }
    (u32::try_from(unseen.len()).unwrap_or(u32::MAX), names)
}

/// "refs/remotes/origin/dev" reads as "dev"; other remotes keep their name.
fn short_remote(reference: &str) -> String {
    let name = reference.strip_prefix("refs/remotes/").unwrap_or(reference);
    name.strip_prefix("origin/").unwrap_or(name).to_string()
}

pub fn github_owner_repo(url: &str) -> Option<(String, String)> {
    let url = url.trim();
    let rest = if let Some(rest) = url.strip_prefix("git@github.com:") {
        rest
    } else if let Some(rest) = url.strip_prefix("ssh://git@github.com/") {
        rest
    } else {
        let index = url.find("github.com/")?;
        &url[index + "github.com/".len()..]
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
    // GitHub's git HTTP endpoint rejects Authorization: Bearer and answers
    // 401 with a Basic challenge. A disabled terminal prompt then fails the fetch.
    let mut raw = b"x-access-token:".to_vec();
    raw.extend(token.as_bytes());
    Some(format!(
        "AUTHORIZATION: Basic {}",
        crate::atoms::sys::encode::base64(&raw)
    ))
}

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
    fn git_https_sends_the_token_as_basic_auth() {
        assert_eq!(
            auth_header(Some("secret")).as_deref(),
            Some("AUTHORIZATION: Basic eC1hY2Nlc3MtdG9rZW46c2VjcmV0")
        );
        assert!(auth_header(Some("bad token")).is_none());
        assert!(auth_header(None).is_none());
    }

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
    fn only_a_checkout_is_deleted() {
        let root = std::env::temp_dir().join(format!("devassist-rm-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("plain")).unwrap();
        write_origin(&root, "demo", "https://github.com/acme/demo.git");
        assert!(remove_checkout(Path::new("demo")).is_err());
        assert!(remove_checkout(&root.join("missing")).is_err());
        assert!(remove_checkout(&root.join("plain")).is_err());
        assert!(root.join("plain").is_dir());
        if let Some(top) = root.ancestors().last() {
            assert!(remove_checkout(top).is_err());
        }
        remove_checkout(&root.join("demo")).unwrap();
        assert!(!root.join("demo").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn listed_checkouts_load_from_anywhere_with_names_of_their_own() {
        let root = std::env::temp_dir().join(format!("devassist-load-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let work = root.join("work");
        let home = root.join("home");
        fs::create_dir_all(&work).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::create_dir_all(root.join("plain")).unwrap();
        write_origin(&work, "api", "https://github.com/acme/api.git");
        write_origin(&home, "api", "https://github.com/me/api.git");
        write_origin(&home, "notes", "git@github.com:me/notes.git");
        let rows = load(&[
            work.join("api"),
            home.join("notes"),
            home.join("api"),
            root.join("plain"),
            root.join("gone"),
        ]);
        let names: Vec<&str> = rows.iter().map(|repo| repo.folder.as_str()).collect();
        assert_eq!(names, ["gone", "home/api", "notes", "plain", "work/api"]);
        assert_eq!(rows[1].owner, "me");
        assert_eq!(rows[4].owner, "acme");
        assert!(!rows[2].ready);
        assert_eq!(rows[0].error.as_deref(), Some("folder is missing"));
        assert_eq!(rows[3].error.as_deref(), Some("not a GitHub checkout"));
        assert!(rows[0].ready);

        assert_eq!(
            existing_checkout(&work, "ACME", "api"),
            Some(work.join("api"))
        );
        assert_eq!(existing_checkout(&work, "me", "api"), None);
        assert_eq!(existing_checkout(&work, "acme", "web"), None);
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
            total_ahead: 0,
            total_behind: 0,
            all_branches_current: true,
            behind_branches: 0,
            remote_new: 0,
            remote_branches: Vec::new(),
            ready: true,
            fetched: false,
            error: None,
            warning: None,
            group: String::new(),
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
        let root = std::env::temp_dir().join(format!("devassist-scan-{}", std::process::id()));
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
        let root = std::env::temp_dir().join(format!("devassist-dest-{}", std::process::id()));
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

    #[test]
    fn sync_label_says_up_to_date_only_when_every_branch_matches() {
        let current = sample("");
        assert_eq!(current.sync_label(), "up to date");

        let mut other = sample("");
        other.all_branches_current = false;
        other.total_ahead = 2;
        other.total_behind = 5;
        assert_eq!(other.sync_label(), "ahead 2, behind 5");

        let mut ahead = sample("");
        ahead.all_branches_current = false;
        ahead.total_ahead = 4;
        assert_eq!(ahead.sync_label(), "ahead 4");

        let mut missing = sample("");
        missing.all_branches_current = false;
        assert_eq!(missing.sync_label(), "no upstream");
    }

    #[test]
    fn sync_label_names_what_waits_on_other_remote_branches() {
        let mut one = sample("");
        one.remote_new = 3;
        one.remote_branches = vec!["dev".into()];
        assert_eq!(one.sync_label(), "3 new on dev");
        assert_eq!(
            one.remote_detail().unwrap(),
            "3 new commits on remote branch dev"
        );

        let mut several = sample("");
        several.behind = Some(2);
        several.remote_new = 12;
        several.remote_branches = ["dev", "docs", "fix/a", "fix/b", "fix/c"]
            .map(String::from)
            .to_vec();
        assert_eq!(several.sync_label(), "behind 2, 12 new on 5 branches");
        assert_eq!(
            several.remote_detail().unwrap(),
            "12 new commits on remote branches dev, docs, fix/a and 2 more"
        );

        let mut detached = sample("");
        detached.branch = "detached".into();
        detached.remote_new = 1;
        detached.remote_branches = vec!["dev".into()];
        assert_eq!(detached.sync_label(), "detached, 1 new on dev");
        assert_eq!(
            detached.remote_detail().unwrap(),
            "1 new commit on remote branch dev"
        );

        assert!(attention_rank(&one) > attention_rank(&sample("")));
        assert!(attention_rank(&several) > attention_rank(&one));
    }

    #[test]
    fn refs_sum_every_local_branch_and_find_unfollowed_remote_branches() {
        let text = "\
refs/heads/main\taaa\trefs/remotes/origin/main\t
refs/heads/feature\tbbb\trefs/remotes/origin/feature\t[ahead 2, behind 1]
refs/heads/old\tccc\trefs/remotes/origin/old\t[behind 4]
refs/heads/local\tddd\t\t
refs/heads/gone\teee\trefs/remotes/origin/gone\t[gone]
refs/remotes/origin/HEAD\taaa\t\t
refs/remotes/origin/main\taaa\t\t
refs/remotes/origin/feature\tfff\t\t
refs/remotes/origin/old\tggg\t\t
refs/remotes/origin/dev\thhh\t\t
refs/remotes/fork/dev\tiii\t\t
";
        let parsed = parse_refs(text);
        assert_eq!(parsed.ahead, 2);
        assert_eq!(parsed.behind, 5);
        assert!(!parsed.all_current);
        assert!(parsed.has_remotes);
        assert_eq!(parsed.local_only, vec!["refs/heads/local".to_string()]);
        assert_eq!(parsed.tracked.len(), 3);
        assert_eq!(
            parsed.untracked,
            vec![
                ("refs/remotes/origin/dev".to_string(), "hhh".to_string()),
                ("refs/remotes/fork/dev".to_string(), "iii".to_string()),
            ]
        );

        let current = parse_refs(
            "refs/heads/main\ta\trefs/remotes/origin/main\t\nrefs/heads/dev\tb\trefs/remotes/origin/dev\t\n",
        );
        assert!(current.all_current);
        assert_eq!(current.ahead, 0);
        assert_eq!(current.behind, 0);
        assert!(current.local_only.is_empty());
        assert!(current.untracked.is_empty());
        assert!(!current.has_remotes);

        let empty = parse_refs("");
        assert!(!empty.all_current);
    }

    #[test]
    fn only_remote_branches_with_unseen_commits_are_named() {
        let untracked = vec![
            ("refs/remotes/origin/dev".to_string(), "hhh".to_string()),
            ("refs/remotes/origin/merged".to_string(), "aaa".to_string()),
            ("refs/remotes/fork/dev".to_string(), "iii".to_string()),
        ];
        let unseen = ["hhh", "h2", "iii"].map(String::from).into_iter().collect();
        let (commits, names) = remote_only(&untracked, &unseen);
        assert_eq!(commits, 3);
        assert_eq!(names, vec!["dev".to_string(), "fork/dev".to_string()]);

        let none = std::collections::HashSet::new();
        assert_eq!(remote_only(&untracked, &none), (0, Vec::new()));
    }

    fn git(dir: &Path, args: &[&str]) {
        let output = git_command()
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn commit(dir: &Path, file: &str) {
        fs::write(dir.join(file), file).unwrap();
        git(dir, &["add", file]);
        git(dir, &["commit", "-q", "-m", file]);
    }

    #[test]
    fn the_ref_list_knows_which_branch_is_checked_out() {
        let refs = parse_ref_list(
            "refs/heads/main\ta\trefs/remotes/origin/main\t[behind 2]\t*\n\
             refs/heads/dev\tb\trefs/remotes/origin/dev\t\t \n",
        );
        assert_eq!(refs.locals.len(), 2);
        assert!(refs.locals[0].head);
        assert_eq!(refs.locals[0].behind, 2);
        assert!(!refs.locals[1].head);
        assert_eq!(refs.locals[1].upstream, "refs/remotes/origin/dev");
    }

    #[test]
    fn update_moves_every_branch_that_can_move_and_adds_new_ones() {
        let root = std::env::temp_dir().join(format!("devassist-update-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        git(&upstream, &["branch", "-q", "dev"]);
        git(&upstream, &["branch", "-q", "mine"]);
        git(&root, &["clone", "-q", "upstream", "clone"]);
        git(&clone, &["branch", "-q", "--track", "dev", "origin/dev"]);
        git(
            &clone,
            &["checkout", "-q", "--track", "-b", "mine", "origin/mine"],
        );
        commit(&clone, "local");
        git(&clone, &["checkout", "-q", "main"]);

        let mut row = LocalRepo::default();
        row.apply(status(&clone).unwrap());
        assert_eq!(row.incoming_branches(), 0);

        // The remote moves on every branch and gets a new one.
        commit(&upstream, "four");
        git(&upstream, &["checkout", "-q", "dev"]);
        commit(&upstream, "two");
        git(&upstream, &["checkout", "-q", "mine"]);
        commit(&upstream, "three");
        git(&upstream, &["checkout", "-q", "-b", "feature", "main"]);
        commit(&upstream, "five");
        git(&upstream, &["checkout", "-q", "main"]);

        git(&clone, &["fetch", "-q"]);
        row.apply(status(&clone).unwrap());
        assert_eq!(row.incoming_branches(), 4);

        let mut lines = Vec::new();
        let mut steps = Vec::new();
        let done = update(
            &clone,
            None,
            |line, _| lines.push(line.to_string()),
            |done, total| steps.push((done, total)),
        )
        .unwrap();
        // Four branches have something new; the one with local commits stays.
        assert_eq!(steps, vec![(0, 4), (1, 4), (2, 4), (3, 4)]);
        assert!(done.current);
        assert_eq!(done.forwarded, vec!["dev".to_string()]);
        assert_eq!(done.created, vec!["feature".to_string()]);
        assert_eq!(
            done.left,
            vec![("mine".to_string(), "has commits of its own".to_string())]
        );
        assert!(done.changed());
        assert!(
            lines
                .iter()
                .any(|line| line == "dev: fast-forwarded 1 commit")
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "feature: new local branch that follows origin/feature")
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "mine: left alone, has commits of its own")
        );
        assert!(clone.join("four").exists());

        // Only the branch with local commits still differs from the remote.
        let after = status(&clone).unwrap();
        assert_eq!(after.branch, "main");
        assert_eq!(after.behind, Some(0));
        assert_eq!(after.total_behind, 1);
        assert_eq!(after.total_ahead, 1);
        assert_eq!(after.remote_new, 0);
        assert!(after.remote_branches.is_empty());

        // A second run finds nothing to do.
        let mut steps = Vec::new();
        let again = update(
            &clone,
            None,
            |_, _| {},
            |done, total| steps.push((done, total)),
        )
        .unwrap();
        assert_eq!(steps, vec![(0, 1)]);
        assert!(!again.changed());
        assert_eq!(again.left.len(), 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn update_follows_a_rewritten_branch_that_has_no_local_work() {
        let root = std::env::temp_dir().join(format!("devassist-rewrite-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        for name in ["bot", "mine"] {
            git(&upstream, &["checkout", "-q", "-b", name, "main"]);
            commit(&upstream, &format!("{name}-old"));
        }
        git(&upstream, &["checkout", "-q", "main"]);
        git(&root, &["clone", "-q", "upstream", "clone"]);
        git(&clone, &["branch", "-q", "--track", "bot", "origin/bot"]);
        git(
            &clone,
            &["checkout", "-q", "--track", "-b", "mine", "origin/mine"],
        );
        commit(&clone, "local");
        git(&clone, &["checkout", "-q", "main"]);

        // The remote moves main on and rewrites both branches on top of it,
        // the way a bot rebases its pull request.
        commit(&upstream, "two");
        for name in ["bot", "mine"] {
            git(&upstream, &["checkout", "-q", "-B", name, "main"]);
            commit(&upstream, &format!("{name}-new"));
        }
        git(&upstream, &["checkout", "-q", "main"]);

        let mut lines = Vec::new();
        let mut steps = Vec::new();
        let done = update(
            &clone,
            None,
            |line, _| lines.push(line.to_string()),
            |done, total| steps.push((done, total)),
        )
        .unwrap();
        assert_eq!(steps, vec![(0, 3), (1, 3), (2, 3)]);
        assert_eq!(done.forwarded, vec!["bot".to_string()]);
        assert_eq!(
            done.left,
            vec![("mine".to_string(), "has commits of its own".to_string())]
        );
        assert!(
            lines
                .iter()
                .any(|line| line == "bot: moved to the rewritten origin/bot"),
            "{lines:?}"
        );

        // The branch nobody worked on matches the remote again.
        let after = list_refs(&clone).unwrap();
        let branch = |name: &str| {
            after
                .locals
                .iter()
                .find(|branch| branch.name == format!("refs/heads/{name}"))
                .map(|branch| (branch.ahead, branch.behind))
        };
        assert_eq!(branch("bot"), Some((0, 0)));
        assert_eq!(branch("mine"), Some((2, 2)));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_fresh_clone_gets_a_local_branch_for_every_branch_with_commits() {
        let root = std::env::temp_dir().join(format!("devassist-clone-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        git(&upstream, &["checkout", "-q", "-b", "dev"]);
        commit(&upstream, "two");
        git(
            &upstream,
            &["checkout", "-q", "-b", "feature/login", "main"],
        );
        commit(&upstream, "three");
        git(&upstream, &["checkout", "-q", "main"]);
        // Holds nothing main does not have.
        git(&upstream, &["branch", "-q", "merged"]);

        clone_repo(&upstream.display().to_string(), &clone, None, |_, _| {}).unwrap();
        let before = status(&clone).unwrap();
        assert_eq!(before.remote_new, 2);

        let mut lines = Vec::new();
        let done = track_remote_branches(&clone, |line, _| lines.push(line.to_string())).unwrap();
        let mut created = done.created.clone();
        created.sort();
        assert_eq!(created, ["dev", "feature/login"]);
        assert!(!done.current);
        assert!(done.forwarded.is_empty());
        assert!(done.left.is_empty());
        assert!(
            lines
                .iter()
                .any(|line| line == "dev: new local branch that follows origin/dev")
        );
        // Nothing was merged or printed for the branch that is checked out.
        assert!(!lines.iter().any(|line| line.contains("up to date")));

        let mut row = LocalRepo::default();
        row.apply(status(&clone).unwrap());
        assert_eq!(row.branch, "main");
        assert_eq!(row.sync_label(), "up to date");
        assert_eq!(row.remote_new, 0);
        // The working tree still is main's.
        assert!(clone.join("one").exists());
        assert!(!clone.join("two").exists());

        // A second pass has nothing left to do.
        let again = track_remote_branches(&clone, |_, _| {}).unwrap();
        assert!(!again.changed());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn status_sees_commits_on_remote_branches_that_are_not_checked_out() {
        let root = std::env::temp_dir().join(format!("devassist-remote-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        git(&root, &["clone", "-q", "upstream", "clone"]);

        let mut row = LocalRepo::default();
        row.apply(status(&clone).unwrap());
        assert_eq!(row.sync_label(), "up to date");

        // The remote gets a new branch with two commits, and one on main.
        git(&upstream, &["checkout", "-q", "-b", "dev"]);
        commit(&upstream, "two");
        commit(&upstream, "three");
        git(&upstream, &["checkout", "-q", "main"]);
        commit(&upstream, "four");
        git(&upstream, &["branch", "-q", "merged"]);
        git(&clone, &["fetch", "-q"]);

        let fetched = status(&clone).unwrap();
        assert_eq!(fetched.behind, Some(1));
        assert_eq!(fetched.remote_new, 2);
        assert_eq!(fetched.remote_branches, vec!["dev".to_string()]);
        row.apply(fetched);
        assert_eq!(row.sync_label(), "behind 1, 2 new on dev");

        // Pulling main leaves the other branch waiting.
        git(&clone, &["merge", "-q", "--ff-only", "origin/main"]);
        row.apply(status(&clone).unwrap());
        assert_eq!(row.sync_label(), "2 new on dev");

        // Having the branch locally settles it.
        git(&clone, &["branch", "-q", "--track", "dev", "origin/dev"]);
        row.apply(status(&clone).unwrap());
        assert_eq!(row.sync_label(), "up to date");

        // A local branch that was never pushed counts as ahead.
        git(&clone, &["checkout", "-q", "--no-track", "-b", "scratch"]);
        commit(&clone, "five");
        row.apply(status(&clone).unwrap());
        assert_eq!(row.sync_label(), "ahead 1");

        let _ = fs::remove_dir_all(&root);
    }
}
