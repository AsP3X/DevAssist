use std::fs;
use std::path::{Path, PathBuf};

use crate::keys::KeyCombo;
use crate::repos::github::{self, RepoSpec};
use crate::repos::gitops::{self, Protocol};
use crate::sys::paths::{env_dir, home_dir, push_path, same_path};

/// The app's name: the default heading and the settings folder.
pub const APP_NAME: &str = "DevAssist";

/// The settings file's name, wherever it lives.
const FILE_NAME: &str = "devassist.toml";
/// What the settings file and its folder were called before the app was
/// renamed. Settings found under the old name are taken over once.
const OLD_FILE_NAME: &str = "artcraft-repos.toml";
const OLD_DIR_NAME: &str = "artcraft-repos";

/// Minutes between automatic pulls of the watched checkouts.
pub const DEFAULT_WATCH_INTERVAL: u32 = 5;
/// The longest interval: one day.
pub const MAX_WATCH_INTERVAL: u32 = 1440;

/// Whether the window is light or dark, or follows the system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Light,
    Dark,
    System,
}

impl Theme {
    fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::System => "system",
        }
    }
}

/// The picture behind the window, by name.
pub const BACKDROPS: [&str; 6] = ["lake", "midday", "golden", "dusk", "night", "forest"];

/// How the window looks. The terminal app keeps these settings and does
/// not use them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Look {
    pub theme: Theme,
    pub backdrop: String,
    pub blur: bool,
}

impl Default for Look {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            backdrop: "lake".to_string(),
            blur: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub title: String,
    /// The title the user chose. None means the title follows the accounts.
    pub custom_title: Option<String>,
    pub owners: Vec<String>,
    /// The accounts were chosen, in the file or in the settings dialog.
    /// Otherwise they follow the checkouts and are not written down.
    pub owners_explicit: bool,
    pub pinned: Vec<RepoSpec>,
    pub protocol: Protocol,
    pub doctor: bool,
    /// Every checkout the overview manages, wherever it is on disk.
    pub checkouts: Vec<PathBuf>,
    /// The folder the Add dialog offers for a new clone.
    pub clone_dir: PathBuf,
    /// Which group of the overview each checkout is in. Groups only sort
    /// the list: they are not folders on disk.
    pub placement: Vec<(PathBuf, String)>,
    /// Groups that are folded shut.
    pub collapsed: Vec<String>,
    /// Look for a newer version at startup. Saved in the settings file.
    pub update_check: bool,
    /// Checkouts that are pulled on a timer while the program runs.
    pub watched: Vec<PathBuf>,
    /// Minutes between those pulls.
    pub watch_interval: u32,
    /// Every fetch also saves the commits of all pull requests.
    pub pull_requests: bool,
    /// What the shell itself is set to.
    pub shell: ShellPrefs,
    /// How the window looks.
    pub look: Look,
    /// False when this start was told not to look, with --no-update-check.
    pub update_check_now: bool,
    /// Where the settings are saved.
    pub config_path: PathBuf,
    /// No settings file exists yet and no flag chose the accounts.
    pub first_run: bool,
    /// The checkouts changed at startup and are not in the file yet.
    pub unsaved: bool,
}

/// The settings of the shell: they are about no tool in particular.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShellPrefs {
    /// The key that opens the palette.
    pub palette_key: KeyCombo,
    /// Where the program starts: `home`, or a tool's name in lowercase.
    pub start: String,
}

impl Default for ShellPrefs {
    fn default() -> Self {
        Self {
            palette_key: KeyCombo::default(),
            start: "home".to_string(),
        }
    }
}

/// What is written to the settings file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub title: Option<String>,
    pub owners: Vec<String>,
    /// False leaves the accounts out, so they keep following the checkouts.
    pub owners_set: bool,
    pub pinned: Vec<RepoSpec>,
    pub protocol: Protocol,
    pub clone_dir: Option<PathBuf>,
    pub checkouts: Vec<PathBuf>,
    pub placement: Vec<(PathBuf, String)>,
    pub collapsed: Vec<String>,
    pub update_check: bool,
    /// Checkouts that are pulled on a timer while the program runs.
    pub watched: Vec<PathBuf>,
    /// Minutes between those pulls.
    pub watch_interval: u32,
    /// Every fetch also saves the commits of all pull requests.
    pub pull_requests: bool,
    /// What the shell itself is set to.
    pub shell: ShellPrefs,
    /// How the window looks.
    pub look: Look,
}

pub enum Startup {
    Help,
    Run(Box<Settings>),
}

#[derive(Debug, Default)]
struct Flags {
    help: bool,
    doctor: bool,
    import: Vec<PathBuf>,
    config: Option<PathBuf>,
    no_owner: bool,
    no_update_check: bool,
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
    clone_dir: Option<PathBuf>,
    checkouts: Vec<PathBuf>,
    placement: Vec<(PathBuf, String)>,
    collapsed: Vec<String>,
    update_check: Option<bool>,
    watched: Vec<PathBuf>,
    watch_interval: Option<u32>,
    pull_requests: Option<bool>,
    palette_key: Option<KeyCombo>,
    start: Option<String>,
    /// Set when any of theme, backdrop or blur is in the file.
    look: Option<Look>,
}

/// The settings file as found at startup.
struct Loaded {
    file: Option<ParsedConfig>,
    /// Where the settings are saved from now on.
    path: PathBuf,
    /// The folder older versions managed. Its clones come along once.
    legacy_root: Option<PathBuf>,
    /// The settings came from an older location.
    migrated: bool,
}

struct Resolved {
    title: String,
    custom_title: Option<String>,
    owners: Vec<String>,
    pinned: Vec<RepoSpec>,
    protocol: Protocol,
}

