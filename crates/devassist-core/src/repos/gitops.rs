use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::sys::paths;
use crate::sys::process::{self, RunError};

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
    /// What the remote has in pull requests that this clone has not
    /// saved. None when the remote was not asked.
    pub pulls: Option<PendingPulls>,
}

/// Pull requests of the remote that differ from what the clone has saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PendingPulls {
    /// Pull requests the clone has no commits of.
    pub new: u32,
    /// Saved pull requests whose commits changed on the remote.
    pub changed: u32,
}

impl PendingPulls {
    pub fn any(self) -> bool {
        self.new > 0 || self.changed > 0
    }
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
    /// Pull requests whose commits are saved in this clone.
    pub pull_requests: u32,
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
    /// Pull requests whose commits are saved in this clone.
    pub pull_requests: u32,
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
        self.pull_requests = info.pull_requests;
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
        info.pull_requests = branches.pull_requests;
    }
    Ok(info)
}

/// How a fetch reaches the remote and what it brings along.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fetch<'a> {
    /// The GitHub token, sent over https for this one command.
    pub token: Option<&'a str>,
    /// Also save the commits of every pull request, under `refs/pull`.
    pub pull_requests: bool,
}

/// Where GitHub publishes the head of each pull request, and where a clone
/// keeps it. That is outside the branches, so nothing else sees them.
const PULL_REFSPEC: &str = "+refs/pull/*/head:refs/pull/*/head";

pub fn check(path: &Path, fetch_remote: bool, how: Fetch) -> RepoCheck {
    let mut warning = None;
    let mut fetched = false;
    if fetch_remote {
        match fetch(path, how) {
            Ok(()) => fetched = true,
            Err(_) if process::was_cancelled() => {}
            Err(err) => warning = Some(err),
        }
    }
    // After a fetch that reached the remote, see what it has in pull
    // requests. A fetch that saves them has left nothing to find.
    let pulls = if !fetched || process::was_cancelled() {
        None
    } else if how.pull_requests {
        Some(PendingPulls::default())
    } else {
        pending_pull_requests(path, how.token).ok()
    };
    RepoCheck {
        info: status(path),
        warning,
        fetched,
        pulls,
    }
}

/// Compares the pull requests of origin with the ones saved in the clone,
/// without fetching anything: which are new, and which got other commits.
pub fn pending_pull_requests(path: &Path, token: Option<&str>) -> Result<PendingPulls, String> {
    let mut args = auth_args(token);
    args.extend([
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("ls-remote"),
        OsString::from("origin"),
        OsString::from("refs/pull/*/head"),
    ]);
    let mut remote: Vec<(String, String)> = Vec::new();
    run_git(args, |line, _| {
        if let Some((tip, name)) = line.trim().split_once('\t')
            && name.starts_with("refs/pull/")
        {
            remote.push((name.trim().to_string(), tip.trim().to_string()));
        }
    })?;
    let saved = pull_tips(path)?;
    let known: std::collections::HashSet<&str> = saved
        .iter()
        .filter_map(|line| line.split_once(' ').map(|(name, _)| name))
        .collect();
    let mut pending = PendingPulls::default();
    for (name, tip) in &remote {
        if saved.contains(&format!("{name} {tip}")) {
            continue;
        }
        if known.contains(name.as_str()) {
            pending.changed += 1;
        } else {
            pending.new += 1;
        }
    }
    Ok(pending)
}

pub fn fetch(path: &Path, how: Fetch) -> Result<(), String> {
    let mut args = auth_args(how.token);
    args.extend([
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("fetch"),
        OsString::from("--prune"),
        OsString::from("--recurse-submodules=no"),
    ]);
    run_git(args, |_, _| {})?;
    if how.pull_requests && !process::was_cancelled() {
        fetch_pull_requests(path, how.token, false, |_, _| {})?;
    }
    Ok(())
}

/// Saves the commits of every pull request of origin under `refs/pull`:
/// open, merged and closed ones alike. It is a backup, so nothing is
/// pruned: a pull request that is gone from the remote stays here.
fn fetch_pull_requests(
    path: &Path,
    token: Option<&str>,
    progress: bool,
    on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    let mut args = auth_args(token);
    args.extend([
        OsString::from("-C"),
        path.as_os_str().to_os_string(),
        OsString::from("fetch"),
        OsString::from("--no-prune"),
        OsString::from("--no-tags"),
        OsString::from("--recurse-submodules=no"),
    ]);
    if progress {
        args.push(OsString::from("--progress"));
    }
    args.extend([OsString::from("origin"), OsString::from(PULL_REFSPEC)]);
    run_git(args, on_line).map_err(|err| format!("pull requests: {err}"))
}

/// What migrating a checkout to another service pushed.
#[derive(Debug, PartialEq, Eq)]
pub struct Migrated {
    /// The remote the address is kept under.
    pub remote: String,
    pub branches: u32,
    pub tags: u32,
}

/// Checks an address a repository is to be pushed to, and tidies it.
/// Web and SSH addresses pass, and a folder that exists, for a bare
/// repository on a disk. Anything Git would read as an option or as a
/// command to run does not.
pub fn push_address(input: &str) -> Result<String, String> {
    let url = input.trim();
    if url.is_empty() {
        return Err("Type the address of the empty repository on the other service.".into());
    }
    let lower = url.to_ascii_lowercase();
    let scheme = ["https://", "http://", "ssh://", "file://"]
        .iter()
        .any(|scheme| lower.starts_with(scheme) && lower.len() > scheme.len());
    let plain =
        !url.starts_with('-') && !url.chars().any(|ch| ch.is_control() || ch.is_whitespace());
    if plain && (scheme || scp_host(url).is_some() || Path::new(url).is_dir()) {
        Ok(url.to_string())
    } else {
        Err("That is not an address Git can push to. It looks like https://host/you/name.git or git@host:you/name.git.".into())
    }
}

/// The host of an address written the short SSH way, `git@host:you/name`.
fn scp_host(url: &str) -> Option<&str> {
    let (left, right) = url.split_once(':')?;
    let (_, host) = left.split_once('@')?;
    (!host.is_empty() && !host.contains('/') && !right.is_empty() && !right.starts_with("//"))
        .then_some(host)
}

/// The host an address points at, such as "gitlab.com". None for a folder.
pub fn address_host(url: &str) -> Option<String> {
    let url = url.trim();
    let host = match url.split_once("://") {
        Some((scheme, _)) if scheme.eq_ignore_ascii_case("file") => return None,
        Some((_, rest)) => {
            let rest = rest.split('/').next().unwrap_or("");
            let rest = rest.rsplit_once('@').map_or(rest, |(_, host)| host);
            rest.split(':').next().unwrap_or("")
        }
        None => scp_host(url)?,
    };
    (!host.is_empty()).then(|| host.to_ascii_lowercase())
}

/// The name the remote of another service gets: "gitlab" for gitlab.com.
/// An address without a usable host is called "mirror".
pub fn remote_name_for(url: &str) -> String {
    let host = address_host(url).unwrap_or_default();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    let name: String = host
        .split('.')
        .next()
        .unwrap_or("")
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-')
        .collect();
    let name = name.trim_matches('-').to_string();
    if name.is_empty() || name == "origin" {
        "mirror".to_string()
    } else {
        name
    }
}

