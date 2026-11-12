use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::config::{self, Settings};
use crate::github::{self, AccountQuery, RemoteRepo};
use crate::gitops::{self, LocalRepo};
use crate::job::{Job, JobEvent};
use crate::pathprompt::{self, PathPrompt, Purpose};
use crate::select::{PointerEffect, PointerEvent};
use crate::settings::{Form, Outcome};
use crate::update::{self, Release};

/// How long the Add search waits after the last key before asking GitHub.
const LOOKUP_DELAY: Duration = Duration::from_millis(350);

/// How long the progress dialog stays after a run that went well.
/// Opening more folders than this at once is refused.
const OPEN_LIMIT: usize = 8;
const JOB_LINGER: Duration = Duration::from_millis(1200);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Add,
    UpdateOne,
    UpdateAll,
    Remove,
    Refresh,
    Quit,
    Confirm,
    Cancel,
    Clone,
    Retry,
    Settings,
    Save,
    Hide,
    Forget,
    Import,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Mode {
    List,
    Filter,
    Add,
    ConfirmRemove {
        folders: Vec<String>,
    },
    /// The dialog that asks for a folder: where to clone, or what to import.
    Path,
    /// The dialog that asks for a group: where to move, or the new name.
    Group,
    /// The actions for the chosen repositories, opened with Enter.
    Menu,
    /// The version dialog: look for an update, install it, restart.
    Update,
    Help,
    Settings,
    Progress,
}

/// The line under the Add search box.
pub struct AddStatus {
    pub text: String,
    pub error: bool,
    pub busy: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    Name,
    NeedsUpdate,
}

#[derive(Default)]
pub struct Hits {
    pub rows: Vec<(Rect, String)>,
    pub buttons: Vec<(Rect, Action)>,
}

/// Something the user asked for. It runs at once when nothing else does,
/// and waits in the queue otherwise.
#[derive(Clone)]
pub enum Queued {
    Update(Vec<String>),
    UpdateAll,
    Remove(Vec<String>),
    /// Clone these into that folder.
    Clone(Vec<RemoteRepo>, PathBuf),
    /// Add the clones that already sit in that folder.
    Import(PathBuf),
    Refresh,
}

impl Queued {
    /// A few words for the status line, such as "update api".
    pub fn label(&self) -> String {
        let named = |verb: &str, names: Vec<String>| match names.as_slice() {
            [only] => format!("{verb} {only}"),
            many => format!("{verb} {}", count_label(many.len())),
        };
        match self {
            Queued::Update(folders) => named("update", folders.clone()),
            Queued::UpdateAll => "update all".to_string(),
            Queued::Remove(folders) => named("remove", folders.clone()),
            Queued::Clone(remotes, _) => {
                named("clone", remotes.iter().map(RemoteRepo::id).collect())
            }
            Queued::Import(dir) => format!("import {}", dir.display()),
            Queued::Refresh => "refresh".to_string(),
        }
    }
}

/// A row name that starts with this is the header of a group, not a
/// repository. What follows is the group's name; nothing follows for the
/// repositories that are in no group.
pub const GROUP_KEY: &str = "group::";

pub fn group_key(name: &str) -> String {
    format!("{GROUP_KEY}{name}")
}

/// The longest name a group may have.
const GROUP_NAME_LIMIT: usize = 40;

/// One line of the overview.
pub enum Row<'a> {
    /// The header of a group. An empty name heads the repositories that
    /// are in no group.
    Group {
        name: String,
        repos: usize,
        /// Repositories in it that have something to bring in.
        incoming: usize,
        collapsed: bool,
    },
    Repo(&'a LocalRepo),
}

impl Row<'_> {
    pub fn key(&self) -> String {
        match self {
            Row::Group { name, .. } => group_key(name),
            Row::Repo(repo) => repo.folder.clone(),
        }
    }
}

/// What is known about a newer version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Nobody has looked yet.
    Unknown,
    Checking,
    /// The running version is the latest.
    Current,
    Available(Release),
    Installing {
        release: Release,
        done: u64,
        total: u64,
    },
    /// This version was put in place. It runs after a restart.
    Installed(String),
    Failed(String),
}

/// One line of the actions menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Update,
    Open,
    Github,
    Group,
    CopyPath,
    Remove,
}

impl MenuItem {
    pub const ALL: [MenuItem; 6] = [
        MenuItem::Update,
        MenuItem::Open,
        MenuItem::Github,
        MenuItem::Group,
        MenuItem::CopyPath,
        MenuItem::Remove,
    ];

    /// What the line says, for one repository or for several.
    pub fn label(self, many: bool) -> &'static str {
        match (self, many) {
            (MenuItem::Update, _) => "Update",
            (MenuItem::Open, false) => "Open the folder",
            (MenuItem::Open, true) => "Open the folders",
            (MenuItem::Github, _) => "Open on GitHub",
            (MenuItem::Group, _) => "Move to a group…",
            (MenuItem::CopyPath, false) => "Copy the path",
            (MenuItem::CopyPath, true) => "Copy the paths",
            (MenuItem::Remove, _) => "Remove…",
        }
    }

    /// The key that runs it straight from the menu. Each is the key that
    /// does the same in the list, where there is one.
    pub fn key(self) -> char {
        match self {
            MenuItem::Update => 'u',
            MenuItem::Open => 'o',
            MenuItem::Github => 'g',
            MenuItem::Group => 'm',
            MenuItem::CopyPath => 'c',
            MenuItem::Remove => 'd',
        }
    }
}

/// The actions menu: what it is about and which line is highlighted.
pub struct Menu {
    pub repos: Vec<String>,
    pub selected: usize,
}

/// The dialog that moves repositories into a group or renames a group.
pub struct GroupPrompt {
    /// The repositories to move, by row name. Empty for a rename.
    pub repos: Vec<String>,
    /// The group that gets a new name.
    pub rename: Option<String>,
    pub input: String,
    pub error: Option<String>,
}

struct Planned {
    folder: String,
    path: PathBuf,
    /// Branches the last check saw something new on.
    expected: u32,
    skip: bool,
    gone: bool,
}

enum Msg {
    Status {
        generation: u64,
        folder: String,
        check: gitops::RepoCheck,
    },
    Github {
        generation: u64,
        catalog: github::Catalog,
    },
    Account {
        key: String,
        result: Result<Vec<RemoteRepo>, String>,
    },
    Login(Option<String>),
    /// The answer to "is there a newer version?".
    Update(Result<Option<Release>, String>),
    UpdateProgress(u64, u64),
    UpdateDone(Result<(), String>),
    Job(JobEvent),
    /// A clone landed here, or a checkout that was already here was found.
    Cloned(PathBuf),
    JobFinished(Result<String, String>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum RemoteState {
    Loading,
    Ready,
    Failed(String),
}

pub struct App {
    /// Every checkout the overview manages, wherever it is on disk.
    pub checkouts: Vec<PathBuf>,
    /// The folder the Add dialog offers for a new clone: the last one used.
    pub clone_dir: PathBuf,
    owners_explicit: bool,
    /// The open folder dialog.
    pub prompt: Option<PathPrompt>,
    /// Checkouts a running remove is deleting.
    removing: Vec<PathBuf>,
    /// Which group each checkout is in. Groups only sort the overview.
    pub placement: Vec<(PathBuf, String)>,
    /// Groups that are folded shut. An empty name is "no group".
    pub collapsed: Vec<String>,
    /// The open group dialog.
    pub group_prompt: Option<GroupPrompt>,
    /// The open actions menu.
    pub menu: Option<Menu>,
    pub title: String,
    pub custom_title: Option<String>,
    pub config_path: PathBuf,
    pub form: Option<Form>,
    pub owners: Vec<String>,
    pub pinned: Vec<github::RepoSpec>,
    pub protocol: gitops::Protocol,
    pub installed: Vec<LocalRepo>,
    pub remotes: Vec<RemoteRepo>,
    pub remotes_state: RemoteState,
    pub catalog_warning: Option<String>,
    pub selected: Option<String>,
    pub add_selected: Option<String>,
    /// Repositories ticked with Space in Add. Enter clones all of them.
    pub add_marked: Vec<RemoteRepo>,
    /// Installed repositories ticked with Space, by folder. Update, Remove
    /// and Open act on all of them.
    pub marked: Vec<String>,
    /// What waits for the running task to finish, oldest first.
    pub queue: VecDeque<Queued>,
    /// The user hid the progress dialog of a run. Queued runs that follow
    /// stay out of the way too.
    dialog_hidden: bool,
    pub mode: Mode,
    pub sort: SortMode,
    pub list_filter: String,
    pub add_filter: String,
    pub list_scroll: usize,
    pub add_scroll: usize,
    /// The running update or clone, or the last one that ran.
    pub job: Option<Job>,
    pub status: String,
    pub status_error: bool,
    pub busy: bool,
    pub fetching: bool,
    pub tick: u64,
    pub mouse: Option<(u16, u16)>,
    pub pointer: crate::select::Pointer,
    pub screen: Vec<Vec<String>>,
    pub copy_note: Option<String>,
    pub copy_failed: bool,
    pub hits: Hits,
    pub should_quit: bool,
    /// Start the program again after quitting: an update was installed.
    pub restart: bool,
    pub update: UpdateState,
    /// A failed look at startup stays quiet; one the user asked for says why.
    update_quiet: bool,
    /// Look for a newer version at startup, as the settings file says.
    update_check: bool,
    generation: u64,
    remote_generation: u64,
    status_pending: usize,
    /// Set at startup: fetch once the first local read is done.
    fetch_after_read: bool,
    last_click: Option<(String, Instant)>,
    /// Repositories of accounts looked up from the Add search, by lowercase name.
    lookups: HashMap<String, Result<Vec<RemoteRepo>, String>>,
    lookup_due: Option<(String, Instant)>,
    lookup_running: Option<String>,
    job_close_at: Option<Instant>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        let first_run = settings.first_run;
        let unsaved = settings.unsaved;
        let look_for_update = settings.update_check_now;
        let mut app = Self::empty(settings);
        update::clean_up();
        if look_for_update {
            app.check_for_update(true);
        }
        app.reload_lists(false, false);
        app.fetch_after_read = true;
        if unsaved {
            // Checkouts taken over at startup go into the settings file.
            app.save_config();
        }
        if first_run {
            app.open_settings(true);
            if app.owners.is_empty() {
                let tx = app.tx.clone();
                thread::spawn(move || {
                    let _ = tx.send(Msg::Login(github::current_login()));
                });
            }
        }
        app
    }

    /// An app that reads no folder and calls no network, for drawing tests.
    #[cfg(test)]
    pub fn for_test(settings: Settings) -> Self {
        let first_run = settings.first_run;
        let mut app = Self::empty(settings);
        app.remotes_state = RemoteState::Ready;
        if first_run {
            app.open_settings(true);
        }
        app
    }

    fn empty(settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            checkouts: settings.checkouts,
            clone_dir: settings.clone_dir,
            owners_explicit: settings.owners_explicit,
            prompt: None,
            removing: Vec::new(),
            placement: settings.placement,
            collapsed: settings.collapsed,
            group_prompt: None,
            menu: None,
            title: settings.title,
            custom_title: settings.custom_title,
            config_path: settings.config_path,
            form: None,
            owners: settings.owners,
            pinned: settings.pinned,
            protocol: settings.protocol,
            installed: Vec::new(),
            remotes: Vec::new(),
            remotes_state: RemoteState::Loading,
            catalog_warning: None,
            selected: None,
            add_selected: None,
            add_marked: Vec::new(),
            marked: Vec::new(),
            queue: VecDeque::new(),
            dialog_hidden: false,
            mode: Mode::List,
            sort: SortMode::Name,
            list_filter: String::new(),
            add_filter: String::new(),
            list_scroll: 0,
            add_scroll: 0,
            job: None,
            job_close_at: None,
            status: "Looking for repositories…".to_string(),
            status_error: false,
            busy: false,
            fetching: false,
            tick: 0,
            mouse: None,
            pointer: crate::select::Pointer::default(),
            screen: Vec::new(),
            copy_note: None,
            copy_failed: false,
            hits: Hits::default(),
            should_quit: false,
            restart: false,
            update: UpdateState::Unknown,
            update_quiet: false,
            update_check: settings.update_check,
            generation: 0,
            remote_generation: 0,
            status_pending: 0,
            fetch_after_read: false,
            last_click: None,
            lookups: HashMap::new(),
            lookup_due: None,
            lookup_running: None,
            tx,
            rx,
        }
    }

