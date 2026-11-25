use std::fmt;

use serde::Deserialize;

use crate::repos::gitops;
use crate::sys::http::{self, Profile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoSpec {
    pub owner: String,
    pub name: String,
}

impl RepoSpec {
    pub fn id(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

#[derive(Debug, Clone)]
pub struct RemoteRepo {
    pub owner: String,
    pub name: String,
    pub description: String,
    pub clone_url: String,
    pub language: String,
    pub archived: bool,
    pub private: bool,
    pub pushed: String,
    pub direct: bool,
}

impl RemoteRepo {
    pub fn id(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }

    pub fn matches_spec(&self, spec: &RepoSpec) -> bool {
        self.owner.eq_ignore_ascii_case(&spec.owner) && self.name.eq_ignore_ascii_case(&spec.name)
    }

    pub fn direct(spec: RepoSpec) -> Self {
        Self {
            owner: spec.owner,
            name: spec.name,
            description: "Clone this repository".to_string(),
            clone_url: String::new(),
            language: String::new(),
            archived: false,
            private: false,
            pushed: String::new(),
            direct: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerKind {
    Organization,
    User,
    AuthenticatedUser,
}

#[derive(Debug)]
pub struct Catalog {
    pub repos: Vec<RemoteRepo>,
    pub errors: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ApiRepo {
    name: String,
    description: Option<String>,
    clone_url: String,
    language: Option<String>,
    archived: bool,
    private: bool,
    pushed_at: Option<String>,
    owner: Option<ApiOwner>,
}

#[derive(Debug, Deserialize)]
struct ApiOwner {
    login: String,
}

#[derive(Debug, Deserialize)]
struct ApiAccount {
    login: String,
    #[serde(rename = "type")]
    kind: String,
}

struct RawResponse {
    body: String,
    link: Option<String>,
}

enum GhError {
    Status {
        code: u16,
        message: String,
    },
    /// GitHub answers no more requests for now. `wait` is how many
    /// seconds it asked to be left alone, where it said so.
    Limited {
        wait: Option<u64>,
    },
    /// No answer came: no connection, or the request timed out.
    Unreachable(String),
    Other(String),
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status { code, message } => write!(f, "GitHub HTTP {code}: {message}"),
            Self::Limited { .. } => write!(
                f,
                "GitHub: rate limit reached. Set GH_TOKEN or run gh auth login"
            ),
            Self::Unreachable(message) | Self::Other(message) => write!(f, "GitHub: {message}"),
        }
    }
}

impl GhError {
    fn not_found(&self) -> bool {
        matches!(self, Self::Status { code: 404, .. })
    }
}

impl From<ApiRepo> for RemoteRepo {
    fn from(repo: ApiRepo) -> Self {
        let pushed = repo
            .pushed_at
            .unwrap_or_default()
            .chars()
            .take(10)
            .collect();
        let owner = repo
            .owner
            .as_ref()
            .map(|owner| owner.login.clone())
            .filter(|login| !login.is_empty())
            .or_else(|| gitops::github_owner_repo(&repo.clone_url).map(|(owner, _)| owner))
            .unwrap_or_default();
        Self {
            owner,
            name: repo.name,
            description: repo
                .description
                .unwrap_or_default()
                .replace(['\n', '\r'], " "),
            clone_url: repo.clone_url,
            language: repo.language.unwrap_or_default(),
            archived: repo.archived,
            private: repo.private,
            pushed,
            direct: false,
        }
    }
}

const RESERVED_OWNERS: &[&str] = &[
    "about",
    "account",
    "apps",
    "codespaces",
    "collections",
    "enterprise",
    "explore",
    "features",
    "issues",
    "login",
    "marketplace",
    "new",
    "notifications",
    "orgs",
    "pricing",
    "pull",
    "search",
    "security",
    "settings",
    "signup",
    "sponsors",
    "topics",
];

pub fn parse_repo_spec(input: &str) -> Option<RepoSpec> {
    let input = input.trim().trim_matches('"').trim();
    if input.is_empty() {
        return None;
    }
    let url = input.to_ascii_lowercase().contains("github.com");
    if let Some((owner, name)) = gitops::github_owner_repo(input)
        && let Some(parsed) = spec(owner, name, url)
    {
        return Some(parsed);
    }
    if url && let Some(rest) = web_path(input) {
        return spec_from_path(rest);
    }
    let (owner, name) = input.split_once('/')?;
    let name = name.trim_matches('/');
    if name.contains('/') {
        return None;
    }
    spec(owner.to_string(), name.to_string(), false)
}

/// An account named in the Add search box, with the text typed after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountQuery {
    pub owner: String,
    pub rest: String,
}

/// Reads `owner/`, `owner/part`, `@owner`, or a GitHub link to an account
/// or one of its repositories.
pub fn parse_account_query(input: &str) -> Option<AccountQuery> {
    let input = input.trim().trim_matches('"').trim();
    if input.is_empty() {
        return None;
    }
    let lower = input.to_ascii_lowercase();
    let (owner, rest, url) =
        if lower.starts_with("git@github.com:") || lower.starts_with("ssh://git@github.com/") {
            let (owner, name) = gitops::github_owner_repo(input)?;
            (owner, name, true)
        } else if lower.contains("github.com") {
            let mut parts = web_path(input)?
                .split(['?', '#'])
                .next()
                .unwrap_or_default()
                .split('/')
                .map(str::trim)
                .filter(|part| !part.is_empty());
            let first = parts.next()?;
            if first.eq_ignore_ascii_case("orgs") {
                (parts.next()?.to_string(), String::new(), true)
            } else {
                let name = parts.next().unwrap_or_default();
                (first.to_string(), name.to_string(), true)
            }
        } else if let Some(owner) = input.strip_prefix('@') {
            (owner.to_string(), String::new(), false)
        } else {
            let (owner, rest) = input.split_once('/')?;
            let rest = rest.trim_end_matches('/');
            if rest.contains('/') {
                return None;
            }
            (owner.trim().to_string(), rest.trim().to_string(), false)
        };
    let rest = rest.strip_suffix(".git").unwrap_or(&rest).to_string();
    if !valid_account(&owner) {
        return None;
    }
    if url
        && RESERVED_OWNERS
            .iter()
            .any(|item| owner.eq_ignore_ascii_case(item))
    {
        return None;
    }
    if !rest
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return None;
    }
    Some(AccountQuery { owner, rest })
}

pub fn valid_account(name: &str) -> bool {
    valid_segment(name)
}

/// Every repository of one user or organization.
pub fn fetch_account(owner: &str) -> Result<Vec<RemoteRepo>, String> {
    let token = token();
    let authed = token.as_deref().and_then(authed_login);
    fetch_owner(owner, token.as_deref(), authed.as_deref())
}

pub fn repos_url(owner: &str, kind: OwnerKind) -> String {
    match kind {
        OwnerKind::Organization => format!(
            "https://api.github.com/orgs/{owner}/repos?per_page=100&type=all&sort=full_name&direction=asc"
        ),
        OwnerKind::User => format!(
            "https://api.github.com/users/{owner}/repos?per_page=100&type=owner&sort=full_name&direction=asc"
        ),
        OwnerKind::AuthenticatedUser => {
            "https://api.github.com/user/repos?per_page=100&affiliation=owner&sort=full_name&direction=asc"
                .to_string()
        }
    }
}

pub fn repo_url(owner: &str, name: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{name}")
}

pub fn fetch_catalog(owners: &[String], pinned: &[RepoSpec]) -> Catalog {
    if owners.is_empty() && pinned.is_empty() {
        return Catalog {
            repos: Vec::new(),
            errors: Vec::new(),
        };
    }
    let token = token();
    let authed = token.as_deref().and_then(authed_login);
    let mut repos = Vec::new();
    let mut errors = Vec::new();
    for owner in owners {
        match fetch_owner(owner, token.as_deref(), authed.as_deref()) {
            Ok(mut page) => repos.append(&mut page),
            Err(err) => errors.push(format!("{owner}: {err}")),
        }
    }
    for spec in pinned {
        if repos.iter().any(|repo| repo.matches_spec(spec)) {
            continue;
        }
        match fetch_one(spec, token.as_deref()) {
            Ok(repo) => repos.push(repo),
            Err(err) => {
                let missing = err.to_lowercase().contains("not found");
                errors.push(format!("{}: {err}", spec.id()));
                if !missing {
                    repos.push(RemoteRepo::direct(spec.clone()));
                }
            }
        }
    }
    repos.sort_by(|a, b| {
        a.owner
            .to_lowercase()
            .cmp(&b.owner.to_lowercase())
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    repos.dedup_by(|a, b| {
        a.owner.eq_ignore_ascii_case(&b.owner) && a.name.eq_ignore_ascii_case(&b.name)
    });
    Catalog { repos, errors }
}

pub fn parse_repo_list(body: &str) -> Result<Vec<RemoteRepo>, String> {
    if body.trim_start().starts_with('{') {
        return Err(github_message(body));
    }
    let repos: Vec<ApiRepo> = serde_json::from_str(body)
        .map_err(|err| format!("GitHub response was not a repository list ({err})"))?;
    Ok(repos.into_iter().map(RemoteRepo::from).collect())
}

pub fn next_link(header: &str) -> Option<String> {
    for part in header.split(',') {
        let part = part.trim();
        if part.contains("rel=\"next\"") {
            let start = part.find('<')? + 1;
            let end = part[start..].find('>')? + start;
            return Some(part[start..end].to_string());
        }
    }
    None
}

pub fn token() -> Option<String> {
    for key in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    gh_cli_token()
}

fn spec(owner: String, name: String, reject_reserved: bool) -> Option<RepoSpec> {
    let name = name.strip_suffix(".git").unwrap_or(&name).to_string();
    if !valid_segment(&owner) || !valid_segment(&name) {
        return None;
    }
    if reject_reserved
        && RESERVED_OWNERS
            .iter()
            .any(|item| owner.eq_ignore_ascii_case(item))
    {
        return None;
    }
    Some(RepoSpec { owner, name })
}

fn spec_from_path(path: &str) -> Option<RepoSpec> {
    let mut parts = path.trim_start_matches('/').split(['/', '?', '#']);
    let owner = parts.next()?.trim();
    let name = parts.next()?.trim();
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    spec(owner.to_string(), name.to_string(), true)
}

fn web_path(input: &str) -> Option<&str> {
    let lower = input.to_ascii_lowercase();
    for prefix in [
        "https://github.com/",
        "http://github.com/",
        "https://www.github.com/",
        "http://www.github.com/",
        "github.com/",
        "www.github.com/",
    ] {
        if lower.starts_with(prefix) {
            return Some(&input[prefix.len()..]);
        }
    }
    None
}

fn valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment.len() <= 100
        && !segment.starts_with('-')
        && !segment.ends_with('-')
        && !segment.ends_with('.')
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn fetch_owner(
    owner: &str,
    token: Option<&str>,
    authed: Option<&str>,
) -> Result<Vec<RemoteRepo>, String> {
    let profile =
        gh_get(&format!("https://api.github.com/users/{owner}"), token).map_err(|err| {
            if err.not_found() {
                format!("GitHub user or organization {owner} was not found")
            } else {
                err.to_string()
            }
        })?;
    let account: ApiAccount = serde_json::from_str(&profile.body)
        .map_err(|err| format!("GitHub profile for {owner} was not readable ({err})"))?;
    let kind = if account.kind.eq_ignore_ascii_case("Organization") {
        OwnerKind::Organization
    } else if authed.is_some_and(|login| login.eq_ignore_ascii_case(&account.login)) {
        OwnerKind::AuthenticatedUser
    } else {
        OwnerKind::User
    };
    fetch_pages(repos_url(owner, kind), token)
}

fn fetch_one(spec: &RepoSpec, token: Option<&str>) -> Result<RemoteRepo, String> {
    let response = gh_get(&repo_url(&spec.owner, &spec.name), token).map_err(|err| {
        if err.not_found() {
            format!("repository {} was not found", spec.id())
        } else {
            err.to_string()
        }
    })?;
    parse_one(&response.body)
}

fn fetch_pages(start: String, token: Option<&str>) -> Result<Vec<RemoteRepo>, String> {
    let mut url = Some(start);
    let mut all = Vec::new();
    let mut pages = 0;
    while let Some(next) = url.take() {
        pages += 1;
        if pages > 20 {
            break;
        }
        let response = gh_get(&next, token).map_err(|err| err.to_string())?;
        all.extend(parse_repo_list(&response.body)?);
        url = response.link.as_deref().and_then(next_link);
    }
    Ok(all)
}

fn parse_one(body: &str) -> Result<RemoteRepo, String> {
    if body.trim_start().starts_with('[') {
        return Err("GitHub returned a list instead of one repository".into());
    }
    if body.trim_start().starts_with('{')
        && body.contains("\"message\"")
        && !body.contains("\"clone_url\"")
    {
        return Err(github_message(body));
    }
    let repo: ApiRepo = serde_json::from_str(body)
        .map_err(|err| format!("GitHub response was not a repository ({err})"))?;
    Ok(repo.into())
}

/// How many pull requests a repository has on GitHub, by state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PullCounts {
    pub open: u32,
    pub merged: u32,
    /// Closed without being merged.
    pub closed: u32,
}

impl PullCounts {
    pub fn total(self) -> u32 {
        self.open
            .saturating_add(self.merged)
            .saturating_add(self.closed)
    }
}

/// How many repositories one question to GitHub asks about.
const PULL_COUNT_BATCH: usize = 40;

/// Asks GitHub how many pull requests each of `repos` (owner, name) has,
/// by state. The answers come in the order asked; None is a repository
/// GitHub does not show to this sign-in. One request covers 40 of them.
pub fn fetch_pull_counts(
    repos: &[(String, String)],
    token: &str,
) -> Result<Vec<Option<PullCounts>>, String> {
    let mut all = Vec::with_capacity(repos.len());
    for batch in repos.chunks(PULL_COUNT_BATCH) {
        let mut response = http::post_json(
            "https://api.github.com/graphql",
            Profile::Quick,
            &[("Accept", "application/vnd.github+json")],
            Some(token),
            &pull_counts_query(batch),
        )
        .map_err(|err| format!("GitHub: {err}"))?;
        let status = response.status();
        let body = response.body_mut().read_to_string().unwrap_or_default();
        if !status.is_success() {
            return Err(format!(
                "GitHub HTTP {}: {}",
                status.as_u16(),
                github_message(&body)
            ));
        }
        all.extend(parse_pull_counts(&body, batch.len())?);
    }
    Ok(all)
}

/// The request body that asks for the pull request counts of `repos`.
pub fn pull_counts_query(repos: &[(String, String)]) -> String {
    let quoted = |text: &str| serde_json::Value::String(text.to_string()).to_string();
    let mut query = String::from("query {");
    for (index, (owner, name)) in repos.iter().enumerate() {
        query.push_str(&format!(
            " r{index}: repository(owner: {}, name: {}) {{ \
             open: pullRequests(states: OPEN) {{ totalCount }} \
             merged: pullRequests(states: MERGED) {{ totalCount }} \
             closed: pullRequests(states: CLOSED) {{ totalCount }} }}",
            quoted(owner),
            quoted(name)
        ));
    }
    query.push_str(" }");
    serde_json::json!({ "query": query }).to_string()
}

/// Reads the answer to `pull_counts_query` for `count` repositories.
pub fn parse_pull_counts(body: &str, count: usize) -> Result<Vec<Option<PullCounts>>, String> {
    let answer: serde_json::Value = serde_json::from_str(body)
        .map_err(|err| format!("GitHub answer was not readable ({err})"))?;
    let Some(data) = answer.get("data").filter(|data| data.is_object()) else {
        let message = answer
            .get("errors")
            .and_then(|errors| errors.get(0))
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
            .unwrap_or("no pull request counts in the answer");
        return Err(format!("GitHub: {message}"));
    };
    let number = |repo: &serde_json::Value, state: &str| {
        repo.get(state)
            .and_then(|pulls| pulls.get("totalCount"))
            .and_then(serde_json::Value::as_u64)
            .map(|count| u32::try_from(count).unwrap_or(u32::MAX))
    };
    Ok((0..count)
        .map(|index| {
            let repo = data.get(format!("r{index}"))?;
            Some(PullCounts {
                open: number(repo, "open")?,
                merged: number(repo, "merged")?,
                closed: number(repo, "closed")?,
            })
        })
        .collect())
}

/// A fork of a repository on GitHub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fork {
    pub owner: String,
    pub name: String,
    /// Changes when the fork does: when it was last pushed to and how
    /// many pull requests and issues it has open. Empty when GitHub does
    /// not say, and then nothing can be concluded from it.
    pub stamp: String,
}

/// What a fork's stamp is made of.
fn fork_stamp(pushed_at: Option<&str>, open: u32) -> String {
    match pushed_at.map(str::trim) {
        Some(pushed) if !pushed.is_empty() => format!("{pushed} {open}"),
        _ => String::new(),
    }
}

/// The forks GitHub lists for a repository, and what kept the list from
/// being complete, if anything did.
#[derive(Debug, Default)]
pub struct Forks {
    pub forks: Vec<Fork>,
    pub warning: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiFork {
    name: String,
    owner: Option<ApiOwner>,
    #[serde(default)]
    forks_count: u32,
    #[serde(default)]
    pushed_at: Option<String>,
    #[serde(default)]
    open_issues_count: u32,
}

/// The most pages of forks read for one repository: 10,000 forks.
const FORK_PAGES: u32 = 100;

pub fn forks_url(owner: &str, name: &str) -> String {
    format!("https://api.github.com/repos/{owner}/{name}/forks?per_page=100&sort=oldest")
}

/// One page of forks, each with how many forks it has itself.
pub fn parse_fork_list(body: &str) -> Result<Vec<(Fork, u32)>, String> {
    if body.trim_start().starts_with('{') {
        return Err(github_message(body));
    }
    let forks: Vec<ApiFork> = serde_json::from_str(body)
        .map_err(|err| format!("GitHub response was not a list of forks ({err})"))?;
    Ok(forks
        .into_iter()
        .filter_map(|fork| {
            let owner = fork.owner.map(|owner| owner.login)?;
            (!owner.is_empty() && !fork.name.is_empty()).then_some((
                Fork {
                    owner,
                    name: fork.name,
                    stamp: fork_stamp(fork.pushed_at.as_deref(), fork.open_issues_count),
                },
                fork.forks_count,
            ))
        })
        .collect())
}

/// Every fork of a repository, and the forks of those forks. `on_line`
/// hears how many are known so far. A fork whose own forks cannot be
/// listed is kept, and the list says that it is not complete.
///
/// A long list is not lost to a limit or a timeout: when GitHub's limit
/// is reached, or it does not answer, the listing pauses and then goes
/// on with the page it was at. `on_line` hears about the pause.
pub fn fetch_forks(
    owner: &str,
    name: &str,
    token: Option<&str>,
    mut on_line: impl FnMut(&str, bool),
) -> Result<Forks, String> {
    let mut found = Forks::default();
    let mut seen = std::collections::HashSet::new();
    seen.insert(format!("{owner}/{name}").to_lowercase());
    let mut waiting = std::collections::VecDeque::new();
    waiting.push_back((owner.to_string(), name.to_string(), true));
    while let Some((owner, name, root)) = waiting.pop_front() {
        let mut url = Some(forks_url(&owner, &name));
        let mut pages = 0;
        while let Some(next) = url.take() {
            if crate::sys::process::was_cancelled() {
                return Err("cancelled".into());
            }
            pages += 1;
            if pages > FORK_PAGES {
                found.warning = Some(format!(
                    "{owner}/{name} has more forks than the first {} that were read",
                    FORK_PAGES * 100
                ));
                break;
            }
            let page = gh_get_patiently(&next, token, &mut on_line)
                .map_err(|err| {
                    if err.not_found() {
                        format!("repository {owner}/{name} was not found")
                    } else {
                        err.to_string()
                    }
                })
                .and_then(|response| Ok((parse_fork_list(&response.body)?, response.link)));
            let (forks, link) = match page {
                Ok(page) => page,
                // The repository itself has to answer. A fork that does
                // not only costs the forks below it.
                Err(err) if root => return Err(err),
                Err(err) => {
                    found.warning = Some(format!("forks of {owner}/{name}: {err}"));
                    break;
                }
            };
            for (fork, below) in forks {
                if !seen.insert(format!("{}/{}", fork.owner, fork.name).to_lowercase()) {
                    continue;
                }
                if below > 0 {
                    waiting.push_back((fork.owner.clone(), fork.name.clone(), false));
                }
                found.forks.push(fork);
            }
            on_line(&format!("Listing forks: {}", found.forks.len()), true);
            url = link.as_deref().and_then(next_link);
        }
    }
    Ok(found)
}

pub fn current_login() -> Option<String> {
    token().and_then(|token| authed_login(&token))
}

fn authed_login(token: &str) -> Option<String> {
    let response = gh_get("https://api.github.com/user", Some(token)).ok()?;
    let account: ApiAccount = serde_json::from_str(&response.body).ok()?;
    if account.login.is_empty() {
        None
    } else {
        Some(account.login)
    }
}

fn gh_get(url: &str, token: Option<&str>) -> Result<RawResponse, GhError> {
    let mut response = http::get(
        url,
        Profile::Quick,
        &[
            ("Accept", "application/vnd.github+json"),
            ("X-GitHub-Api-Version", "2022-11-28"),
        ],
        token,
    )
    .map_err(GhError::Unreachable)?;
    let status = response.status();
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_string)
    };
    let link = header("link");
    let remaining = header("x-ratelimit-remaining");
    let retry_after = header("retry-after");
    let reset = header("x-ratelimit-reset");
    let body = response.body_mut().read_to_string();
    if status.is_success() {
        let body = body.map_err(|err| GhError::Other(err.to_string()))?;
        return Ok(RawResponse { body, link });
    }
    let code = status.as_u16();
    if matches!(code, 403 | 429) && (remaining.as_deref() == Some("0") || retry_after.is_some()) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_secs());
        return Err(GhError::Limited {
            wait: limit_wait(retry_after.as_deref(), reset.as_deref(), now),
        });
    }
    Err(GhError::Status {
        code,
        message: github_message(&body.unwrap_or_default()),
    })
}