pub fn startup() -> Result<Startup, String> {
    let flags = parse_flags(std::env::args().skip(1))?;
    if flags.help {
        return Ok(Startup::Help);
    }
    for folder in &flags.import {
        if !folder.is_dir() {
            return Err(format!("{} is not a folder", folder.display()));
        }
    }
    let loaded = load_file(&flags)?;
    let first_run = loaded.file.is_none() && !flags.owners_explicit && !flags.no_owner;
    let stored = saved_from(loaded.file.as_ref().unwrap_or(&ParsedConfig::default()));
    let mut checkouts = stored.checkouts;
    let known = checkouts.len();
    let mut folders: Vec<PathBuf> = flags
        .import
        .iter()
        .map(|folder| std::path::absolute(folder).unwrap_or_else(|_| folder.clone()))
        .collect();
    folders.extend(loaded.legacy_root.clone());
    for folder in &folders {
        for repo in gitops::scan(folder) {
            push_path(&mut checkouts, repo.path);
        }
    }
    let clone_dir = stored
        .clone_dir
        .or_else(|| flags.import.first().and(folders.first().cloned()))
        .or_else(|| {
            checkouts
                .first()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .or_else(home_dir)
        .unwrap_or_else(|| PathBuf::from("."));
    let owners_explicit = stored.owners_set;
    let unsaved = checkouts.len() != known || loaded.migrated;
    let placement = stored.placement;
    let collapsed = stored.collapsed;
    let update_check = stored.update_check;
    // A watched path that is no longer on the list is dropped.
    let watched: Vec<PathBuf> = stored
        .watched
        .into_iter()
        .filter(|path| checkouts.iter().any(|known| same_path(known, path)))
        .collect();
    let watch_interval = stored.watch_interval;
    let pull_requests = stored.pull_requests;
    let shell = stored.shell;
    let look = stored.look;
    let discovered = discovered_owners(&checkouts);
    let resolved = resolve(&flags, loaded.file, &discovered);
    Ok(Startup::Run(Box::new(Settings {
        title: resolved.title,
        custom_title: resolved.custom_title,
        owners: resolved.owners,
        owners_explicit,
        pinned: resolved.pinned,
        protocol: resolved.protocol,
        doctor: flags.doctor,
        checkouts,
        clone_dir,
        placement,
        collapsed,
        update_check,
        watched,
        watch_interval,
        pull_requests,
        shell,
        look,
        update_check_now: update_check && !flags.no_update_check,
        config_path: loaded.path,
        first_run,
        unsaved,
    })))
}

pub fn print_help() {
    println!(
        "\
DevAssist
  Add, update, and remove local GitHub clones, wherever they are on disk.

Usage
  devassist [--owner NAME]... [--repo OWNER/NAME]... [--import PATH]...
  devassist --doctor
  devassist --help

The app can be started from any folder. It keeps a list of the clones it
manages in its settings file. Adding a repository asks where to put it.

Options
  --import PATH      Add every GitHub clone directly inside PATH to the
                     list. --root is the same option.
  --owner NAME       GitHub user or organization to list. Repeat to track
                     several. --org is the same option. Default: your signed-in
                     account when the clones use several, otherwise the one
                     account they already use.
  --no-owner         Do not list a user or organization. Add repositories by
                     owner/name from the Add screen instead.
  --repo OWNER/NAME  Pin one repository. A GitHub URL is also accepted.
  --protocol MODE    auto, https, or ssh. auto follows the remotes of the
                     clones already listed, otherwise https.
  --title TEXT       Heading shown in the app.
  --config PATH      Settings file. Default: devassist.toml in
                     %APPDATA%\\DevAssist on Windows, in
                     ~/.config/devassist elsewhere. A file an older
                     version kept in the clone folder or beside the
                     executable is taken over once, with that folder's clones.
  --no-update-check  Do not look for a newer version at this start. Put
                     update_check = off in the settings file to never look.
  --doctor           Print the settings and the GitHub check, then exit.

Settings file
  title = My Tools
  owner = my-user
  owner = my-org
  repo = someone/tool
  protocol = ssh
  clone_dir = C:\\dev
  checkout = C:\\dev\\tool
  group = Client work
  checkout = D:\\work\\client\\api

  Checkouts after a group line are in that group. Groups only sort the
  overview: nothing moves on disk.

  owners = none lists no account. Lines starting with # are comments.
  The app rewrites this file when the settings or the list change.

Keys
  a add      u update         U update all      d remove or forget
  i import   r refresh        s sort            o open folder
  c settings l last update or clone             q quit
  v version: look for an update and install it

Drag across text and release to copy it. Ctrl-C copies a selection,
or quits when nothing is selected. Esc clears a selection.
In Add, type owner/repo or paste a GitHub URL to clone any repository.
Type owner/ or paste a link to an account to list all of its repositories.
Space selects several repositories there. Enter asks for the folder.
Import adds clones you already have, from any folder.
Starting and Refresh fetch remotes. Update fast-forwards every branch
and adds a local branch for each new remote one. It never merges.
Esc also cancels the git command in progress.
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
            "--no-update-check" => flags.no_update_check = true,
            "--import" | "--root" => flags
                .import
                .push(PathBuf::from(need(&mut args, "--import")?)),
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

fn discovered_owners(checkouts: &[PathBuf]) -> Vec<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for repo in gitops::load(checkouts) {
        if repo.owner.is_empty() {
            continue;
        }
        if let Some((_, count)) = counts
            .iter_mut()
            .find(|(owner, _)| owner.eq_ignore_ascii_case(&repo.owner))
        {
            *count += 1;
        } else {
            counts.push((repo.owner, 1));
        }
    }
    let login = if counts.len() > 1 {
        github::current_login()
    } else {
        None
    };
    choose_accounts(&counts, login.as_deref())
}

/// One account is that account. Several accounts use the signed-in user,
/// so a cloned dependency does not become a second catalog.
fn choose_accounts(counts: &[(String, usize)], login: Option<&str>) -> Vec<String> {
    if counts.len() <= 1 {
        return counts.iter().map(|(owner, _)| owner.clone()).collect();
    }
    if let Some(login) = login {
        if let Some((owner, _)) = counts
            .iter()
            .find(|(owner, _)| owner.eq_ignore_ascii_case(login))
        {
            return vec![owner.clone()];
        }
        return vec![login.to_string()];
    }
    let mut ranked = counts.to_vec();
    ranked.sort_by(|left, right| {
        right.1.cmp(&left.1).then_with(|| {
            left.0
                .to_ascii_lowercase()
                .cmp(&right.0.to_ascii_lowercase())
        })
    });
    let most = ranked[0].1;
    if ranked.iter().filter(|(_, count)| *count == most).count() == 1 {
        vec![ranked[0].0.clone()]
    } else {
        Vec::new()
    }
}

fn resolve(flags: &Flags, file: Option<ParsedConfig>, discovered: &[String]) -> Resolved {
    let mut owners = discovered.to_vec();
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
    let custom_title = title.clone();
    let title = title.unwrap_or_else(default_title);
    Resolved {
        title,
        custom_title,
        owners,
        pinned,
        protocol,
    }
}

/// The heading when the user has not chosen one.
pub fn default_title() -> String {
    APP_NAME.to_string()
}

/// Finds the settings file. It lives in the user's own settings folder, so
/// the app does not care where it is started from or where it is installed.
fn load_file(flags: &Flags) -> Result<Loaded, String> {
    if let Some(path) = &flags.config {
        let path = std::path::absolute(path).unwrap_or_else(|_| path.clone());
        let file = if path.is_file() {
            Some(read_config(&path)?)
        } else {
            None
        };
        return Ok(Loaded {
            file,
            path,
            legacy_root: None,
            migrated: false,
        });
    }
    let path = user_config_path();
    if path.is_file() {
        return Ok(Loaded {
            file: Some(read_config(&path)?),
            path,
            legacy_root: None,
            migrated: false,
        });
    }
    // The settings of the app under its old name move to the new place.
    if let Some(old) = user_config_dir()
        .map(|base| base.join(OLD_DIR_NAME).join(OLD_FILE_NAME))
        .filter(|old| old.is_file())
    {
        return Ok(Loaded {
            file: Some(read_config(&old)?),
            path,
            legacy_root: None,
            migrated: true,
        });
    }
    // Older versions managed one folder and kept the settings in it, or
    // beside the executable. Take both over once.
    let root = gitops::default_root();
    let legacy = [
        Some(root.join(OLD_FILE_NAME)),
        exe_dir().map(|dir| dir.join(OLD_FILE_NAME)),
    ]
    .into_iter()
    .flatten()
    .find(|file| file.is_file());
    let file = match &legacy {
        Some(file) => Some(read_config(file)?),
        None => None,
    };
    Ok(Loaded {
        migrated: file.is_some(),
        file,
        path,
        legacy_root: Some(root),
    })
}

/// The settings a file holds, with the defaults for what it leaves out.
fn saved_from(file: &ParsedConfig) -> Saved {
    let defaults = ShellPrefs::default();
    Saved {
        title: file.title.clone(),
        owners: file.owners.clone(),
        owners_set: file.owners_explicit,
        pinned: file.pinned.clone(),
        protocol: file.protocol.unwrap_or(Protocol::Auto),
        clone_dir: file.clone_dir.clone(),
        checkouts: file.checkouts.clone(),
        placement: file.placement.clone(),
        collapsed: file.collapsed.clone(),
        update_check: file.update_check.unwrap_or(true),
        watched: file.watched.clone(),
        watch_interval: file.watch_interval.unwrap_or(DEFAULT_WATCH_INTERVAL),
        pull_requests: file.pull_requests.unwrap_or(false),
        shell: ShellPrefs {
            palette_key: file.palette_key.unwrap_or(defaults.palette_key),
            start: file.start.clone().unwrap_or(defaults.start),
        },
        look: file.look.clone().unwrap_or_default(),
    }
}

fn read_config(path: &Path) -> Result<ParsedConfig, String> {
    let text = fs::read_to_string(path)
        .map_err(|err| format!("could not read {}: {err}", path.display()))?;
    parse_config(&text).map_err(|err| format!("{}: {err}", path.display()))
}

/// The folder programs keep their per-user settings in.
fn user_config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env_dir("APPDATA")
    } else {
        env_dir("XDG_CONFIG_HOME").or_else(|| home_dir().map(|home| home.join(".config")))
    }
    .or_else(home_dir)
}

/// %APPDATA%\DevAssist on Windows, ~/.config/devassist elsewhere.
fn user_config_path() -> PathBuf {
    let folder = if cfg!(windows) {
        APP_NAME.to_string()
    } else {
        APP_NAME.to_lowercase()
    };
    match user_config_dir() {
        Some(base) => base.join(folder).join(FILE_NAME),
        None => exe_dir().unwrap_or_default().join(FILE_NAME),
    }
}

pub fn save(path: &Path, saved: &Saved) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|err| format!("could not create {}: {err}", parent.display()))?;
    }
    fs::write(path, render(saved))
        .map_err(|err| format!("could not save {}: {err}", path.display()))
}