/// Runs a Git command that only reads, and hands back its lines.
fn git_lines(path: &Path, args: &[&str]) -> Result<Vec<String>, String> {
    let output = git_command()
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .map_err(|err| format!("could not start git ({err}). Is Git installed?"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(first_line(err.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

/// Pushes a complete copy of a checkout to `url`, a repository on another
/// service: every branch and every tag. A branch is taken from origin
/// where the local one is missing or only behind, so nothing that was
/// fetched stays behind. Nothing is forced, and the checkout keeps its
/// origin; the address is remembered as one more remote, named after the
/// host. The sign-in for GitHub is not sent along: the other service
/// uses what Git has for it, a saved login or an SSH key.
pub fn migrate_to(
    path: &Path,
    url: &str,
    mut on_line: impl FnMut(&str, bool),
) -> Result<Migrated, String> {
    let url = push_address(url)?;
    let same = |a: &str, b: &str| {
        let tidy = |text: &str| {
            text.trim()
                .trim_end_matches('/')
                .trim_end_matches(".git")
                .to_ascii_lowercase()
        };
        tidy(a) == tidy(b)
    };
    let origin = git_lines(path, &["config", "--get", "remote.origin.url"])
        .ok()
        .and_then(|lines| lines.into_iter().next());
    if origin.as_deref().is_some_and(|origin| same(origin, &url)) {
        return Err("That is the address the repository has already.".into());
    }

    let refs = git_lines(
        path,
        &[
            "for-each-ref",
            "--format=%(refname)%09%(objectname)",
            "refs/heads",
            "refs/remotes/origin",
            "refs/tags",
        ],
    )?;
    let mut locals: std::collections::HashMap<String, String> = Default::default();
    let mut remotes: Vec<(String, String)> = Vec::new();
    let mut tags = 0u32;
    for line in &refs {
        let Some((name, tip)) = line.split_once('\t') else {
            continue;
        };
        if let Some(branch) = name.strip_prefix("refs/heads/") {
            locals.insert(branch.to_string(), tip.to_string());
        } else if let Some(branch) = name.strip_prefix("refs/remotes/origin/") {
            if branch != "HEAD" {
                remotes.push((branch.to_string(), tip.to_string()));
            }
        } else if name.starts_with("refs/tags/") {
            tags += 1;
        }
    }
    // Each branch with the ref it is pushed from.
    let mut sources: std::collections::BTreeMap<String, String> = locals
        .keys()
        .map(|branch| (branch.clone(), format!("refs/heads/{branch}")))
        .collect();
    for (branch, tip) in &remotes {
        let from_origin = match locals.get(branch) {
            None => true,
            Some(local) if local == tip => false,
            // Only behind: origin has the newer commits.
            Some(local) => is_ancestor(path, local, tip),
        };
        if from_origin {
            sources.insert(branch.clone(), format!("refs/remotes/origin/{branch}"));
        }
    }
    if sources.is_empty() {
        return Err("there is no branch to push".into());
    }
    // The default branch goes first and alone: a service makes the first
    // branch it gets the default one.
    let short = |args: &[&str]| {
        git_lines(path, args)
            .ok()
            .and_then(|lines| lines.into_iter().next())
    };
    let default = short(&["symbolic-ref", "-q", "--short", "refs/remotes/origin/HEAD"])
        .and_then(|name| name.strip_prefix("origin/").map(str::to_string))
        .filter(|name| sources.contains_key(name))
        .or_else(|| {
            short(&["symbolic-ref", "-q", "--short", "HEAD"])
                .filter(|name| sources.contains_key(name))
        });
    let spec = |branch: &str| format!("{}:refs/heads/{branch}", sources[branch]);
    let mut batches: Vec<Vec<String>> = Vec::new();
    if let Some(default) = &default {
        batches.push(vec![spec(default)]);
    }
    let rest: Vec<String> = sources
        .keys()
        .filter(|branch| Some(*branch) != default.as_ref())
        .map(|branch| spec(branch))
        .collect();
    batches.extend(rest.chunks(100).map(<[String]>::to_vec));
    if tags > 0 {
        batches.push(vec!["refs/tags/*:refs/tags/*".to_string()]);
    }
    for batch in &batches {
        if process::was_cancelled() {
            return Err("cancelled".into());
        }
        let mut args = vec![
            OsString::from("-C"),
            path.as_os_str().to_os_string(),
            OsString::from("push"),
            OsString::from("--progress"),
            OsString::from(&url),
        ];
        args.extend(batch.iter().map(OsString::from));
        run_git(args, &mut on_line)?;
    }

    // Remember the address, so the copy can be brought up to date later.
    let remote = remote_name_for(&url);
    let known = git_lines(path, &["config", "--get", &format!("remote.{remote}.url")])
        .is_ok_and(|lines| !lines.is_empty());
    let verb = if known { "set-url" } else { "add" };
    git_lines(path, &["remote", verb, &remote, &url])?;
    Ok(Migrated {
        remote,
        branches: u32::try_from(sources.len()).unwrap_or(u32::MAX),
        tags,
    })
}

/// Moves a checkout to `to`, a path that must be free. On the same drive
/// that is a rename; onto another drive everything is copied first and
/// the original deleted after. A warning comes back when the copy is
/// complete but the original could not be deleted.
///
/// `on_line` hears what is going on, the way Git reports a transfer: while
/// copying, a line with a percentage and the files done out of all.
pub fn move_checkout(
    from: &Path,
    to: &Path,
    mut on_line: impl FnMut(&str, bool),
) -> Result<Option<String>, String> {
    if !from.is_dir() {
        return Err(format!("{} is not there any more", from.display()));
    }
    if to.exists() {
        return Err(format!("{} is there already", to.display()));
    }
    if to.starts_with(from) {
        return Err("a folder cannot be moved into itself".into());
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    match fs::rename(from, to) {
        Ok(()) => return Ok(None),
        Err(err) if err.kind() == std::io::ErrorKind::CrossesDevices => {}
        Err(err) => return Err(format!("could not move {}: {err}", from.display())),
    }
    copy_then_delete(from, to, &mut on_line)
}

/// The slow way to move a folder: copies everything to `to`, a path that
/// is free, and deletes `from` once the copy is complete.
fn copy_then_delete(
    from: &Path,
    to: &Path,
    on_line: &mut dyn FnMut(&str, bool),
) -> Result<Option<String>, String> {
    on_line(
        "Another drive: copying, then deleting the old folder.",
        false,
    );
    on_line("Counting files…", true);
    let total = count_files(from);
    let mut done = 0u64;
    let mut shown = u64::MAX;
    let mut copied = |on_line: &mut dyn FnMut(&str, bool)| {
        done += 1;
        // One line for each percent, not one for each file.
        let percent = (done * 100).checked_div(total).unwrap_or(100).min(100);
        if percent != shown {
            shown = percent;
            on_line(
                &format!("Copying files: {percent:>3}% ({done}/{total})"),
                true,
            );
        }
    };
    if let Err(err) = copy_tree(from, to, &mut |_| copied(on_line)) {
        // Leave nothing half-copied behind; the original is untouched.
        on_line("Removing what was copied so far…", true);
        let _ = remove_tree(to);
        return Err(format!("could not copy to {}: {err}", to.display()));
    }
    on_line(&format!("Copied {total} files."), false);
    on_line("Deleting the old folder…", true);
    Ok(remove_tree(from).err().map(|err| {
        format!(
            "copied, but the old folder {} could not be deleted: {err}",
            from.display()
        )
    }))
}

/// How many files a folder holds, with everything below it. What cannot
/// be read is not counted.
fn count_files(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => count_files(&entry.path()),
            _ => 1,
        })
        .sum()
}

/// Copies a folder with everything in it. `on_file` hears each file that
/// has been copied.
fn copy_tree(from: &Path, to: &Path, on_file: &mut dyn FnMut(&Path)) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|err| format!("{}: {err}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|err| format!("{}: {err}", from.display()))?;
    for entry in entries {
        if process::was_cancelled() {
            return Err("cancelled".into());
        }
        let entry = entry.map_err(|err| format!("{}: {err}", from.display()))?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|err| format!("{}: {err}", source.display()))?;
        if kind.is_dir() {
            copy_tree(&source, &target, on_file)?;
        } else {
            fs::copy(&source, &target).map_err(|err| format!("{}: {err}", source.display()))?;
            on_file(&source);
        }
    }
    Ok(())
}

/// What fetching the pull requests of one checkout found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SavedPulls {
    /// How many pull requests the clone holds now.
    pub total: u32,
    /// How many of them are new or got other commits.
    pub changed: u32,
}

/// Saves the pull requests of one checkout now, whatever the setting says:
/// new ones, and ones whose commits changed. Nothing is removed.
pub fn save_pull_requests(
    path: &Path,
    token: Option<&str>,
    on_line: impl FnMut(&str, bool),
) -> Result<SavedPulls, String> {
    let before = pull_tips(path)?;
    fetch_pull_requests(path, token, true, on_line)?;
    let after = pull_tips(path)?;
    let changed = after.iter().filter(|tip| !before.contains(*tip)).count();
    Ok(SavedPulls {
        total: u32::try_from(after.len()).unwrap_or(u32::MAX),
        changed: u32::try_from(changed).unwrap_or(u32::MAX),
    })
}

/// Every saved pull request with the commit it points at, one per entry.
fn pull_tips(path: &Path) -> Result<std::collections::HashSet<String>, String> {
    let output = git_command()
        .arg("-C")
        .arg(path)
        .arg("for-each-ref")
        .arg("--format=%(refname) %(objectname)")
        .arg("refs/pull")
        .output()
        .map_err(|err| format!("git for-each-ref: {err}"))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(first_line(err.trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect())
}

/// What collecting the forks of one repository did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ForksSaved {
    /// The folder was made by this run.
    pub created: bool,
    /// Forks that were fetched.
    pub fetched: u32,
    /// Forks left alone: GitHub says they are as they were the last time
    /// they were fetched.
    pub unchanged: u32,
    /// Forks GitHub still lists but no longer has. They are not tried
    /// again; what was fetched from them stays.
    pub gone: u32,
    /// Forks that could not be fetched, as `owner/name`.
    pub failed: Vec<String>,
    /// Forks that have commits of their own, each with a folder of its
    /// files.
    pub folders: u32,
    /// Branches the folder holds now, of the original and of every fork.
    pub branches: u32,
    /// Pull requests it holds now.
    pub pulls: u32,
}

/// Marks a repository as the collection of the forks of `owner/name`.
const FORKS_KEY: &str = "devassist.forksof";

/// The folder the forks of a checkout go into: beside it, under its name
/// with `-fork` added.
pub fn forks_folder(path: &Path) -> Option<PathBuf> {
    let name = path.file_name()?.to_str()?;
    Some(path.with_file_name(format!("{name}-fork")))
}

/// Whether the forks of a checkout have been cloned beside it.
pub fn forks_cloned(path: &Path) -> bool {
    forks_folder(path).is_some_and(|dest| dest.join(".git").is_dir())
}

/// A fork to fetch into the collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkSource {
    pub owner: String,
    pub name: String,
    /// Something that changes when the fork does. A fork whose stamp is
    /// the one it was last fetched with is not fetched again. Empty when
    /// nothing is known: the fork is fetched every time.
    pub stamp: String,
}

/// How many forks are fetched at the same time.
const FORK_JOBS: usize = 8;

/// The stamps the forks were last fetched with, by owner: a file in the
/// collection's `.git`, one `owner<tab>stamp` a line.
fn stamps_file(dest: &Path) -> PathBuf {
    dest.join(".git").join("devassist-forks")
}

fn read_stamps(dest: &Path) -> std::collections::HashMap<String, String> {
    fs::read_to_string(stamps_file(dest))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (owner, stamp) = line.split_once('\t')?;
            Some((owner.to_string(), stamp.to_string()))
        })
        .collect()
}

/// Written beside the old file and then put in its place, so that a run
/// that is cut short leaves the old one whole.
fn write_stamps(dest: &Path, stamps: &std::collections::HashMap<String, String>) {
    let mut lines: Vec<String> = stamps
        .iter()
        .map(|(owner, stamp)| format!("{owner}\t{stamp}\n"))
        .collect();
    lines.sort_unstable();
    let file = stamps_file(dest);
    let draft = file.with_extension("new");
    if fs::write(&draft, lines.concat()).is_ok() {
        let _ = fs::rename(&draft, &file);
    }
}

