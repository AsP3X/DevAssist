use std::fs;
use std::path::{Path, PathBuf};

use crate::github::{self, RepoSpec};
use crate::gitops::{self, Protocol};

const DEFAULT_OWNER: &str = "storytold";

#[derive(Clone, Debug)]
pub struct Settings {
    pub root: PathBuf,
    pub title: String,
    pub owners: Vec<String>,
    pub pinned: Vec<RepoSpec>,
    pub protocol: Protocol,
    pub doctor: bool,
}

pub enum Startup {
    Help,
    Run(Settings),
}

#[derive(Debug, Default)]
struct Flags {
    help: bool,
    doctor: bool,
    root: Option<PathBuf>,
    config: Option<PathBuf>,
    no_owner: bool,
    owners: Vec<String>,
    owners_explicit: bool,
    pinned: Vec<RepoSpec>,
    title: Option<String>,
    protocol: Option<Protocol>,
}

#[derive(Debug, Default)]
struct ParsedConfig {
    title: Option<String>,
    owners: Vec<String>,
    owners_explicit: bool,
    pinned: Vec<RepoSpec>,
    protocol: Option<Protocol>,
}

struct Resolved {
    title: String,
    owners: Vec<String>,
    pinned: Vec<RepoSpec>,
    protocol: Protocol,
}

pub fn startup() -> Result<Startup, String> {
    let flags = parse_flags(std::env::args().skip(1))?;
    if flags.help {
        return Ok(Startup::Help);
    }
    let root = flags.root.clone().unwrap_or_else(gitops::default_root);
    if !root.is_dir() {
        return Err(format!("{} is not a folder", root.display()));
    }
    let file = load_file(&flags, &root)?;
    let resolved = resolve(&flags, file);
    Ok(Startup::Run(Settings {
        root,
        title: resolved.title,
        owners: resolved.owners,
        pinned: resolved.pinned,
        protocol: resolved.protocol,
        doctor: flags.doctor,
    }))
}

pub fn print_help() {
    println!(
        "\
artcraft-repos
  Add, update, and remove local GitHub clones in one folder.

Usage
  artcraft-repos [--root PATH] [--owner NAME]... [--repo OWNER/NAME]...
  artcraft-repos --doctor
  artcraft-repos --help

Options
  --root PATH        Folder that holds the clones. Defaults to the current
                     directory when it already contains GitHub clones.
  --owner NAME       GitHub user or organization to list. Repeat to track
                     several. --org is the same option. Default: storytold.
  --no-owner         Do not list a user or organization. Add repositories by
                     owner/name from the Add screen instead.
  --repo OWNER/NAME  Pin one repository. A GitHub URL is also accepted.
  --protocol MODE    auto, https, or ssh. auto follows the remotes already
                     in the folder, otherwise https.
  --title TEXT       Heading shown in the app.
  --config PATH      Settings file. Without this flag the app reads
                     ROOT/artcraft-repos.toml, then a file beside the
                     executable.
  --doctor           Print the folder and GitHub check, then exit.

Settings file
  title = My Tools
  owner = my-user
  owner = my-org
  repo = someone/tool
  protocol = ssh

  Lines starting with # are comments.

Keys
  a add      u update one     U update all      d remove
  r refresh  s sort           o open folder     q quit

In Add, type owner/repo or paste a GitHub URL to clone any repository.
Refresh fetches remotes. Update pulls with --ff-only.
Esc cancels the git command in progress.
"
    );
}

fn parse_flags<I>(args: I) -> Result<Flags, String>
where
    I: IntoIterator<Item = String>,
{
    let mut flags = Flags::default();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => flags.help = true,
            "--doctor" => flags.doctor = true,
            "--no-owner" => flags.no_owner = true,
            "--root" => flags.root = Some(PathBuf::from(need(&mut args, "--root")?)),
            "--config" => flags.config = Some(PathBuf::from(need(&mut args, "--config")?)),
            "--title" => flags.title = Some(need(&mut args, "--title")?),
            "--protocol" => {
                let value = need(&mut args, "--protocol")?;
                flags.protocol = Some(Protocol::parse(&value)?);
            }
            "--owner" | "--org" => push_owner(&mut flags, &need(&mut args, "--owner")?)?,
            "--repo" => {
                let value = need(&mut args, "--repo")?;
                let spec = github::parse_repo_spec(&value)
                    .ok_or_else(|| "--repo expects owner/name or a GitHub URL".to_string())?;
                push_spec(&mut flags.pinned, spec);
            }
            other if other.starts_with('-') => {
                return Err(format!("unknown option {other}. Try --help"));
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    if flags.no_owner && flags.owners_explicit {
        return Err("use either --owner or --no-owner, not both".into());
    }
    Ok(flags)
}

fn need(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next().ok_or_else(|| format!("{flag} needs a value"))
}

fn push_owner(flags: &mut Flags, value: &str) -> Result<(), String> {
    let mut any = false;
    for part in value.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if part.contains('/') {
            return Err(
                "--owner expects a user or organization. Use --repo owner/name for one repository."
                    .into(),
            );
        }
        push_unique(&mut flags.owners, part);
        any = true;
    }
    if !any {
        return Err("--owner needs a GitHub user or organization".into());
    }
    flags.owners_explicit = true;
    Ok(())
}