    pub fn drain(&mut self) -> bool {
        let started = self.start_due_lookup() | self.close_finished_job();
        let mut messages = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            messages.push(message);
        }
        let arrived = !messages.is_empty();
        for message in messages {
            self.on_msg(message);
        }
        let next = self.start_queued();
        arrived || started || next
    }

    pub fn waiting(&self) -> bool {
        self.busy
            || self.fetching
            || self.status_pending > 0
            || self.remotes_state == RemoteState::Loading
            || self.lookup_due.is_some()
            || self.lookup_running.is_some()
            || self.job_close_at.is_some()
            || !self.queue.is_empty()
            || matches!(
                self.update,
                UpdateState::Checking | UpdateState::Installing { .. }
            )
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.paste(text),
            Event::Mouse(mouse) => {
                self.mouse = Some((mouse.column, mouse.row));
                let at = (mouse.column, mouse.row);
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => {
                        self.copy_note = None;
                        self.copy_failed = false;
                        self.pointer.on(PointerEvent::Down, at);
                    }
                    MouseEventKind::Drag(MouseButton::Left) => {
                        self.pointer.on(PointerEvent::Drag, at);
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        match self.pointer.on(PointerEvent::Up, at) {
                            PointerEffect::Click(x, y) => self.click(x, y),
                            PointerEffect::Copy => self.copy_selection(),
                            PointerEffect::None => {}
                        }
                    }
                    MouseEventKind::Down(MouseButton::Right) => {
                        self.dismiss_selection();
                        self.click(mouse.column, mouse.row);
                    }
                    MouseEventKind::ScrollUp => {
                        self.dismiss_selection();
                        self.wheel(-3);
                    }
                    MouseEventKind::ScrollDown => {
                        self.dismiss_selection();
                        self.wheel(3);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// The installed repositories the filter lets through, sorted.
    fn sorted_matches(&self) -> Vec<&LocalRepo> {
        let mut repos: Vec<&LocalRepo> = self
            .installed
            .iter()
            .filter(|repo| list_matches(repo, &self.list_filter))
            .collect();
        if self.sort == SortMode::NeedsUpdate {
            repos.sort_by(|a, b| {
                gitops::attention_rank(b)
                    .cmp(&gitops::attention_rank(a))
                    .then(a.folder.to_lowercase().cmp(&b.folder.to_lowercase()))
            });
        }
        repos
    }

    /// The lines of the overview. Without groups that is the repositories.
    /// With groups, each group has a header and its repositories under it,
    /// in the order of the group names; what is in no group comes last.
    /// A folded group shows its header only. A filter unfolds everything.
    pub fn rows(&self) -> Vec<Row<'_>> {
        let repos = self.sorted_matches();
        if !self.has_groups() {
            return repos.into_iter().map(Row::Repo).collect();
        }
        let filtering = !self.list_filter.trim().is_empty();
        let mut names: Vec<String> = Vec::new();
        for repo in &repos {
            if !repo.group.is_empty() && !names.contains(&repo.group) {
                names.push(repo.group.clone());
            }
        }
        names.sort_by_key(|name| name.to_lowercase());
        if repos.iter().any(|repo| repo.group.is_empty()) {
            names.push(String::new());
        }
        let mut rows = Vec::new();
        for name in names {
            let members: Vec<&LocalRepo> = repos
                .iter()
                .copied()
                .filter(|repo| repo.group == name)
                .collect();
            let collapsed = !filtering && self.collapsed.contains(&name);
            rows.push(Row::Group {
                name,
                repos: members.len(),
                incoming: members.iter().filter(|repo| repo.has_incoming()).count(),
                collapsed,
            });
            if !collapsed {
                rows.extend(members.into_iter().map(Row::Repo));
            }
        }
        rows
    }

    /// The repositories the overview shows, in its order.
    pub fn visible_installed(&self) -> Vec<&LocalRepo> {
        self.rows()
            .into_iter()
            .filter_map(|row| match row {
                Row::Repo(repo) => Some(repo),
                Row::Group { .. } => None,
            })
            .collect()
    }

    fn has_groups(&self) -> bool {
        self.installed.iter().any(|repo| !repo.group.is_empty())
    }

    /// The groups in use, by name.
    pub fn groups(&self) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for repo in &self.installed {
            if !repo.group.is_empty() && !names.contains(&repo.group) {
                names.push(repo.group.clone());
            }
        }
        names.sort_by_key(|name| name.to_lowercase());
        names
    }

    /// The group whose header is highlighted.
    pub fn selected_group(&self) -> Option<String> {
        self.selected
            .as_ref()?
            .strip_prefix(GROUP_KEY)
            .map(str::to_string)
    }

    /// The repositories of a group that the filter lets through.
    fn group_members(&self, group: &str) -> Vec<String> {
        self.sorted_matches()
            .into_iter()
            .filter(|repo| repo.group == group)
            .map(|repo| repo.folder.clone())
            .collect()
    }

    /// Reads the listed checkouts again and puts each into its group.
    fn load_rows(&mut self) {
        self.installed = gitops::load(&self.checkouts);
        self.assign_groups();
    }

    fn assign_groups(&mut self) {
        for repo in &mut self.installed {
            repo.group = self
                .placement
                .iter()
                .find(|(path, _)| config::same_path(path, &repo.path))
                .map(|(_, group)| group.clone())
                .unwrap_or_default();
        }
    }

    /// Rows of the Add dialog: the default accounts filtered by the search,
    /// plus the repositories of an account named in the search.
    pub fn add_list(&self) -> Vec<RemoteRepo> {
        let query = self.add_query();
        let wanted = |repo: &RemoteRepo| {
            remote_matches(repo, &self.add_filter)
                || query
                    .as_ref()
                    .is_some_and(|query| account_matches(repo, query))
        };
        let mut remotes: Vec<RemoteRepo> = self
            .remotes
            .iter()
            .filter(|repo| wanted(repo))
            .cloned()
            .collect();
        if let Some(query) = &query
            && let Some(Ok(found)) = self.lookups.get(&query.owner.to_ascii_lowercase())
        {
            for repo in found.iter().filter(|repo| account_matches(repo, query)) {
                if !remotes
                    .iter()
                    .any(|listed| listed.id().eq_ignore_ascii_case(&repo.id()))
                {
                    remotes.push(repo.clone());
                }
            }
        }
        remotes.sort_by(|a, b| {
            a.archived.cmp(&b.archived).then(
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then(a.owner.to_lowercase().cmp(&b.owner.to_lowercase())),
            )
        });
        if let Some(spec) = github::parse_repo_spec(&self.add_filter) {
            let listed = remotes.iter().any(|repo| repo.matches_spec(&spec));
            if !listed {
                remotes.insert(0, RemoteRepo::direct(spec));
            }
        }
        remotes
    }

    /// The account the Add search names, as in `owner/` or a GitHub link.
    pub fn add_account(&self) -> Option<String> {
        self.add_query().map(|query| query.owner)
    }

    /// A plain search word that could be an account. Tab lists it.
    pub fn add_word(&self) -> Option<String> {
        let word = self.add_filter.trim();
        let plain = !word.is_empty()
            && word.len() <= 39
            && word
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
        (plain && github::valid_account(word)).then(|| word.to_string())
    }

    pub fn add_status(&self) -> AddStatus {
        let plain = |text: String| AddStatus {
            text,
            error: false,
            busy: false,
        };
        let failed = |text: String| AddStatus {
            text,
            error: true,
            busy: false,
        };
        let loading = |text: String| AddStatus {
            text,
            error: false,
            busy: true,
        };
        if let Some(query) = self.add_query()
            && !self.in_catalog(&query.owner)
        {
            let site = format!("github.com/{}", query.owner);
            return match self.lookups.get(&query.owner.to_ascii_lowercase()) {
                Some(Ok(repos)) => {
                    let cloned = repos.iter().filter(|repo| self.is_installed(repo)).count();
                    plain(format!(
                        "{site} · {} · {cloned} cloned",
                        count_label(repos.len())
                    ))
                }
                Some(Err(err)) => failed(err.clone()),
                None => loading(format!("Loading {site}…")),
            };
        }
        match &self.remotes_state {
            RemoteState::Loading => loading("Loading repositories from GitHub…".to_string()),
            RemoteState::Failed(err) => failed(err.clone()),
            RemoteState::Ready => {
                let other = match self.add_word() {
                    Some(word) => format!("tab lists github.com/{word}"),
                    None => "type owner/ or paste a link to list any account".to_string(),
                };
                if self.owners.is_empty() && self.pinned.is_empty() {
                    return plain(format!("No default account · {other}"));
                }
                let cloned = self
                    .remotes
                    .iter()
                    .filter(|repo| self.is_installed(repo))
                    .count();
                plain(format!(
                    "{} · {cloned} cloned · {other}",
                    count_label(self.remotes.len())
                ))
            }
        }
    }

    pub fn add_empty_message(&self) -> String {
        if let Some(query) = self.add_query()
            && !self.in_catalog(&query.owner)
        {
            return match self.lookups.get(&query.owner.to_ascii_lowercase()) {
                Some(Ok(repos)) if repos.is_empty() => format!(
                    "github.com/{} has no repositories you can see.",
                    query.owner
                ),
                Some(Ok(_)) => "Nothing matches that search.".to_string(),
                _ => String::new(),
            };
        }
        match &self.remotes_state {
            RemoteState::Loading | RemoteState::Failed(_) => String::new(),
            RemoteState::Ready if self.add_filter.is_empty() => {
                "Type owner/repo, owner/ for a whole account, or paste a GitHub link.".to_string()
            }
            RemoteState::Ready => match self.add_word() {
                Some(word) => {
                    format!("Nothing in your list matches. Press Enter to list github.com/{word}.")
                }
                None => "Nothing matches that search.".to_string(),
            },
        }
    }

    fn add_query(&self) -> Option<AccountQuery> {
        github::parse_account_query(&self.add_filter)
    }

    /// The default accounts already hold this account's repositories.
    fn in_catalog(&self, owner: &str) -> bool {
        self.owners
            .iter()
            .any(|item| item.eq_ignore_ascii_case(owner))
    }

    pub fn missing_count(&self) -> usize {
        self.remotes
            .iter()
            .filter(|repo| !self.is_installed(repo))
            .count()
    }

    pub fn about(&self, repo: &LocalRepo) -> String {
        if let Some(description) = self
            .remotes
            .iter()
            .find(|remote| {
                remote.owner.eq_ignore_ascii_case(&repo.owner)
                    && remote.name.eq_ignore_ascii_case(&repo.repo)
            })
            .map(|remote| remote.description.clone())
            .filter(|text| !text.is_empty())
        {
            return description;
        }
        String::new()
    }

    pub fn is_marked(&self, remote: &RemoteRepo) -> bool {
        let id = remote.id();
        self.add_marked
            .iter()
            .any(|marked| marked.id().eq_ignore_ascii_case(&id))
    }

    /// Ticks or unticks the highlighted row, then moves down so Space can
    /// be pressed again. A repository that is already cloned is skipped.
    fn toggle_mark(&mut self) {
        let Some(id) = self.add_selected.clone() else {
            return;
        };
        let Some(remote) = self
            .add_list()
            .into_iter()
            .find(|repo| repo.id().eq_ignore_ascii_case(&id))
        else {
            return;
        };
        if let Some(index) = self
            .add_marked
            .iter()
            .position(|marked| marked.id().eq_ignore_ascii_case(&id))
        {
            self.add_marked.remove(index);
        } else if !self.is_installed(&remote) {
            self.add_marked.push(remote);
        }
        self.move_by(1);
    }

    pub fn is_installed(&self, remote: &RemoteRepo) -> bool {
        self.installed.iter().any(|local| {
            local.owner.eq_ignore_ascii_case(&remote.owner)
                && local.repo.eq_ignore_ascii_case(&remote.name)
        })
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            if self.pointer.selection.is_some() {
                self.copy_selection();
            } else {
                self.quit();
            }
            return;
        }
        if key.code == KeyCode::Esc && self.pointer.selection.is_some() {
            self.dismiss_selection();
            return;
        }
        if self.pointer.selection.is_some() || self.copy_note.is_some() {
            self.pointer.selection = None;
            self.copy_note = None;
            self.copy_failed = false;
        }
        match self.mode.clone() {
            Mode::Help => self.on_help_key(key),
            Mode::Settings => self.on_settings_key(key),
            Mode::Progress => self.on_progress_key(key),
            Mode::ConfirmRemove { .. } => self.on_confirm_key(key),
            Mode::Path => self.on_path_key(key),
            Mode::Group => self.on_group_key(key, ctrl),
            Mode::Menu => self.on_menu_key(key),
            Mode::Update => self.on_update_key(key),
            Mode::Add => self.on_add_key(key, ctrl),
            Mode::Filter => self.on_filter_key(key, ctrl),
            Mode::List => self.on_list_key(key, ctrl),
        }
    }

    fn on_list_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') if ctrl => self.mark_all_visible(),
            KeyCode::Char(' ') => self.toggle_list_mark(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            KeyCode::Enter => self.enter_row(),
            KeyCode::Left => self.fold_selected(true),
            KeyCode::Right => self.fold_selected(false),
            KeyCode::Char('m') | KeyCode::Char('M') => self.open_group_prompt(),
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('u') => self.update_chosen(),
            KeyCode::Char('U') => self.update_all(),
            KeyCode::Char('d') | KeyCode::Delete => self.begin_remove(),
            KeyCode::Char('r') | KeyCode::F(5) => self.refresh(),
            KeyCode::Char('s') | KeyCode::Char('S') => self.toggle_sort(),
            KeyCode::Char('o') | KeyCode::Char('O') => self.open_selected(),
            KeyCode::Char('i') | KeyCode::Char('I') => self.open_import(),
            KeyCode::Char('c') | KeyCode::Char(',') | KeyCode::F(2) => self.open_settings(false),
            KeyCode::Char('l') | KeyCode::Char('L') => self.show_job(),
            KeyCode::Char('v') | KeyCode::Char('V') => self.open_update(),
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.list_filter.clear();
            }
            KeyCode::Esc if self.busy || self.fetching => self.cancel_work(),
            KeyCode::Esc => self.marked.clear(),
            _ => {}
        }
    }

    pub fn is_marked_local(&self, folder: &str) -> bool {
        self.marked.iter().any(|marked| marked == folder)
    }

    /// Ticks or unticks the highlighted repository, then moves down so
    /// Space can be pressed again. On a group's header it ticks the whole
    /// group, or unticks it when all of it is ticked.
    fn toggle_list_mark(&mut self) {
        let names = self.current_names();
        let Some(row) = self
            .selected
            .clone()
            .filter(|row| names.contains(row))
            .or_else(|| names.first().cloned())
        else {
            return;
        };
        if let Some(group) = row.strip_prefix(GROUP_KEY) {
            let members = self.group_members(group);
            if members.iter().all(|name| self.marked.contains(name)) {
                self.marked.retain(|marked| !members.contains(marked));
            } else {
                for name in members {
                    if !self.marked.contains(&name) {
                        self.marked.push(name);
                    }
                }
            }
        } else if let Some(index) = self.marked.iter().position(|marked| *marked == row) {
            self.marked.remove(index);
        } else {
            self.marked.push(row.clone());
        }
        self.selected = Some(row);
        self.move_by(1);
    }

    /// Ticks every repository the list shows. When they all are ticked
    /// already, unticks them.
    fn mark_all_visible(&mut self) {
        let names: Vec<String> = self
            .visible_installed()
            .into_iter()
            .map(|repo| repo.folder.clone())
            .collect();
        if names.iter().all(|name| self.marked.contains(name)) {
            self.marked.retain(|marked| !names.contains(marked));
            return;
        }
        for name in names {
            if !self.marked.contains(&name) {
                self.marked.push(name);
            }
        }
    }

    /// The repositories an action applies to: the ticked ones in the
    /// order they are installed, else the highlighted one, or the whole
    /// group when a group's header is highlighted.
    fn chosen(&self) -> Vec<String> {
        if self.marked.is_empty() {
            return match self.selected_group() {
                Some(group) => self.group_members(&group),
                None => self.selected.clone().into_iter().collect(),
            };
        }
        self.installed
            .iter()
            .filter(|repo| self.marked.contains(&repo.folder))
            .map(|repo| repo.folder.clone())
            .collect()
    }

    /// Enter in the list: folds or unfolds a group, and opens the actions
    /// menu for a repository or for what is ticked.
    fn enter_row(&mut self) {
        match self.selected_group() {
            Some(group) if self.marked.is_empty() => self.toggle_group(&group),
            _ => self.open_menu(),
        }
    }

    /// Asks GitHub for a newer version, on a thread of its own.
    fn check_for_update(&mut self, quiet: bool) {
        if matches!(
            self.update,
            UpdateState::Checking | UpdateState::Installing { .. } | UpdateState::Installed(_)
        ) {
            return;
        }
        self.update = UpdateState::Checking;
        self.update_quiet = quiet;
        let tx = self.tx.clone();
        thread::spawn(move || {
            let token = github::token();
            let _ = tx.send(Msg::Update(update::check(token.as_deref())));
        });
    }

    /// `v`: the version dialog. It looks for an update unless one is
    /// already known or being installed.
    fn open_update(&mut self) {
        self.mode = Mode::Update;
        if matches!(
            self.update,
            UpdateState::Unknown | UpdateState::Current | UpdateState::Failed(_)
        ) {
            self.check_for_update(false);
        }
    }

    fn on_update_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.mode = Mode::List,
            KeyCode::Enter => self.update_enter(),
            _ => {}
        }
    }

    /// Enter in the version dialog: install what is available, restart
    /// into what was installed, or look again.
    fn update_enter(&mut self) {
        match self.update.clone() {
            UpdateState::Available(release) => self.install_update(release),
            UpdateState::Installed(_) => {
                self.restart = true;
                self.quit();
            }
            UpdateState::Current | UpdateState::Failed(_) | UpdateState::Unknown => {
                self.check_for_update(false);
            }
            UpdateState::Checking | UpdateState::Installing { .. } => {}
        }
    }

    fn install_update(&mut self, release: Release) {
        let total = release
            .download
            .as_ref()
            .map_or(0, |download| download.size);
        self.update = UpdateState::Installing {
            release: release.clone(),
            done: 0,
            total,
        };
        let tx = self.tx.clone();
        thread::spawn(move || {
            let token = github::token();
            let result = update::install(&release, token.as_deref(), |done, total| {
                let _ = tx.send(Msg::UpdateProgress(done, total));
            });
            let _ = tx.send(Msg::UpdateDone(result));
        });
    }

    fn open_menu(&mut self) {
        let repos = self.chosen();
        if repos.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        self.menu = Some(Menu { repos, selected: 0 });
        self.mode = Mode::Menu;
    }

    fn close_menu(&mut self) {
        self.menu = None;
        self.mode = Mode::List;
    }

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Some(menu) = self.menu.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        let last = MenuItem::ALL.len() - 1;
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => self.close_menu(),
            KeyCode::Up | KeyCode::Char('k') => menu.selected = menu.selected.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => menu.selected = (menu.selected + 1).min(last),
            KeyCode::Home => menu.selected = 0,
            KeyCode::End => menu.selected = last,
            KeyCode::Enter => {
                let item = MenuItem::ALL[menu.selected.min(last)];
                self.run_menu(item);
            }
            KeyCode::Char(ch) => {
                let wanted = ch.to_ascii_lowercase();
                if let Some(item) = MenuItem::ALL.into_iter().find(|item| item.key() == wanted) {
                    self.run_menu(item);
                }
            }
            _ => {}
        }
    }

    /// Closes the menu and does what the line says, to the repositories
    /// that are ticked or highlighted.
    fn run_menu(&mut self, item: MenuItem) {
        self.close_menu();
        match item {
            MenuItem::Update => self.update_chosen(),
            MenuItem::Open => self.open_selected(),
            MenuItem::Github => self.open_on_github(),
            MenuItem::Group => self.open_group_prompt(),
            MenuItem::CopyPath => self.copy_paths(),
            MenuItem::Remove => self.begin_remove(),
        }
    }

    /// The chosen repositories that still are on the list.
    fn chosen_repos(&self) -> Vec<&LocalRepo> {
        let names = self.chosen();
        self.installed
            .iter()
            .filter(|repo| names.contains(&repo.folder))
            .collect()
    }

    fn open_on_github(&mut self) {
        let pages: Vec<String> = self
            .chosen_repos()
            .into_iter()
            .filter(|repo| !repo.owner.is_empty())
            .map(|repo| format!("https://github.com/{}/{}", repo.owner, repo.repo))
            .collect();
        if pages.is_empty() {
            self.notice("There is no GitHub page for this.", true);
            return;
        }
        if pages.len() > OPEN_LIMIT {
            self.notice(
                format!(
                    "{} pages are too many to open at once. Select {OPEN_LIMIT} or fewer.",
                    pages.len()
                ),
                true,
            );
            return;
        }
        let failed = pages
            .iter()
            .filter_map(|page| gitops::open_url(page).err())
            .next_back();
        match (failed, pages.as_slice()) {
            (Some(err), _) => self.notice(err, true),
            (None, [page]) => self.notice(format!("Opened {page}."), false),
            (None, _) => self.notice(format!("Opened {} pages on GitHub.", pages.len()), false),
        }
    }

    /// Puts the paths of the chosen repositories on the clipboard, one a line.
    fn copy_paths(&mut self) {
        let paths: Vec<String> = self
            .chosen_repos()
            .into_iter()
            .map(|repo| repo.path.display().to_string())
            .collect();
        if paths.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        match crate::clipboard::set_text(&paths.join("\n")) {
            Err(err) => self.notice(err, true),
            Ok(()) => match paths.as_slice() {
                [path] => self.notice(format!("Copied {path}."), false),
                many => self.notice(format!("Copied {} paths.", many.len()), false),
            },
        }
    }

    fn toggle_group(&mut self, group: &str) {
        if let Some(index) = self.collapsed.iter().position(|name| name == group) {
            self.collapsed.remove(index);
        } else {
            self.collapsed.push(group.to_string());
        }
        self.save_config();
    }

    /// Left folds the highlighted group, Right unfolds it. Left on a
    /// repository goes to the header of its group.
    fn fold_selected(&mut self, fold: bool) {
        match self.selected_group() {
            Some(group) => {
                if self.collapsed.contains(&group) != fold {
                    self.toggle_group(&group);
                }
            }
            None if fold && self.has_groups() => {
                let group = self
                    .selected
                    .as_ref()
                    .and_then(|name| self.installed.iter().find(|repo| &repo.folder == name))
                    .map(|repo| repo.group.clone());
                if let Some(group) = group {
                    self.selected = Some(group_key(&group));
                }
            }
            None => {}
        }
    }

    /// `m`: asks which group the chosen repositories go into. On a group's
    /// header it asks for the group's new name.
    fn open_group_prompt(&mut self) {
        let prompt = match self.selected_group().filter(|_| self.marked.is_empty()) {
            Some(group) if group.is_empty() => {
                self.notice(
                    "Select repositories here and press m to put them into a group.",
                    false,
                );
                return;
            }
            Some(group) => GroupPrompt {
                repos: Vec::new(),
                input: group.clone(),
                rename: Some(group),
                error: None,
            },
            None => {
                let repos = self.chosen();
                let Some(first) = repos.first() else {
                    self.notice("Select a repository first.", true);
                    return;
                };
                let input = self
                    .installed
                    .iter()
                    .find(|repo| &repo.folder == first)
                    .map(|repo| repo.group.clone())
                    .unwrap_or_default();
                GroupPrompt {
                    repos,
                    rename: None,
                    input,
                    error: None,
                }
            }
        };
        self.group_prompt = Some(prompt);
        self.mode = Mode::Group;
    }

    fn on_group_key(&mut self, key: KeyEvent, ctrl: bool) {
        let groups = self.groups();
        let Some(prompt) = self.group_prompt.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_group(),
            KeyCode::Enter => self.accept_group(),
            // Walks through the groups that exist.
            KeyCode::Up | KeyCode::Down if !groups.is_empty() => {
                let at = groups
                    .iter()
                    .position(|group| group.eq_ignore_ascii_case(prompt.input.trim()));
                let last = groups.len() - 1;
                let next = match (at, key.code == KeyCode::Down) {
                    (Some(index), true) if index < last => index + 1,
                    (Some(index), false) if index > 0 => index - 1,
                    (_, true) => 0,
                    (_, false) => last,
                };
                prompt.input = groups[next].clone();
                prompt.error = None;
            }
            KeyCode::Backspace => {
                prompt.input.pop();
                prompt.error = None;
            }
            KeyCode::Char('u') | KeyCode::Char('U') if ctrl => {
                prompt.input.clear();
                prompt.error = None;
            }
            KeyCode::Char(ch) if !ctrl => {
                prompt.input.push(ch);
                prompt.error = None;
            }
            _ => {}
        }
    }

    fn close_group(&mut self) {
        self.group_prompt = None;
        self.mode = Mode::List;
    }

    /// Enter in the group dialog: moves the repositories, or renames.
    fn accept_group(&mut self) {
        let Some(prompt) = self.group_prompt.as_mut() else {
            return;
        };
        let name = match clean_group_name(&prompt.input) {
            Ok(name) => name,
            Err(err) => {
                prompt.error = Some(err);
                return;
            }
        };
        if prompt.rename.is_some() && name.is_empty() {
            prompt.error = Some("A group needs a name.".to_string());
            return;
        }
        let Some(prompt) = self.group_prompt.take() else {
            return;
        };
        self.mode = Mode::List;
        // A group that exists keeps its spelling: "work" joins "Work".
        let name = self
            .groups()
            .into_iter()
            .find(|group| group.eq_ignore_ascii_case(&name))
            .filter(|group| prompt.rename.as_ref() != Some(group))
            .unwrap_or(name);
        let text = match prompt.rename {
            Some(old) => {
                for (_, group) in &mut self.placement {
                    if *group == old {
                        *group = name.clone();
                    }
                }
                let folded = self.collapsed.contains(&old);
                self.collapsed
                    .retain(|group| *group != old && *group != name);
                if folded {
                    self.collapsed.push(name.clone());
                }
                self.selected = Some(group_key(&name));
                format!("{old} is called {name} now.")
            }
            None => {
                let paths: Vec<PathBuf> = self
                    .installed
                    .iter()
                    .filter(|repo| prompt.repos.contains(&repo.folder))
                    .map(|repo| repo.path.clone())
                    .collect();
                self.placement
                    .retain(|(path, _)| !paths.iter().any(|moved| config::same_path(moved, path)));
                if !name.is_empty() {
                    self.placement
                        .extend(paths.into_iter().map(|path| (path, name.clone())));
                    // What was just moved stays in view.
                    self.collapsed.retain(|group| *group != name);
                }
                self.marked.clear();
                let what = match prompt.repos.as_slice() {
                    [one] => format!("{one} is"),
                    many => format!("{} are", count_label(many.len())),
                };
                if name.is_empty() {
                    format!("{what} in no group now.")
                } else {
                    format!("{what} in {name} now.")
                }
            }
        };
        self.assign_groups();
        self.snap_visible_selection();
        self.notice(text, false);
        self.save_config();
    }

    /// `d` on a group's header: the group goes, its repositories stay.
    fn dissolve_group(&mut self, group: &str) {
        if group.is_empty() {
            self.notice("These repositories are in no group.", false);
            return;
        }
        let count = self
            .installed
            .iter()
            .filter(|repo| repo.group == group)
            .count();
        self.placement.retain(|(_, name)| name != group);
        self.collapsed.retain(|name| name != group);
        self.assign_groups();
        self.snap_visible_selection();
        self.notice(
            format!(
                "The group {group} is gone. Its {} stay on the list.",
                count_label(count)
            ),
            false,
        );
        self.save_config();
    }

    /// Forgets ticks for folders that are gone.
    fn prune_marks(&mut self) {
        let installed = &self.installed;
        self.marked
            .retain(|marked| installed.iter().any(|repo| repo.folder == *marked));
    }

    fn on_filter_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.list_filter.clear();
                self.list_scroll = 0;
            }
            KeyCode::Backspace => {
                self.list_filter.pop();
                self.list_scroll = 0;
                self.snap_visible_selection();
            }
            KeyCode::Char('u') if ctrl => {
                self.list_filter.clear();
                self.list_scroll = 0;
                self.snap_visible_selection();
            }
            KeyCode::Char('w') if ctrl => {
                delete_word(&mut self.list_filter);
                self.list_scroll = 0;
                self.snap_visible_selection();
            }
            KeyCode::Enter => self.enter_row(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            KeyCode::Char(ch) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.list_filter.push(ch);
                self.list_scroll = 0;
                self.snap_visible_selection();
            }
            _ => {}
        }
    }

    fn on_add_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Backspace => {
                self.add_filter.pop();
                self.note_add_filter_changed();
            }
            KeyCode::Char('u') if ctrl => {
                self.add_filter.clear();
                self.note_add_filter_changed();
            }
            KeyCode::Char('w') if ctrl => {
                delete_word(&mut self.add_filter);
                self.note_add_filter_changed();
            }
            KeyCode::Enter => self.clone_selected(),
            KeyCode::Tab => self.list_typed_account(),
            KeyCode::Char(' ') if !ctrl => self.toggle_mark(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            KeyCode::Char(ch) if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) => {
                self.add_filter.push(ch);
                self.note_add_filter_changed();
            }
            _ => {}
        }
    }

    fn on_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => self.mode = Mode::List,
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => self.confirm_remove(),
            KeyCode::Char('f') | KeyCode::Char('F') => self.forget_chosen(),
            _ => {}
        }
    }

    fn open_import(&mut self) {
        self.prompt = Some(PathPrompt::new(Purpose::Import, &self.clone_dir));
        self.mode = Mode::Path;
    }

    fn on_path_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.prompt.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match prompt.on_key(key) {
            pathprompt::Outcome::Stay => {}
            pathprompt::Outcome::Accept(folder) => self.use_folder(folder),
            pathprompt::Outcome::Close => self.close_path(),
        }
    }

    /// The Clone or Import button of the folder dialog.
    fn accept_path(&mut self) {
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        match prompt.folder() {
            Ok(folder) => self.use_folder(folder),
            Err(err) => {
                prompt.matches.clear();
                prompt.error = Some(err);
            }
        }
    }

    fn use_folder(&mut self, folder: PathBuf) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        self.mode = Mode::List;
        match prompt.purpose {
            Purpose::Clone(remotes) => {
                self.add_marked.clear();
                // The next clone is offered the same folder.
                if !config::same_path(&self.clone_dir, &folder) {
                    self.clone_dir = folder.clone();
                    self.save_config();
                }
                self.submit(Queued::Clone(remotes, folder));
            }
            Purpose::Import => self.submit(Queued::Import(folder)),
        }
    }

    /// Esc in the folder dialog. It goes back to Add when it asked where
    /// to clone, with the selection still in place.
    fn close_path(&mut self) {
        let cloning = matches!(
            self.prompt.take().map(|prompt| prompt.purpose),
            Some(Purpose::Clone(_))
        );
        self.mode = if cloning { Mode::Add } else { Mode::List };
    }

    /// Writes the settings and the list of checkouts.
    fn save_config(&mut self) {
        let saved = config::Saved {
            title: self.custom_title.clone(),
            owners: self.owners.clone(),
            owners_set: self.owners_explicit,
            pinned: self.pinned.clone(),
            protocol: self.protocol,
            clone_dir: Some(self.clone_dir.clone()),
            checkouts: self.checkouts.clone(),
            placement: self.placement.clone(),
            collapsed: self.collapsed.clone(),
            update_check: self.update_check,
        };
        if let Err(err) = config::save(&self.config_path, &saved) {
            self.notice(err, true);
        }
    }

    /// Takes the repositories of the remove dialog off the list. Their
    /// folders stay on disk.
    fn forget_chosen(&mut self) {
        let Mode::ConfirmRemove { folders } = self.mode.clone() else {
            return;
        };
        self.mode = Mode::List;
        self.marked.clear();
        let paths: Vec<PathBuf> = self
            .installed
            .iter()
            .filter(|repo| folders.contains(&repo.folder))
            .map(|repo| repo.path.clone())
            .collect();
        self.checkouts
            .retain(|path| !paths.iter().any(|gone| config::same_path(gone, path)));
        self.installed
            .retain(|repo| !folders.contains(&repo.folder));
        self.snap_visible_selection();
        let text = match folders.as_slice() {
            [one] => format!("{one} is off the list. Its folder stays on disk."),
            many => format!(
                "{} are off the list. Their folders stay on disk.",
                count_label(many.len())
            ),
        };
        self.notice(text, false);
        self.save_config();
    }

    /// Lists the GitHub clones in `dir`: the folder itself and the folders
    /// directly inside it. Returns how many were new.
    fn register_folder(&mut self, dir: &Path) -> Result<usize, String> {
        let mut found: Vec<PathBuf> = gitops::read_checkout(dir)
            .map(|repo| repo.path)
            .into_iter()
            .collect();
        found.extend(gitops::scan(dir).into_iter().map(|repo| repo.path));
        if found.is_empty() {
            return Err(format!("No GitHub clones in {}.", dir.display()));
        }
        let mut added = 0;
        for path in found {
            added += usize::from(config::push_path(&mut self.checkouts, path));
        }
        if added > 0 {
            self.save_config();
        }
        Ok(added)
    }

    fn import_folder(&mut self, dir: &Path) {
        match self.register_folder(dir) {
            Err(err) => self.notice(err, true),
            Ok(0) => self.notice(
                format!("Everything in {} is on the list already.", dir.display()),
                false,
            ),
            Ok(added) => {
                if !self.status_error {
                    self.notice(
                        format!("Added {} from {}.", count_label(added), dir.display()),
                        false,
                    );
                }
                self.reload_lists(true, false);
            }
        }
    }

    /// After a remove: checkouts whose folder is gone leave the list.
    fn settle_removed(&mut self) {
        let removing = std::mem::take(&mut self.removing);
        if removing.is_empty() {
            return;
        }
        let before = self.checkouts.len();
        self.checkouts.retain(|path| {
            path.exists() || !removing.iter().any(|gone| config::same_path(gone, path))
        });
        if self.checkouts.len() != before {
            self.save_config();
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let Some(form) = self.form.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match form.on_key(key) {
            Outcome::Stay => {}
            Outcome::Save => self.save_settings(),
            Outcome::Close => self.close_settings(),
        }
    }

    fn on_progress_key(&mut self, key: KeyEvent) {
        let running = self.job.as_ref().is_some_and(Job::running);
        match key.code {
            KeyCode::Esc if running => self.cancel_work(),
            KeyCode::Char('h') | KeyCode::Char('H') if running => self.hide_job(),
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char(' ') if !running => {
                self.hide_job()
            }
            KeyCode::Up | KeyCode::Char('k') => self.pick_job(-1),
            KeyCode::Down | KeyCode::Char('j') => self.pick_job(1),
            KeyCode::PageUp => self.scroll_job(5),
            KeyCode::PageDown => self.scroll_job(-5),
            _ => {}
        }
    }

    /// Closes the progress dialog. A run that is still going carries on.
    fn hide_job(&mut self) {
        self.job_close_at = None;
        self.dialog_hidden = self.job.as_ref().is_some_and(Job::running);
        self.mode = Mode::List;
    }

    fn show_job(&mut self) {
        if self.job.is_some() {
            self.job_close_at = None;
            self.dialog_hidden = false;
            self.mode = Mode::Progress;
        } else {
            self.notice("Nothing has been updated or cloned yet.", false);
        }
    }

    /// Shows the repository above or below in the progress dialog. A run
    /// of one repository scrolls its output instead.
    fn pick_job(&mut self, delta: isize) {
        self.job_close_at = None;
        if let Some(job) = self.job.as_mut() {
            if job.items.len() > 1 {
                job.pick(delta);
            } else {
                job.scroll_by(-delta);
            }
        }
    }

    /// Scrolls git's output. Reading it keeps the dialog open.
    fn scroll_job(&mut self, delta: isize) {
        self.job_close_at = None;
        if let Some(job) = self.job.as_mut() {
            job.scroll_by(delta);
        }
    }

    fn close_finished_job(&mut self) -> bool {
        if !self.job_close_at.is_some_and(|at| Instant::now() >= at) {
            return false;
        }
        self.job_close_at = None;
        if self.mode == Mode::Progress {
            self.mode = Mode::List;
        }
        true
    }

    fn wheel(&mut self, delta: isize) {
        if self.mode == Mode::Progress {
            self.scroll_job(-delta);
        } else {
            self.move_by(delta);
        }
    }

    fn on_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q') => {
                self.mode = Mode::List;
            }
            _ => {}
        }
    }

    fn paste(&mut self, text: String) {
        let text: String = text
            .chars()
            .filter(|ch| !matches!(ch, '\r' | '\n'))
            .collect();
        if text.is_empty() {
            return;
        }
        match self.mode {
            Mode::Add => {
                self.add_filter.push_str(&text);
                self.note_add_filter_changed();
            }
            Mode::Filter => {
                self.list_filter.push_str(&text);
                self.list_scroll = 0;
                self.snap_visible_selection();
            }
            Mode::Settings => {
                if let Some(form) = self.form.as_mut() {
                    form.paste(&text);
                }
            }
            Mode::Path => {
                if let Some(prompt) = self.prompt.as_mut() {
                    prompt.paste(&text);
                }
            }
            Mode::Group => {
                if let Some(prompt) = self.group_prompt.as_mut() {
                    prompt.input.push_str(&text);
                    prompt.error = None;
                }
            }
            _ => {}
        }
    }

    fn dismiss_selection(&mut self) {
        self.pointer = crate::select::Pointer::default();
        self.copy_note = None;
        self.copy_failed = false;
    }

    fn copy_selection(&mut self) {
        let Some(sel) = self.pointer.selection else {
            return;
        };
        let text = crate::select::selected_text(&self.screen, sel.anchor, sel.head);
        if text.is_empty() {
            self.pointer.selection = None;
            self.copy_note = None;
            self.copy_failed = false;
            return;
        }
        match crate::clipboard::set_text(&text) {
            Ok(()) => {
                self.copy_note = Some("Copied to the clipboard".to_string());
                self.copy_failed = false;
            }
            Err(err) => {
                self.copy_note = Some(err);
                self.copy_failed = true;
            }
        }
    }

    fn click(&mut self, x: u16, y: u16) {
        let buttons = self.hits.buttons.clone();
        for (rect, action) in &buttons {
            if contains(*rect, x, y) {
                self.run(*action);
                return;
            }
        }
        let rows = self.hits.rows.clone();
        for (rect, name) in &rows {
            if contains(*rect, x, y) {
                self.click_row(name.clone());
                return;
            }
        }
    }

    fn click_row(&mut self, name: String) {
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|(previous, at)| {
            previous.eq_ignore_ascii_case(&name)
                && now.duration_since(*at) < Duration::from_millis(400)
        });
        self.last_click = Some((name.clone(), now));
        if self.mode == Mode::Menu {
            if let Some(item) = name
                .strip_prefix("menu:")
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| MenuItem::ALL.get(index).copied())
            {
                self.run_menu(item);
            }
        } else if self.mode == Mode::Progress {
            if let Some(job) = self.job.as_mut()
                && let Some(index) = name
                    .strip_prefix("job:")
                    .and_then(|index| index.parse().ok())
            {
                self.job_close_at = None;
                job.pick_index(index);
            }
        } else if self.mode == Mode::Settings {
            if let Some(form) = self.form.as_mut() {
                form.click(&name);
            }
        } else if self.mode == Mode::Add {
            self.add_selected = Some(name);
            if double {
                self.clone_selected();
            }
        } else if matches!(self.mode, Mode::List | Mode::Filter) {
            self.selected = Some(name.clone());
            // A click on a group's header folds or unfolds it.
            if let Some(group) = name.strip_prefix(GROUP_KEY) {
                self.toggle_group(group);
            } else if double {
                self.update_one();
            }
        }
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Add => self.open_add(),
            Action::UpdateOne => self.update_chosen(),
            Action::UpdateAll => self.update_all(),
            Action::Remove => self.begin_remove(),
            Action::Refresh | Action::Retry => {
                if self.mode == Mode::Add && matches!(action, Action::Retry) {
                    self.retry_add();
                } else {
                    self.refresh();
                }
            }
            Action::Quit => self.quit(),
            Action::Confirm if self.mode == Mode::Path => self.accept_path(),
            Action::Confirm if self.mode == Mode::Group => self.accept_group(),
            Action::Cancel if self.mode == Mode::Menu => self.close_menu(),
            Action::Confirm if self.mode == Mode::Update => self.update_enter(),
            Action::Cancel if self.mode == Mode::Group => self.close_group(),
            Action::Confirm => self.confirm_remove(),
            Action::Forget => self.forget_chosen(),
            Action::Import => self.open_import(),
            Action::Cancel if self.mode == Mode::Path => self.close_path(),
            Action::Cancel if self.mode == Mode::Settings => self.close_settings(),
            Action::Cancel if self.mode == Mode::Progress => {
                if self.job.as_ref().is_some_and(Job::running) {
                    self.cancel_work();
                } else {
                    self.hide_job();
                }
            }
            Action::Hide => self.hide_job(),
            Action::Cancel => self.mode = Mode::List,
            Action::Clone => self.clone_selected(),
            Action::Settings => self.open_settings(false),
            Action::Save => self.save_settings(),
        }
    }

    fn move_by(&mut self, delta: isize) {
        if !matches!(self.mode, Mode::List | Mode::Filter | Mode::Add) {
            return;
        }
        let names = self.current_names();
        if names.is_empty() {
            return;
        }
        let slot = self.current_selected();
        let index = slot
            .as_ref()
            .and_then(|name| {
                names
                    .iter()
                    .position(|item| item.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);
        let next = (index as isize + delta).clamp(0, names.len() as isize - 1) as usize;
        let name = names[next].clone();
        if self.mode == Mode::Add {
            self.add_selected = Some(name);
        } else {
            self.selected = Some(name);
        }
    }

    fn move_to_end(&mut self, start: bool) {
        let names = self.current_names();
        let Some(name) = (if start { names.first() } else { names.last() }).cloned() else {
            return;
        };
        if self.mode == Mode::Add {
            self.add_selected = Some(name);
        } else {
            self.selected = Some(name);
        }
    }

    fn current_names(&self) -> Vec<String> {
        if self.mode == Mode::Add {
            self.add_list().into_iter().map(|repo| repo.id()).collect()
        } else {
            self.rows().iter().map(Row::key).collect()
        }
    }

    fn current_selected(&self) -> Option<String> {
        if self.mode == Mode::Add {
            self.add_selected.clone()
        } else {
            self.selected.clone()
        }
    }

    pub fn ensure_selection(&mut self) {
        if self.mode == Mode::Add {
            let names = self.current_names();
            if names.is_empty() {
                return;
            }
            let valid = self
                .add_selected
                .as_ref()
                .is_some_and(|name| names.iter().any(|item| item.eq_ignore_ascii_case(name)));
            if !valid {
                self.add_selected = self.preferred_add_id();
            }
            return;
        }
        let still_here =
            self.selected
                .as_ref()
                .is_some_and(|name| match name.strip_prefix(GROUP_KEY) {
                    Some(group) => self.installed.iter().any(|repo| repo.group == group),
                    None => self.installed.iter().any(|repo| &repo.folder == name),
                });
        if !still_here {
            self.selected = self.current_names().into_iter().next();
        }
    }

    fn preferred_add_id(&self) -> Option<String> {
        let list = self.add_list();
        list.iter()
            .find(|repo| !repo.direct && !self.is_installed(repo))
            .or_else(|| list.iter().find(|repo| !self.is_installed(repo)))
            .or_else(|| list.first())
            .map(|repo| repo.id())
    }

    fn snap_visible_selection(&mut self) {
        let names = self.current_names();
        if names.is_empty() {
            return;
        }
        if self
            .selected
            .as_ref()
            .is_none_or(|name| !names.iter().any(|item| item == name))
        {
            self.selected = Some(names[0].clone());
        }
    }

    fn note_add_filter_changed(&mut self) {
        self.add_scroll = 0;
        self.plan_lookup();
        self.settle_add_selection();
    }

    /// Selects the repository the search names exactly. Otherwise keeps a
    /// listed selection, and leaves the "clone what I typed" row for the
    /// first real match.
    fn settle_add_selection(&mut self) {
        let list = self.add_list();
        let real = |id: &str| {
            list.iter()
                .any(|repo| !repo.direct && repo.id().eq_ignore_ascii_case(id))
        };
        if let Some(spec) = github::parse_repo_spec(&self.add_filter)
            && real(&spec.id())
        {
            self.add_selected = Some(spec.id());
            return;
        }
        if !self.add_selected.as_deref().is_some_and(real) {
            self.add_selected = self.preferred_add_id();
        }
    }

    /// Schedules a GitHub lookup for the account in the search box, unless
    /// its repositories are already at hand.
    fn plan_lookup(&mut self) {
        self.lookup_due = None;
        let Some(query) = self.add_query() else {
            return;
        };
        let key = query.owner.to_ascii_lowercase();
        if self.in_catalog(&query.owner)
            || self.lookups.contains_key(&key)
            || self.lookup_running.as_deref() == Some(key.as_str())
        {
            return;
        }
        self.lookup_due = Some((query.owner, Instant::now() + LOOKUP_DELAY));
    }

    fn start_due_lookup(&mut self) -> bool {
        let due = self
            .lookup_due
            .as_ref()
            .is_some_and(|(_, at)| Instant::now() >= *at);
        if !due {
            return false;
        }
        let Some((owner, _)) = self.lookup_due.take() else {
            return false;
        };
        if self.mode != Mode::Add {
            return false;
        }
        self.start_lookup(owner);
        true
    }

    fn start_lookup(&mut self, owner: String) {
        let key = owner.to_ascii_lowercase();
        self.lookup_running = Some(key.clone());
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = github::fetch_account(&owner);
            let _ = tx.send(Msg::Account { key, result });
        });
    }

    fn retry_add(&mut self) {
        match self.add_query() {
            Some(query) if !self.in_catalog(&query.owner) => {
                let key = query.owner.to_ascii_lowercase();
                if self.lookup_running.as_deref() != Some(key.as_str()) {
                    self.lookups.remove(&key);
                    self.lookup_due = None;
                    self.start_lookup(query.owner);
                }
            }
            _ => self.reload_github_only(),
        }
    }

    /// Turns a plain search word into `word/`, which lists that account.
    fn list_typed_account(&mut self) {
        if let Some(word) = self.add_word() {
            self.add_filter = format!("{word}/");
            self.note_add_filter_changed();
        }
    }

    fn open_add(&mut self) {
        self.mode = Mode::Add;
        self.add_filter.clear();
        self.add_scroll = 0;
        self.add_selected = None;
        self.add_marked.clear();
        self.lookup_due = None;
        if matches!(self.remotes_state, RemoteState::Failed(_)) {
            self.reload_github_only();
        }
    }

    fn open_settings(&mut self, welcome: bool) {
        self.form = Some(Form::new(
            welcome,
            &self.owners,
            &self.pinned,
            self.protocol,
            self.custom_title.as_deref(),
            self.config_path.clone(),
        ));
        self.mode = Mode::Settings;
    }

    fn close_settings(&mut self) {
        let welcome = self.form.take().is_some_and(|form| form.welcome);
        self.mode = Mode::List;
        if welcome {
            self.notice("Setup skipped. Press c to open the settings.", false);
        }
    }

    fn save_settings(&mut self) {
        let Some(form) = self.form.as_mut() else {
            return;
        };
        let Some(mut saved) = form.validate() else {
            return;
        };
        saved.clone_dir = Some(self.clone_dir.clone());
        saved.checkouts = self.checkouts.clone();
        saved.placement = self.placement.clone();
        saved.collapsed = self.collapsed.clone();
        saved.update_check = self.update_check;
        if let Err(err) = config::save(&form.path, &saved) {
            form.error = Some(err);
            return;
        }
        let path = form.path.display().to_string();
        self.form = None;
        self.mode = Mode::List;
        self.owners = saved.owners;
        self.owners_explicit = true;
        self.pinned = saved.pinned;
        self.protocol = saved.protocol;
        self.title = saved.title.clone().unwrap_or_else(config::default_title);
        self.custom_title = saved.title;
        self.notice(format!("Settings saved to {path}."), false);
        self.reload_github_only();
    }

    fn toggle_sort(&mut self) {
        self.sort = match self.sort {
            SortMode::Name => SortMode::NeedsUpdate,
            SortMode::NeedsUpdate => SortMode::Name,
        };
        self.list_scroll = 0;
        let label = match self.sort {
            SortMode::Name => "Sorted by name.",
            SortMode::NeedsUpdate => "Sorted by what needs attention.",
        };
        self.notice(label, false);
    }

    fn open_selected(&mut self) {
        let folders = self.chosen();
        if folders.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        if folders.len() > OPEN_LIMIT {
            self.notice(
                format!(
                    "{} folders are too many to open at once. Select {OPEN_LIMIT} or fewer.",
                    folders.len()
                ),
                true,
            );
            return;
        }
        let mut failed = None;
        for folder in &folders {
            let Some(repo) = self.installed.iter().find(|repo| repo.folder == *folder) else {
                continue;
            };
            if let Err(err) = gitops::open_folder(&repo.path) {
                failed = Some(err);
            }
        }
        match (failed, folders.as_slice()) {
            (Some(err), _) => self.notice(err, true),
            (None, [folder]) => self.notice(format!("Opened {folder}."), false),
            (None, _) => self.notice(format!("Opened {} folders.", folders.len()), false),
        }
    }

    /// Updates the ticked repositories, or the highlighted one.
    fn update_chosen(&mut self) {
        let folders = self.chosen();
        if folders.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        self.marked.clear();
        self.mode = Mode::List;
        self.submit(Queued::Update(folders));
    }

    /// Updates the highlighted repository, whatever is ticked.
    fn update_one(&mut self) {
        if self.selected_group().is_some() {
            self.update_chosen();
            return;
        }
        let Some(folder) = self.selected.clone() else {
            self.notice("Select a repository first.", true);
            return;
        };
        self.mode = Mode::List;
        self.submit(Queued::Update(vec![folder]));
    }

    fn update_all(&mut self) {
        if self.installed.is_empty() {
            self.notice("There are no repositories to update.", true);
            return;
        }
        self.mode = Mode::List;
        self.submit(Queued::UpdateAll);
    }

    /// Updates the named folders: one on its own, several as a batch.
    fn update_folders(&mut self, folders: &[String]) {
        let mut work: Vec<Planned> = self
            .installed
            .iter()
            .filter(|repo| folders.contains(&repo.folder))
            .map(|repo| Planned {
                folder: repo.folder.clone(),
                path: repo.path.clone(),
                expected: repo.incoming_branches(),
                skip: false,
                gone: repo.ready && repo.error.is_none() && repo.upstream_gone,
            })
            .collect();
        if work.len() > 1 {
            let total = work.len();
            self.update_batch(work, 0, total);
            return;
        }
        let Some(Planned {
            folder,
            path,
            expected,
            ..
        }) = work.pop()
        else {
            self.notice(
                format!("{} is no longer in this folder.", folders.join(", ")),
                true,
            );
            return;
        };
        let job = Job::new(format!("Updating {folder}"), vec![folder.clone()], 0);
        self.start_job(format!("Updating {folder}…"), Some(job), move |tx| {
            let token = github::token();
            send(tx, JobEvent::Start(0));
            if pull_one(tx, &path, token.as_deref(), expected)? {
                Ok(format!("{folder} is already up to date."))
            } else {
                Ok(format!("Updated {folder}."))
            }
        });
    }

    fn run_update_all(&mut self) {
        let total = self.installed.len();
        if total == 0 {
            self.notice("There are no repositories to update.", true);
            return;
        }
        let work: Vec<Planned> = self
            .installed
            .iter()
            .map(|repo| {
                let gone = repo.ready && repo.error.is_none() && repo.upstream_gone;
                let skip = !gone
                    && repo.ready
                    && repo.fetched
                    && repo.error.is_none()
                    && repo.warning.is_none()
                    && repo.behind == Some(0)
                    && !repo.has_incoming();
                Planned {
                    folder: repo.folder.clone(),
                    path: repo.path.clone(),
                    expected: repo.incoming_branches(),
                    skip,
                    gone,
                }
            })
            .filter(|item| !item.skip)
            .collect();
        let skipped = total - work.len();
        if work.is_empty() {
            self.notice(format!("All {total} repositories are up to date."), false);
            return;
        }
        self.update_batch(work, skipped, total);
    }

    /// Updates `work` one after another in the progress dialog. `skipped`
    /// of the `total` were left out because they had nothing new.
    fn update_batch(&mut self, work: Vec<Planned>, skipped: usize, total: usize) {
        let count = work.len();
        let names = work.iter().map(|item| item.folder.clone()).collect();
        let job = Job::new(format!("Updating {}", count_label(count)), names, skipped);
        let label = format!("Updating {}…", count_label(count));
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let mut updated = 0;
            let mut current = skipped;
            let mut failed = Vec::new();
            for (index, item) in work.iter().enumerate() {
                if gitops::was_cancelled() {
                    return Err("cancelled".into());
                }
                if item.gone {
                    failed.push(item.folder.clone());
                    send(tx, JobEvent::Failed(index, "upstream is gone".into()));
                    continue;
                }
                send(tx, JobEvent::Start(index));
                match pull_one(tx, &item.path, token.as_deref(), item.expected) {
                    Ok(true) => {
                        current += 1;
                        send(tx, JobEvent::Done(index, "up to date".into()));
                    }
                    Ok(false) => {
                        updated += 1;
                        send(tx, JobEvent::Done(index, "updated".into()));
                    }
                    Err(err) => {
                        if gitops::was_cancelled() {
                            return Err(err);
                        }
                        failed.push(item.folder.clone());
                        send(tx, JobEvent::Failed(index, err));
                    }
                }
            }
            if failed.is_empty() {
                if updated == 0 {
                    Ok(format!("All {total} repositories are up to date."))
                } else if current == 0 {
                    Ok(format!("Updated {updated}."))
                } else {
                    Ok(format!("Updated {updated}. {current} already up to date."))
                }
            } else {
                Err(format!(
                    "Updated {updated}, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ))
            }
        });
    }

    fn begin_remove(&mut self) {
        if self.marked.is_empty()
            && let Some(group) = self.selected_group()
        {
            self.dissolve_group(&group);
            return;
        }
        let folders = self.chosen();
        if folders.is_empty() {
            self.notice("Select a repository to remove.", true);
            return;
        }
        self.mode = Mode::ConfirmRemove { folders };
    }

    fn confirm_remove(&mut self) {
        let Mode::ConfirmRemove { folders } = self.mode.clone() else {
            return;
        };
        self.mode = Mode::List;
        self.marked.clear();
        self.submit(Queued::Remove(folders));
    }

    fn remove_folders(&mut self, folders: Vec<String>) {
        let targets: Vec<(String, PathBuf)> = self
            .installed
            .iter()
            .filter(|repo| folders.contains(&repo.folder))
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        if targets.is_empty() {
            self.notice(
                format!("{} is no longer on the list.", folders.join(", ")),
                true,
            );
            return;
        }
        self.removing
            .extend(targets.iter().map(|(_, path)| path.clone()));
        if let [(folder, path)] = targets.as_slice() {
            let (folder, path) = (folder.clone(), path.clone());
            self.start_job(format!("Removing {folder}…"), None, move |_tx| {
                remove_or_gone(&path)?;
                Ok(format!("Removed {folder}."))
            });
            return;
        }
        let label = format!("Removing {}…", count_label(targets.len()));
        self.start_job(label, None, move |_tx| {
            let mut removed = 0;
            let mut failed = Vec::new();
            for (folder, path) in &targets {
                if gitops::was_cancelled() {
                    return Err("cancelled".into());
                }
                match remove_or_gone(path) {
                    Ok(()) => removed += 1,
                    Err(err) => failed.push(format!("{folder} ({err})")),
                }
            }
            if failed.is_empty() {
                Ok(format!("Removed {}.", count_label(removed)))
            } else {
                Err(format!(
                    "Removed {removed}, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ))
            }
        });
    }

    /// Enter in Add: asks where the chosen repositories should go.
    fn clone_selected(&mut self) {
        if !self.add_marked.is_empty() {
            let remotes: Vec<RemoteRepo> = self
                .add_marked
                .iter()
                .filter(|remote| !self.is_installed(remote))
                .cloned()
                .collect();
            if remotes.is_empty() {
                self.notice("Those repositories are on the list already.", true);
                return;
            }
            self.prompt = Some(PathPrompt::new(Purpose::Clone(remotes), &self.clone_dir));
            self.mode = Mode::Path;
            return;
        }
        if self.add_list().is_empty() && self.add_word().is_some() {
            self.list_typed_account();
            return;
        }
        let Some(id) = self.add_selected.clone() else {
            self.notice("Select a repository to add.", true);
            return;
        };
        let Some(remote) = self
            .add_list()
            .into_iter()
            .find(|repo| repo.id().eq_ignore_ascii_case(&id))
        else {
            self.notice("Select a repository to add.", true);
            return;
        };
        if self.is_installed(&remote) {
            self.notice(format!("{} is on the list already.", remote.id()), true);
            return;
        }
        self.prompt = Some(PathPrompt::new(
            Purpose::Clone(vec![remote]),
            &self.clone_dir,
        ));
        self.mode = Mode::Path;
    }

    /// Clones `remotes` into `dir`, one after another. One failure does
    /// not stop the rest. A checkout of the same repository that already
    /// sits there is put on the list instead.
    fn clone_remotes(&mut self, remotes: Vec<RemoteRepo>, dir: PathBuf) {
        let jobs: Vec<(String, String, String)> = remotes
            .into_iter()
            .filter(|remote| !self.is_installed(remote))
            .map(|remote| {
                let url = gitops::clone_url(
                    &remote.owner,
                    &remote.name,
                    self.protocol,
                    &self.installed,
                    &remote.clone_url,
                );
                (remote.owner, remote.name, url)
            })
            .collect();
        if jobs.is_empty() {
            self.notice("Those repositories are on the list already.", true);
            return;
        }
        if let Err(err) = std::fs::create_dir_all(&dir) {
            self.notice(format!("could not create {}: {err}", dir.display()), true);
            return;
        }
        let root = dir;
        let count = jobs.len();
        let names: Vec<String> = jobs
            .iter()
            .map(|(owner, name, _)| format!("{owner}/{name}"))
            .collect();
        let (title, label) = match jobs.as_slice() {
            [(owner, name, _)] => (
                format!("Cloning {owner}/{name}"),
                format!("Cloning {name}…"),
            ),
            _ => (
                format!("Cloning {}", count_label(count)),
                format!("Cloning {}…", count_label(count)),
            ),
        };
        let job = Job::new(title, names, 0);
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let single = jobs.len() == 1;
            let mut cloned = 0;
            let mut adopted = 0;
            // Local branches made for the other branches of the clones.
            let mut branches = 0;
            let mut failed = Vec::new();
            for (index, (owner, name, url)) in jobs.iter().enumerate() {
                if gitops::was_cancelled() {
                    return Err("cancelled".into());
                }
                let id = format!("{owner}/{name}");
                if let Some(path) = gitops::existing_checkout(&root, owner, name) {
                    send(tx, JobEvent::Start(index));
                    send_line(
                        tx,
                        &format!(
                            "{} is a checkout of {id} already. It is on the list now.",
                            path.display()
                        ),
                        false,
                    );
                    let _ = tx.send(Msg::Cloned(path));
                    send(tx, JobEvent::Done(index, "already there".into()));
                    adopted += 1;
                    continue;
                }
                let dest = match gitops::clone_destination(&root, owner, name) {
                    Ok(dest) => dest,
                    Err(err) if single => return Err(err),
                    Err(err) => {
                        send(tx, JobEvent::Failed(index, err));
                        failed.push(id);
                        continue;
                    }
                };
                send(tx, JobEvent::Start(index));
                match gitops::clone_repo(url, &dest, token.as_deref(), |line, progress| {
                    send_line(tx, line, progress)
                }) {
                    Ok(()) => {
                        cloned += 1;
                        // A clone only checks out the default branch. The
                        // others get local branches, as an update makes them.
                        match gitops::track_remote_branches(&dest, |line, progress| {
                            send_line(tx, line, progress)
                        }) {
                            Ok(done) => branches += done.created.len(),
                            Err(err) => {
                                send_line(tx, &format!("other branches: {err}"), false);
                            }
                        }
                        let _ = tx.send(Msg::Cloned(dest));
                        send(tx, JobEvent::Done(index, "cloned".into()));
                    }
                    Err(err) => {
                        if single || gitops::was_cancelled() {
                            return Err(err);
                        }
                        send(tx, JobEvent::Failed(index, err));
                        failed.push(id);
                    }
                }
            }
            let place = root.display();
            let extra = match branches {
                0 => String::new(),
                1 => " 1 more branch was set up.".to_string(),
                count => format!(" {count} more branches were set up."),
            };
            match (failed.is_empty(), jobs.as_slice()) {
                (true, [(_, name, _)]) if adopted == 1 => Ok(format!(
                    "{name} was in {place} already. It is on the list now."
                )),
                (true, [(_, name, _)]) => Ok(format!("Cloned {name} into {place}.{extra}")),
                (true, _) if adopted > 0 => Ok(format!(
                    "Cloned {cloned} into {place}. {adopted} already there.{extra}"
                )),
                (true, _) => Ok(format!(
                    "Cloned {} into {place}.{extra}",
                    count_label(cloned)
                )),
                (false, _) => Err(format!(
                    "Cloned {cloned}, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                )),
            }
        });
    }

    /// Fetches and re-reads everything, unless that is happening or queued
    /// already.
    fn refresh(&mut self) {
        if self.fetching && !self.busy {
            self.notice("Already checking the repositories.", false);
        } else if self
            .queue
            .iter()
            .any(|task| matches!(task, Queued::Refresh))
        {
            self.notice("A refresh is already queued.", false);
        } else {
            self.submit(Queued::Refresh);
        }
    }

    /// Runs `task` now, or queues it behind whatever is running.
    fn submit(&mut self, task: Queued) {
        if !(self.busy || self.fetching) && self.queue.is_empty() {
            self.dialog_hidden = false;
            self.begin(task);
            return;
        }
        let label = task.label();
        self.queue.push_back(task);
        let text = match self.queue.len() {
            1 => format!("Queued: {label}. It starts when the current task is done."),
            count => format!("Queued: {label}. {count} tasks are waiting."),
        };
        self.notice(text, false);
    }

    fn begin(&mut self, task: Queued) {
        match task {
            Queued::Update(folders) => self.update_folders(&folders),
            Queued::UpdateAll => self.run_update_all(),
            Queued::Remove(folders) => self.remove_folders(folders),
            Queued::Clone(remotes, dir) => self.clone_remotes(remotes, dir),
            Queued::Import(dir) => self.import_folder(&dir),
            Queued::Refresh => self.reload_lists(false, true),
        }
    }

    /// Starts the oldest queued task once nothing else runs. A run that
    /// failed stays on screen first: the queue waits until its dialog is
    /// closed.
    fn start_queued(&mut self) -> bool {
        if self.busy || self.fetching || self.queue.is_empty() {
            return false;
        }
        let failed_on_screen = self.mode == Mode::Progress
            && self
                .job
                .as_ref()
                .is_some_and(|job| matches!(job.result, Some(Err(_))));
        if failed_on_screen {
            return false;
        }
        let Some(task) = self.queue.pop_front() else {
            return false;
        };
        // A dialog the user hid stays hidden for the runs that follow.
        let before = self.mode.clone();
        self.begin(task);
        if self.dialog_hidden && before != Mode::Progress && self.mode == Mode::Progress {
            self.mode = before;
        }
        true
    }

    /// Forgets everything that waits. Returns how many tasks that was.
    fn drop_queue(&mut self) -> usize {
        let dropped = self.queue.len();
        self.queue.clear();
        dropped
    }

    fn cancel_work(&mut self) {
        if self.busy || self.fetching {
            gitops::request_cancel();
            if let Some(job) = self.job.as_mut().filter(|job| job.running()) {
                job.cancelling = true;
            }
            match self.drop_queue() {
                0 => self.notice("Cancelling…", false),
                1 => self.notice("Cancelling… The queued task is dropped too.", false),
                count => self.notice(
                    format!("Cancelling… The {count} queued tasks are dropped too."),
                    false,
                ),
            }
        }
    }

    fn quit(&mut self) {
        // Stop git only if this app started some. The cancel flag is shared
        // by everything in the process, so it is not raised without need.
        if self.busy || self.fetching || self.status_pending > 0 {
            gitops::request_cancel();
        }
        self.should_quit = true;
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.status_error = error;
    }

    fn ready_message(&self) -> String {
        let behind = self
            .installed
            .iter()
            .filter(|repo| repo.sync_counts().1 > 0)
            .count();
        let mut text = if self.installed.is_empty() {
            "No repositories installed.".to_string()
        } else if behind == 0 {
            format!("{} installed.", self.installed.len())
        } else if behind == 1 {
            format!("{} installed. 1 is behind.", self.installed.len())
        } else {
            format!("{} installed. {behind} are behind.", self.installed.len())
        };
        match self
            .installed
            .iter()
            .filter(|repo| repo.remote_new > 0)
            .count()
        {
            0 => {}
            1 => text.push_str(" 1 has new commits on other branches."),
            n => text.push_str(&format!(" {n} have new commits on other branches.")),
        }
        text
    }

    /// Runs `work` on its own thread. With a `job`, the progress dialog
    /// opens and follows it; without one, only the status line does.
    fn start_job<F>(&mut self, label: String, job: Option<Job>, work: F)
    where
        F: FnOnce(&Sender<Msg>) -> Result<String, String> + Send + 'static,
    {
        gitops::clear_cancel();
        self.busy = true;
        self.job_close_at = None;
        if let Some(job) = job {
            self.job = Some(job);
            // Someone in another dialog is not pulled out of it.
            if matches!(self.mode, Mode::List | Mode::Filter | Mode::Progress) {
                self.mode = Mode::Progress;
            }
        }
        self.notice(label, false);
        let tx = self.tx.clone();
        thread::spawn(move || {
            let result = work(&tx);
            let _ = tx.send(Msg::JobFinished(result));
        });
    }

    pub fn reload_lists(&mut self, keep_status: bool, fetch: bool) {
        if self.busy || self.fetching {
            self.notice("Wait for the current Git command to finish.", true);
            return;
        }
        gitops::clear_cancel();
        self.generation += 1;
        self.fetching = fetch;
        self.fetch_after_read = false;
        self.load_rows();
        self.prune_marks();
        if !keep_status {
            self.notice(
                if fetch {
                    format!("Checking {} repositories…", self.installed.len())
                } else {
                    format!("Reading {} repositories…", self.installed.len())
                },
                false,
            );
        }
        self.spawn_status(self.generation, fetch);
        self.reload_github_only();
    }

    /// True once: when the first local read after startup is done and
    /// nothing else is going on.
    fn startup_fetch_due(&mut self) -> bool {
        std::mem::take(&mut self.fetch_after_read)
            && !self.busy
            && !self.installed.is_empty()
            && !gitops::was_cancelled()
    }

    /// Fetches every repository in the background. The rows keep what
    /// they show and change as each fetch lands.
    fn fetch_in_place(&mut self) {
        self.generation += 1;
        self.fetching = true;
        self.notice(
            format!("Checking {} repositories…", self.installed.len()),
            false,
        );
        self.spawn_status(self.generation, true);
    }

    fn reload_github_only(&mut self) {
        self.remote_generation += 1;
        self.remotes_state = RemoteState::Loading;
        let generation = self.remote_generation;
        let owners = self.owners.clone();
        let pinned = self.pinned.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let catalog = github::fetch_catalog(&owners, &pinned);
            let _ = tx.send(Msg::Github {
                generation,
                catalog,
            });
        });
    }

    fn spawn_status(&mut self, generation: u64, fetch: bool) {
        let repos: Vec<(String, PathBuf)> = self
            .installed
            .iter()
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        self.status_pending = repos.len();
        if repos.is_empty() {
            self.fetching = false;
            if !self.busy
                && (self.status.starts_with("Reading") || self.status.starts_with("Checking"))
            {
                self.notice("No repositories installed.".to_string(), false);
            }
            return;
        }
        let token = if fetch { github::token() } else { None };
        let queue = std::sync::Arc::new(std::sync::Mutex::new(repos));
        let workers = (if fetch { 4 } else { 8 }).min(self.status_pending.max(1));
        for _ in 0..workers {
            let queue = queue.clone();
            let tx = self.tx.clone();
            let token = token.clone();
            thread::spawn(move || {
                loop {
                    let next = queue.lock().ok().and_then(|mut queue| queue.pop());
                    let Some((folder, path)) = next else {
                        break;
                    };
                    let fetch_this = fetch && !gitops::was_cancelled();
                    let check = gitops::check(&path, fetch_this, token.as_deref());
                    if tx
                        .send(Msg::Status {
                            generation,
                            folder,
                            check,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
    }

    fn on_msg(&mut self, message: Msg) {
        match message {
            Msg::Status {
                generation,
                folder,
                check,
            } => {
                if generation != self.generation {
                    return;
                }
                if let Some(repo) = self.installed.iter_mut().find(|repo| repo.folder == folder) {
                    repo.warning = check.warning;
                    repo.fetched = check.fetched;
                    match check.info {
                        Ok(info) => {
                            repo.apply(info);
                        }
                        Err(err) => {
                            repo.ready = true;
                            repo.error = Some(err);
                        }
                    }
                }
                self.status_pending = self.status_pending.saturating_sub(1);
                if self.status_pending == 0 {
                    if self.startup_fetch_due() {
                        self.fetch_in_place();
                        return;
                    }
                    self.fetching = false;
                    if gitops::was_cancelled() {
                        gitops::clear_cancel();
                        if !self.busy {
                            self.notice("Cancelled.".to_string(), false);
                        }
                    } else if !self.busy
                        && (self.status.starts_with("Reading")
                            || self.status.starts_with("Checking"))
                    {
                        self.notice(self.ready_message(), false);
                    }
                }
            }
            Msg::Github {
                generation,
                catalog,
            } => {
                if generation != self.remote_generation {
                    return;
                }
                let failed = catalog.repos.is_empty() && !catalog.errors.is_empty();
                self.catalog_warning = if catalog.errors.is_empty() {
                    None
                } else {
                    Some(catalog.errors.join(" · "))
                };
                if failed {
                    let err = self
                        .catalog_warning
                        .clone()
                        .unwrap_or_else(|| "GitHub request failed".to_string());
                    self.remotes.clear();
                    self.remotes_state = RemoteState::Failed(err.clone());
                    if !self.busy {
                        self.notice(err, true);
                    }
                } else {
                    self.remotes = catalog.repos;
                    self.remotes_state = RemoteState::Ready;
                    if let Some(warning) = self.catalog_warning.clone() {
                        if !self.busy && !self.fetching {
                            self.notice(warning, true);
                        }
                    } else if self.status_error && !self.busy && !self.fetching {
                        self.notice(self.ready_message(), false);
                    }
                }
            }
            Msg::Account { key, result } => {
                if self.lookup_running.as_deref() == Some(key.as_str()) {
                    self.lookup_running = None;
                }
                self.lookups.insert(key, result);
                if self.mode == Mode::Add {
                    self.settle_add_selection();
                }
            }
            Msg::Update(result) => {
                self.update = match result {
                    Ok(Some(release)) => {
                        if !self.busy && self.mode != Mode::Update {
                            self.notice(
                                format!(
                                    "DevAssist {} is available. Press v to install it.",
                                    release.version
                                ),
                                false,
                            );
                        }
                        UpdateState::Available(release)
                    }
                    Ok(None) => UpdateState::Current,
                    // Nobody asked: a failed look at startup is not news.
                    Err(_) if self.update_quiet => UpdateState::Unknown,
                    Err(err) => UpdateState::Failed(err),
                };
            }
            Msg::UpdateProgress(done, total) => {
                if let UpdateState::Installing {
                    done: shown,
                    total: of,
                    ..
                } = &mut self.update
                {
                    *shown = done;
                    *of = total;
                }
            }
            Msg::UpdateDone(result) => {
                let version = match &self.update {
                    UpdateState::Installing { release, .. } => release.version.clone(),
                    _ => String::new(),
                };
                self.update = match result {
                    Ok(()) => {
                        self.notice(
                            format!("DevAssist {version} is installed. Restart to use it."),
                            false,
                        );
                        UpdateState::Installed(version)
                    }
                    Err(err) => UpdateState::Failed(err),
                };
            }
            Msg::Login(login) => {
                if let (Some(form), Some(login)) = (self.form.as_mut(), login) {
                    form.suggest_account(&login);
                }
            }
            Msg::Job(event) => {
                if let Some(job) = self.job.as_mut() {
                    job.on(event);
                }
            }
            Msg::Cloned(path) => {
                if config::push_path(&mut self.checkouts, path) {
                    self.save_config();
                }
            }
            Msg::JobFinished(result) => {
                self.busy = false;
                let cancelled = result.is_err() && gitops::was_cancelled();
                if let Some(job) = self.job.as_mut().filter(|job| job.running()) {
                    job.finish(result.clone(), cancelled);
                    if self.mode == Mode::Progress && result.is_ok() {
                        self.job_close_at = Some(Instant::now() + JOB_LINGER);
                    }
                }
                match result {
                    Err(_) if cancelled => self.notice("Cancelled.".to_string(), false),
                    Ok(message) => self.notice(message, false),
                    Err(err) => self.notice(err, true),
                }
                gitops::clear_cancel();
                self.generation += 1;
                self.fetching = false;
                self.settle_removed();
                self.load_rows();
                self.prune_marks();
                self.spawn_status(self.generation, false);
            }
        }
    }
}

fn send(tx: &Sender<Msg>, event: JobEvent) {
    let _ = tx.send(Msg::Job(event));
}

fn send_line(tx: &Sender<Msg>, line: &str, progress: bool) {
    send(tx, JobEvent::Line(line.to_string(), progress));
}

/// Updates one checkout on every branch. True means nothing moved.
/// `expected` is what the last check saw, shown until the fetch has
/// counted the branches again.
fn pull_one(
    tx: &Sender<Msg>,
    path: &Path,
    token: Option<&str>,
    expected: u32,
) -> Result<bool, String> {
    if expected > 0 {
        send(tx, JobEvent::Steps(0, expected));
    }
    let updated = gitops::update(
        path,
        token,
        |line, progress| send_line(tx, line, progress),
        |done, total| send(tx, JobEvent::Steps(done, total)),
    )?;
    Ok(!updated.changed())
}

fn list_matches(repo: &LocalRepo, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let filter = filter.to_lowercase();
    repo.folder.to_lowercase().contains(&filter)
        || repo.repo.to_lowercase().contains(&filter)
        || repo.owner.to_lowercase().contains(&filter)
        || repo.branch.to_lowercase().contains(&filter)
        || repo.path.to_string_lossy().to_lowercase().contains(&filter)
}

/// Tidies a typed group name. Empty means "no group".
fn clean_group_name(input: &str) -> Result<String, String> {
    let name = input.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.chars().any(char::is_control) {
        return Err("A group name cannot hold control characters.".to_string());
    }
    if name.chars().count() > GROUP_NAME_LIMIT {
        return Err(format!(
            "A group name can be {GROUP_NAME_LIMIT} characters at most."
        ));
    }
    if name.starts_with(GROUP_KEY) {
        return Err(format!("A group name cannot start with {GROUP_KEY}"));
    }
    Ok(name)
}

/// Deletes a checkout. One whose folder is gone already counts as removed.
fn remove_or_gone(path: &Path) -> Result<(), String> {
    if path.exists() {
        gitops::remove_checkout(path)
    } else {
        Ok(())
    }
}

fn remote_matches(repo: &RemoteRepo, filter: &str) -> bool {
    let filter = filter.trim();
    if filter.is_empty() {
        return true;
    }
    let filter = filter.to_lowercase();
    repo.name.to_lowercase().contains(&filter)
        || repo.owner.to_lowercase().contains(&filter)
        || repo.id().to_lowercase().contains(&filter)
        || repo.description.to_lowercase().contains(&filter)
        || repo.language.to_lowercase().contains(&filter)
}

fn account_matches(repo: &RemoteRepo, query: &AccountQuery) -> bool {
    repo.owner.eq_ignore_ascii_case(&query.owner)
        && repo
            .name
            .to_lowercase()
            .contains(&query.rest.to_lowercase())
}

fn count_label(count: usize) -> String {
    if count == 1 {
        "1 repository".to_string()
    } else {
        format!("{count} repositories")
    }
}

pub(crate) fn delete_word(text: &mut String) {
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| !ch.is_whitespace())
    {
        text.pop();
    }
    while text
        .chars()
        .next_back()
        .is_some_and(|ch| ch.is_whitespace())
    {
        text.pop();
    }
}

fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x
        && y >= rect.y
        && x < rect.x.saturating_add(rect.width)
        && y < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> Settings {
        Settings {
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: false,
            update_check_now: false,
            clone_dir: std::env::temp_dir().join("devassist-clones"),
            owners_explicit: true,
            unsaved: false,
            title: "Test".into(),
            custom_title: None,
            owners: vec!["storytold".into()],
            pinned: Vec::new(),
            protocol: gitops::Protocol::Https,
            doctor: false,
            config_path: std::env::temp_dir().join(format!(
                "devassist-test-{}-{:?}.toml",
                std::process::id(),
                thread::current().id()
            )),
            first_run: false,
        }
    }

    /// A folder that reads as a GitHub checkout: `.git/config` with an origin.
    fn fake_checkout(parent: &Path, name: &str) -> PathBuf {
        let path = parent.join(name);
        std::fs::create_dir_all(path.join(".git")).unwrap();
        std::fs::write(
            path.join(".git").join("config"),
            format!("[remote \"origin\"]\n\turl = https://github.com/acme/{name}.git\n"),
        )
        .unwrap();
        path
    }

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("devassist-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn adding_asks_for_the_folder_and_remembers_it() {
        let root = scratch("add");
        let mut app = App::empty(test_settings());
        app.clone_dir = root.join("first");
        app.remotes = vec![remote("acme", "one"), remote("acme", "two")];
        app.remotes_state = RemoteState::Ready;
        app.mode = Mode::Add;
        app.add_selected = Some("acme/one".into());
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.add_marked.len(), 2);

        // Enter opens the folder dialog with the last folder filled in.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Path);
        let prompt = app.prompt.as_ref().unwrap();
        assert_eq!(prompt.input, root.join("first").display().to_string());
        assert!(matches!(&prompt.purpose, Purpose::Clone(remotes) if remotes.len() == 2));

        // Esc goes back to Add and keeps what was ticked.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::Add);
        assert_eq!(app.add_marked.len(), 2);

        // Another folder is typed and accepted while something else runs.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.busy = true;
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        app.paste(root.join("second").display().to_string());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.add_marked.is_empty());
        assert!(config::same_path(&app.clone_dir, &root.join("second")));
        match app.queue.front() {
            Some(Queued::Clone(remotes, dir)) => {
                assert_eq!(remotes.len(), 2);
                assert!(config::same_path(dir, &root.join("second")));
            }
            _ => panic!("the clone should be queued"),
        }
        // The folder is remembered across launches.
        let file = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(file.contains(&format!("clone_dir = {}", root.join("second").display())));
        let _ = std::fs::remove_file(&app.config_path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_relative_folder_is_refused_in_the_dialog() {
        let mut app = App::empty(test_settings());
        press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
        assert!(app.mode == Mode::Path);
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        app.paste("some/folder".into());
        app.run(Action::Confirm);
        assert!(app.mode == Mode::Path);
        assert!(app.prompt.as_ref().unwrap().error.is_some());
        app.run(Action::Cancel);
        assert!(app.mode == Mode::List);
        assert!(app.prompt.is_none());
    }

    #[test]
    fn importing_a_folder_lists_its_clones_once() {
        let root = scratch("import");
        fake_checkout(&root, "api");
        fake_checkout(&root, "web");
        std::fs::create_dir_all(root.join("notes")).unwrap();
        let mut app = App::empty(test_settings());
        assert_eq!(app.register_folder(&root), Ok(2));
        assert_eq!(app.checkouts.len(), 2);
        assert_eq!(app.register_folder(&root), Ok(0));
        // A single checkout can be named directly.
        let lone = fake_checkout(&scratch("import-lone"), "tool");
        assert_eq!(app.register_folder(&lone), Ok(1));
        assert!(app.register_folder(&root.join("notes")).is_err());
        let file = std::fs::read_to_string(&app.config_path).unwrap();
        assert_eq!(file.matches("checkout = ").count(), 3);

        // While something runs, an import waits like everything else.
        app.clone_dir = root.clone();
        app.busy = true;
        press(&mut app, KeyCode::Char('i'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.queue.len(), 1);
        assert!(app.queue[0].label().starts_with("import "));
        let _ = std::fs::remove_file(&app.config_path);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(lone.parent().unwrap());
    }

    #[test]
    fn forgetting_takes_a_repository_off_the_list_and_keeps_its_folder() {
        let root = scratch("forget");
        let api = fake_checkout(&root, "api");
        let web = fake_checkout(&root, "web");
        let mut app = App::empty(test_settings());
        app.checkouts = vec![api.clone(), web.clone()];
        app.installed = gitops::load(&app.checkouts);
        app.selected = Some("api".into());
        press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(app.checkouts, vec![web.clone()]);
        assert_eq!(app.installed.len(), 1);
        assert!(api.is_dir(), "forgetting must not delete the folder");
        assert_eq!(app.status, "api is off the list. Its folder stays on disk.");
        let file = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(!file.contains(&format!("checkout = {}", api.display())));
        assert!(file.contains(&format!("checkout = {}", web.display())));

        // A clone that lands is listed; a removed one drops off once it is gone.
        app.on_msg(Msg::Cloned(api.clone()));
        app.on_msg(Msg::Cloned(api.clone()));
        assert_eq!(app.checkouts.len(), 2);
        app.removing = vec![api.clone(), web.clone()];
        std::fs::remove_dir_all(&web).unwrap();
        app.settle_removed();
        assert_eq!(app.checkouts, vec![api.clone()]);
        assert!(app.removing.is_empty());
        let _ = std::fs::remove_file(&app.config_path);
        let _ = std::fs::remove_dir_all(&root);
    }

    fn installed(owner: &str, repo: &str) -> LocalRepo {
        LocalRepo {
            folder: repo.into(),
            owner: owner.into(),
            repo: repo.into(),
            origin: format!("https://github.com/{owner}/{repo}.git"),
            path: PathBuf::from(repo),
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

    fn remote(owner: &str, name: &str) -> RemoteRepo {
        RemoteRepo {
            owner: owner.into(),
            name: name.into(),
            description: String::new(),
            clone_url: format!("https://github.com/{owner}/{name}.git"),
            language: "Rust".into(),
            archived: false,
            private: false,
            pushed: String::new(),
            direct: false,
        }
    }

    #[test]
    fn add_list_offers_a_repository_outside_the_catalog() {
        let mut app = App::empty(test_settings());
        app.add_filter = "other/tool".into();
        let list = app.add_list();
        assert_eq!(list.len(), 1);
        assert!(list[0].direct);
        assert_eq!(list[0].id(), "other/tool");
    }

    #[test]
    fn add_list_keeps_an_installed_repository_visible() {
        let mut app = App::empty(test_settings());
        app.installed.push(installed("other", "tool"));
        app.add_filter = "other/tool".into();
        let list = app.add_list();
        assert_eq!(list.len(), 1);
        assert!(app.is_installed(&list[0]));
    }

    #[test]
    fn add_list_shows_installed_catalog_repositories_but_prefers_a_new_one() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.installed.push(installed("storytold", "alpha"));
        app.remotes.push(remote("storytold", "alpha"));
        app.remotes.push(remote("storytold", "beta"));
        let list = app.add_list();
        assert_eq!(list.len(), 2);
        assert!(app.is_installed(&list[0]));
        assert!(!app.is_installed(&list[1]));
        app.ensure_selection();
        assert_eq!(app.add_selected.as_deref(), Some("storytold/beta"));
    }

    #[test]
    fn a_link_to_another_account_lists_its_repositories() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.remotes_state = RemoteState::Ready;
        app.remotes.push(remote("storytold", "alpha"));
        app.add_filter = "https://github.com/UCS-Care-Group".into();
        app.note_add_filter_changed();
        assert!(app.lookup_due.is_some());
        assert!(app.add_status().busy);
        assert!(app.add_list().is_empty());

        app.on_msg(Msg::Account {
            key: "ucs-care-group".into(),
            result: Ok(vec![
                remote("UCS-Care-Group", "portal"),
                remote("UCS-Care-Group", "api"),
            ]),
        });
        let ids: Vec<String> = app.add_list().iter().map(RemoteRepo::id).collect();
        assert_eq!(ids, vec!["UCS-Care-Group/api", "UCS-Care-Group/portal"]);
        assert_eq!(app.add_selected.as_deref(), Some("UCS-Care-Group/api"));
        assert!(app.add_status().text.contains("2 repositories"));

        app.add_filter = "ucs-care-group/por".into();
        app.note_add_filter_changed();
        assert!(app.lookup_due.is_none());
        let ids: Vec<String> = app.add_list().iter().map(RemoteRepo::id).collect();
        assert_eq!(ids, vec!["ucs-care-group/por", "UCS-Care-Group/portal"]);
        assert_eq!(app.add_selected.as_deref(), Some("UCS-Care-Group/portal"));
    }

    #[test]
    fn space_ticks_several_repositories_and_keeps_them_across_searches() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.remotes_state = RemoteState::Ready;
        app.installed.push(installed("storytold", "beta"));
        for name in ["alpha", "beta", "gamma"] {
            app.remotes.push(remote("storytold", name));
        }
        let space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE);
        app.ensure_selection();
        assert_eq!(app.add_selected.as_deref(), Some("storytold/alpha"));

        app.on_key(space);
        assert_eq!(app.add_selected.as_deref(), Some("storytold/beta"));
        app.on_key(space);
        assert_eq!(app.add_selected.as_deref(), Some("storytold/gamma"));
        app.on_key(space);
        let marked: Vec<String> = app.add_marked.iter().map(RemoteRepo::id).collect();
        assert_eq!(marked, vec!["storytold/alpha", "storytold/gamma"]);
        assert!(app.add_filter.is_empty());

        app.on_key(space);
        assert_eq!(app.add_marked.len(), 1);
        for ch in "zzz".chars() {
            app.on_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert!(app.add_list().is_empty());
        assert!(app.is_marked(&remote("Storytold", "Alpha")));
    }

    fn running_job(app: &mut App, names: &[&str]) {
        let names = names.iter().map(|name| (*name).to_string()).collect();
        app.job = Some(Job::new("Cloning", names, 0));
        app.mode = Mode::Progress;
        app.busy = true;
    }

    #[test]
    fn a_run_that_went_well_closes_its_dialog_by_itself() {
        let mut app = App::empty(test_settings());
        running_job(&mut app, &["a/one"]);
        app.on_msg(Msg::Job(JobEvent::Start(0)));
        app.on_msg(Msg::Job(JobEvent::Line(
            "Cloning into 'one'...".into(),
            false,
        )));
        app.on_msg(Msg::JobFinished(Ok("Cloned one.".into())));
        let job = app.job.as_ref().unwrap();
        assert!(!job.running());
        assert_eq!(job.items[0].output.len(), 1);
        assert!(app.mode == Mode::Progress);
        assert!(app.waiting());
        assert!(!app.close_finished_job());
        app.job_close_at = Some(Instant::now());
        assert!(app.close_finished_job());
        assert!(app.mode == Mode::List);
        assert_eq!(app.status, "Cloned one.");

        app.on_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert!(app.mode == Mode::Progress);
        assert!(app.job_close_at.is_none());
    }

    #[test]
    fn a_failed_run_keeps_its_dialog_open_until_it_is_closed() {
        let mut app = App::empty(test_settings());
        running_job(&mut app, &["a/one", "a/two"]);
        app.on_msg(Msg::Job(JobEvent::Start(0)));
        app.on_msg(Msg::Job(JobEvent::Failed(0, "fatal: not found".into())));
        app.on_msg(Msg::JobFinished(Err("Cloned 0, 1 failed: a/one".into())));
        assert!(app.mode == Mode::Progress);
        assert!(app.job_close_at.is_none());
        assert!(app.status_error);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.mode == Mode::List);
    }

    #[test]
    fn arrows_and_clicks_pick_a_repository_in_the_progress_dialog() {
        let mut app = App::empty(test_settings());
        running_job(&mut app, &["a/one", "a/two", "a/three"]);
        app.on_msg(Msg::Job(JobEvent::Start(0)));
        app.job_close_at = Some(Instant::now() + JOB_LINGER);
        app.on_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.job.as_ref().unwrap().shown(), 1);
        assert!(app.job_close_at.is_none());
        app.click_row("job:2".into());
        assert_eq!(app.job.as_ref().unwrap().shown(), 2);
        app.click_row("job:9".into());
        assert_eq!(app.job.as_ref().unwrap().shown(), 2);
    }

    #[test]
    fn hiding_a_running_job_leaves_it_running() {
        let mut app = App::empty(test_settings());
        running_job(&mut app, &["a/one"]);
        app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.mode == Mode::Progress);
        app.on_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert!(app.mode == Mode::List);
        assert!(app.busy);
        assert!(app.job.as_ref().unwrap().running());
        app.on_msg(Msg::JobFinished(Ok("Cloned one.".into())));
        assert!(app.mode == Mode::List);
        assert!(app.job_close_at.is_none());
    }

    #[test]
    fn a_default_account_is_not_looked_up_again() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.remotes_state = RemoteState::Ready;
        app.remotes.push(remote("storytold", "alpha"));
        app.remotes.push(remote("storytold", "beta"));
        app.add_filter = "storytold/al".into();
        app.note_add_filter_changed();
        assert!(app.lookup_due.is_none());
        let list = app.add_list();
        assert_eq!(list.len(), 2);
        assert!(list[0].direct);
        assert_eq!(app.add_selected.as_deref(), Some("storytold/alpha"));
    }

    #[test]
    fn a_plain_word_becomes_an_account_search_on_tab() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.remotes_state = RemoteState::Ready;
        app.add_filter = "UCS-Care-Group".into();
        assert!(
            app.add_empty_message()
                .contains("github.com/UCS-Care-Group")
        );
        app.list_typed_account();
        assert_eq!(app.add_filter, "UCS-Care-Group/");
        assert_eq!(app.add_account().as_deref(), Some("UCS-Care-Group"));
        app.add_filter = "two words".into();
        assert!(app.add_word().is_none());
    }

    #[test]
    fn a_failed_lookup_shows_the_error() {
        let mut app = App::empty(test_settings());
        app.mode = Mode::Add;
        app.remotes_state = RemoteState::Ready;
        app.add_filter = "@nobody".into();
        app.on_msg(Msg::Account {
            key: "nobody".into(),
            result: Err("GitHub user or organization nobody was not found".into()),
        });
        let status = app.add_status();
        assert!(status.error);
        assert!(status.text.contains("not found"));
    }

    #[test]
    fn saving_settings_writes_the_file_and_applies_them() {
        let path = std::env::temp_dir().join(format!(
            "devassist-test-{}-{:?}.toml",
            std::process::id(),
            thread::current().id()
        ));
        let mut settings = test_settings();
        settings.config_path = path.clone();
        let mut app = App::empty(settings);
        app.open_settings(true);
        assert!(app.mode == Mode::Settings);
        let form = app.form.as_mut().unwrap();
        form.accounts.clear();
        form.title = "Desk".into();
        form.click("protocol:ssh");
        app.save_settings();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(app.mode == Mode::List);
        assert!(app.form.is_none());
        assert!(app.owners.is_empty());
        assert_eq!(app.title, "Desk");
        assert!(app.protocol == gitops::Protocol::Ssh);
        assert!(text.contains("owners = none"));
        assert!(text.contains("title = Desk"));
        assert!(text.contains("protocol = ssh"));
    }

    #[test]
    fn a_mistake_keeps_the_settings_dialog_open() {
        let mut app = App::empty(test_settings());
        app.open_settings(false);
        app.form.as_mut().unwrap().accounts = "ada/tool".into();
        app.save_settings();
        assert!(app.mode == Mode::Settings);
        assert!(app.form.as_ref().unwrap().error.is_some());
        app.run(Action::Cancel);
        assert!(app.mode == Mode::List);
        assert_eq!(app.owners, vec!["storytold".to_string()]);
    }

    #[test]
    fn missing_count_treats_the_same_name_on_another_owner_as_new() {
        let mut app = App::empty(test_settings());
        app.installed.push(installed("storytold", "photocraft"));
        app.remotes.push(remote("storytold", "photocraft"));
        app.remotes.push(remote("other", "photocraft"));
        assert_eq!(app.missing_count(), 1);
    }

    #[test]
    fn needs_update_sort_puts_behind_repositories_first() {
        let mut app = App::empty(test_settings());
        app.sort = SortMode::NeedsUpdate;
        let clean = installed("acme", "clean");
        let mut behind = installed("acme", "behind");
        behind.behind = Some(4);
        app.installed = vec![clean, behind];
        let visible = app.visible_installed();
        assert_eq!(visible[0].folder, "behind");
    }

    #[test]
    fn new_commits_on_other_remote_branches_count_as_needing_attention() {
        let mut app = App::empty(test_settings());
        app.sort = SortMode::NeedsUpdate;
        let clean = installed("acme", "clean");
        let mut branchy = installed("acme", "branchy");
        branchy.remote_new = 3;
        branchy.remote_branches = vec!["dev".into()];
        let mut behind = installed("acme", "behind");
        behind.behind = Some(1);
        app.installed = vec![clean, branchy, behind];
        let order: Vec<&str> = app
            .visible_installed()
            .iter()
            .map(|repo| repo.folder.as_str())
            .collect();
        assert_eq!(order, ["behind", "branchy", "clean"]);
        assert_eq!(
            app.ready_message(),
            "3 installed. 1 is behind. 1 has new commits on other branches."
        );
    }

    #[test]
    fn the_startup_fetch_is_due_once_and_only_when_idle() {
        let mut app = App::empty(test_settings());
        app.installed = vec![installed("acme", "one")];
        assert!(!app.startup_fetch_due());

        app.fetch_after_read = true;
        assert!(app.startup_fetch_due());
        assert!(!app.startup_fetch_due());

        // A run that started in the meantime wins, and the fetch is dropped.
        app.fetch_after_read = true;
        app.busy = true;
        assert!(!app.startup_fetch_due());
        app.busy = false;
        assert!(!app.startup_fetch_due());

        // Nothing installed, nothing to fetch.
        app.installed.clear();
        app.fetch_after_read = true;
        assert!(!app.startup_fetch_due());
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
        app.on_key(KeyEvent::new(code, modifiers));
    }

    #[test]
    fn space_selects_several_installed_repositories() {
        let mut app = App::empty(test_settings());
        app.installed = vec![
            installed("acme", "one"),
            installed("acme", "two"),
            installed("acme", "three"),
        ];
        // Nothing ticked: actions apply to the highlighted row.
        app.selected = Some("two".into());
        assert_eq!(app.chosen(), ["two"]);

        app.selected = Some("one".into());
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.selected.as_deref(), Some("two"));
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.is_marked_local("one"));
        assert!(!app.is_marked_local("two"));
        assert!(app.is_marked_local("three"));
        assert_eq!(app.chosen(), ["one", "three"]);

        // Space again unticks.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.chosen(), ["one"]);

        // Remove asks about every ticked repository.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(
            app.mode
                == Mode::ConfirmRemove {
                    folders: vec!["one".into(), "three".into()]
                }
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(app.marked.len(), 2);

        // Esc in the list clears the ticks.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.marked.is_empty());
    }

    #[test]
    fn ctrl_a_selects_what_the_list_shows_and_gone_folders_drop_out() {
        let mut app = App::empty(test_settings());
        app.installed = vec![
            installed("acme", "api"),
            installed("acme", "app"),
            installed("acme", "docs"),
        ];
        app.list_filter = "ap".into();
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(app.chosen(), ["api", "app"]);
        assert!(app.mode == Mode::List, "ctrl-a must not open Add");

        // A second ctrl-a unticks them again.
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert!(app.marked.is_empty());

        app.list_filter.clear();
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(app.marked.len(), 3);
        app.installed.remove(1);
        app.prune_marks();
        assert_eq!(app.chosen(), ["api", "docs"]);
    }

    #[test]
    fn too_many_folders_are_not_opened_at_once() {
        let mut app = App::empty(test_settings());
        app.installed = (0..OPEN_LIMIT + 1)
            .map(|index| installed("acme", &format!("repo{index}")))
            .collect();
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('o'), KeyModifiers::NONE);
        assert!(app.status_error);
        assert!(app.status.contains("too many to open at once"));
    }

    fn labels(app: &App) -> Vec<String> {
        app.queue.iter().map(Queued::label).collect()
    }

    #[test]
    fn actions_started_while_something_runs_are_queued_in_order() {
        let mut app = App::empty(test_settings());
        app.installed = vec![installed("acme", "one"), installed("acme", "two")];
        app.selected = Some("one".into());
        app.busy = true;

        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(labels(&app), ["update one"]);
        assert_eq!(
            app.status,
            "Queued: update one. It starts when the current task is done."
        );
        assert!(!app.status_error);

        press(&mut app, KeyCode::Char('U'), KeyModifiers::NONE);
        // Remove still asks first, then waits its turn.
        press(&mut app, KeyCode::Char('a'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(matches!(app.mode, Mode::ConfirmRemove { .. }));
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(app.status, "Queued: refresh. 4 tasks are waiting.");
        // A second refresh adds nothing.
        press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(
            labels(&app),
            [
                "update one",
                "update all",
                "remove 2 repositories",
                "refresh"
            ]
        );

        // Nothing starts while the first task still runs.
        assert!(!app.start_queued());
        assert!(app.waiting());
        assert_eq!(app.drop_queue(), 4);
        assert!(app.queue.is_empty());
    }

    #[test]
    fn a_background_fetch_queues_actions_too() {
        let mut app = App::empty(test_settings());
        app.installed = vec![installed("acme", "one")];
        app.selected = Some("one".into());
        app.fetching = true;
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(labels(&app), ["update one"]);
        press(&mut app, KeyCode::Char('r'), KeyModifiers::NONE);
        assert_eq!(app.status, "Already checking the repositories.");
        assert_eq!(app.queue.len(), 1);
    }

    #[test]
    fn the_queue_runs_in_order_and_waits_for_a_failed_dialog() {
        let mut app = App::empty(test_settings());
        app.queue.push_back(Queued::Update(vec!["gone".into()]));
        app.queue.push_back(Queued::Update(vec!["gone-too".into()]));
        running_job(&mut app, &["a/one"]);
        assert!(!app.start_queued());

        // The run fails: its dialog stays, and so does the queue.
        app.on_msg(Msg::JobFinished(Err("boom".into())));
        assert!(app.mode == Mode::Progress);
        assert!(!app.start_queued());
        assert_eq!(app.queue.len(), 2);

        // Closing the dialog lets the next task start.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.start_queued());
        assert_eq!(app.status, "gone is no longer in this folder.");
        assert_eq!(labels(&app), ["update gone-too"]);
        assert!(app.start_queued());
        assert_eq!(app.status, "gone-too is no longer in this folder.");
        assert!(!app.start_queued());
        assert!(app.queue.is_empty());
    }

    fn keys(app: &App) -> Vec<String> {
        app.rows().iter().map(Row::key).collect()
    }

    fn type_text(app: &mut App, text: &str) {
        for ch in text.chars() {
            press(app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
    }

    /// Four repositories at paths of their own, so groups can be saved.
    fn grouped_app() -> App {
        let mut app = App::empty(test_settings());
        for name in ["api", "docs", "notes", "web"] {
            let mut repo = installed("acme", name);
            repo.path = PathBuf::from("/srv/code").join(name);
            app.checkouts.push(repo.path.clone());
            app.installed.push(repo);
        }
        app.selected = Some("api".into());
        app
    }

    #[test]
    fn repositories_can_be_put_into_groups() {
        let mut app = grouped_app();
        // Without groups the overview is the plain list.
        assert_eq!(keys(&app), ["api", "docs", "notes", "web"]);

        // Tick two, press m, type a name.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        assert!(app.mode == Mode::Group);
        assert_eq!(app.group_prompt.as_ref().unwrap().repos, ["api", "web"]);
        type_text(&mut app, "  Work  ");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.marked.is_empty());
        assert_eq!(app.status, "2 repositories are in Work now.");
        assert_eq!(
            keys(&app),
            ["group::Work", "api", "web", "group::", "docs", "notes"]
        );
        assert_eq!(app.groups(), ["Work"]);

        // A second group; "work" in another spelling joins the first.
        app.selected = Some("notes".into());
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        type_text(&mut app, "home");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.selected = Some("docs".into());
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        type_text(&mut app, "work");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.status, "docs is in Work now.");
        assert_eq!(
            keys(&app),
            ["group::home", "notes", "group::Work", "api", "docs", "web"]
        );

        // The settings file keeps the groups.
        let file = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(file.contains("group = home\n"));
        assert!(file.contains("group = Work\n"));

        // In the dialog, the arrows walk through the groups that exist,
        // and an empty name takes a repository out of its group.
        app.selected = Some("docs".into());
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        assert_eq!(app.group_prompt.as_ref().unwrap().input, "Work");
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.group_prompt.as_ref().unwrap().input, "home");
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.status, "docs is in no group now.");
        assert_eq!(
            keys(&app),
            [
                "group::home",
                "notes",
                "group::Work",
                "api",
                "web",
                "group::",
                "docs"
            ]
        );
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn a_group_folds_and_acts_as_a_whole() {
        let mut app = grouped_app();
        app.placement = vec![
            (PathBuf::from("/srv/code/api"), "Work".into()),
            (PathBuf::from("/srv/code/web"), "Work".into()),
        ];
        app.assign_groups();
        app.selected = Some("group::Work".into());

        // An action on the header means every repository in the group.
        assert_eq!(app.chosen(), ["api", "web"]);
        app.busy = true;
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(labels(&app), ["update 2 repositories"]);
        app.drop_queue();
        app.busy = false;

        // Space ticks the whole group, and again unticks it.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.marked, ["api", "web"]);
        app.selected = Some("group::Work".into());
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert!(app.marked.is_empty());

        // Enter folds it: the header stays, its repositories hide.
        app.selected = Some("group::Work".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(keys(&app), ["group::Work", "group::", "docs", "notes"]);
        assert_eq!(app.collapsed, ["Work"]);
        assert_eq!(app.visible_installed().len(), 2);
        // Still the whole group for an action.
        assert_eq!(app.chosen(), ["api", "web"]);
        // A filter looks inside folded groups.
        app.list_filter = "web".into();
        assert_eq!(keys(&app), ["group::Work", "web"]);
        app.list_filter.clear();
        // Right unfolds, Left folds, Left on a repository goes to its header.
        press(&mut app, KeyCode::Right, KeyModifiers::NONE);
        assert!(app.collapsed.is_empty());
        app.selected = Some("web".into());
        press(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.selected.as_deref(), Some("group::Work"));
        press(&mut app, KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(app.collapsed, ["Work"]);

        // m on the header renames the group and keeps it folded.
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        assert_eq!(
            app.group_prompt.as_ref().unwrap().rename.as_deref(),
            Some("Work")
        );
        press(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Group, "an empty name is refused");
        type_text(&mut app, "Client");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.status, "Work is called Client now.");
        assert_eq!(app.selected.as_deref(), Some("group::Client"));
        assert_eq!(app.collapsed, ["Client"]);
        assert_eq!(app.groups(), ["Client"]);

        // d on the header dissolves the group and deletes nothing.
        press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(keys(&app), ["api", "docs", "notes", "web"]);
        assert_eq!(app.installed.len(), 4);
        assert_eq!(
            app.status,
            "The group Client is gone. Its 2 repositories stay on the list."
        );
        assert!(app.placement.is_empty());
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn group_names_are_tidied_and_checked() {
        assert_eq!(
            clean_group_name("  Client   work "),
            Ok("Client work".to_string())
        );
        assert_eq!(clean_group_name("   "), Ok(String::new()));
        assert!(clean_group_name(&"x".repeat(GROUP_NAME_LIMIT + 1)).is_err());
        assert!(clean_group_name("group::x").is_err());
    }

    #[test]
    fn enter_opens_the_actions_menu_for_the_selection() {
        let mut app = grouped_app();
        app.selected = Some("docs".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);
        assert_eq!(app.menu.as_ref().unwrap().repos, ["docs"]);
        assert!(!app.busy, "enter alone starts nothing");

        // Esc closes it and nothing happened.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.menu.is_none());
        assert!(app.queue.is_empty());

        // With several ticked it is about all of them.
        app.selected = Some("api".into());
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(app.menu.as_ref().unwrap().repos, ["api", "docs"]);

        // The arrows move, and stop at the ends.
        press(&mut app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(app.menu.as_ref().unwrap().selected, 0);
        for _ in 0..20 {
            press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        }
        assert_eq!(app.menu.as_ref().unwrap().selected, MenuItem::ALL.len() - 1);
        assert_eq!(MenuItem::ALL[MenuItem::ALL.len() - 1], MenuItem::Remove);

        // Enter on the last line asks before removing, for both.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.menu.is_none());
        assert!(
            app.mode
                == Mode::ConfirmRemove {
                    folders: vec!["api".into(), "docs".into()]
                }
        );
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        // A letter runs its line at once: m opens the group dialog.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('m'), KeyModifiers::NONE);
        assert!(app.mode == Mode::Group);
        assert_eq!(app.group_prompt.as_ref().unwrap().repos, ["api", "docs"]);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);

        // Update from the menu goes the usual way: here it queues.
        app.busy = true;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(labels(&app), ["update 2 repositories"]);
        app.drop_queue();
        app.busy = false;

        // A click on a line runs it too.
        app.selected = Some("web".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        app.click_row("menu:5".into());
        assert!(matches!(app.mode, Mode::ConfirmRemove { .. }));
    }

    #[test]
    fn enter_on_a_group_still_folds_it_and_every_menu_key_is_its_own() {
        let mut app = grouped_app();
        app.placement = vec![(PathBuf::from("/srv/code/api"), "Work".into())];
        app.assign_groups();
        app.selected = Some("group::Work".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(app.collapsed, ["Work"]);

        let mut keys: Vec<char> = MenuItem::ALL.iter().map(|item| item.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), MenuItem::ALL.len());
        // None of them is a key that moves or closes the menu.
        assert!(!keys.iter().any(|key| "jkq".contains(*key)));
        let _ = std::fs::remove_file(&app.config_path);
    }

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            notes: "- Something new".into(),
            page: format!("https://github.com/AsP3X/DevAssist/releases/tag/v{version}"),
            download: Some(update::Asset {
                name: format!("devassist-{version}-x64.exe"),
                url: "https://api.github.com/assets/1".into(),
                size: 4_000_000,
            }),
            sums: None,
        }
    }

    #[test]
    fn a_newer_version_is_announced_and_installing_ends_in_a_restart() {
        let mut app = App::empty(test_settings());
        assert_eq!(app.update, UpdateState::Unknown);
        assert!(!app.restart);

        // The look at startup found one: the status line says so.
        app.update = UpdateState::Checking;
        app.on_msg(Msg::Update(Ok(Some(release("9.0.0")))));
        assert_eq!(app.update, UpdateState::Available(release("9.0.0")));
        assert_eq!(
            app.status,
            "DevAssist 9.0.0 is available. Press v to install it."
        );

        // While it downloads, progress arrives; then it is in place.
        app.update = UpdateState::Installing {
            release: release("9.0.0"),
            done: 0,
            total: 4_000_000,
        };
        assert!(app.waiting());
        app.on_msg(Msg::UpdateProgress(1_000_000, 4_000_000));
        assert!(matches!(
            app.update,
            UpdateState::Installing {
                done: 1_000_000,
                ..
            }
        ));
        app.on_msg(Msg::UpdateDone(Ok(())));
        assert_eq!(app.update, UpdateState::Installed("9.0.0".into()));
        assert_eq!(
            app.status,
            "DevAssist 9.0.0 is installed. Restart to use it."
        );

        // Enter in the version dialog restarts into it.
        app.mode = Mode::Update;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.restart);
        assert!(app.should_quit);
    }

    #[test]
    fn a_failed_look_is_quiet_at_startup_and_said_when_asked_for() {
        let mut app = App::empty(test_settings());
        app.update = UpdateState::Checking;
        app.update_quiet = true;
        app.on_msg(Msg::Update(Err("could not reach GitHub".into())));
        assert_eq!(app.update, UpdateState::Unknown);
        assert!(!app.status_error);

        app.update = UpdateState::Checking;
        app.update_quiet = false;
        app.on_msg(Msg::Update(Err("could not reach GitHub".into())));
        assert_eq!(
            app.update,
            UpdateState::Failed("could not reach GitHub".into())
        );

        app.update = UpdateState::Checking;
        app.on_msg(Msg::Update(Ok(None)));
        assert_eq!(app.update, UpdateState::Current);

        // A failed install says why and can be tried again.
        app.update = UpdateState::Installing {
            release: release("9.0.0"),
            done: 10,
            total: 20,
        };
        app.on_msg(Msg::UpdateDone(Err("the download stopped".into())));
        assert_eq!(
            app.update,
            UpdateState::Failed("the download stopped".into())
        );
        assert!(!app.restart);

        // Esc leaves the dialog without doing anything.
        app.mode = Mode::Update;
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(!app.should_quit);
    }

    #[test]
    fn filter_matches_the_owner() {
        let mut app = App::empty(test_settings());
        app.installed = vec![
            installed("storytold", "photocraft"),
            installed("other", "tool"),
        ];
        app.list_filter = "other".into();
        let visible = app.visible_installed();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].repo, "tool");
    }

    #[test]
    fn delete_word_removes_the_last_word() {
        let mut text = "hello world".to_string();
        delete_word(&mut text);
        assert_eq!(text, "hello");
    }
}