/// How the stamp of a fork starts that GitHub no longer has. What
/// follows is the stamp it had when that was found: with another stamp
/// the fork is back and is fetched again.
const GONE: &str = "gone ";

/// Whether a failed fetch says that the repository is not there, as
/// GitHub does for one that was deleted or made private.
fn says_gone(err: &str) -> bool {
    let err = err.to_ascii_lowercase();
    err.contains("repository") && err.contains("not found")
}

/// One fork in the collection, as the fork overview shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkInfo {
    /// The account it belongs to, which is also the name of its remote.
    pub owner: String,
    /// What its repository is called.
    pub name: String,
    pub branches: u32,
    pub pulls: u32,
    /// The most commits one of its branches has that the main branch of
    /// the original lacks. None when that could not be counted.
    pub ahead: Option<u32>,
    /// When its newest commit was made, in seconds since 1970. 0 for a
    /// fork nothing was fetched from.
    pub date: i64,
    /// It has commits of its own, and with them a folder of its files.
    pub folder: bool,
    /// GitHub no longer has it.
    pub gone: bool,
}

/// The forks a collection holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForkList {
    /// The repository they are forks of, as `owner/name`.
    pub original: String,
    /// The folder of the collection.
    pub dest: PathBuf,
    /// By owner, in the order of the alphabet.
    pub forks: Vec<ForkInfo>,
}

/// Reads the forks that were cloned beside the checkout `path`: what is
/// on disk, without asking GitHub.
pub fn list_forks(path: &Path) -> Result<ForkList, String> {
    use std::collections::BTreeMap;
    let dest = forks_folder(path).ok_or("has no folder name")?;
    if !dest.join(".git").is_dir() {
        return Err("The forks are not cloned yet.".into());
    }
    let original = git_lines(&dest, &["config", "--get", FORKS_KEY])
        .ok()
        .and_then(|lines| lines.into_iter().next())
        .ok_or_else(|| format!("{} does not hold forks", dest.display()))?;
    let first = original.split('/').next().unwrap_or_default().to_string();
    let stamps = read_stamps(&dest);
    let mut forks: BTreeMap<String, ForkInfo> = BTreeMap::new();
    for line in
        git_lines(&dest, &["config", "--get-regexp", r"^remote\..*\.url$"]).unwrap_or_default()
    {
        let Some((key, url)) = line.split_once(' ') else {
            continue;
        };
        let Some(owner) = key
            .strip_prefix("remote.")
            .and_then(|rest| rest.strip_suffix(".url"))
        else {
            continue;
        };
        if owner.eq_ignore_ascii_case(&first) {
            continue;
        }
        let url = url.trim();
        let name = github_owner_repo(url).map_or_else(
            || {
                let last = url.trim_end_matches(['/', '\\']);
                let last = last.rsplit(['/', '\\']).next().unwrap_or_default();
                last.strip_suffix(".git").unwrap_or(last).to_string()
            },
            |(_, name)| name,
        );
        forks.insert(
            owner.to_string(),
            ForkInfo {
                owner: owner.to_string(),
                name,
                branches: 0,
                pulls: 0,
                ahead: None,
                date: 0,
                folder: dest.join(owner).join(".git").exists(),
                gone: stamps
                    .get(owner)
                    .is_some_and(|stamp| stamp.starts_with(GONE)),
            },
        );
    }
    // Every branch with its date and, where this Git can count it, how
    // far it is ahead of the main branch of the original.
    let counted = format!(
        "--format=%(refname) %(committerdate:unix) %(ahead-behind:refs/remotes/{first}/HEAD)"
    );
    let plain = "--format=%(refname) %(committerdate:unix)";
    let branches = git_lines(&dest, &["for-each-ref", &counted, "refs/remotes"])
        .or_else(|_| git_lines(&dest, &["for-each-ref", plain, "refs/remotes"]))?;
    for line in branches {
        let mut parts = line.split_whitespace();
        let (Some(name), date, ahead) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let Some((owner, branch)) = name
            .strip_prefix("refs/remotes/")
            .and_then(|rest| rest.split_once('/'))
        else {
            continue;
        };
        if branch == "HEAD" {
            continue;
        }
        let Some(fork) = forks.get_mut(owner) else {
            continue;
        };
        fork.branches += 1;
        fork.date = fork
            .date
            .max(date.and_then(|date| date.parse().ok()).unwrap_or(0));
        if let Some(ahead) = ahead.and_then(|ahead| ahead.parse::<u32>().ok()) {
            fork.ahead = Some(fork.ahead.unwrap_or(0).max(ahead));
        }
    }
    for name in
        git_lines(&dest, &["for-each-ref", "--format=%(refname)", "refs/pull"]).unwrap_or_default()
    {
        let owner = name
            .strip_prefix("refs/pull/")
            .and_then(|rest| rest.split_once('/'));
        if let Some(fork) = owner.and_then(|(owner, _)| forks.get_mut(owner)) {
            fork.pulls += 1;
        }
    }
    Ok(ForkList {
        original,
        dest,
        forks: forks.into_values().collect(),
    })
}

/// What a run does with one fork.
#[derive(Clone, Copy)]
enum ForkPlan {
    Fetch,
    /// As it was when it was last fetched.
    Unchanged,
    /// GitHub did not have it the last time, and lists it as it was then.
    Gone,
    /// It cannot be a remote.
    Skip,
}

/// What `git remote add --no-tags` writes for a fork, with its pull
/// requests kept under its own name. `name` is a valid remote name.
fn remote_section(name: &str, url: &str) -> String {
    let url = url.replace('\\', "\\\\").replace('"', "\\\"");
    format!(
        "[remote \"{name}\"]\n\turl = \"{url}\"\n\tfetch = +refs/heads/*:refs/remotes/{name}/*\n\tfetch = +refs/pull/*/head:refs/pull/{name}/*/head\n\ttagOpt = --no-tags\n"
    )
}

/// Adds sections to the end of the collection's configuration, all in
/// one write: a `git remote add` each takes minutes for a thousand forks.
fn add_to_config(dest: &Path, sections: &str) -> Result<(), String> {
    use std::io::Write;
    if sections.is_empty() {
        return Ok(());
    }
    let file = dest.join(".git").join("config");
    let old = fs::read_to_string(&file).map_err(|err| format!("{}: {err}", file.display()))?;
    let gap = if old.is_empty() || old.ends_with('\n') {
        ""
    } else {
        "\n"
    };
    fs::OpenOptions::new()
        .append(true)
        .open(&file)
        .and_then(|mut config| config.write_all(format!("{gap}{sections}").as_bytes()))
        .map_err(|err| format!("{}: {err}", file.display()))
}

/// What a fetch running beside the others reports.
enum ForkNews {
    Line(String),
    Done(usize, Result<(), String>),
}

/// The address a repository on GitHub is fetched from, over SSH when the
/// checkout's own origin uses SSH.
pub fn fork_url(origin: &str, owner: &str, name: &str) -> String {
    if is_ssh(origin) {
        format!("git@github.com:{owner}/{name}.git")
    } else {
        format!("https://github.com/{owner}/{name}.git")
    }
}

/// An account name that can be the name of a remote as it is.
fn valid_remote_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
}