fn resolve(flags: &Flags, file: Option<ParsedConfig>) -> Resolved {
    let mut owners = vec![DEFAULT_OWNER.to_string()];
    let mut pinned = Vec::new();
    let mut title = None;
    let mut protocol = Protocol::Auto;

    if let Some(file) = file {
        if file.owners_explicit {
            owners = file.owners;
        }
        pinned = file.pinned;
        title = file.title;
        if let Some(value) = file.protocol {
            protocol = value;
        }
    }
    if flags.no_owner {
        owners.clear();
    } else if flags.owners_explicit {
        owners = flags.owners.clone();
    }
    for spec in &flags.pinned {
        push_spec(&mut pinned, spec.clone());
    }
    if let Some(value) = flags.title.clone() {
        title = Some(value);
    }
    if let Some(value) = flags.protocol {
        protocol = value;
    }
    let title = title.unwrap_or_else(|| default_title(&owners));
    Resolved {
        title,
        owners,
        pinned,
        protocol,
    }
}

fn default_title(owners: &[String]) -> String {
    match owners {
        [owner] if owner.eq_ignore_ascii_case(DEFAULT_OWNER) => "Artcraft Repos".to_string(),
        [owner] => format!("{owner} repos"),
        _ => "GitHub Repos".to_string(),
    }
}

fn load_file(flags: &Flags, root: &Path) -> Result<Option<ParsedConfig>, String> {
    let path = if let Some(path) = &flags.config {
        if !path.is_file() {
            return Err(format!("settings file {} was not found", path.display()));
        }
        path.clone()
    } else {
        let in_root = root.join("artcraft-repos.toml");
        if in_root.is_file() {
            in_root
        } else if let Some(beside) = exe_dir()
            .map(|dir| dir.join("artcraft-repos.toml"))
            .filter(|path| path.is_file())
        {
            beside
        } else {
            return Ok(None);
        }
    };
    let text = fs::read_to_string(&path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    parse_config(&text)
        .map(Some)
        .map_err(|err| format!("{}: {err}", path.display()))
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn parse_config(text: &str) -> Result<ParsedConfig, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parsed = ParsedConfig::default();
    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            return Err(format!("line {line_no}: expected key = value"));
        };
        let key = key.trim();
        let value = unquote(value.trim());
        if value.is_empty() {
            return Err(format!("line {line_no}: {key} needs a value"));
        }
        match key {
            "title" => {
                if parsed.title.is_some() {
                    return Err(format!("line {line_no}: title is already set"));
                }
                parsed.title = Some(value.to_string());
            }
            "owner" | "org" => {
                if value.contains('/') {
                    return Err(format!(
                        "line {line_no}: owner should be a user or organization. Use repo = owner/name"
                    ));
                }
                push_unique(&mut parsed.owners, value);
                parsed.owners_explicit = true;
            }
            "repo" => {
                let spec = github::parse_repo_spec(value)
                    .ok_or_else(|| format!("line {line_no}: repo should look like owner/name"))?;
                push_spec(&mut parsed.pinned, spec);
            }
            "protocol" => {
                if parsed.protocol.is_some() {
                    return Err(format!("line {line_no}: protocol is already set"));
                }
                parsed.protocol =
                    Some(Protocol::parse(value).map_err(|err| format!("line {line_no}: {err}"))?);
            }
            other => return Err(format!("line {line_no}: unknown setting {other}")),
        }
    }
    Ok(parsed)
}