/// How many seconds GitHub wants to be left alone: what `Retry-After`
/// says, else the time until the limit is reset, which the
/// `X-RateLimit-Reset` header gives as seconds since 1970.
fn limit_wait(retry_after: Option<&str>, reset: Option<&str>, now: u64) -> Option<u64> {
    let number = |text: Option<&str>| text.and_then(|text| text.trim().parse::<u64>().ok());
    number(retry_after).or_else(|| number(reset).map(|reset| reset.saturating_sub(now)))
}

/// The pauses before a request that got no answer is tried again.
const RETRY_PAUSES: [u64; 3] = [5, 20, 60];

/// The longest single wait for a limit: GitHub resets it every hour.
const LONGEST_WAIT: u64 = 3660;

/// A GET that waits instead of giving up. When GitHub's limit is reached
/// it pauses until the limit is reset and asks again, however often that
/// takes; when no answer comes it pauses and tries three more times.
/// `on_line` hears about every pause, and cancelling ends one.
fn gh_get_patiently(
    url: &str,
    token: Option<&str>,
    on_line: &mut dyn FnMut(&str, bool),
) -> Result<RawResponse, GhError> {
    let mut tries = 0;
    loop {
        let (why, wait) = match gh_get(url, token) {
            Err(GhError::Limited { wait }) => (
                "GitHub's limit is reached",
                // A little longer than asked, so the clocks need not agree.
                wait.unwrap_or(60).min(LONGEST_WAIT) + 2,
            ),
            Err(GhError::Unreachable(_)) if tries < RETRY_PAUSES.len() => {
                tries += 1;
                ("GitHub did not answer", RETRY_PAUSES[tries - 1])
            }
            other => return other,
        };
        gitops::pause_for(wait, why, on_line).map_err(GhError::Other)?;
    }
}