/// Collects the forks of the checkout `source` in the folder `dest`,
/// which is made when it is not there. Its `.git` is one repository that
/// has the original and every fork as a remote named after its owner:
/// the branches of `bob`'s fork are `bob/<branch>`, its pull requests
/// `refs/pull/bob/<number>/head`. Forks share nearly all their commits,
/// so one repository holds them in the room of one.
///
/// Every fork that has commits the original lacks gets a folder named
/// after its owner, `dest/bob`, with its files: a worktree of that one
/// repository. A fork that changed nothing gets none.
///
/// What `source` has already is taken from disk first. A fork that cannot
/// be fetched is named in the result and the rest goes on. Nothing is
/// pruned: a branch or fork that disappears on GitHub stays here. Running
/// it again brings new forks and new commits.
///
/// The forks are fetched several at a time. One whose stamp is the one
/// it was last fetched with is left alone, so that running it again on
/// hundreds of forks only costs the few that changed.
pub fn save_forks(
    source: &Path,
    dest: &Path,
    original: &(String, String),
    forks: &[ForkSource],
    url_of: &dyn Fn(&str, &str) -> String,
    token: Option<&str>,
    mut on_line: impl FnMut(&str, bool),
) -> Result<ForksSaved, String> {
    let (owner, name) = original;
    let id = format!("{owner}/{name}");
    if !valid_remote_name(owner) {
        return Err(format!("{owner} cannot be the name of a remote"));
    }
    let created = !dest.exists();
    if created {
        let args = [
            OsStr::new("-c"),
            OsStr::new("init.defaultBranch=main"),
            OsStr::new("init"),
            OsStr::new("-q"),
            OsStr::new("--"),
            dest.as_os_str(),
        ];
        run_git(args, &mut on_line)?;
        if let Err(err) = git_lines(dest, &["config", FORKS_KEY, &id]) {
            let _ = remove_tree(dest);
            return Err(err);
        }
    } else {
        let known = dest
            .join(".git")
            .exists()
            .then(|| git_lines(dest, &["config", "--get", FORKS_KEY]).ok())
            .flatten()
            .and_then(|lines| lines.into_iter().next());
        if !known.is_some_and(|known| known.eq_ignore_ascii_case(&id)) {
            return Err(format!(
                "{} is there already and does not hold the forks of {id}",
                dest.display()
            ));
        }
    }
    let mut saved = ForksSaved {
        created,
        ..ForksSaved::default()
    };
    store_only(dest, &mut on_line);

    // The remotes it has, with their addresses.
    let known: std::collections::HashMap<String, String> =
        git_lines(dest, &["config", "--get-regexp", r"^remote\..*\.url$"])
            .unwrap_or_default()
            .iter()
            .filter_map(|line| {
                let (key, url) = line.split_once(' ')?;
                let name = key.strip_prefix("remote.")?.strip_suffix(".url")?;
                Some((name.to_string(), url.trim().to_string()))
            })
            .collect();

    // The original comes first, so that every fork only adds what it has
    // of its own.
    set_fork_remote(dest, &known, owner, &url_of(owner, name), true)?;
    if created {
        on_line(
            &format!("Taking what {} has already.", source.display()),
            false,
        );
        let mut args: Vec<OsString> = ["-C".into(), dest.as_os_str().to_os_string()].into();
        args.extend(
            ["fetch", "--quiet", "--no-tags", "--recurse-submodules=no"].map(OsString::from),
        );
        args.push(source.as_os_str().to_os_string());
        args.extend([
            OsString::from(format!("+refs/remotes/origin/*:refs/remotes/{owner}/*")),
            OsString::from("^refs/remotes/origin/HEAD"),
            OsString::from(format!("+refs/pull/*/head:refs/pull/{owner}/*/head")),
        ]);
        // Only a shortcut: what it misses comes from GitHub next.
        if let Err(err) = run_git(args, &mut on_line) {
            on_line(&format!("Not taken from the local clone: {err}"), false);
        }
    }
    on_line(&format!("Fetching {id}, the original."), false);
    fetch_fork(dest, owner, token, &mut on_line)?;
    // What to do with each fork. Every remote is in the configuration
    // before the first fetch starts: a fetch cannot read it in the
    // moment it is written.
    let total = forks.len();
    let mut stamps = read_stamps(dest);
    let mut plans = Vec::with_capacity(total);
    let mut fresh = String::new();
    let mut added = std::collections::HashSet::new();
    for fork in forks {
        let name = &fork.owner;
        let plan = if !valid_remote_name(name)
            || name.eq_ignore_ascii_case(owner)
            || !added.insert(name.to_ascii_lowercase())
        {
            on_line(&format!("{name} cannot be the name of a remote."), false);
            ForkPlan::Skip
        } else {
            let url = url_of(name, &fork.name);
            let gone = stamps
                .get(name)
                .and_then(|stamp| stamp.strip_prefix(GONE))
                .is_some_and(|stamp| stamp == fork.stamp);
            match known.get(name) {
                _ if gone => ForkPlan::Gone,
                Some(old) if *old == url => {
                    if !fork.stamp.is_empty() && stamps.get(name) == Some(&fork.stamp) {
                        ForkPlan::Unchanged
                    } else {
                        ForkPlan::Fetch
                    }
                }
                Some(_) => match git_lines(dest, &["remote", "set-url", "--", name, &url]) {
                    Ok(_) => ForkPlan::Fetch,
                    Err(err) => {
                        on_line(&err, false);
                        ForkPlan::Skip
                    }
                },
                None => {
                    fresh.push_str(&remote_section(name, &url));
                    ForkPlan::Fetch
                }
            }
        };
        plans.push(plan);
    }
    add_to_config(dest, &fresh)?;

    // The forks, several at a time. This thread hands them out and hears
    // what the fetches say.
    let mut failed: Vec<(usize, String)> = Vec::new();
    let mut cancelled = false;
    let (job_tx, job_rx) = std::sync::mpsc::channel::<(usize, String)>();
    let job_rx = std::sync::Mutex::new(job_rx);
    let (news_tx, news_rx) = std::sync::mpsc::channel::<ForkNews>();
    std::thread::scope(|scope| {
        for _ in 0..FORK_JOBS.min(total) {
            let news = news_tx.clone();
            let jobs = &job_rx;
            scope.spawn(move || {
                loop {
                    let next = jobs.lock().unwrap_or_else(|err| err.into_inner()).recv();
                    let Ok((index, remote)) = next else { break };
                    // Their own progress would fight over the one bar.
                    let result = fetch_fork(dest, &remote, token, |line, progress| {
                        if !progress {
                            let _ = news.send(ForkNews::Line(line.to_string()));
                        }
                    });
                    let _ = news.send(ForkNews::Done(index, result));
                }
            });
        }
        drop(news_tx);
        let (mut next, mut flying, mut done, mut unsaved) = (0, 0, 0, 0);
        let progress = |done: usize, on_line: &mut dyn FnMut(&str, bool)| {
            on_line(
                &format!(
                    "Fetching forks: {:>3}% ({done}/{total})",
                    done * 100 / total
                ),
                true,
            );
        };
        loop {
            while !cancelled && flying < FORK_JOBS && next < total {
                if process::was_cancelled() {
                    cancelled = true;
                    break;
                }
                let index = next;
                next += 1;
                let fork = &forks[index];
                let full = format!("{}/{}", fork.owner, fork.name);
                let handed_out = match plans[index] {
                    ForkPlan::Skip => Err(()),
                    ForkPlan::Unchanged => {
                        saved.unchanged += 1;
                        Ok(false)
                    }
                    ForkPlan::Gone => {
                        saved.gone += 1;
                        Ok(false)
                    }
                    ForkPlan::Fetch => {
                        on_line(&format!("Fetching {full}."), false);
                        job_tx
                            .send((index, fork.owner.clone()))
                            .map(|()| true)
                            .map_err(drop)
                    }
                };
                match handed_out {
                    Ok(true) => {
                        flying += 1;
                        continue;
                    }
                    Ok(false) => {}
                    Err(()) => {
                        on_line(&format!("{full} was not fetched."), false);
                        failed.push((index, full));
                    }
                }
                done += 1;
                progress(done, &mut on_line);
            }
            if flying == 0 {
                if cancelled || next >= total {
                    break;
                }
                continue;
            }
            match news_rx.recv() {
                Ok(ForkNews::Line(line)) => on_line(&line, false),
                Ok(ForkNews::Done(index, result)) => {
                    flying -= 1;
                    done += 1;
                    let fork = &forks[index];
                    match result {
                        Ok(()) => {
                            saved.fetched += 1;
                            if fork.stamp.is_empty() {
                                stamps.remove(&fork.owner);
                            } else {
                                stamps.insert(fork.owner.clone(), fork.stamp.clone());
                            }
                            unsaved += 1;
                            if unsaved >= 100 {
                                write_stamps(dest, &stamps);
                                unsaved = 0;
                            }
                        }
                        Err(_) if process::was_cancelled() => cancelled = true,
                        Err(err) if says_gone(&err) => {
                            // Not an error to report every time: it is
                            // left out from now on, and what it brought
                            // before stays.
                            on_line(
                                &format!(
                                    "{}/{} is no longer on GitHub. What was fetched from it stays.",
                                    fork.owner, fork.name
                                ),
                                false,
                            );
                            saved.gone += 1;
                            stamps.insert(fork.owner.clone(), format!("{GONE}{}", fork.stamp));
                            unsaved += 1;
                        }
                        Err(_) => {
                            let full = format!("{}/{}", fork.owner, fork.name);
                            on_line(&format!("{full} was not fetched."), false);
                            failed.push((index, full));
                        }
                    }
                    progress(done, &mut on_line);
                }
                Err(_) => break,
            }
        }
        // Ends the fetching threads: nothing more is handed out.
        drop(job_tx);
    });
    write_stamps(dest, &stamps);
    if cancelled {
        return Err("cancelled".into());
    }
    failed.sort();
    saved.failed = failed.into_iter().map(|(_, fork)| fork).collect();
    if saved.unchanged > 0 {
        on_line(
            &format!(
                "{} forks are as they were when they were last fetched.",
                saved.unchanged
            ),
            false,
        );
    }
    if saved.gone > 0 {
        on_line(
            &format!(
                "{} forks are no longer on GitHub and were left as they are.",
                saved.gone
            ),
            false,
        );
    }
    // The fetches left tidying up to the end, where nothing else runs.
    let tidy = [
        OsStr::new("-C"),
        dest.as_os_str(),
        OsStr::new("-c"),
        OsStr::new("gc.autoDetach=false"),
        OsStr::new("gc"),
        OsStr::new("--auto"),
        OsStr::new("--quiet"),
    ];
    let _ = run_git(tidy, |_, _| {});

    let count = |prefix: &str| {
        git_lines(dest, &["for-each-ref", "--format=%(refname)", prefix]).map_or(0, |lines| {
            let refs = lines.iter().filter(|name| !name.ends_with("/HEAD")).count();
            u32::try_from(refs).unwrap_or(u32::MAX)
        })
    };
    saved.branches = count("refs/remotes");
    saved.pulls = count("refs/pull");
    saved.folders = fork_folders(dest, owner, &mut on_line)?;
    Ok(saved)
}

/// The tree without any file, which every Git repository knows.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Makes the top of the fork collection hold no checkout of its own, so
/// that it shows the folders of the forks and nothing else. A collection
/// made by an earlier version had the files of the original there: they
/// are taken away, unless one of them was changed.
fn store_only(dest: &Path, on_line: &mut dyn FnMut(&str, bool)) {
    let first = |args: &[&str]| {
        git_lines(dest, args)
            .ok()
            .and_then(|lines| lines.into_iter().next())
    };
    if first(&["config", "--get", "core.bare"]).as_deref() == Some("true") {
        return;
    }
    // A file that was only touched, as by a copy of the folder, is not
    // a changed one: look at the contents first.
    let _ = git_lines(dest, &["update-index", "-q", "--refresh"]);
    if first(&["rev-parse", "-q", "--verify", "HEAD"]).is_some()
        && let Err(err) = git_lines(dest, &["read-tree", "-m", "-u", "HEAD", EMPTY_TREE])
    {
        on_line(
            &format!("The files of the original stay in the folder: {err}"),
            false,
        );
        return;
    }
    let _ = git_lines(dest, &["config", "core.bare", "true"]);
}