/// Writes the settings this program changed since `before` and keeps
/// every other setting as the file has it now, so a change another
/// DevAssist made in the meantime stays. A file that cannot be read or
/// parsed is written over with `after` whole.
pub fn save_changes(path: &Path, before: &Saved, after: &Saved) -> Result<(), String> {
    match read_config(path) {
        Ok(file) => save(path, &merge(before, after, saved_from(&file))),
        Err(_) => save(path, after),
    }
}

/// `after` where it differs from `before`, otherwise `file`. Every field
/// is named, so a new one does not compile until it is merged here too.
fn merge(before: &Saved, after: &Saved, file: Saved) -> Saved {
    fn pick<T: Clone + PartialEq>(before: &T, after: &T, file: T) -> T {
        if after == before { file } else { after.clone() }
    }
    let Saved {
        title,
        owners,
        owners_set,
        pinned,
        protocol,
        clone_dir,
        checkouts,
        placement,
        collapsed,
        update_check,
        watched,
        watch_interval,
        pull_requests,
        shell,
        look,
    } = file;
    // The accounts and whether they were chosen are one setting: taken
    // apart, accounts confirmed unchanged would be written as none.
    let (owners, owners_set) = pick(
        &(&before.owners, before.owners_set),
        &(&after.owners, after.owners_set),
        (&owners, owners_set),
    );
    let owners = owners.clone();
    Saved {
        title: pick(&before.title, &after.title, title),
        owners,
        owners_set,
        pinned: pick(&before.pinned, &after.pinned, pinned),
        protocol: pick(&before.protocol, &after.protocol, protocol),
        clone_dir: pick(&before.clone_dir, &after.clone_dir, clone_dir),
        checkouts: pick(&before.checkouts, &after.checkouts, checkouts),
        placement: pick(&before.placement, &after.placement, placement),
        collapsed: pick(&before.collapsed, &after.collapsed, collapsed),
        update_check: pick(&before.update_check, &after.update_check, update_check),
        watched: pick(&before.watched, &after.watched, watched),
        watch_interval: pick(
            &before.watch_interval,
            &after.watch_interval,
            watch_interval,
        ),
        pull_requests: pick(&before.pull_requests, &after.pull_requests, pull_requests),
        shell: pick(&before.shell, &after.shell, shell),
        look: pick(&before.look, &after.look, look),
    }
}

