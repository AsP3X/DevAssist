use std::fmt;
use std::sync::OnceLock;
use std::time::Duration;

use serde::Deserialize;

use crate::gitops;

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
    Status { code: u16, message: String },
    Other(String),
}

impl fmt::Display for GhError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Status { code, message } => write!(f, "GitHub HTTP {code}: {message}"),
            Self::Other(message) => write!(f, "GitHub: {message}"),
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

fn authed_login(token: &str) -> Option<String> {
    let response = gh_get("https://api.github.com/user", Some(token)).ok()?;
    let account: ApiAccount = serde_json::from_str(&response.body).ok()?;
    if account.login.is_empty() {
        None
    } else {
        Some(account.login)
    }
}

fn agent() -> &'static ureq::Agent {
    static AGENT: OnceLock<ureq::Agent> = OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build()
    })
}

fn gh_get(url: &str, token: Option<&str>) -> Result<RawResponse, GhError> {
    let mut request = agent()
        .get(url)
        .set("User-Agent", "artcraft-repos")
        .set("Accept", "application/vnd.github+json")
        .set("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = token {
        request = request.set("Authorization", &format!("Bearer {token}"));
    }
    match request.call() {
        Ok(response) => {
            let link = response.header("link").map(str::to_string);
            let body = response
                .into_string()
                .map_err(|err| GhError::Other(err.to_string()))?;
            Ok(RawResponse { body, link })
        }
        Err(ureq::Error::Status(code, response)) => {
            let remaining = response.header("x-ratelimit-remaining").map(str::to_string);
            let body = response.into_string().unwrap_or_default();
            if matches!(code, 403 | 429) && remaining.as_deref() == Some("0") {
                return Err(GhError::Other(
                    "rate limit reached. Set GH_TOKEN or run gh auth login".into(),
                ));
            }
            Err(GhError::Status {
                code,
                message: github_message(&body),
            })
        }
        Err(err) => Err(GhError::Other(err.to_string())),
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
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
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
    fn an_empty_catalog_does_not_call_github() {
        let catalog = fetch_catalog(&[], &[]);
        assert!(catalog.repos.is_empty());
        assert!(catalog.errors.is_empty());
    }
}