/// One branch of a fork: its full name, its commit and when that was made.
struct ForkBranch {
    name: String,
    commit: String,
    date: i64,
}

/// The branch whose files a fork's folder shows, or None for a fork
/// that has no commit the original lacks. It is the fork's default
/// branch when that one has such commits, else the branch with the
/// newest commit among those that do.
fn branch_to_show<'a>(
    branches: &'a [ForkBranch],
    default: Option<&str>,
    original: &std::collections::HashSet<String>,
) -> Option<&'a ForkBranch> {
    let mut changed = branches
        .iter()
        .filter(|branch| !original.contains(&branch.commit));
    let newest = changed.clone().max_by_key(|branch| branch.date);
    changed
        .find(|branch| Some(branch.name.as_str()) == default)
        .or(newest)
}

/// Gives every fork with commits of its own a folder in the collection,
/// named after its owner, and brings the folders that are there to what
/// was fetched. Hands back how many forks have one. A folder someone
/// changed files in keeps them.
fn fork_folders(
    dest: &Path,
    original: &str,
    on_line: &mut dyn FnMut(&str, bool),
) -> Result<u32, String> {
    use std::collections::{BTreeMap, HashMap, HashSet};
    let _ = git_lines(dest, &["worktree", "prune"]);
    // Every commit of the original: a branch that ends on one of them
    // has nothing of its own.
    let pattern = format!("--remotes={original}");
    let own: HashSet<String> = git_lines(dest, &["rev-list", &pattern])?
        .into_iter()
        .collect();
    let format = "--format=%(objectname) %(committerdate:unix) %(refname) %(symref)";
    let mut forks: BTreeMap<String, (Vec<ForkBranch>, Option<String>)> = BTreeMap::new();
    for line in git_lines(dest, &["for-each-ref", format, "refs/remotes"])? {
        let mut parts = line.split_whitespace();
        let (Some(commit), Some(date), Some(name)) = (parts.next(), parts.next(), parts.next())
        else {
            continue;
        };
        let Some((owner, branch)) = name
            .strip_prefix("refs/remotes/")
            .and_then(|rest| rest.split_once('/'))
        else {
            continue;
        };
        if owner.eq_ignore_ascii_case(original) {
            continue;
        }
        let fork = forks.entry(owner.to_string()).or_default();
        if branch == "HEAD" {
            fork.1 = parts.next().map(str::to_string);
        } else {
            fork.0.push(ForkBranch {
                name: name.to_string(),
                commit: commit.to_string(),
                date: date.parse().unwrap_or(0),
            });
        }
    }
    // The folders that are there, with the commit each shows.
    let mut shown: HashMap<String, String> = HashMap::new();
    let mut folder = None;
    for line in git_lines(dest, &["worktree", "list", "--porcelain"]).unwrap_or_default() {
        if let Some(path) = line.strip_prefix("worktree ") {
            folder = Path::new(path)
                .file_name()
                .and_then(OsStr::to_str)
                .map(str::to_lowercase);
        } else if let (Some(commit), Some(folder)) = (line.strip_prefix("HEAD "), folder.take()) {
            shown.insert(folder, commit.to_string());
        }
    }

    let changed: Vec<(&String, &ForkBranch)> = forks
        .iter()
        .filter_map(|(owner, (branches, default))| {
            Some((owner, branch_to_show(branches, default.as_deref(), &own)?))
        })
        .collect();
    let total = changed.len();
    let mut folders = 0;
    for (index, (owner, branch)) in changed.into_iter().enumerate() {
        if process::was_cancelled() {
            return Err("cancelled".into());
        }
        let path = dest.join(owner);
        let made = if shown.get(&owner.to_lowercase()) == Some(&branch.commit) {
            Ok(())
        } else if path.join(".git").exists() {
            // A folder from before: its files follow the fork.
            git_lines(&path, &["checkout", "-q", "--detach", &branch.name]).map(drop)
        } else if path.exists() {
            Err("a folder of that name is there already".to_string())
        } else {
            let mut cmd = git_command();
            cmd.arg("-C")
                .arg(dest)
                .args(["worktree", "add", "-q", "--detach"])
                .arg(&path)
                .arg(&branch.name);
            match cmd.output() {
                Ok(output) if output.status.success() => Ok(()),
                Ok(output) => Err(last_line(&String::from_utf8_lossy(&output.stderr))),
                Err(err) => Err(format!("could not start git ({err})")),
            }
        };
        match made {
            Ok(()) => folders += 1,
            Err(err) => on_line(&format!("No folder for {owner}: {err}"), false),
        }
        let done = index + 1;
        on_line(
            &format!(
                "Making folders: {:>3}% ({done}/{total})",
                done * 100 / total
            ),
            true,
        );
    }
    Ok(folders)
}