fn render(saved: &Saved) -> String {
    let mut out = String::from("# devassist settings. Edit this file, or press c in the app.\n");
    if let Some(title) = &saved.title {
        out.push_str(&format!("title = {title}\n"));
    }
    if saved.owners_set {
        if saved.owners.is_empty() {
            out.push_str("owners = none\n");
        }
        for owner in &saved.owners {
            out.push_str(&format!("owner = {owner}\n"));
        }
    }
    for spec in &saved.pinned {
        out.push_str(&format!("repo = {}\n", spec.id()));
    }
    out.push_str(&format!("protocol = {}\n", saved.protocol));
    if let Some(dir) = &saved.clone_dir {
        out.push_str(&format!("clone_dir = {}\n", dir.display()));
    }
    // Checkouts without a group come first; each group follows as a
    // `group =` line with its checkouts under it.
    let group_of = |checkout: &Path| {
        saved
            .placement
            .iter()
            .find(|(path, _)| same_path(path, checkout))
            .map(|(_, group)| group.as_str())
            .filter(|group| !group.trim().is_empty())
    };
    let mut groups: Vec<&str> = Vec::new();
    for checkout in &saved.checkouts {
        match group_of(checkout) {
            Some(group) if !groups.contains(&group) => groups.push(group),
            Some(_) => {}
            None => out.push_str(&format!("checkout = {}\n", checkout.display())),
        }
    }
    groups.sort_by_key(|group| group.to_lowercase());
    for group in groups {
        out.push_str(&format!("group = {group}\n"));
        for checkout in &saved.checkouts {
            if group_of(checkout) == Some(group) {
                out.push_str(&format!("checkout = {}\n", checkout.display()));
            }
        }
    }
    for group in &saved.collapsed {
        out.push_str(&format!("collapsed = {group}\n"));
    }
    if !saved.update_check {
        out.push_str("update_check = off\n");
    }
    for path in &saved.watched {
        out.push_str(&format!("watch = {}\n", path.display()));
    }
    if saved.watch_interval != DEFAULT_WATCH_INTERVAL {
        out.push_str(&format!("watch_interval = {}\n", saved.watch_interval));
    }
    if saved.pull_requests {
        out.push_str("pull_requests = on\n");
    }
    let defaults = ShellPrefs::default();
    if saved.shell.palette_key != defaults.palette_key {
        out.push_str(&format!(
            "palette_key = {}\n",
            saved.shell.palette_key.label()
        ));
    }
    if saved.shell.start != defaults.start {
        out.push_str(&format!("start = {}\n", saved.shell.start));
    }
    let look = Look::default();
    if saved.look.theme != look.theme {
        out.push_str(&format!("theme = {}\n", saved.look.theme.name()));
    }
    if saved.look.backdrop != look.backdrop {
        out.push_str(&format!("backdrop = {}\n", saved.look.backdrop));
    }
    if saved.look.blur != look.blur {
        out.push_str(&format!(
            "blur = {}\n",
            if saved.look.blur { "on" } else { "off" }
        ));
    }
    out
}

/// Minutes between automatic pulls, as typed: a whole number from 1 up to
/// a day. Empty means the default.
pub fn parse_watch_interval(text: &str) -> Result<u32, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(DEFAULT_WATCH_INTERVAL);
    }
    match text.parse::<u32>() {
        Ok(minutes) if (1..=MAX_WATCH_INTERVAL).contains(&minutes) => Ok(minutes),
        _ => Err(format!(
            "The interval is a number of minutes from 1 to {MAX_WATCH_INTERVAL}."
        )),
    }
}

/// Accounts typed into the settings dialog, separated by commas or spaces.
/// A link to an account or `@name` is accepted too.
pub fn parse_owner_list(text: &str) -> Result<Vec<String>, String> {
    let mut owners = Vec::new();
    for part in split_list(text) {
        let name = match github::parse_account_query(part) {
            Some(query) if query.rest.is_empty() => query.owner,
            Some(_) => {
                return Err(format!(
                    "{part} is a repository. Put it under Pinned repos."
                ));
            }
            None if github::valid_account(part) => part.to_string(),
            None => return Err(format!("{part} is not a GitHub account name.")),
        };
        push_unique(&mut owners, &name);
    }
    Ok(owners)
}