fn unquote(value: &str) -> &str {
    let bytes = value.as_bytes();
    if value.len() >= 2
        && ((bytes[0] == b'"' && bytes[bytes.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[bytes.len() - 1] == b'\''))
    {
        &value[1..value.len() - 1]
    } else {
        value
    }
}

fn push_unique(list: &mut Vec<String>, value: &str) {
    if list.iter().any(|item| item.eq_ignore_ascii_case(value)) {
        return;
    }
    list.push(value.to_string());
}

fn push_spec(list: &mut Vec<RepoSpec>, spec: RepoSpec) {
    if list
        .iter()
        .any(|item| item.id().eq_ignore_ascii_case(&spec.id()))
    {
        return;
    }
    list.push(spec);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flags_from(args: &[&str]) -> Flags {
        parse_flags(args.iter().map(|arg| (*arg).to_string())).unwrap()
    }

    #[test]
    fn defaults_to_storytold_and_the_artcraft_title() {
        let resolved = resolve(&Flags::default(), None);
        assert_eq!(resolved.owners, vec!["storytold".to_string()]);
        assert_eq!(resolved.title, "Artcraft Repos");
        assert_eq!(resolved.protocol, Protocol::Auto);
        assert!(resolved.pinned.is_empty());
    }

    #[test]
    fn file_owner_replaces_the_default() {
        let file = parse_config(
            "\u{feff}# workspace\n\nowner = my-user\nprotocol = ssh\ntitle = \"My Tools\"\n",
        )
        .unwrap();
        let resolved = resolve(&Flags::default(), Some(file));
        assert_eq!(resolved.owners, vec!["my-user".to_string()]);
        assert_eq!(resolved.protocol, Protocol::Ssh);
        assert_eq!(resolved.title, "My Tools");
    }

    #[test]
    fn several_owners_use_a_generic_title() {
        let file = parse_config("owner = alpha\nowner = beta\nowner = ALPHA\n").unwrap();
        let resolved = resolve(&Flags::default(), Some(file));
        assert_eq!(
            resolved.owners,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(resolved.title, "GitHub Repos");
    }

    #[test]
    fn no_owner_clears_a_configured_catalog() {
        let flags = Flags {
            no_owner: true,
            ..Flags::default()
        };
        let file = parse_config("owner = my-user\nrepo = someone/tool\n").unwrap();
        let resolved = resolve(&flags, Some(file));
        assert!(resolved.owners.is_empty());
        assert_eq!(resolved.pinned[0].id(), "someone/tool");
        assert_eq!(resolved.title, "GitHub Repos");
    }

    #[test]
    fn cli_owner_and_protocol_replace_the_file() {
        let flags = Flags {
            owners: vec!["cli".into()],
            owners_explicit: true,
            protocol: Some(Protocol::Https),
            title: Some("Desk".into()),
            ..Flags::default()
        };
        let file = parse_config("owner = file-user\nprotocol = ssh\ntitle = File\n").unwrap();
        let resolved = resolve(&flags, Some(file));
        assert_eq!(resolved.owners, vec!["cli".to_string()]);
        assert_eq!(resolved.protocol, Protocol::Https);
        assert_eq!(resolved.title, "Desk");
    }

    #[test]
    fn repo_flag_keeps_the_default_owner() {
        let flags = flags_from(&["--repo", "https://github.com/other/tool"]);
        let resolved = resolve(&flags, None);
        assert_eq!(resolved.owners, vec!["storytold".to_string()]);
        assert_eq!(resolved.title, "Artcraft Repos");
        assert_eq!(resolved.pinned[0].id(), "other/tool");
    }

    #[test]
    fn org_remains_an_alias_and_commas_split_owners() {
        let flags = flags_from(&["--org", "one, two"]);
        assert!(flags.owners_explicit);
        assert_eq!(flags.owners, vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn owner_flag_rejects_a_repository() {
        let err = parse_flags(["--owner", "ada/tool"].into_iter().map(str::to_string)).unwrap_err();
        assert!(err.contains("--repo"));
    }

    #[test]
    fn config_rejects_unknown_keys_and_slashes_in_owners() {
        assert!(
            parse_config("theme = dark\n")
                .unwrap_err()
                .contains("unknown")
        );
        assert!(
            parse_config("owner = ada/tool\n")
                .unwrap_err()
                .contains("repo =")
        );
        assert!(parse_config("repo = not a repo\n").is_err());
        assert!(parse_config("protocol = ftp\n").is_err());
    }
}