/// "1:05" for 65 seconds, "1:01:05" from an hour on.
fn clock(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// Waits `seconds` and says so: one line that it pauses and why, a
/// countdown that replaces itself every second, and a line when it goes
/// on. Cancelling ends the wait at once, with an error.
pub fn pause_for(
    seconds: u64,
    why: &str,
    on_line: &mut dyn FnMut(&str, bool),
) -> Result<(), String> {
    if seconds == 0 {
        return Ok(());
    }
    on_line(&format!("{why}. Paused for {}.", clock(seconds)), false);
    for left in (1..=seconds).rev() {
        on_line(&format!("Paused, going on in {}", clock(left)), true);
        for _ in 0..4 {
            if process::was_cancelled() {
                return Err("cancelled".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    on_line("Going on.", false);
    Ok(())
}

/// Whether a failed fetch is worth trying again after a pause: the
/// connection timed out or broke, or the server asked for patience. A
/// repository that is gone or refuses the sign-in is not.
fn worth_retrying(err: &str) -> bool {
    let err = err.to_ascii_lowercase();
    [
        "timed out",
        "timeout",
        "failed to connect",
        "could not resolve host",
        "connection reset",
        "connection was reset",
        "connection refused",
        "rpc failed",
        "early eof",
        "unexpected disconnect",
        "error: 429",
        "error: 500",
        "error: 502",
        "error: 503",
        "error: 504",
    ]
    .iter()
    .any(|sign| err.contains(sign))
}

/// The pauses before a fetch that timed out is tried again.
const FETCH_PAUSES: [u64; 3] = [10, 30, 90];

/// Makes `name` a remote of the fork collection, or gives it the address
/// it has now. A new remote also fetches its pull requests, kept under
/// its own name, and only the original brings tags: every fork has the
/// same ones.
fn set_fork_remote(
    dest: &Path,
    known: &std::collections::HashMap<String, String>,
    name: &str,
    url: &str,
    tags: bool,
) -> Result<(), String> {
    match known.get(name) {
        Some(old) if old == url => Ok(()),
        Some(_) => git_lines(dest, &["remote", "set-url", "--", name, url]).map(drop),
        None => {
            let mut add = vec!["remote", "add"];
            if !tags {
                add.push("--no-tags");
            }
            add.extend(["--", name, url]);
            git_lines(dest, &add)?;
            let key = format!("remote.{name}.fetch");
            let pulls = format!("+refs/pull/*/head:refs/pull/{name}/*/head");
            git_lines(dest, &["config", "--add", &key, &pulls]).map(drop)
        }
    }
}

/// Fetches the branches and pull requests of one remote of the fork
/// collection. A fetch that times out pauses and is tried again, three
/// times at most; what it had brought by then is kept.
fn fetch_fork(
    dest: &Path,
    name: &str,
    token: Option<&str>,
    mut on_line: impl FnMut(&str, bool),
) -> Result<(), String> {
    let mut tries = 0;
    loop {
        let mut args = auth_args(token);
        args.extend([OsString::from("-C"), dest.as_os_str().to_os_string()]);
        // Several of these run at once: none writes FETCH_HEAD or
        // starts tidying up while the others fetch.
        args.extend(
            [
                "fetch",
                "--no-prune",
                "--recurse-submodules=no",
                "--no-write-fetch-head",
                "--no-auto-maintenance",
                "--",
                name,
            ]
            .map(OsString::from),
        );
        match run_git(args, &mut on_line) {
            Err(err)
                if tries < FETCH_PAUSES.len()
                    && worth_retrying(&err)
                    && !process::was_cancelled() =>
            {
                tries += 1;
                pause_for(FETCH_PAUSES[tries - 1], "The fetch timed out", &mut on_line)?;
            }
            done => return done,
        }
    }
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
///
/// With `how.pull_requests`, the commits of every pull request are saved
/// as well, after the branches are fetched.
pub fn update(
    path: &Path,
    how: Fetch,
    mut on_line: impl FnMut(&str, bool),
    on_steps: impl FnMut(u32, u32),
) -> Result<Updated, String> {
    let mut args = auth_args(how.token);
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
    if how.pull_requests {
        fetch_pull_requests(path, how.token, true, &mut on_line)?;
        if process::was_cancelled() {
            return Err("cancelled".into());
        }
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
/// Git reads that from the upstream's reflog. A remote that deletes the
/// branch and makes it again takes that reflog with it, so the branch's own
/// reflog is asked as well: whether the upstream is what last put it where
/// it is. With neither left, the branch counts as having commits of its own.
fn upstream_was_rewritten(path: &Path, upstream: &str, local: &str) -> bool {
    let read = |args: &[&str]| {
        let output = git_command().arg("-C").arg(path).args(args).output().ok()?;
        let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
        (output.status.success() && !text.is_empty()).then_some(text)
    };
    let tip = read(&["rev-parse", "-q", "--verify", local]);
    if tip.is_none() {
        return false;
    }
    if tip == read(&["merge-base", "--fork-point", upstream, local]) {
        return true;
    }
    read(&["log", "-g", "-1", "--format=%gs", local])
        .is_some_and(|entry| set_from_upstream(&entry, upstream, local))
}

/// True when a reflog entry of a local branch says the branch was put there
/// from its upstream: created from it, or moved to it by an update. A
/// commit, reset, rebase or merge on the branch reads differently.
fn set_from_upstream(entry: &str, upstream: &str, local: &str) -> bool {
    if let Some(source) = entry.strip_prefix("branch: Created from ") {
        return source == upstream || source == short_upstream(upstream);
    }
    entry.starts_with("fetch ")
        && [": fast-forward", ": forced-update"].iter().any(|how| {
            entry
                .strip_suffix(how)
                .is_some_and(|rest| rest.ends_with(&format!("{upstream}:{local}")))
        })
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
        pull_requests: 0,
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
    /// Saved pull requests.
    pull_requests: u32,
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
    /// Saved pull requests: the refs under refs/pull.
    pull_requests: u32,
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
        .arg("refs/pull")
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
    let list = list_refs(path)?;
    let parsed = summarize(&list);
    let mut branches = Branches {
        pull_requests: list.pull_requests,
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
    let mut pull_requests = 0;
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
        if name.starts_with("refs/pull/") {
            pull_requests += 1;
        } else if name.starts_with("refs/remotes/") {
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
        pull_requests,
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
        crate::sys::encode::base64(&raw)
    ))
}

fn git_command() -> Command {
    #[cfg(feature = "testing")]
    crate::sys::note(crate::sys::Reach::Program);
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
            pull_requests: 0,
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
    fn addresses_to_push_to_are_checked_and_named() {
        for good in [
            "https://gitlab.com/you/api.git",
            "  git@codeberg.org:you/api.git ",
            "ssh://git@git.example.com:2222/you/api",
        ] {
            assert_eq!(push_address(good).unwrap(), good.trim());
        }
        // Nothing Git would take as an option or as a command to run.
        for bad in [
            "",
            "--upload-pack=calc",
            "ext::sh -c calc",
            "ext::calc",
            "https://",
            "gitlab.com/you/api",
            "https://gitlab.com/you/a pi.git",
        ] {
            assert!(push_address(bad).is_err(), "{bad}");
        }
        assert_eq!(remote_name_for("https://gitlab.com/you/api.git"), "gitlab");
        assert_eq!(remote_name_for("git@codeberg.org:you/api.git"), "codeberg");
        assert_eq!(
            remote_name_for("https://me:secret@www.bitbucket.org/a/b"),
            "bitbucket"
        );
        assert_eq!(remote_name_for("https://origin.example.com/a/b"), "mirror");
        assert_eq!(remote_name_for("file:///srv/git/api.git"), "mirror");
        assert_eq!(
            address_host("ssh://git@git.example.com:2222/you/api").as_deref(),
            Some("git.example.com")
        );
    }

    #[test]
    fn migrating_pushes_every_branch_and_tag_and_keeps_origin() {
        let root = std::env::temp_dir().join(format!("devassist-migrate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        let target = root.join("target.git");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        git(&upstream, &["tag", "v1"]);
        git(&upstream, &["checkout", "-q", "-b", "dev"]);
        commit(&upstream, "dev-work");
        git(&upstream, &["checkout", "-q", "-b", "old"]);
        git(&upstream, &["checkout", "-q", "main"]);
        git(&root, &["clone", "-q", "upstream", "clone"]);
        git(&root, &["init", "-q", "--bare", "target.git"]);
        // main has a commit of its own; old is behind what origin has;
        // dev exists on origin only.
        commit(&clone, "local");
        git(&clone, &["branch", "-q", "old", "origin/old~1"]);
        let tip = |dir: &Path, name: &str| {
            let output = git_command()
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "--verify", "-q", name])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let address = target.display().to_string();

        let done = migrate_to(&clone, &address, |_, _| {}).unwrap();
        assert_eq!(
            done,
            Migrated {
                remote: "mirror".into(),
                branches: 3,
                tags: 1
            }
        );
        assert_eq!(
            tip(&target, "refs/heads/main"),
            tip(&clone, "refs/heads/main")
        );
        assert_eq!(
            tip(&target, "refs/heads/dev"),
            tip(&clone, "refs/remotes/origin/dev")
        );
        assert_eq!(
            tip(&target, "refs/heads/old"),
            tip(&clone, "refs/remotes/origin/old")
        );
        assert_eq!(tip(&target, "refs/tags/v1"), tip(&clone, "refs/tags/v1"));
        assert_eq!(tip(&target, "refs/heads/HEAD"), "");
        // The checkout still follows where it came from, and knows the copy.
        let config = fs::read_to_string(clone.join(".git").join("config")).unwrap();
        assert!(
            origin_url(&config).unwrap().contains("upstream"),
            "{config}"
        );
        assert!(config.contains("[remote \"mirror\"]"), "{config}");

        // Again, after more work: the copy follows, the remote stays one.
        commit(&clone, "later");
        migrate_to(&clone, &address, |_, _| {}).unwrap();
        assert_eq!(
            tip(&target, "refs/heads/main"),
            tip(&clone, "refs/heads/main")
        );
        let config = fs::read_to_string(clone.join(".git").join("config")).unwrap();
        assert_eq!(config.matches("[remote \"mirror\"]").count(), 1);

        // Its own origin is not somewhere else.
        let origin = upstream.display().to_string();
        assert!(migrate_to(&clone, &origin, |_, _| {}).is_err());
        let _ = remove_tree(&root);
    }

    #[test]
    fn a_checkout_moves_to_a_free_path_and_stays_a_repository() {
        let root = std::env::temp_dir().join(format!("devassist-move-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let old = root.join("here").join("api");
        fs::create_dir_all(root.join("here")).unwrap();
        git(&root.join("here"), &["init", "-q", "-b", "main", "api"]);
        commit(&old, "one");
        fs::create_dir_all(root.join("taken").join("api")).unwrap();

        // A path that is in use, or inside the checkout, is refused.
        let taken = move_checkout(&old, &root.join("taken").join("api"), |_, _| {}).unwrap_err();
        assert!(taken.contains("there already"), "{taken}");
        let inside = move_checkout(&old, &old.join("deeper").join("api"), |_, _| {}).unwrap_err();
        assert!(inside.contains("into itself"), "{inside}");
        assert!(old.join(".git").exists());

        // The folder it goes into is made when it is not there yet.
        let new = root.join("there").join("deeper").join("api");
        assert_eq!(move_checkout(&old, &new, |_, _| {}), Ok(None));
        assert!(!old.exists());
        assert_eq!(status(&new).unwrap().branch, "main");

        // Copying, as onto another drive, brings everything along.
        let copy = root.join("copy");
        let mut files = 0;
        copy_tree(&new, &copy, &mut |_| files += 1).unwrap();
        assert!(files > 0);
        assert_eq!(files, count_files(&new));
        assert_eq!(count_files(&copy), files);
        assert_eq!(status(&copy).unwrap().branch, "main");
        assert!(move_checkout(&old, &copy, |_, _| {}).is_err());

        // The slow way reports how far it is, and leaves no original.
        let far = root.join("far").join("api");
        fs::create_dir_all(root.join("far")).unwrap();
        let mut lines: Vec<(String, bool)> = Vec::new();
        let warning = copy_then_delete(&new, &far, &mut |line, progress| {
            lines.push((line.to_string(), progress))
        });
        assert_eq!(warning, Ok(None));
        assert!(!new.exists());
        assert_eq!(status(&far).unwrap().branch, "main");
        let last = format!("Copying files: 100% ({files}/{files})");
        assert!(lines.contains(&(last, true)), "{lines:?}");
        assert!(lines.contains(&(format!("Copied {files} files."), false)));
        // Never more lines than percents, however many files there are.
        assert!(lines.len() <= 110, "{}", lines.len());
        let _ = remove_tree(&root);
    }

    #[test]
    fn a_forks_folder_shows_the_branch_that_has_its_work() {
        let branch = |name: &str, commit: &str, date: i64| ForkBranch {
            name: format!("refs/remotes/bob/{name}"),
            commit: commit.to_string(),
            date,
        };
        let original: std::collections::HashSet<String> =
            ["a", "b"].iter().map(|id| (*id).to_string()).collect();
        let show = |branches: &[ForkBranch], default: Option<&str>| {
            branch_to_show(branches, default, &original).map(|branch| branch.name.clone())
        };
        let main = Some("refs/remotes/bob/main");
        // Nothing of its own, even on an older commit of the original.
        assert_eq!(
            show(&[branch("main", "b", 5), branch("old", "a", 1)], main),
            None
        );
        // Its default branch when that has work of its own.
        let both = [branch("main", "x", 5), branch("fix", "y", 9)];
        assert_eq!(show(&both, main).as_deref(), main);
        // Else the newest branch that has.
        let side = [
            branch("main", "b", 5),
            branch("fix", "y", 7),
            branch("try", "z", 9),
        ];
        assert_eq!(show(&side, main).as_deref(), Some("refs/remotes/bob/try"));
        assert_eq!(show(&side, None).as_deref(), Some("refs/remotes/bob/try"));
    }

    #[test]
    fn a_pause_counts_down_and_only_a_timeout_is_tried_again() {
        assert_eq!(clock(65), "1:05");
        assert_eq!(clock(9), "0:09");
        assert_eq!(clock(3665), "1:01:05");
        let mut lines = Vec::new();
        let mut on_line = |line: &str, progress: bool| lines.push((line.to_string(), progress));
        assert_eq!(pause_for(0, "Nothing", &mut on_line), Ok(()));
        assert_eq!(
            pause_for(1, "GitHub's limit is reached", &mut on_line),
            Ok(())
        );
        let said: Vec<(&str, bool)> = lines.iter().map(|(line, p)| (line.as_str(), *p)).collect();
        assert_eq!(
            said,
            [
                ("GitHub's limit is reached. Paused for 0:01.", false),
                ("Paused, going on in 0:01", true),
                ("Going on.", false),
            ]
        );
        for again in [
            "fatal: unable to access 'https://github.com/a/b.git/': Failed to connect to github.com port 443 after 21050 ms: Timed out",
            "fatal: unable to access 'https://github.com/a/b.git/': The requested URL returned error: 429",
            "error: RPC failed; curl 56 Recv failure: Connection was reset",
            "ssh: connect to host github.com port 22: Connection timed out",
        ] {
            assert!(worth_retrying(again), "{again}");
        }
        for stays in [
            "fatal: repository 'https://github.com/a/b.git/' not found",
            "fatal: unable to access 'https://github.com/a/b.git/': The requested URL returned error: 403",
            "fatal: Authentication failed for 'https://github.com/a/b.git/'",
        ] {
            assert!(!worth_retrying(stays), "{stays}");
        }
    }

    #[test]
    fn the_forks_of_a_checkout_are_collected_in_one_repository_beside_it() {
        let root = std::env::temp_dir().join(format!("devassist-forks-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tip = |dir: &Path, name: &str| {
            git_lines(dir, &["rev-parse", "-q", "--verify", name])
                .ok()
                .and_then(|lines| lines.into_iter().next())
        };
        // The original on "GitHub", with a tag and a pull request.
        let ada = root.join("ada");
        git(&root, &["init", "-q", "-b", "main", "ada"]);
        commit(&ada, "one");
        git(&ada, &["tag", "v1"]);
        git(&ada, &["update-ref", "refs/pull/7/head", "HEAD"]);
        // Two forks: one with a branch and a pull request of its own.
        git(&root, &["clone", "-q", "ada", "bob"]);
        let bob = root.join("bob");
        git(&bob, &["checkout", "-q", "-b", "feature"]);
        commit(&bob, "bobs");
        git(&bob, &["update-ref", "refs/pull/1/head", "HEAD"]);
        git(&root, &["clone", "-q", "ada", "eve"]);
        // The checkout the forks are collected for.
        git(&root, &["clone", "-q", "ada", "api"]);
        let api = root.join("api");
        let dest = forks_folder(&api).unwrap();
        assert_eq!(dest, root.join("api-fork"));

        let original = ("ada".to_string(), "api".to_string());
        let fork = |owner: &str| ForkSource {
            owner: owner.to_string(),
            name: "api".to_string(),
            stamp: String::new(),
        };
        let forks = [fork("bob"), fork("eve"), fork("gone"), fork("bad name")];
        let url_of = |owner: &str, _: &str| root.join(owner).display().to_string();
        let mut lines = Vec::new();
        let saved = save_forks(&api, &dest, &original, &forks, &url_of, None, |line, _| {
            lines.push(line.to_string())
        })
        .unwrap();
        assert!(saved.created);
        assert_eq!(saved.fetched, 2, "{lines:?}");
        assert_eq!(saved.failed, ["gone/api", "bad name/api"]);
        // main of all three and bob's branch; both pull requests.
        assert_eq!(saved.branches, 4, "{lines:?}");
        assert_eq!(saved.pulls, 2);
        assert_eq!(tip(&dest, "refs/remotes/ada/main"), tip(&ada, "main"));
        assert_eq!(tip(&dest, "refs/remotes/bob/feature"), tip(&bob, "feature"));
        assert_eq!(tip(&dest, "refs/remotes/eve/main"), tip(&ada, "main"));
        assert_eq!(tip(&dest, "refs/pull/ada/7/head"), tip(&ada, "main"));
        assert_eq!(tip(&dest, "refs/pull/bob/1/head"), tip(&bob, "feature"));
        assert_eq!(tip(&dest, "refs/tags/v1"), tip(&ada, "v1"));
        assert!(lines.contains(&"Fetching forks: 100% (4/4)".to_string()));
        // The fork that changed something has a folder with its files.
        // The one that did not has none, and neither has the original.
        assert_eq!(saved.folders, 1, "{lines:?}");
        assert!(dest.join("bob").join("bobs").exists());
        assert_eq!(tip(&dest.join("bob"), "HEAD"), tip(&bob, "feature"));
        assert!(!dest.join("eve").exists());
        assert!(!dest.join("ada").exists());
        assert!(!dest.join("one").exists());

        // Again: what is new comes in, nothing is made twice.
        commit(&bob, "more");
        let again = save_forks(
            &api,
            &dest,
            &original,
            &forks[..2],
            &url_of,
            None,
            |_, _| {},
        );
        let again = again.unwrap();
        assert!(!again.created);
        assert_eq!((again.fetched, again.branches), (2, 4));
        assert_eq!(tip(&dest, "refs/remotes/bob/feature"), tip(&bob, "feature"));
        // The folder follows its fork.
        assert_eq!(again.folders, 1);
        assert!(dest.join("bob").join("more").exists());

        // A collection of the first kind had the files of the original at
        // the top: they go, the forks stay.
        git(&dest, &["config", "core.bare", "false"]);
        git(&dest, &["checkout", "-q", "-f", "-B", "main", "ada/main"]);
        assert!(dest.join("one").exists());
        let moved = save_forks(
            &api,
            &dest,
            &original,
            &forks[..2],
            &url_of,
            None,
            |_, _| {},
        );
        assert_eq!(moved.unwrap().folders, 1);
        assert!(!dest.join("one").exists());
        assert!(dest.join("bob").join("more").exists());
        assert!(dest.join(".git").is_dir());
        let config = fs::read_to_string(dest.join(".git").join("config")).unwrap();
        assert_eq!(config.matches("[remote \"bob\"]").count(), 1, "{config}");

        // A folder that is something else is left alone.
        let other = ("ada".to_string(), "other".to_string());
        let refused = save_forks(&api, &dest, &other, &forks[..1], &url_of, None, |_, _| {});
        assert!(refused.unwrap_err().contains("there already"));
        assert!(save_forks(&api, &bob, &original, &forks[..1], &url_of, None, |_, _| {}).is_err());
        assert!(fork_url("git@github.com:ada/api.git", "bob", "api").starts_with("git@"));
        assert!(fork_url("https://github.com/ada/api", "bob", "api").starts_with("https://"));
        let _ = remove_tree(&root);
    }

    #[test]
    fn a_fork_that_has_not_changed_is_not_fetched_again() {
        let root = std::env::temp_dir().join(format!("devassist-stamps-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let tip = |dir: &Path, name: &str| {
            git_lines(dir, &["rev-parse", "-q", "--verify", name])
                .ok()
                .and_then(|lines| lines.into_iter().next())
        };
        git(&root, &["init", "-q", "-b", "main", "ada"]);
        commit(&root.join("ada"), "one");
        // More forks than are fetched at once.
        let owners: Vec<String> = (0..FORK_JOBS + 3).map(|n| format!("fork{n}")).collect();
        for owner in &owners {
            git(&root, &["clone", "-q", "ada", owner]);
        }
        let bob = root.join(&owners[0]);
        commit(&bob, "bobs");
        git(&root, &["clone", "-q", "ada", "api"]);
        let api = root.join("api");
        let dest = forks_folder(&api).unwrap();
        assert!(!forks_cloned(&api));

        let original = ("ada".to_string(), "api".to_string());
        let stamped = |stamp: &str| -> Vec<ForkSource> {
            owners
                .iter()
                .map(|owner| ForkSource {
                    owner: owner.clone(),
                    name: "api".to_string(),
                    stamp: stamp.to_string(),
                })
                .chain([ForkSource {
                    owner: "gone".to_string(),
                    name: "api".to_string(),
                    stamp: stamp.to_string(),
                }])
                .collect()
        };
        let url_of = |owner: &str, _: &str| root.join(owner).display().to_string();
        let count = u32::try_from(owners.len()).unwrap();
        let mut lines = Vec::new();
        let first = save_forks(
            &api,
            &dest,
            &original,
            &stamped("monday 0"),
            &url_of,
            None,
            |line, _| lines.push(line.to_string()),
        )
        .unwrap();
        assert_eq!((first.fetched, first.unchanged), (count, 0), "{lines:?}");
        assert_eq!(first.failed, ["gone/api"]);
        assert_eq!(first.folders, 1);
        assert!(forks_cloned(&api));
        let total = owners.len() + 1;
        assert!(lines.contains(&format!("Fetching forks: 100% ({total}/{total})")));

        // The same stamps: nothing is fetched, though there is something
        // new. Only the fork that never came is tried again.
        commit(&bob, "more");
        let old = tip(&dest, &format!("refs/remotes/{}/main", owners[0]));
        let same = save_forks(
            &api,
            &dest,
            &original,
            &stamped("monday 0"),
            &url_of,
            None,
            |_, _| {},
        )
        .unwrap();
        assert_eq!((same.fetched, same.unchanged), (0, count));
        assert_eq!(same.failed, ["gone/api"]);
        assert_eq!(tip(&dest, &format!("refs/remotes/{}/main", owners[0])), old);
        assert_eq!((same.folders, same.branches), (1, first.branches));

        // One fork has another stamp: that one is fetched.
        let mut forks = stamped("monday 0");
        forks[0].stamp = "tuesday 0".to_string();
        let moved = save_forks(&api, &dest, &original, &forks, &url_of, None, |_, _| {}).unwrap();
        assert_eq!((moved.fetched, moved.unchanged), (1, count - 1));
        assert_eq!(
            tip(&dest, &format!("refs/remotes/{}/main", owners[0])),
            tip(&bob, "main")
        );
        assert!(dest.join(&owners[0]).join("more").exists());

        // GitHub no longer has one of them: it is left out, and what it
        // brought stays. With another stamp it is back and fetched.
        let mut stamps = read_stamps(&dest);
        stamps.insert(owners[1].clone(), format!("{GONE}monday 0"));
        write_stamps(&dest, &stamps);
        let left = save_forks(&api, &dest, &original, &forks, &url_of, None, |_, _| {}).unwrap();
        assert_eq!((left.fetched, left.unchanged, left.gone), (0, count - 1, 1));
        assert!(tip(&dest, &format!("refs/remotes/{}/main", owners[1])).is_some());
        forks[1].stamp = "friday 0".to_string();
        let back = save_forks(&api, &dest, &original, &forks, &url_of, None, |_, _| {}).unwrap();
        assert_eq!((back.fetched, back.unchanged, back.gone), (1, count - 1, 0));
        forks[1].stamp = "monday 0".to_string();
        write_stamps(&dest, &stamps);

        // What the fork overview reads from the folder.
        let list = list_forks(&api).unwrap();
        assert_eq!(list.original, "ada/api");
        assert_eq!(list.dest, dest);
        assert_eq!(list.forks.len(), owners.len() + 1);
        let of = |owner: &str| list.forks.iter().find(|fork| fork.owner == owner).unwrap();
        let bobs = of(&owners[0]);
        assert_eq!((bobs.branches, bobs.folder, bobs.gone), (1, true, false));
        assert_eq!(bobs.ahead, Some(2), "{bobs:?}");
        assert!(bobs.date > 0);
        let quiet = of(&owners[2]);
        assert_eq!((quiet.branches, quiet.folder), (1, false));
        assert_eq!(quiet.ahead, Some(0));
        assert!(of(&owners[1]).gone);
        let never = of("gone");
        assert_eq!((never.branches, never.date, never.folder), (0, 0, false));
        assert!(list_forks(&root.join("ada")).is_err());
        let _ = remove_tree(&root);
    }

    #[test]
    fn a_repository_that_is_not_found_counts_as_gone() {
        assert!(says_gone(
            "fatal: repository 'https://github.com/bob/api.git/' not found"
        ));
        assert!(says_gone("ERROR: Repository not found."));
        assert!(!says_gone(
            "fatal: unable to access: Could not resolve host"
        ));
        assert!(!says_gone(
            "fatal: 'x' does not appear to be a git repository"
        ));
    }

    #[test]
    fn pull_requests_are_saved_apart_from_the_branches_and_never_pruned() {
        let root = std::env::temp_dir().join(format!("devassist-pulls-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        git(&root, &["init", "-q", "-b", "main", "upstream"]);
        commit(&upstream, "one");
        git(&root, &["clone", "-q", "upstream", "clone"]);

        // Two pull requests the way GitHub publishes them: a ref under
        // refs/pull whose commits are on no branch.
        for number in ["1", "2"] {
            git(&upstream, &["checkout", "-q", "-b", "work", "main"]);
            commit(&upstream, &format!("pull-{number}"));
            let head = format!("refs/pull/{number}/head");
            git(&upstream, &["update-ref", &head, "HEAD"]);
            git(&upstream, &["checkout", "-q", "main"]);
            git(&upstream, &["branch", "-q", "-D", "work"]);
        }
        let tip = |dir: &Path, name: &str| {
            let output = git_command()
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "--verify", "-q", name])
                .output()
                .unwrap();
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
        };
        let saving = Fetch {
            token: None,
            pull_requests: true,
        };

        // Without the setting an update leaves them on the remote.
        update(&clone, Fetch::default(), |_, _| {}, |_, _| {}).unwrap();
        assert_eq!(tip(&clone, "refs/pull/1/head"), None);
        assert_eq!(status(&clone).unwrap().pull_requests, 0);

        // With it, their commits are in the clone, and nothing else moves:
        // no branch is made for them and nothing counts as new.
        let done = update(&clone, saving, |_, _| {}, |_, _| {}).unwrap();
        assert!(!done.changed(), "{done:?}");
        for number in ["1", "2"] {
            let head = format!("refs/pull/{number}/head");
            assert!(tip(&clone, &head).is_some());
            assert_eq!(tip(&clone, &head), tip(&upstream, &head));
        }
        let info = status(&clone).unwrap();
        assert_eq!(info.pull_requests, 2);
        assert_eq!(info.remote_new, 0);
        assert!(info.all_branches_current);
        assert_eq!(tip(&clone, "refs/heads/work"), None);

        // A pull request that gets another commit follows; one the remote
        // drops stays here. A plain fetch does the same as an update.
        git(&upstream, &["checkout", "-q", "refs/pull/1/head"]);
        commit(&upstream, "more");
        git(&upstream, &["update-ref", "refs/pull/1/head", "HEAD"]);
        git(&upstream, &["checkout", "-q", "main"]);
        let kept = tip(&clone, "refs/pull/2/head");
        git(&upstream, &["update-ref", "-d", "refs/pull/2/head"]);
        fetch(&clone, saving).unwrap();
        assert_eq!(
            tip(&clone, "refs/pull/1/head"),
            tip(&upstream, "refs/pull/1/head")
        );
        assert_eq!(tip(&clone, "refs/pull/2/head"), kept);
        assert_eq!(status(&clone).unwrap().pull_requests, 2);

        // Fetching them on request needs no setting, and says what came:
        // here a new pull request and one with another commit.
        assert_eq!(
            pending_pull_requests(&clone, None).unwrap(),
            PendingPulls::default()
        );
        git(&upstream, &["checkout", "-q", "refs/pull/1/head"]);
        commit(&upstream, "even-more");
        git(&upstream, &["update-ref", "refs/pull/1/head", "HEAD"]);
        git(&upstream, &["update-ref", "refs/pull/3/head", "HEAD"]);
        git(&upstream, &["checkout", "-q", "main"]);
        // Before that, a look at the remote says the same without saving
        // anything. The pull request that is gone there is not counted.
        let waiting = PendingPulls { new: 1, changed: 1 };
        assert!(waiting.any());
        assert_eq!(pending_pull_requests(&clone, None).unwrap(), waiting);
        assert_eq!(tip(&clone, "refs/pull/3/head"), None);
        // A check that fetches reports it; one that saves them has none left.
        assert_eq!(check(&clone, true, Fetch::default()).pulls, Some(waiting));
        assert_eq!(check(&clone, false, Fetch::default()).pulls, None);
        let saved = save_pull_requests(&clone, None, |_, _| {}).unwrap();
        assert_eq!(
            saved,
            SavedPulls {
                total: 3,
                changed: 2
            }
        );
        assert_eq!(tip(&clone, "refs/pull/2/head"), kept);
        assert_eq!(
            pending_pull_requests(&clone, None).unwrap(),
            PendingPulls::default()
        );
        let again = save_pull_requests(&clone, None, |_, _| {}).unwrap();
        assert_eq!(
            again,
            SavedPulls {
                total: 3,
                changed: 0
            }
        );
        let _ = fs::remove_dir_all(&root);
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
            Fetch::default(),
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
            Fetch::default(),
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
            Fetch::default(),
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
    fn only_a_branch_set_from_its_upstream_counts_as_untouched() {
        let upstream = "refs/remotes/origin/bot";
        let local = "refs/heads/bot";
        for entry in [
            "branch: Created from refs/remotes/origin/bot",
            "branch: Created from origin/bot",
            "fetch --no-tags --no-prune --recurse-submodules=no . refs/remotes/origin/bot:refs/heads/bot: fast-forward",
            "fetch --no-tags --no-prune --recurse-submodules=no . +refs/remotes/origin/bot:refs/heads/bot: forced-update",
        ] {
            assert!(set_from_upstream(entry, upstream, local), "{entry}");
        }
        for entry in [
            "commit: work",
            "branch: Created from HEAD",
            "branch: Created from origin/other",
            "reset: moving to origin/bot~1",
            "rebase (finish): refs/heads/bot onto abc",
            "fetch . refs/heads/other:refs/heads/bot: fast-forward",
        ] {
            assert!(!set_from_upstream(entry, upstream, local), "{entry}");
        }
    }

    #[test]
    fn update_follows_a_branch_the_remote_deleted_and_made_again() {
        let root = std::env::temp_dir().join(format!("devassist-remade-{}", std::process::id()));
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

        let ahead_behind = |name: &str| {
            list_refs(&clone)
                .unwrap()
                .locals
                .iter()
                .find(|branch| branch.name == format!("refs/heads/{name}"))
                .map(|branch| (branch.ahead, branch.behind))
        };

        // Twice over, the way a bot's branch is merged, deleted and opened
        // again: the first time the local branch is as it was created, the
        // second time it is where the update before put it.
        for round in ["first", "second"] {
            // The remote deletes both branches, and the update prunes what
            // the checkout knew of them, history included.
            git(&upstream, &["branch", "-q", "-D", "bot", "mine"]);
            update(&clone, Fetch::default(), |_, _| {}, |_, _| {}).unwrap();

            // Then it makes them again on top of a main that moved on.
            commit(&upstream, &format!("main-{round}"));
            for name in ["bot", "mine"] {
                git(&upstream, &["checkout", "-q", "-b", name, "main"]);
                commit(&upstream, &format!("{name}-{round}"));
            }
            git(&upstream, &["checkout", "-q", "main"]);

            let mut lines = Vec::new();
            let done = update(
                &clone,
                Fetch::default(),
                |line, _| lines.push(line.to_string()),
                |_, _| {},
            )
            .unwrap();
            assert_eq!(done.forwarded, vec!["bot".to_string()], "{lines:?}");
            assert_eq!(
                done.left,
                vec![("mine".to_string(), "has commits of its own".to_string())]
            );
            assert_eq!(ahead_behind("bot"), Some((0, 0)));
        }
        assert_eq!(ahead_behind("mine"), Some((2, 3)));

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