/// Repositories typed into the settings dialog as owner/name or links.
pub fn parse_pinned_list(text: &str) -> Result<Vec<RepoSpec>, String> {
    let mut pinned = Vec::new();
    for part in split_list(text) {
        let spec = github::parse_repo_spec(part)
            .ok_or_else(|| format!("{part} should look like owner/name."))?;
        push_spec(&mut pinned, spec);
    }
    Ok(pinned)
}

fn split_list(text: &str) -> impl Iterator<Item = &str> {
    text.split(|ch: char| ch == ',' || ch == ';' || ch.is_whitespace())
        .map(str::trim)
        .filter(|part| !part.is_empty())
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
}

fn parse_config(text: &str) -> Result<ParsedConfig, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut parsed = ParsedConfig::default();
    let mut no_owners = false;
    // The `group =` line the following checkouts belong to.
    let mut group: Option<String> = None;
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
        // The repositories in no group are a group with an empty name, so
        // folding them writes `collapsed =` with nothing after it.
        if value.is_empty() && key != "collapsed" {
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
            "owners" => {
                if !value.eq_ignore_ascii_case("none") {
                    return Err(format!(
                        "line {line_no}: owners only accepts none. Use owner = name for an account"
                    ));
                }
                no_owners = true;
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
            "clone_dir" => parsed.clone_dir = Some(PathBuf::from(value)),
            "group" => group = Some(value.to_string()),
            "checkout" => {
                let path = PathBuf::from(value);
                if let Some(group) = &group {
                    parsed
                        .placement
                        .retain(|(known, _)| !same_path(known, &path));
                    parsed.placement.push((path.clone(), group.clone()));
                }
                push_path(&mut parsed.checkouts, path);
            }
            "collapsed" => push_unique(&mut parsed.collapsed, value),
            "update_check" => {
                parsed.update_check = Some(match value.to_ascii_lowercase().as_str() {
                    "on" | "yes" | "true" => true,
                    "off" | "no" | "false" => false,
                    _ => return Err(format!("line {line_no}: update_check is on or off")),
                });
            }
            "watch" => {
                push_path(&mut parsed.watched, PathBuf::from(value));
            }
            "watch_interval" => {
                parsed.watch_interval = Some(
                    parse_watch_interval(value)
                        .map_err(|err| format!("line {line_no}: watch_interval: {err}"))?,
                );
            }
            "pull_requests" => {
                parsed.pull_requests = Some(match value.to_ascii_lowercase().as_str() {
                    "on" | "yes" | "true" => true,
                    "off" | "no" | "false" => false,
                    _ => return Err(format!("line {line_no}: pull_requests is on or off")),
                });
            }
            "palette_key" => {
                parsed.palette_key = Some(
                    KeyCombo::parse(value)
                        .map_err(|err| format!("line {line_no}: palette_key: {err}"))?,
                );
            }
            "start" => {
                let place = value.trim().to_lowercase();
                if place.is_empty() {
                    return Err(format!(
                        "line {line_no}: start is home or the name of a tool"
                    ));
                }
                parsed.start = Some(place);
            }
            "theme" => {
                parsed.look.get_or_insert_with(Look::default).theme =
                    match value.to_ascii_lowercase().as_str() {
                        "light" => Theme::Light,
                        "dark" => Theme::Dark,
                        "system" => Theme::System,
                        _ => return Err(format!("line {line_no}: theme is light, dark or system")),
                    };
            }
            "backdrop" => {
                let name = value.to_ascii_lowercase();
                if !BACKDROPS.contains(&name.as_str()) {
                    return Err(format!(
                        "line {line_no}: backdrop is lake, midday, golden, dusk, night or forest"
                    ));
                }
                parsed.look.get_or_insert_with(Look::default).backdrop = name;
            }
            "blur" => {
                parsed.look.get_or_insert_with(Look::default).blur =
                    match value.to_ascii_lowercase().as_str() {
                        "on" | "yes" | "true" => true,
                        "off" | "no" | "false" => false,
                        _ => return Err(format!("line {line_no}: blur is on or off")),
                    };
            }
            other => return Err(format!("line {line_no}: unknown setting {other}")),
        }
    }
    if no_owners && !parsed.owners.is_empty() {
        return Err("owners = none cannot be combined with owner lines".into());
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
    fn an_empty_folder_has_no_preset_account() {
        let resolved = resolve(&Flags::default(), None, &[]);
        assert!(resolved.owners.is_empty());
        assert_eq!(resolved.title, "DevAssist");
        assert_eq!(resolved.protocol, Protocol::Auto);
        assert!(resolved.pinned.is_empty());
    }

    #[test]
    fn owners_come_from_the_checkouts_and_the_title_is_the_app_name() {
        let resolved = resolve(&Flags::default(), None, &["storytold".into()]);
        assert_eq!(resolved.owners, vec!["storytold".to_string()]);
        assert_eq!(resolved.title, "DevAssist");

        let resolved = resolve(
            &Flags::default(),
            None,
            &["ahwootech".into(), "AsP3X".into()],
        );
        assert_eq!(
            resolved.owners,
            vec!["ahwootech".to_string(), "AsP3X".to_string()]
        );
        assert_eq!(resolved.title, "DevAssist");
    }

    #[test]
    fn several_accounts_keep_only_the_signed_in_one() {
        let counts = vec![("ahwootech".to_string(), 1), ("AsP3X".to_string(), 2)];
        assert_eq!(
            choose_accounts(&counts, Some("AsP3X")),
            vec!["AsP3X".to_string()]
        );
        assert_eq!(choose_accounts(&counts, None), vec!["AsP3X".to_string()]);
        let tied = vec![("ahwootech".to_string(), 1), ("AsP3X".to_string(), 1)];
        assert!(choose_accounts(&tied, None).is_empty());
        assert_eq!(
            choose_accounts(&tied, Some("AsP3X")),
            vec!["AsP3X".to_string()]
        );
    }

    #[test]
    fn file_owner_replaces_the_default() {
        let file = parse_config(
            "\u{feff}# workspace\n\nowner = my-user\nprotocol = ssh\ntitle = \"My Tools\"\n",
        )
        .unwrap();
        let resolved = resolve(&Flags::default(), Some(file), &["storytold".into()]);
        assert_eq!(resolved.owners, vec!["my-user".to_string()]);
        assert_eq!(resolved.protocol, Protocol::Ssh);
        assert_eq!(resolved.title, "My Tools");
    }

    #[test]
    fn several_owners_keep_the_app_name_as_title() {
        let file = parse_config("owner = alpha\nowner = beta\nowner = ALPHA\n").unwrap();
        let resolved = resolve(&Flags::default(), Some(file), &["storytold".into()]);
        assert_eq!(
            resolved.owners,
            vec!["alpha".to_string(), "beta".to_string()]
        );
        assert_eq!(resolved.title, "DevAssist");
    }

    #[test]
    fn no_owner_clears_a_configured_catalog() {
        let flags = Flags {
            no_owner: true,
            ..Flags::default()
        };
        let file = parse_config("owner = my-user\nrepo = someone/tool\n").unwrap();
        let resolved = resolve(&flags, Some(file), &["storytold".into()]);
        assert!(resolved.owners.is_empty());
        assert_eq!(resolved.pinned[0].id(), "someone/tool");
        assert_eq!(resolved.title, "DevAssist");
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
        let resolved = resolve(&flags, Some(file), &["storytold".into()]);
        assert_eq!(resolved.owners, vec!["cli".to_string()]);
        assert_eq!(resolved.protocol, Protocol::Https);
        assert_eq!(resolved.title, "Desk");
    }

    #[test]
    fn repo_flag_keeps_the_accounts_found_in_the_folder() {
        let flags = flags_from(&["--repo", "https://github.com/other/tool"]);
        let resolved = resolve(&flags, None, &["AsP3X".into()]);
        assert_eq!(resolved.owners, vec!["AsP3X".to_string()]);
        assert_eq!(resolved.title, "DevAssist");
        assert_eq!(resolved.pinned[0].id(), "other/tool");
    }

    #[test]
    fn saved_settings_read_back_the_same() {
        let saved = Saved {
            title: Some("My Tools".into()),
            owners: vec!["AsP3X".into(), "UCS-Care-Group".into()],
            pinned: vec![RepoSpec {
                owner: "someone".into(),
                name: "tool".into(),
            }],
            owners_set: true,
            protocol: Protocol::Ssh,
            clone_dir: Some(PathBuf::from("C:\\dev")),
            checkouts: vec![
                PathBuf::from("C:\\dev\\tool"),
                PathBuf::from("D:\\work = client\\api"),
            ],
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        let parsed = parse_config(&render(&saved)).unwrap();
        assert_eq!(parsed.title, saved.title);
        assert_eq!(parsed.owners, saved.owners);
        assert!(parsed.owners_explicit);
        assert_eq!(parsed.pinned, saved.pinned);
        assert_eq!(parsed.protocol, Some(Protocol::Ssh));
        assert_eq!(parsed.clone_dir, saved.clone_dir);
        assert_eq!(parsed.checkouts, saved.checkouts);
    }

    #[test]
    fn accounts_that_were_never_chosen_are_not_written_down() {
        let saved = Saved {
            title: None,
            owners: vec!["AsP3X".into()],
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: vec![PathBuf::from("/srv/code/api")],
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        let parsed = parse_config(&render(&saved)).unwrap();
        assert!(!parsed.owners_explicit);
        assert!(parsed.owners.is_empty());
        assert_eq!(parsed.checkouts, saved.checkouts);
    }

    #[test]
    fn a_checkout_is_listed_once_and_import_is_a_flag() {
        let mut list = vec![PathBuf::from("/srv/code/api")];
        assert!(!push_path(&mut list, PathBuf::from("/srv/code/api/")));
        assert!(push_path(&mut list, PathBuf::from("/srv/code/web")));
        assert_eq!(list.len(), 2);
        assert!(same_path(Path::new("a\\b"), Path::new("a/b/")));
        assert_eq!(
            same_path(Path::new("/A/b"), Path::new("/a/b")),
            cfg!(windows)
        );

        let flags = flags_from(&["--import", "one", "--root", "two"]);
        assert_eq!(
            flags.import,
            vec![PathBuf::from("one"), PathBuf::from("two")]
        );
        let parsed = parse_config("checkout = /a\ncheckout = /a/\ncheckout = /b\n").unwrap();
        assert_eq!(parsed.checkouts.len(), 2);
    }

    #[test]
    fn groups_are_written_as_sections_and_read_back() {
        let path = |name: &str| PathBuf::from("/srv").join(name);
        let saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: vec![path("api"), path("loose"), path("notes"), path("web")],
            placement: vec![
                (path("web"), "Work".into()),
                (path("notes"), "home".into()),
                (path("api"), "Work".into()),
                // Not a listed checkout: it is not written.
                (path("gone"), "Old".into()),
            ],
            collapsed: vec!["home".into()],
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        let text = render(&saved);
        let lines: Vec<&str> = text.lines().skip(2).collect();
        assert_eq!(
            lines,
            [
                format!("checkout = {}", path("loose").display()),
                "group = home".to_string(),
                format!("checkout = {}", path("notes").display()),
                "group = Work".to_string(),
                format!("checkout = {}", path("api").display()),
                format!("checkout = {}", path("web").display()),
                "collapsed = home".to_string(),
            ]
        );
        let parsed = parse_config(&text).unwrap();
        assert_eq!(parsed.checkouts.len(), 4);
        assert_eq!(
            parsed.placement,
            vec![
                (path("notes"), "home".to_string()),
                (path("api"), "Work".to_string()),
                (path("web"), "Work".to_string()),
            ]
        );
        assert_eq!(parsed.collapsed, vec!["home".to_string()]);
    }

    #[test]
    fn folding_the_repositories_in_no_group_survives_a_restart() {
        // "No group" has an empty name, so its line has nothing after `=`.
        let saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: vec!["Work".into(), String::new()],
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        let text = render(&saved);
        assert!(text.lines().any(|line| line.trim_end() == "collapsed ="));
        let parsed = parse_config(&text).unwrap();
        assert_eq!(parsed.collapsed, vec!["Work".to_string(), String::new()]);

        // Any other setting still needs a value.
        assert_eq!(
            parse_config("title =").unwrap_err(),
            "line 1: title needs a value"
        );
    }

    #[test]
    fn the_palette_key_and_the_start_are_read_checked_and_written_back() {
        let parsed = parse_config("palette_key = Ctrl-K\nstart = Docker\n").unwrap();
        assert_eq!(parsed.palette_key, Some(KeyCombo::parse("ctrl-k").unwrap()));
        assert_eq!(parsed.start.as_deref(), Some("docker"));
        let nothing = parse_config("").unwrap();
        assert_eq!(nothing.palette_key, None);
        assert_eq!(nothing.start, None);

        // A key that cannot work is refused, with the line and the reason.
        let err = parse_config("title = x\npalette_key = p\n").unwrap_err();
        assert_eq!(
            err,
            "line 2: palette_key: a letter or digit needs ctrl or alt, such as ctrl-k"
        );
        assert!(parse_config("start =\n").is_err());

        // Saving keeps both; the defaults are left out of the file.
        let mut saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs {
                palette_key: KeyCombo::parse("alt-f2").unwrap(),
                start: "docker".to_string(),
            },
            look: Look::default(),
        };
        let text = render(&saved);
        assert!(text.contains("palette_key = alt-f2\n"), "{text}");
        assert!(text.contains("start = docker\n"), "{text}");
        let parsed = parse_config(&text).unwrap();
        assert_eq!(parsed.palette_key, Some(saved.shell.palette_key));
        saved.shell = ShellPrefs::default();
        let text = render(&saved);
        assert!(!text.contains("palette_key"));
        assert!(!text.contains("start ="));
    }

    #[test]
    fn watched_checkouts_and_their_interval_round_trip() {
        let parsed = parse_config(
            "checkout = /srv/code/api\nwatch = /srv/code/api\nwatch = /srv/code/api\nwatch_interval = 30\n",
        )
        .unwrap();
        assert_eq!(parsed.watched, vec![PathBuf::from("/srv/code/api")]);
        assert_eq!(parsed.watch_interval, Some(30));
        assert_eq!(parse_config("title = x\n").unwrap().watch_interval, None);
        assert!(parse_config("watch_interval = 0\n").is_err());
        assert!(parse_config("watch_interval = often\n").is_err());
        assert!(parse_config("watch_interval = 1441\n").is_err());

        let text = "checkout = /srv/code/api\nwatch = /srv/code/api\nwatch_interval = 30\n";
        let parsed = parse_config(text).unwrap();
        let saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: parsed.checkouts.clone(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: parsed.watched.clone(),
            watch_interval: 30,
            pull_requests: false,
            shell: Default::default(),
            look: Look::default(),
        };
        let again = parse_config(&render(&saved)).unwrap();
        assert_eq!(again.watched, parsed.watched);
        assert_eq!(again.watch_interval, Some(30));
        assert!(!render(&saved).contains("pull_requests"));
        assert_eq!(again.pull_requests, None);
        let backing_up = Saved {
            pull_requests: true,
            ..saved.clone()
        };
        let text = render(&backing_up);
        assert!(text.contains("pull_requests = on\n"));
        assert_eq!(parse_config(&text).unwrap().pull_requests, Some(true));
        assert!(parse_config("pull_requests = some\n").is_err());
        // The default interval is not written down.
        let quiet = Saved {
            watch_interval: DEFAULT_WATCH_INTERVAL,
            ..saved
        };
        assert!(!render(&quiet).contains("watch_interval"));
    }

    #[test]
    fn the_update_check_can_be_turned_off() {
        assert_eq!(
            parse_config("update_check = off\n").unwrap().update_check,
            Some(false)
        );
        assert_eq!(
            parse_config("update_check = Yes\n").unwrap().update_check,
            Some(true)
        );
        assert_eq!(parse_config("title = x\n").unwrap().update_check, None);
        assert!(parse_config("update_check = maybe\n").is_err());
        assert!(flags_from(&["--no-update-check"]).no_update_check);

        let mut saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        // On is the default and is not written down.
        assert!(!render(&saved).contains("update_check"));
        saved.update_check = false;
        assert!(render(&saved).contains("update_check = off\n"));
    }

    #[test]
    fn saving_no_accounts_stays_empty_instead_of_following_the_folder() {
        let saved = Saved {
            title: None,
            owners: Vec::new(),
            pinned: Vec::new(),
            owners_set: true,
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        };
        let parsed = parse_config(&render(&saved)).unwrap();
        let resolved = resolve(&Flags::default(), Some(parsed), &["storytold".into()]);
        assert!(resolved.owners.is_empty());
        assert!(resolved.custom_title.is_none());
        assert!(parse_config("owners = none\nowner = ada\n").is_err());
        assert!(parse_config("owners = ada\n").is_err());
    }

    #[test]
    fn the_settings_dialog_accepts_names_links_and_separators() {
        assert_eq!(
            parse_owner_list("AsP3X, https://github.com/UCS-Care-Group  @storytold asp3x").unwrap(),
            vec![
                "AsP3X".to_string(),
                "UCS-Care-Group".to_string(),
                "storytold".to_string()
            ]
        );
        assert!(parse_owner_list("").unwrap().is_empty());
        assert!(parse_owner_list("ada/tool").unwrap_err().contains("Pinned"));
        assert!(parse_owner_list("not!valid").is_err());
        let pinned = parse_pinned_list("someone/tool, https://github.com/other/thing").unwrap();
        assert_eq!(pinned[0].id(), "someone/tool");
        assert_eq!(pinned[1].id(), "other/thing");
        assert!(parse_pinned_list("justaname").is_err());
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
            parse_config("colour = blue\n")
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

    #[test]
    fn the_look_settings_are_read() {
        let parsed = parse_config("theme = dark\nbackdrop = golden\nblur = off\n").unwrap();
        assert_eq!(
            parsed.look,
            Some(Look {
                theme: Theme::Dark,
                backdrop: "golden".into(),
                blur: false
            })
        );
    }

    #[test]
    fn a_look_setting_that_is_not_allowed_names_what_is() {
        assert_eq!(
            parse_config("theme = purple\n").unwrap_err(),
            "line 1: theme is light, dark or system"
        );
        assert_eq!(
            parse_config("blur = maybe\n").unwrap_err(),
            "line 1: blur is on or off"
        );
        assert_eq!(
            parse_config("backdrop = moon\n").unwrap_err(),
            "line 1: backdrop is lake, midday, golden, dusk, night or forest"
        );
    }

    #[test]
    fn the_look_settings_survive_a_save() {
        let look = Look {
            theme: Theme::Light,
            backdrop: "night".into(),
            blur: false,
        };
        let saved = Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: look.clone(),
        };
        let path = std::env::temp_dir().join(format!(
            "devassist-look-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ));
        save(&path, &saved).unwrap();
        let read = read_config(&path);
        let _ = fs::remove_file(&path);
        assert_eq!(read.unwrap().look, Some(look));

        // The default look is not written down.
        let plain = Saved {
            look: Look::default(),
            ..saved
        };
        let text = render(&plain);
        assert!(!text.contains("theme") && !text.contains("backdrop") && !text.contains("blur"));
        assert_eq!(parse_config(&text).unwrap().look, None);
    }

    /// A settings file of its own for one test.
    fn temp_file(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "devassist-{name}-{}-{:?}.toml",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    /// One checkout in no group, watched every five minutes.
    fn one_checkout() -> Saved {
        Saved {
            title: None,
            owners: Vec::new(),
            owners_set: false,
            pinned: Vec::new(),
            protocol: Protocol::Auto,
            clone_dir: None,
            checkouts: vec![PathBuf::from("/srv/code/api")],
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: true,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            shell: ShellPrefs::default(),
            look: Look::default(),
        }
    }

    /// The file as it reads back now.
    fn read_back(path: &Path) -> Saved {
        let read = read_config(path);
        let _ = fs::remove_file(path);
        saved_from(&read.unwrap())
    }

    #[test]
    fn a_save_keeps_what_the_other_program_changed() {
        let path = temp_file("merge-other");
        let before = one_checkout();
        save(&path, &before).unwrap();
        // The other program changes the interval in the file.
        save(
            &path,
            &Saved {
                watch_interval: 30,
                ..before.clone()
            },
        )
        .unwrap();
        let after = Saved {
            placement: vec![(PathBuf::from("/srv/code/api"), "work".to_string())],
            ..before.clone()
        };
        save_changes(&path, &before, &after).unwrap();
        let read = read_back(&path);
        assert_eq!(read.watch_interval, 30);
        assert_eq!(read.placement, after.placement);
    }

    #[test]
    fn a_save_takes_the_persons_change_over_the_files() {
        let path = temp_file("merge-mine");
        let before = one_checkout();
        save(
            &path,
            &Saved {
                watch_interval: 30,
                ..before.clone()
            },
        )
        .unwrap();
        let after = Saved {
            watch_interval: 10,
            ..before.clone()
        };
        save_changes(&path, &before, &after).unwrap();
        assert_eq!(read_back(&path).watch_interval, 10);
    }

    #[test]
    fn a_save_keeps_the_accounts_together_with_whether_they_were_chosen() {
        // The accounts followed the checkouts and were not in the file.
        // The settings dialog confirms them unchanged: they are chosen now.
        let path = temp_file("merge-accounts");
        let before = Saved {
            owners: vec!["storytold".into()],
            ..one_checkout()
        };
        save(&path, &before).unwrap();
        let after = Saved {
            owners_set: true,
            ..before.clone()
        };
        save_changes(&path, &before, &after).unwrap();
        let read = read_back(&path);
        assert!(read.owners_set);
        assert_eq!(read.owners, vec!["storytold".to_string()]);
    }

    #[test]
    fn a_save_writes_everything_when_the_file_is_gone() {
        let path = temp_file("merge-gone");
        let _ = fs::remove_file(&path);
        let before = one_checkout();
        let after = Saved {
            watch_interval: 10,
            collapsed: vec!["work".into()],
            ..before.clone()
        };
        assert_eq!(save_changes(&path, &before, &after), Ok(()));
        assert_eq!(read_back(&path), after);
    }

    #[test]
    fn a_save_writes_everything_when_the_file_no_longer_parses() {
        let path = temp_file("merge-broken");
        fs::write(&path, "nonsense = 1\n").unwrap();
        let before = one_checkout();
        let after = Saved {
            title: Some("Desk".into()),
            ..before.clone()
        };
        assert_eq!(save_changes(&path, &before, &after), Ok(()));
        assert_eq!(read_back(&path), after);
    }
}