fn github_message(body: &str) -> String {
    #[derive(Deserialize)]
    struct ApiError {
        message: Option<String>,
    }
    serde_json::from_str::<ApiError>(body)
        .ok()
        .and_then(|err| err.message)
        .filter(|message| !message.is_empty())
        .unwrap_or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                "request failed".to_string()
            } else {
                trimmed.chars().take(180).collect()
            }
        })
}

fn gh_cli_token() -> Option<String> {
    let mut cmd = std::process::Command::new("gh");
    cmd.args(["auth", "token"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .env("GH_PROMPT_DISABLED", "1");
    crate::sys::process::quiet(&mut cmd);
    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let token = String::from_utf8(output.stdout).ok()?;
    let token = token.trim().to_string();
    if token.is_empty() { None } else { Some(token) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pull_request_counts_are_asked_and_read_per_repository() {
        let repos = vec![
            ("acme".to_string(), "api".to_string()),
            ("acme".to_string(), "we\"b".to_string()),
        ];
        let body = pull_counts_query(&repos);
        let sent: serde_json::Value = serde_json::from_str(&body).unwrap();
        let query = sent["query"].as_str().unwrap();
        assert!(query.contains(r#"r0: repository(owner: "acme", name: "api")"#));
        // A quote in a name cannot break out of the question.
        assert!(query.contains(r#"name: "we\"b""#), "{query}");
        assert!(query.contains("merged: pullRequests(states: MERGED)"));

        // The second repository is one GitHub does not show.
        let answer = r#"{"data": {
            "r0": {"open": {"totalCount": 2}, "merged": {"totalCount": 14}, "closed": {"totalCount": 3}},
            "r1": null
        }, "errors": [{"message": "Could not resolve to a Repository"}]}"#;
        let counts = parse_pull_counts(answer, 2).unwrap();
        assert_eq!(
            counts,
            [
                Some(PullCounts {
                    open: 2,
                    merged: 14,
                    closed: 3
                }),
                None
            ]
        );
        assert_eq!(counts[0].unwrap().total(), 19);

        let refused = r#"{"errors": [{"message": "Bad credentials"}]}"#;
        assert_eq!(
            parse_pull_counts(refused, 1).unwrap_err(),
            "GitHub: Bad credentials"
        );
    }

    /// `cargo test -- --ignored live_pull_counts`
    #[test]
    #[ignore = "asks GitHub, and needs a sign-in"]
    fn live_pull_counts_come_back_for_a_public_repository() {
        let token = token().expect("GH_TOKEN or gh auth login");
        let repos = vec![
            ("rust-lang".to_string(), "rustlings".to_string()),
            (
                "rust-lang".to_string(),
                "no-such-repository-here".to_string(),
            ),
        ];
        let counts = fetch_pull_counts(&repos, &token).unwrap();
        println!("{counts:?}");
        assert!(counts[0].unwrap().merged > 0);
        assert_eq!(counts[1], None);
    }

    #[test]
    fn parses_a_repository_page() {
        let body = r#"[{
            "name": "photocraft",
            "description": "Photo editor",
            "clone_url": "https://github.com/storytold/photocraft.git",
            "language": "Rust",
            "archived": false,
            "private": false,
            "pushed_at": "2026-10-12T01:02:03Z"
        }]"#;
        let repos = parse_repo_list(body).unwrap();
        assert_eq!(repos.len(), 1);
        assert_eq!(repos[0].name, "photocraft");
        assert_eq!(repos[0].owner, "storytold");
        assert_eq!(repos[0].pushed, "2026-10-12");
        assert_eq!(repos[0].language, "Rust");
        assert!(!repos[0].direct);
    }

    #[test]
    fn prefers_the_owner_object_over_the_clone_url() {
        let body = r#"[{
            "name": "photocraft",
            "description": null,
            "clone_url": "https://github.com/someone/photocraft.git",
            "language": null,
            "archived": false,
            "private": true,
            "pushed_at": null,
            "owner": { "login": "Storytold" }
        }]"#;
        let repos = parse_repo_list(body).unwrap();
        assert_eq!(repos[0].owner, "Storytold");
        assert!(repos[0].private);
    }

    #[test]
    fn reads_the_next_page_link() {
        let header = "<https://api.github.com/orgs/storytold/repos?page=2>; rel=\"next\", <https://api.github.com/orgs/storytold/repos?page=3>; rel=\"last\"";
        assert_eq!(
            next_link(header).as_deref(),
            Some("https://api.github.com/orgs/storytold/repos?page=2")
        );
        assert!(next_link("<https://api.github.com/x>; rel=\"last\"").is_none());
    }

    #[test]
    fn parses_owner_repo_shorthand_and_github_urls() {
        let spec = parse_repo_spec("  Other/Tool.git  ").unwrap();
        assert_eq!(spec.owner, "Other");
        assert_eq!(spec.name, "Tool");
        assert_eq!(
            parse_repo_spec("https://github.com/storytold/photocraft")
                .unwrap()
                .id(),
            "storytold/photocraft"
        );
        assert_eq!(
            parse_repo_spec("https://github.com/storytold/photocraft/tree/main")
                .unwrap()
                .id(),
            "storytold/photocraft"
        );
        assert_eq!(
            parse_repo_spec("git@github.com:storytold/photocraft.git")
                .unwrap()
                .id(),
            "storytold/photocraft"
        );
        assert_eq!(
            parse_repo_spec("https://www.github.com/storytold/photocraft?tab=readme")
                .unwrap()
                .id(),
            "storytold/photocraft"
        );
        assert!(parse_repo_spec("photocraft").is_none());
        assert!(parse_repo_spec("https://github.com/settings/profile").is_none());
        assert!(parse_repo_spec("https://gitlab.com/storytold/photocraft").is_none());
        assert!(parse_repo_spec("https://github.com/orgs/storytold/repositories").is_none());
    }

    #[test]
    fn reads_an_account_from_shorthand_and_links() {
        let query = |text: &str| parse_account_query(text).map(|q| (q.owner, q.rest));
        let account = |owner: &str, rest: &str| Some((owner.to_string(), rest.to_string()));
        assert_eq!(
            query("https://github.com/UCS-Care-Group"),
            account("UCS-Care-Group", "")
        );
        assert_eq!(
            query("github.com/UCS-Care-Group/"),
            account("UCS-Care-Group", "")
        );
        assert_eq!(
            query("https://github.com/orgs/storytold/repositories"),
            account("storytold", "")
        );
        assert_eq!(
            query("https://github.com/storytold/photocraft/tree/main"),
            account("storytold", "photocraft")
        );
        assert_eq!(
            query("git@github.com:storytold/photocraft.git"),
            account("storytold", "photocraft")
        );
        assert_eq!(query("storytold/"), account("storytold", ""));
        assert_eq!(query("storytold/pho"), account("storytold", "pho"));
        assert_eq!(query("@storytold"), account("storytold", ""));
        assert_eq!(query("photocraft"), None);
        assert_eq!(query("https://github.com/settings/profile"), None);
        assert_eq!(query("https://gitlab.com/storytold"), None);
        assert_eq!(query("a/b/c"), None);
        assert_eq!(query("bad name/"), None);
    }

    #[test]
    fn builds_list_urls_for_orgs_users_and_the_signed_in_account() {
        assert!(repos_url("storytold", OwnerKind::Organization).contains("/orgs/storytold/repos"));
        assert!(repos_url("ada", OwnerKind::User).contains("/users/ada/repos"));
        assert!(repos_url("ada", OwnerKind::User).contains("type=owner"));
        assert!(repos_url("ada", OwnerKind::AuthenticatedUser).contains("/user/repos"));
        assert_eq!(
            repo_url("storytold", "photocraft"),
            "https://api.github.com/repos/storytold/photocraft"
        );
    }

    #[test]
    fn a_page_of_forks_names_each_fork_and_how_many_forks_it_has() {
        assert!(forks_url("ada", "tool").contains("/repos/ada/tool/forks?per_page=100"));
        let body = r#"[
            {"name": "tool", "owner": {"login": "bob"}, "forks_count": 2,
             "pushed_at": "2026-10-12T01:02:03Z", "open_issues_count": 3},
            {"name": "tool-renamed", "owner": {"login": "eve"}, "pushed_at": null},
            {"name": "orphan", "owner": null}
        ]"#;
        let fork = |owner: &str, name: &str, stamp: &str| Fork {
            owner: owner.into(),
            name: name.into(),
            stamp: stamp.into(),
        };
        // The stamp changes with a push and with a pull request. Without
        // a time of the last push there is none.
        assert_eq!(
            parse_fork_list(body).unwrap(),
            [
                (fork("bob", "tool", "2026-10-12T01:02:03Z 3"), 2),
                (fork("eve", "tool-renamed", ""), 0)
            ]
        );
        assert_eq!(parse_fork_list("[]").unwrap(), []);
        assert_eq!(
            parse_fork_list(r#"{"message": "Not Found"}"#).unwrap_err(),
            "Not Found"
        );
    }

    #[test]
    fn a_reached_limit_says_how_long_to_wait() {
        // Retry-After counts seconds; the reset is a time of day.
        assert_eq!(limit_wait(Some("30"), Some("5000"), 1000), Some(30));
        assert_eq!(limit_wait(None, Some("1600"), 1000), Some(600));
        // A reset that has passed already means: ask again now.
        assert_eq!(limit_wait(None, Some("900"), 1000), Some(0));
        assert_eq!(limit_wait(None, None, 1000), None);
        assert_eq!(limit_wait(Some("soon"), None, 1000), None);
        // It reads as before where nothing waits for it.
        let limited = GhError::Limited { wait: Some(5) }.to_string();
        assert!(limited.contains("rate limit reached"), "{limited}");
    }

    #[test]
    fn a_request_pauses_at_the_limit_and_then_goes_on() {
        use std::io::{Read, Write};
        // A server that refuses the first request for the limit and
        // answers the second.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/forks", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let replies = [
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nConnection: close\r\nContent-Length: 2\r\n\r\n{}",
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 2\r\n\r\n[]",
            ];
            for reply in replies {
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
            }
        });
        let mut lines = Vec::new();
        let mut on_line = |line: &str, progress: bool| {
            if !progress {
                lines.push(line.to_string());
            }
        };
        let answer = gh_get_patiently(&url, None, &mut on_line);
        server.join().unwrap();
        assert_eq!(answer.ok().map(|answer| answer.body).as_deref(), Some("[]"));
        assert_eq!(
            lines,
            ["GitHub's limit is reached. Paused for 0:02.", "Going on."]
        );
    }

    #[test]
    fn an_empty_catalog_does_not_call_github() {
        let catalog = fetch_catalog(&[], &[]);
        assert!(catalog.repos.is_empty());
        assert!(catalog.errors.is_empty());
    }
}
