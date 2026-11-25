//! What the repositories tool knows and does. Its keys and its mouse are
//! in `app`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use devassist_core::list;
use devassist_core::prompt::{self as pathprompt, PathPrompt};
use devassist_core::repos::form::Form;

use crate::atoms::sys::open;
use crate::atoms::sys::paths;
use crate::atoms::sys::process;
use crate::atoms::task::job::{Job, JobEvent};
use crate::atoms::task::worker::Worker;
use crate::features::repos::github::{self, AccountQuery, RemoteRepo};
use crate::features::repos::gitops::{self, LocalRepo};
use crate::features::repos::gitprogress;
use crate::shell::config::{self, Settings};

/// How long the Add search waits after the last key before asking GitHub.
const LOOKUP_DELAY: Duration = Duration::from_millis(350);

/// How long the progress dialog stays after a run that went well.
/// Opening more folders than this at once is refused.
pub const OPEN_LIMIT: usize = 8;
pub const JOB_LINGER: Duration = Duration::from_millis(1200);
/// How the status line starts while an automatic pull runs.
const WATCH_LABEL: &str = "Auto-pull: checking";

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
    /// The dialog that asks for the address on another service.
    Service,
    /// The actions for the chosen repositories, opened with Enter.
    Menu,
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
    /// Save the pull requests of these folders now.
    PullRequests(Vec<String>),
    /// Collect the forks of these folders, each in a folder beside it.
    Forks(Vec<String>),
    /// Move these checkouts, each by row name and path, into that folder.
    Move(Vec<(String, PathBuf)>, PathBuf),
    /// Push the checkout with this row name and path to that address.
    Service(String, PathBuf, String),
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
            Queued::PullRequests(folders) => named("fetch the pull requests of", folders.clone()),
            Queued::Forks(folders) => named("fetch the forks of", folders.clone()),
            Queued::Service(name, _, _) => format!("push {name} to another service"),
            Queued::Move(repos, _) => {
                named("move", repos.iter().map(|(name, _)| name.clone()).collect())
            }
        }
    }
}

/// What the folder dialog is asking for.
#[derive(Clone)]
pub enum Purpose {
    /// Clone these repositories into the folder.
    Clone(Vec<RemoteRepo>),
    /// Add the clones that already sit in the folder to the overview.
    Import,
    /// Move these checkouts, each by row name and path, into the folder.
    Move(Vec<(String, PathBuf)>),
}

impl Purpose {
    /// The dialog for this purpose. Only an import needs a folder that is
    /// already there: cloning creates it.
    pub fn prompt(self, start: &Path) -> PathPrompt<Purpose> {
        let must_exist = matches!(self, Purpose::Import);
        PathPrompt::new(self, start, must_exist)
    }
}

/// Where a single repository ends up, to show under the field.
pub fn clone_target(prompt: &PathPrompt<Purpose>) -> Option<PathBuf> {
    match &prompt.purpose {
        Purpose::Clone(remotes) if remotes.len() == 1 => Some(
            pathprompt::resolve(&prompt.input)
                .ok()?
                .join(&remotes[0].name),
        ),
        Purpose::Move(repos) if repos.len() == 1 => Some(
            pathprompt::resolve(&prompt.input)
                .ok()?
                .join(repos[0].1.file_name()?),
        ),
        _ => None,
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

/// One line of the actions menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuItem {
    Update,
    Open,
    Github,
    Group,
    CopyPath,
    Remove,
    /// Start or stop pulling the repositories on a timer.
    Watch,
    /// Fetch the pull requests of the repositories now.
    PullRequests,
    /// Clone every fork of the repositories into a folder beside each.
    Forks,
    /// The overview of the forks that were cloned of one repository.
    ShowForks,
    /// Bring every fork folder that is there up to date, whatever is
    /// selected.
    AllForks,
    /// Opens the lines that take a repository somewhere else.
    Migrate,
}

/// One line of the Migrate submenu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MigrateItem {
    /// Move the checkout to another folder on disk.
    Folder,
    /// Push a complete copy to a repository on another service.
    Service,
}

impl MigrateItem {
    pub const ALL: [MigrateItem; 2] = [MigrateItem::Folder, MigrateItem::Service];

    pub fn label(self) -> &'static str {
        match self {
            MigrateItem::Folder => "Move to another folder…",
            MigrateItem::Service => "Push to another service…",
        }
    }

    pub fn key(self) -> char {
        match self {
            MigrateItem::Folder => 'f',
            MigrateItem::Service => 's',
        }
    }
}

/// The dialog that asks where on another service a repository goes.
pub struct ServicePrompt {
    /// The repository, by row name.
    pub repo: String,
    pub path: PathBuf,
    pub input: String,
    pub error: Option<String>,
}

impl MenuItem {
    pub const ALL: [MenuItem; 12] = [
        MenuItem::Update,
        MenuItem::Open,
        MenuItem::Github,
        MenuItem::Group,
        MenuItem::CopyPath,
        MenuItem::Watch,
        MenuItem::PullRequests,
        MenuItem::ShowForks,
        MenuItem::Forks,
        MenuItem::AllForks,
        MenuItem::Migrate,
        MenuItem::Remove,
    ];

    /// What the line says, for one repository or for several.
    pub fn label(self, many: bool) -> &'static str {
        match (self, many) {
            (MenuItem::Update, _) => "Update",
            (MenuItem::Watch, _) => "Auto-pull on or off",
            (MenuItem::PullRequests, _) => "Fetch the pull requests",
            (MenuItem::Forks, _) => "Clone the forks",
            (MenuItem::ShowForks, _) => "Show the forks",
            (MenuItem::AllForks, _) => "Update all cloned forks",
            (MenuItem::Migrate, _) => "Migrate ▸",
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
            MenuItem::Watch => 'w',
            MenuItem::PullRequests => 'p',
            MenuItem::Forks => 'n',
            MenuItem::ShowForks => 'f',
            MenuItem::AllForks => 'a',
            MenuItem::Migrate => 't',
            MenuItem::Open => 'o',
            MenuItem::Github => 'g',
            MenuItem::Group => 'm',
            MenuItem::CopyPath => 'c',
            MenuItem::Remove => 'd',
        }
    }
}

/// How the fork overview is sorted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkSort {
    /// Forks with commits of their own first, the one furthest ahead on
    /// top.
    Changed,
    /// By the date of the newest commit.
    Newest,
    Name,
}

impl ForkSort {
    pub fn label(self) -> &'static str {
        match self {
            ForkSort::Changed => "most changes first",
            ForkSort::Newest => "newest commit first",
            ForkSort::Name => "by name",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ForkSort::Changed => ForkSort::Newest,
            ForkSort::Newest => ForkSort::Name,
            ForkSort::Name => ForkSort::Changed,
        }
    }
}

/// The overview of the forks of one repository, which takes the place
/// of the list of repositories while it is open.
pub struct ForksView {
    /// The repository they are forks of, by row name.
    pub repo: String,
    /// Where that repository is checked out.
    path: PathBuf,
    /// It on GitHub, as `owner/name`.
    pub original: String,
    /// The folder the forks are in.
    pub dest: PathBuf,
    pub forks: Vec<gitops::ForkInfo>,
    /// The folder is being read.
    pub loading: bool,
    pub error: Option<String>,
    /// The highlighted fork, by owner.
    pub selected: Option<String>,
    pub scroll: usize,
    pub sort: ForkSort,
    pub filter: String,
    /// The filter is being typed.
    pub filtering: bool,
    /// Only forks with commits of their own are shown.
    pub changed_only: bool,
}

impl ForksView {
    /// The forks the list shows, in its order.
    pub fn rows(&self) -> Vec<&gitops::ForkInfo> {
        let filter = self.filter.trim().to_lowercase();
        let mut rows: Vec<&gitops::ForkInfo> = self
            .forks
            .iter()
            .filter(|fork| !self.changed_only || fork.folder)
            .filter(|fork| {
                filter.is_empty()
                    || fork.owner.to_lowercase().contains(&filter)
                    || fork.name.to_lowercase().contains(&filter)
            })
            .collect();
        let name = |fork: &gitops::ForkInfo| fork.owner.to_lowercase();
        match self.sort {
            ForkSort::Name => rows.sort_by_key(|fork| name(fork)),
            ForkSort::Newest => {
                rows.sort_by_key(|fork| (std::cmp::Reverse(fork.date), name(fork)));
            }
            ForkSort::Changed => rows.sort_by_key(|fork| {
                (
                    !fork.folder,
                    std::cmp::Reverse(if fork.folder { fork.ahead } else { None }),
                    std::cmp::Reverse(fork.date),
                    name(fork),
                )
            }),
        }
        rows
    }

    /// The highlighted fork, or the first one when none is.
    pub fn chosen(&self) -> Option<&gitops::ForkInfo> {
        let rows = self.rows();
        self.selected
            .as_ref()
            .and_then(|owner| rows.iter().find(|fork| &fork.owner == owner).copied())
            .or_else(|| rows.first().copied())
    }
}

/// The actions menu: what it is about and which line is highlighted.
pub struct Menu {
    pub repos: Vec<String>,
    pub selected: usize,
    /// The Migrate submenu is open, with this line highlighted.
    pub migrate: Option<usize>,
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

pub enum Msg {
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
    Job(JobEvent),
    /// An automatic pull of the watched repositories is done.
    Watched {
        /// The repositories that got something new.
        updated: Vec<String>,
        /// The ones that could not be pulled, each with the reason.
        failed: Vec<(String, String)>,
    },
    /// The fork folder of the open fork overview was read.
    Forks {
        generation: u64,
        result: Result<gitops::ForkList, String>,
    },
    /// A clone landed here, or a checkout that was already here was found.
    Cloned(PathBuf),
    JobFinished(Result<String, String>),
    /// The pull requests of the checkout at this path were just saved.
    PullsSaved(PathBuf),
    /// A checkout was moved to another folder.
    Moved {
        from: PathBuf,
        to: PathBuf,
    },
    /// GitHub said how many pull requests these repositories have.
    Pulls {
        generation: u64,
        /// "owner/name" in lowercase, with the counts of each.
        counts: Vec<(String, github::PullCounts)>,
    },
}

#[derive(Clone, PartialEq, Eq)]
pub enum RemoteState {
    Loading,
    Ready,
    Failed(String),
}

pub struct Repos {
    /// Every checkout the overview manages, wherever it is on disk.
    pub checkouts: Vec<PathBuf>,
    /// The folder the Add dialog offers for a new clone: the last one used.
    pub clone_dir: PathBuf,
    owners_explicit: bool,
    /// The open folder dialog.
    pub prompt: Option<PathPrompt<Purpose>>,
    /// Checkouts a running remove is deleting.
    pub removing: Vec<PathBuf>,
    /// Which group each checkout is in. Groups only sort the overview.
    pub placement: Vec<(PathBuf, String)>,
    /// Groups that are folded shut. An empty name is "no group".
    pub collapsed: Vec<String>,
    /// The open group dialog.
    pub group_prompt: Option<GroupPrompt>,
    /// The open dialog for the address on another service.
    pub service_prompt: Option<ServicePrompt>,
    /// The open actions menu.
    pub menu: Option<Menu>,
    /// The open fork overview. It stays under the dialogs opened from it.
    pub forks: Option<ForksView>,
    pub forks_generation: u64,
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
    pub should_quit: bool,
    /// Look for a newer version at startup: the setting, kept to be saved.
    update_check: bool,
    /// Checkouts that are pulled on a timer while the program runs.
    pub watched: Vec<PathBuf>,
    /// Minutes between those pulls.
    pub watch_interval: u32,
    /// Every fetch also saves the commits of all pull requests.
    pub pull_requests: bool,
    /// Pull requests on GitHub by state, by "owner/name" in lowercase.
    pub pulls: HashMap<String, github::PullCounts>,
    pulls_generation: u64,
    /// Pull requests the remote has that a checkout has not saved, as the
    /// last fetch found them, by the path of the checkout.
    pub pulls_pending: HashMap<PathBuf, gitops::PendingPulls>,
    /// When the next automatic pull is due.
    pub watch_due: Instant,
    /// What the status line said before an automatic pull took it over.
    /// A pull that brings nothing puts it back.
    watch_status: Option<(String, bool)>,
    /// The shell's settings, kept to be saved with the rest.
    shell: config::ShellPrefs,
    /// The window's look, kept to be saved with the rest. Not used here.
    look: config::Look,
    generation: u64,
    remote_generation: u64,
    status_pending: usize,
    /// Set at startup: fetch once the first local read is done.
    fetch_after_read: bool,
    /// Repositories of accounts looked up from the Add search, by lowercase name.
    lookups: HashMap<String, Result<Vec<RemoteRepo>, String>>,
    lookup_due: Option<(String, Instant)>,
    lookup_running: Option<String>,
    pub job_close_at: Option<Instant>,
    worker: Worker<Msg>,
    /// The settings as this program last read or wrote them. A save writes
    /// only what changed since, so what another DevAssist changed in the
    /// file stays. None writes everything: the settings changed at startup
    /// and are not in the file yet.
    last_saved: Option<config::Saved>,
}

impl Repos {
    pub fn new(settings: Settings) -> Self {
        let first_run = settings.first_run;
        let unsaved = settings.unsaved;
        let mut app = Self::empty(settings);
        app.reload_lists(false, false);
        app.fetch_after_read = true;
        if unsaved {
            // Checkouts taken over at startup go into the settings file.
            app.save_config();
        }
        if first_run {
            app.open_settings(true);
            if app.owners.is_empty() {
                app.worker.spawn(|tx| {
                    let _ = tx.send(Msg::Login(github::current_login()));
                });
            }
        }
        app
    }

    /// A tool that reads no folder and calls no network.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn offline(settings: Settings) -> Self {
        let first_run = settings.first_run;
        let mut app = Self::empty(settings);
        app.remotes_state = RemoteState::Ready;
        if first_run {
            app.open_settings(true);
        }
        app
    }

    pub fn empty(settings: Settings) -> Self {
        let unsaved = settings.unsaved;
        let mut app = Self {
            checkouts: settings.checkouts,
            clone_dir: settings.clone_dir,
            owners_explicit: settings.owners_explicit,
            prompt: None,
            removing: Vec::new(),
            placement: settings.placement,
            collapsed: settings.collapsed,
            group_prompt: None,
            service_prompt: None,
            menu: None,
            forks: None,
            forks_generation: 0,
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
            should_quit: false,
            update_check: settings.update_check,
            watched: settings.watched,
            watch_interval: settings.watch_interval,
            pull_requests: settings.pull_requests,
            // The first pull runs as soon as the startup check is done.
            watch_due: Instant::now(),
            watch_status: None,
            pulls: HashMap::new(),
            pulls_generation: 0,
            pulls_pending: HashMap::new(),
            shell: settings.shell,
            look: settings.look,
            generation: 0,
            remote_generation: 0,
            status_pending: 0,
            fetch_after_read: false,
            lookups: HashMap::new(),
            lookup_due: None,
            lookup_running: None,
            worker: Worker::default(),
            last_saved: None,
        };
        if !unsaved {
            app.last_saved = Some(app.saved());
        }
        app
    }

    pub fn drain(&mut self) -> bool {
        let started = self.start_due_lookup() | self.close_finished_job();
        let messages = self.worker.drain();
        let arrived = !messages.is_empty();
        for message in messages {
            self.on_msg(message);
        }
        let next = self.start_queued();
        let watch = !next && self.start_due_watch();
        arrived || started || next || watch
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

    pub fn assign_groups(&mut self) {
        for repo in &mut self.installed {
            repo.group = self
                .placement
                .iter()
                .find(|(path, _)| paths::same_path(path, &repo.path))
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
    pub fn toggle_mark(&mut self) {
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

    /// Opens the overview of the forks of the highlighted repository,
    /// read from the folder they were cloned into.
    pub fn open_forks(&mut self) {
        self.mode = Mode::List;
        let repo = self
            .selected
            .as_ref()
            .filter(|_| self.marked.len() <= 1)
            .and_then(|folder| {
                let folder = self.marked.first().unwrap_or(folder);
                self.installed.iter().find(|repo| &repo.folder == folder)
            });
        let Some(repo) = repo else {
            self.notice("Highlight one repository to see its forks.", true);
            return;
        };
        if !gitops::forks_cloned(&repo.path) {
            self.notice(
                format!(
                    "The forks of {} are not cloned yet. Enter, then n clones them.",
                    repo.folder
                ),
                true,
            );
            return;
        }
        self.forks = Some(ForksView {
            repo: repo.folder.clone(),
            path: repo.path.clone(),
            original: format!("{}/{}", repo.owner, repo.repo),
            dest: gitops::forks_folder(&repo.path).unwrap_or_default(),
            forks: Vec::new(),
            loading: false,
            error: None,
            selected: None,
            scroll: 0,
            sort: ForkSort::Changed,
            filter: String::new(),
            filtering: false,
            changed_only: false,
        });
        self.load_forks();
    }

    /// Reads the fork folder of the open overview again, in the background.
    pub fn load_forks(&mut self) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        view.loading = true;
        view.error = None;
        self.forks_generation += 1;
        let generation = self.forks_generation;
        let path = view.path.clone();
        self.worker.spawn(move |tx| {
            let result = gitops::list_forks(&path);
            let _ = tx.send(Msg::Forks { generation, result });
        });
    }

    pub fn close_forks(&mut self) {
        self.forks = None;
        self.forks_generation += 1;
        self.mode = Mode::List;
    }

    pub fn move_fork(&mut self, delta: isize) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        let names: Vec<String> = view
            .rows()
            .into_iter()
            .map(|fork| fork.owner.clone())
            .collect();
        let delta = delta.clamp(-(names.len() as isize), names.len() as isize);
        list::move_by(&mut view.selected, &names, delta);
    }

    /// Opens the folder with the files of the highlighted fork.
    pub fn open_fork_folder(&mut self) {
        let Some((fork, dest)) = self
            .forks
            .as_ref()
            .and_then(|view| Some((view.chosen()?.clone(), view.dest.clone())))
        else {
            return;
        };
        if !fork.folder {
            self.notice(
                format!(
                    "{} has no commits of its own, so it has no folder. g opens it on GitHub.",
                    fork.owner
                ),
                true,
            );
            return;
        }
        let folder = dest.join(&fork.owner);
        match open::folder(&folder) {
            Ok(()) => self.notice(format!("Opened {}.", folder.display()), false),
            Err(err) => self.notice(err, true),
        }
    }

    pub fn open_fork_on_github(&mut self) {
        let Some(fork) = self.forks.as_ref().and_then(|view| view.chosen().cloned()) else {
            return;
        };
        if fork.gone {
            self.notice(
                format!("{}/{} is no longer on GitHub.", fork.owner, fork.name),
                true,
            );
            return;
        }
        let page = format!("https://github.com/{}/{}", fork.owner, fork.name);
        match open::url(&page) {
            Ok(()) => self.notice(format!("Opened {page}."), false),
            Err(err) => self.notice(err, true),
        }
    }

    /// Fetches the forks of the repository whose overview is open.
    pub fn update_shown_forks(&mut self) {
        if let Some(repo) = self.forks.as_ref().map(|view| view.repo.clone()) {
            self.submit(Queued::Forks(vec![repo]));
        }
    }

    pub fn is_marked_local(&self, folder: &str) -> bool {
        self.marked.iter().any(|marked| marked == folder)
    }

    /// Ticks or unticks the highlighted repository, then moves down so
    /// Space can be pressed again. On a group's header it ticks the whole
    /// group, or unticks it when all of it is ticked.
    pub fn toggle_list_mark(&mut self) {
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
    pub fn mark_all_visible(&mut self) {
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
    pub fn chosen(&self) -> Vec<String> {
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
    pub fn enter_row(&mut self) {
        match self.selected_group() {
            Some(group) if self.marked.is_empty() => self.toggle_group(&group),
            _ => self.open_menu(),
        }
    }

    fn open_menu(&mut self) {
        let repos = self.chosen();
        if repos.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        self.menu = Some(Menu {
            repos,
            selected: 0,
            migrate: None,
        });
        self.mode = Mode::Menu;
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
        self.mode = Mode::List;
    }

    /// Closes the menu and does what a line of the Migrate submenu says.
    pub fn run_migrate(&mut self, item: MigrateItem) {
        self.close_menu();
        match item {
            MigrateItem::Folder => self.open_move(),
            MigrateItem::Service => self.open_service(),
        }
    }

    /// Asks for the address of the repository on another service. One
    /// repository at a time: each goes to an address of its own.
    fn open_service(&mut self) {
        let repos: Vec<(String, PathBuf)> = self
            .chosen_repos()
            .iter()
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        match repos.as_slice() {
            [] => self.notice("Select a repository first.", true),
            [(repo, path)] => {
                self.service_prompt = Some(ServicePrompt {
                    repo: repo.clone(),
                    path: path.clone(),
                    input: String::new(),
                    error: None,
                });
                self.mode = Mode::Service;
            }
            _ => self.notice(
                "Select one repository: each needs an address of its own.",
                true,
            ),
        }
    }

    pub fn close_service(&mut self) {
        self.service_prompt = None;
        self.mode = Mode::List;
    }

    /// Enter in the service dialog: pushes, when the address can be one.
    pub fn accept_service(&mut self) {
        let Some(prompt) = self.service_prompt.as_mut() else {
            return;
        };
        let url = match gitops::push_address(&prompt.input) {
            Ok(url) => url,
            Err(err) => {
                prompt.error = Some(err);
                return;
            }
        };
        let Some(prompt) = self.service_prompt.take() else {
            return;
        };
        self.mode = Mode::List;
        self.marked.clear();
        self.submit(Queued::Service(prompt.repo, prompt.path, url));
    }

    /// Pushes every branch and tag of one checkout to another service, in
    /// the progress dialog. The checkout itself stays as it is.
    fn push_to_service(&mut self, name: String, path: PathBuf, url: String) {
        let host = gitops::address_host(&url).unwrap_or_else(|| url.clone());
        let job = gitprogress::job(format!("Pushing {name} to {host}"), vec![name.clone()], 0);
        self.start_job(
            format!("Pushing {name} to {host}…"),
            Some(job),
            move |tx| {
                send(tx, JobEvent::Start(0));
                let done = gitops::migrate_to(&path, &url, |line, progress| {
                    send_line(tx, line, progress)
                })?;
                let count = |count: u32, one: &str, many: &str| match count {
                    1 => format!("1 {one}"),
                    count => format!("{count} {many}"),
                };
                Ok(format!(
                    "Pushed {name} to {host}: {} and {}. The address is kept as the remote {}.",
                    count(done.branches, "branch", "branches"),
                    count(done.tags, "tag", "tags"),
                    done.remote
                ))
            },
        );
    }

    /// Asks which folder the chosen repositories should move to.
    fn open_move(&mut self) {
        let repos: Vec<(String, PathBuf)> = self
            .chosen_repos()
            .iter()
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        let Some((_, first)) = repos.first() else {
            self.notice("Select a repository first.", true);
            return;
        };
        // Start where the first one is now.
        let start = first
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.clone_dir.clone());
        self.prompt = Some(Purpose::Move(repos).prompt(&start));
        self.mode = Mode::Path;
    }

    /// Moves the checkouts into `dir` one after another, each under the
    /// folder name it has now, and keeps them on the list.
    fn move_checkouts(&mut self, repos: Vec<(String, PathBuf)>, dir: PathBuf) {
        let named = match repos.as_slice() {
            [(name, _)] => name.clone(),
            many => count_label(many.len()),
        };
        let names = repos.iter().map(|(name, _)| name.clone()).collect();
        let job = gitprogress::job(format!("Moving {named}"), names, 0);
        self.start_job(format!("Moving {named}…"), Some(job), move |tx| {
            let mut moved = 0;
            let mut stayed = 0;
            let mut failed = Vec::new();
            for (index, (name, from)) in repos.iter().enumerate() {
                if process::was_cancelled() {
                    return Err("cancelled".into());
                }
                send(tx, JobEvent::Start(index));
                let Some(folder) = from.file_name() else {
                    failed.push(name.clone());
                    send(tx, JobEvent::Failed(index, "has no folder name".into()));
                    continue;
                };
                let to = dir.join(folder);
                if paths::same_path(from, &to) {
                    stayed += 1;
                    send(tx, JobEvent::Done(index, "is there already".into()));
                    continue;
                }
                send_line(tx, &format!("{} → {}", from.display(), to.display()), false);
                let moving = gitops::move_checkout(from, &to, |line, progress| {
                    send_line(tx, line, progress)
                });
                match moving {
                    Ok(warning) => {
                        moved += 1;
                        let _ = tx.send(Msg::Moved {
                            from: from.clone(),
                            to,
                        });
                        if let Some(warning) = &warning {
                            send_line(tx, warning, false);
                        }
                        send(tx, JobEvent::Done(index, "moved".into()));
                    }
                    Err(err) => {
                        failed.push(name.clone());
                        send(tx, JobEvent::Failed(index, err));
                    }
                }
            }
            if !failed.is_empty() {
                return Err(format!(
                    "Moved {moved}, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ));
            }
            Ok(match (moved, stayed) {
                (0, _) => format!("{named} is in {} already.", dir.display()),
                (_, 0) => format!("Moved {named} to {}.", dir.display()),
                _ => format!(
                    "Moved {moved} to {}. {stayed} already there.",
                    dir.display()
                ),
            })
        });
    }

    /// A checkout is in another folder now: everything that names it by
    /// its path follows, and the settings are saved.
    fn on_moved(&mut self, from: &Path, to: PathBuf) {
        for path in self.checkouts.iter_mut().chain(self.watched.iter_mut()) {
            if paths::same_path(path, from) {
                *path = to.clone();
            }
        }
        for (path, _) in &mut self.placement {
            if paths::same_path(path, from) {
                *path = to.clone();
            }
        }
        if let Some(pending) = self.pulls_pending.remove(from) {
            self.pulls_pending.insert(to.clone(), pending);
        }
        for repo in &mut self.installed {
            if paths::same_path(&repo.path, from) {
                repo.path = to.clone();
            }
        }
        self.save_config();
    }

    /// Does what the line says, to the repositories that are ticked or
    /// highlighted. Every line closes the menu, except the one that opens
    /// a submenu.
    pub fn run_menu(&mut self, item: MenuItem) {
        if item == MenuItem::Migrate {
            if let Some(menu) = self.menu.as_mut() {
                menu.migrate = Some(0);
            }
            return;
        }
        self.close_menu();
        match item {
            MenuItem::Update => self.update_chosen(),
            MenuItem::Watch => self.toggle_watch(),
            MenuItem::PullRequests => self.fetch_pull_requests_chosen(),
            MenuItem::Forks => self.clone_forks_chosen(),
            MenuItem::ShowForks => self.open_forks(),
            MenuItem::AllForks => self.update_all_forks(),
            MenuItem::Migrate => {}
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
            .filter_map(|page| open::url(page).err())
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
        match crate::atoms::sys::clipboard::set_text(&paths.join("\n")) {
            Err(err) => self.notice(err, true),
            Ok(()) => match paths.as_slice() {
                [path] => self.notice(format!("Copied {path}."), false),
                many => self.notice(format!("Copied {} paths.", many.len()), false),
            },
        }
    }

    pub fn toggle_group(&mut self, group: &str) {
        if let Some(index) = self.collapsed.iter().position(|name| name == group) {
            self.collapsed.remove(index);
        } else {
            self.collapsed.push(group.to_string());
        }
        self.save_config();
    }

    /// Left folds the highlighted group, Right unfolds it. Left on a
    /// repository goes to the header of its group.
    pub fn fold_selected(&mut self, fold: bool) {
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
    pub fn open_group_prompt(&mut self) {
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

    pub fn close_group(&mut self) {
        self.group_prompt = None;
        self.mode = Mode::List;
    }

    /// Enter in the group dialog: moves the repositories, or renames.
    pub fn accept_group(&mut self) {
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
                    .retain(|(path, _)| !paths.iter().any(|moved| paths::same_path(moved, path)));
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
    pub fn prune_marks(&mut self) {
        let installed = &self.installed;
        self.marked
            .retain(|marked| installed.iter().any(|repo| repo.folder == *marked));
    }

    pub fn open_import(&mut self) {
        self.prompt = Some(Purpose::Import.prompt(&self.clone_dir));
        self.mode = Mode::Path;
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

    pub fn use_folder(&mut self, folder: PathBuf) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        self.mode = Mode::List;
        match prompt.purpose {
            Purpose::Clone(remotes) => {
                self.add_marked.clear();
                // The next clone is offered the same folder.
                if !paths::same_path(&self.clone_dir, &folder) {
                    self.clone_dir = folder.clone();
                    self.save_config();
                }
                self.submit(Queued::Clone(remotes, folder));
            }
            Purpose::Import => self.submit(Queued::Import(folder)),
            Purpose::Move(repos) => {
                self.marked.clear();
                self.submit(Queued::Move(repos, folder));
            }
        }
    }

    /// Esc in the folder dialog. It goes back to Add when it asked where
    /// to clone, with the selection still in place.
    pub fn close_path(&mut self) {
        let cloning = matches!(
            self.prompt.take().map(|prompt| prompt.purpose),
            Some(Purpose::Clone(_))
        );
        self.mode = if cloning { Mode::Add } else { Mode::List };
    }

    /// Writes the settings and the list of checkouts.
    pub fn save_config(&mut self) {
        // A checkout that left the list is no longer watched.
        let checkouts = &self.checkouts;
        self.watched
            .retain(|path| checkouts.iter().any(|known| paths::same_path(known, path)));
        let saved = self.saved();
        if let Err(err) = self.write_settings(&self.config_path.clone(), saved) {
            self.notice(err, true);
        }
    }

    /// Writes what changed since the last read or write, and remembers it.
    fn write_settings(&mut self, path: &Path, saved: config::Saved) -> Result<(), String> {
        match &self.last_saved {
            Some(before) => config::save_changes(path, before, &saved)?,
            None => config::save(path, &saved)?,
        }
        self.last_saved = Some(saved);
        Ok(())
    }

    /// The settings and the list of checkouts as they are written.
    fn saved(&self) -> config::Saved {
        config::Saved {
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
            watched: self.watched.clone(),
            watch_interval: self.watch_interval,
            pull_requests: self.pull_requests,
            shell: self.shell.clone(),
            look: self.look.clone(),
        }
    }

    /// Takes the repositories of the remove dialog off the list. Their
    /// folders stay on disk.
    pub fn forget_chosen(&mut self) {
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
            .retain(|path| !paths.iter().any(|gone| paths::same_path(gone, path)));
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
    pub fn register_folder(&mut self, dir: &Path) -> Result<usize, String> {
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
            added += usize::from(paths::push_path(&mut self.checkouts, path));
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
    pub fn settle_removed(&mut self) {
        let removing = std::mem::take(&mut self.removing);
        if removing.is_empty() {
            return;
        }
        let before = self.checkouts.len();
        self.checkouts.retain(|path| {
            path.exists() || !removing.iter().any(|gone| paths::same_path(gone, path))
        });
        if self.checkouts.len() != before {
            self.save_config();
        }
    }

    /// Closes the progress dialog. A run that is still going carries on.
    pub fn hide_job(&mut self) {
        self.job_close_at = None;
        self.dialog_hidden = self.job.as_ref().is_some_and(Job::running);
        self.mode = Mode::List;
    }

    pub fn show_job(&mut self) {
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
    pub fn pick_job(&mut self, delta: isize) {
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
    pub fn scroll_job(&mut self, delta: isize) {
        self.job_close_at = None;
        if let Some(job) = self.job.as_mut() {
            job.scroll_by(delta);
        }
    }

    pub fn close_finished_job(&mut self) -> bool {
        if !self.job_close_at.is_some_and(|at| Instant::now() >= at) {
            return false;
        }
        self.job_close_at = None;
        if self.mode == Mode::Progress {
            self.mode = Mode::List;
        }
        true
    }

    pub fn paste(&mut self, text: String) {
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
            Mode::Service => {
                if let Some(prompt) = self.service_prompt.as_mut() {
                    prompt.input.push_str(text.trim());
                    prompt.error = None;
                }
            }
            _ => {}
        }
    }

    pub fn run(&mut self, action: Action) {
        // The buttons under the fork overview.
        if self.forks.is_some() && self.mode == Mode::List {
            match action {
                Action::Cancel => return self.close_forks(),
                Action::UpdateOne => return self.update_shown_forks(),
                Action::Refresh => return self.load_forks(),
                _ => {}
            }
        }
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
            Action::Confirm if self.mode == Mode::Service => self.accept_service(),
            Action::Cancel if self.mode == Mode::Menu => self.close_menu(),
            Action::Cancel if self.mode == Mode::Group => self.close_group(),
            Action::Cancel if self.mode == Mode::Service => self.close_service(),
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

    pub fn move_by(&mut self, delta: isize) {
        if !matches!(self.mode, Mode::List | Mode::Filter | Mode::Add) {
            return;
        }
        let names = self.current_names();
        list::move_by(self.chosen_slot(), &names, delta);
    }

    pub fn move_to_end(&mut self, start: bool) {
        let names = self.current_names();
        list::move_to_end(self.chosen_slot(), &names, start);
    }

    /// The choice of the list that is on screen: Add's, or the overview's.
    fn chosen_slot(&mut self) -> &mut Option<String> {
        if self.mode == Mode::Add {
            &mut self.add_selected
        } else {
            &mut self.selected
        }
    }

    fn current_names(&self) -> Vec<String> {
        if self.mode == Mode::Add {
            self.add_list().into_iter().map(|repo| repo.id()).collect()
        } else {
            self.rows().iter().map(Row::key).collect()
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

    pub fn snap_visible_selection(&mut self) {
        let names = self.current_names();
        list::snap(&mut self.selected, &names);
    }

    pub fn note_add_filter_changed(&mut self) {
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
        self.worker.spawn(move |tx| {
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
    pub fn list_typed_account(&mut self) {
        if let Some(word) = self.add_word() {
            self.add_filter = format!("{word}/");
            self.note_add_filter_changed();
        }
    }

    pub fn open_add(&mut self) {
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

    pub fn open_settings(&mut self, welcome: bool) {
        let mut form = Form::new(
            welcome,
            &self.owners,
            &self.pinned,
            self.protocol,
            self.watch_interval,
            self.custom_title.as_deref(),
            self.config_path.clone(),
        );
        form.pull_requests = self.pull_requests;
        self.form = Some(form);
        self.mode = Mode::Settings;
    }

    pub fn close_settings(&mut self) {
        let welcome = self.form.take().is_some_and(|form| form.welcome);
        self.mode = Mode::List;
        if welcome {
            self.notice("Setup skipped. Press c to open the settings.", false);
        }
    }

    pub fn save_settings(&mut self) {
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
        saved.watched = self.watched.clone();
        saved.shell = self.shell.clone();
        saved.look = self.look.clone();
        let pulls_turned_on = saved.pull_requests && !self.pull_requests;
        let path = form.path.clone();
        if let Err(err) = self.write_settings(&path, saved.clone()) {
            if let Some(form) = self.form.as_mut() {
                form.error = Some(err);
            }
            return;
        }
        let path = path.display().to_string();
        self.form = None;
        self.mode = Mode::List;
        self.owners = saved.owners;
        self.owners_explicit = true;
        self.pinned = saved.pinned;
        self.protocol = saved.protocol;
        self.pull_requests = saved.pull_requests;
        if self.watch_interval != saved.watch_interval {
            // A new interval counts from now.
            self.watch_interval = saved.watch_interval;
            self.watch_due = Instant::now() + self.watch_every();
        }
        self.title = saved.title.clone().unwrap_or_else(config::default_title);
        self.custom_title = saved.title;
        if pulls_turned_on {
            self.notice(
                "Settings saved. Pull requests are saved with the next fetch: press r.",
                false,
            );
        } else {
            self.notice(format!("Settings saved to {path}."), false);
        }
        self.reload_github_only();
    }

    pub fn toggle_sort(&mut self) {
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

    pub fn open_selected(&mut self) {
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
            if let Err(err) = open::folder(&repo.path) {
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
    pub fn update_chosen(&mut self) {
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
    pub fn update_one(&mut self) {
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

    /// Saves the pull requests of the ticked repositories, or the
    /// highlighted one, now. It does not need the setting to be on.
    fn fetch_pull_requests_chosen(&mut self) {
        let folders = self.chosen();
        self.mode = Mode::List;
        if folders.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        self.marked.clear();
        self.submit(Queued::PullRequests(folders));
    }

    /// Fetches the pull requests of the named folders one after another
    /// in the progress dialog: new ones, and ones that got more commits.
    fn save_pull_requests(&mut self, folders: &[String]) {
        let work: Vec<(String, PathBuf)> = self
            .installed
            .iter()
            .filter(|repo| folders.contains(&repo.folder))
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        if work.is_empty() {
            self.notice(
                format!("{} is no longer in this folder.", folders.join(", ")),
                true,
            );
            return;
        }
        let named = match work.as_slice() {
            [(folder, _)] => folder.clone(),
            many => count_label(many.len()),
        };
        let names = work.iter().map(|(folder, _)| folder.clone()).collect();
        let job = gitprogress::job(format!("Fetching the pull requests of {named}"), names, 0);
        let label = format!("Fetching the pull requests of {named}…");
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let mut total = 0;
            let mut changed = 0;
            let mut failed = Vec::new();
            for (index, (folder, path)) in work.iter().enumerate() {
                if process::was_cancelled() {
                    return Err("cancelled".into());
                }
                send(tx, JobEvent::Start(index));
                let saved = gitops::save_pull_requests(path, token.as_deref(), |line, progress| {
                    send_line(tx, line, progress)
                });
                match saved {
                    Ok(saved) => {
                        let _ = tx.send(Msg::PullsSaved(path.clone()));
                        total += saved.total;
                        changed += saved.changed;
                        let note = match (saved.total, saved.changed) {
                            (0, _) => "no pull requests".to_string(),
                            (_, 0) => "nothing new".to_string(),
                            (_, count) => format!("{count} new or changed"),
                        };
                        send(tx, JobEvent::Done(index, note));
                    }
                    Err(err) => {
                        if process::was_cancelled() {
                            return Err(err);
                        }
                        failed.push(folder.clone());
                        send(tx, JobEvent::Failed(index, err));
                    }
                }
            }
            if !failed.is_empty() {
                return Err(format!(
                    "Pull requests: {changed} new or changed, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ));
            }
            Ok(match (total, changed) {
                (0, _) => format!("{named} has no pull requests."),
                (_, 0) => format!("Pull requests of {named}: {total} saved, nothing new."),
                _ => format!("Pull requests of {named}: {total} saved, {changed} new or changed."),
            })
        });
    }

    /// Collects the forks of the ticked repositories, or the highlighted
    /// one, each in a folder beside the checkout.
    fn clone_forks_chosen(&mut self) {
        let folders = self.chosen();
        self.mode = Mode::List;
        if folders.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        self.marked.clear();
        self.submit(Queued::Forks(folders));
    }

    /// Brings the forks of every repository up to date that has them
    /// cloned beside it. The selection and the ticks play no part.
    fn update_all_forks(&mut self) {
        let folders: Vec<String> = self
            .installed
            .iter()
            .filter(|repo| gitops::forks_cloned(&repo.path))
            .map(|repo| repo.folder.clone())
            .collect();
        self.mode = Mode::List;
        if folders.is_empty() {
            self.notice(
                "No forks are cloned yet. Clone the forks of a repository first.",
                true,
            );
            return;
        }
        self.submit(Queued::Forks(folders));
    }

    /// Asks GitHub for the forks of the named folders and fetches them
    /// one repository after another in the progress dialog. The forks of
    /// `api` go into `api-fork` beside it: one repository with every fork
    /// as a remote, with all its branches and pull requests.
    fn clone_forks(&mut self, folders: &[String]) {
        struct Work {
            folder: String,
            path: PathBuf,
            original: (String, String),
            origin: String,
        }
        let work: Vec<Work> = self
            .installed
            .iter()
            .filter(|repo| folders.contains(&repo.folder))
            .map(|repo| Work {
                folder: repo.folder.clone(),
                path: repo.path.clone(),
                original: (repo.owner.clone(), repo.repo.clone()),
                origin: repo.origin.clone(),
            })
            .collect();
        if work.is_empty() {
            self.notice(
                format!("{} is no longer in this folder.", folders.join(", ")),
                true,
            );
            return;
        }
        let named = match work.as_slice() {
            [only] => only.folder.clone(),
            many => count_label(many.len()),
        };
        let names = work.iter().map(|work| work.folder.clone()).collect();
        // With every folder there already it is an update.
        let verb = if work.iter().all(|work| gitops::forks_cloned(&work.path)) {
            "Updating"
        } else {
            "Cloning"
        };
        let job = gitprogress::job(format!("{verb} the forks of {named}"), names, 0);
        let label = format!("{verb} the forks of {named}…");
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let mut fetched = 0;
            let mut kept = 0;
            let mut gone = 0;
            let mut missed = 0;
            let mut failed = Vec::new();
            let mut last = None;
            for (index, work) in work.iter().enumerate() {
                if process::was_cancelled() {
                    return Err("cancelled".into());
                }
                send(tx, JobEvent::Start(index));
                match forks_of(tx, &work.path, &work.original, &work.origin, &token) {
                    Ok(None) => send(tx, JobEvent::Done(index, "no forks".into())),
                    Ok(Some((dest, saved))) => {
                        fetched += saved.fetched;
                        kept += saved.unchanged + saved.gone;
                        gone += saved.gone;
                        missed += saved.failed.len();
                        let mut note = format!(
                            "{} forks, {} fetched, {} with a folder",
                            saved.fetched + saved.unchanged + saved.gone,
                            saved.fetched,
                            saved.folders
                        );
                        if saved.gone > 0 {
                            note.push_str(&format!(", {} gone from GitHub", saved.gone));
                        }
                        if !saved.failed.is_empty() {
                            note.push_str(&format!(", {} not fetched", saved.failed.len()));
                        }
                        last = Some((dest, saved));
                        send(tx, JobEvent::Done(index, note));
                    }
                    Err(err) => {
                        if process::was_cancelled() {
                            return Err(err);
                        }
                        failed.push(work.folder.clone());
                        send(tx, JobEvent::Failed(index, err));
                    }
                }
            }
            if !failed.is_empty() {
                return Err(format!(
                    "Forks: {fetched} fetched, {kept} unchanged, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ));
            }
            let mut missed = match missed {
                0 => String::new(),
                count => format!(" {count} could not be fetched."),
            };
            if gone > 0 {
                missed.push_str(&format!(
                    " {gone} are no longer on GitHub and were left as they are."
                ));
            }
            Ok(match (&last, work.len()) {
                (None, _) => format!("{named} has no forks."),
                (Some((dest, saved)), 1) => format!(
                    "Forks of {named}: {} in {}, {} fetched now, {} with changes and a folder. {} branches, {} pull requests.{missed}",
                    saved.fetched + saved.unchanged + saved.gone,
                    dest.display(),
                    saved.fetched,
                    saved.folders,
                    saved.branches,
                    saved.pulls
                ),
                _ => format!(
                    "Forks of {named}: {} in the -fork folders beside the repositories, {fetched} fetched now.{missed}",
                    fetched + kept
                ),
            })
        });
    }

    pub fn is_watched(&self, path: &Path) -> bool {
        self.watched
            .iter()
            .any(|known| paths::same_path(known, path))
    }

    fn watch_every(&self) -> Duration {
        Duration::from_secs(u64::from(self.watch_interval.max(1)) * 60)
    }

    /// Starts or stops the automatic pull for the chosen repositories.
    /// When all of them are watched already it stops; otherwise the rest
    /// join, and everything watched is pulled right away.
    pub fn toggle_watch(&mut self) {
        let repos: Vec<(String, PathBuf)> = self
            .chosen_repos()
            .iter()
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        self.mode = Mode::List;
        if repos.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        let named = match repos.as_slice() {
            [(folder, _)] => folder.clone(),
            many => count_label(many.len()),
        };
        if repos.iter().all(|(_, path)| self.is_watched(path)) {
            self.watched
                .retain(|known| !repos.iter().any(|(_, path)| paths::same_path(known, path)));
            self.notice(format!("Auto-pull is off for {named}."), false);
        } else {
            for (_, path) in &repos {
                paths::push_path(&mut self.watched, path.clone());
            }
            self.watch_due = Instant::now();
            let every = match self.watch_interval {
                1 => "every minute".to_string(),
                minutes => format!("every {minutes} minutes"),
            };
            self.notice(
                format!("Auto-pull is on for {named}: {every} while DevAssist runs."),
                false,
            );
        }
        self.marked.clear();
        self.save_config();
    }

    /// Pulls the watched repositories once their interval is over and
    /// nothing else runs. It stays out of the progress dialog: the status
    /// line says what came in.
    fn start_due_watch(&mut self) -> bool {
        if self.watched.is_empty()
            || self.busy
            || self.fetching
            || self.status_pending > 0
            || self.fetch_after_read
            || !self.queue.is_empty()
            || Instant::now() < self.watch_due
        {
            return false;
        }
        self.watch_due = Instant::now() + self.watch_every();
        let work: Vec<(String, PathBuf)> = self
            .installed
            .iter()
            .filter(|repo| self.is_watched(&repo.path) && !repo.upstream_gone)
            .map(|repo| (repo.folder.clone(), repo.path.clone()))
            .collect();
        if work.is_empty() {
            return false;
        }
        process::clear_cancel();
        self.busy = true;
        self.watch_status = Some((self.status.clone(), self.status_error));
        let named = match work.as_slice() {
            [(folder, _)] => folder.clone(),
            many => count_label(many.len()),
        };
        self.notice(format!("{WATCH_LABEL} {named}…"), false);
        let pull_requests = self.pull_requests;
        self.worker.spawn(move |tx| {
            let token = github::token();
            let how = gitops::Fetch {
                token: token.as_deref(),
                pull_requests,
            };
            let mut updated = Vec::new();
            let mut failed = Vec::new();
            for (folder, path) in work {
                if process::was_cancelled() {
                    break;
                }
                match gitops::update(&path, how, |_, _| {}, |_, _| {}) {
                    Ok(result) if result.changed() => updated.push(folder),
                    Ok(_) => {}
                    Err(err) => failed.push((folder, err)),
                }
            }
            let _ = tx.send(Msg::Watched { updated, failed });
        });
        true
    }

    /// An automatic pull is done: say what came in, and read the rows
    /// again where they stand.
    fn on_watched(&mut self, updated: Vec<String>, failed: Vec<(String, String)>) {
        self.busy = false;
        let cancelled = process::was_cancelled();
        process::clear_cancel();
        let before = self.watch_status.take();
        if cancelled {
            self.notice("Cancelled.", false);
        } else if let [(folder, err)] = failed.as_slice() {
            self.notice(format!("Auto-pull of {folder} failed: {err}"), true);
        } else if !failed.is_empty() {
            let names: Vec<&str> = failed.iter().map(|(folder, _)| folder.as_str()).collect();
            self.notice(
                format!(
                    "Auto-pull failed for {}: {}",
                    count_label(failed.len()),
                    names.join(", ")
                ),
                true,
            );
        } else if !updated.is_empty() {
            self.notice(format!("Auto-pull updated {}.", updated.join(", ")), false);
        } else if let Some((text, error)) = before
            && self.status.starts_with(WATCH_LABEL)
        {
            // Nothing new: the status line goes back to what it said.
            self.notice(text, error);
        }
        self.generation += 1;
        self.spawn_status(self.generation, false);
    }

    pub fn update_all(&mut self) {
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
        let job = gitprogress::job(format!("Updating {folder}"), vec![folder.clone()], 0);
        let pull_requests = self.pull_requests;
        self.start_job(format!("Updating {folder}…"), Some(job), move |tx| {
            let token = github::token();
            let how = gitops::Fetch {
                token: token.as_deref(),
                pull_requests,
            };
            send(tx, JobEvent::Start(0));
            if pull_one(tx, &path, how, expected)? {
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
        let job = gitprogress::job(format!("Updating {}", count_label(count)), names, skipped);
        let label = format!("Updating {}…", count_label(count));
        let pull_requests = self.pull_requests;
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let how = gitops::Fetch {
                token: token.as_deref(),
                pull_requests,
            };
            let mut updated = 0;
            let mut current = skipped;
            let mut failed = Vec::new();
            for (index, item) in work.iter().enumerate() {
                if process::was_cancelled() {
                    return Err("cancelled".into());
                }
                if item.gone {
                    failed.push(item.folder.clone());
                    send(tx, JobEvent::Failed(index, "upstream is gone".into()));
                    continue;
                }
                send(tx, JobEvent::Start(index));
                match pull_one(tx, &item.path, how, item.expected) {
                    Ok(true) => {
                        current += 1;
                        send(tx, JobEvent::Done(index, "up to date".into()));
                    }
                    Ok(false) => {
                        updated += 1;
                        send(tx, JobEvent::Done(index, "updated".into()));
                    }
                    Err(err) => {
                        if process::was_cancelled() {
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

    pub fn begin_remove(&mut self) {
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

    pub fn confirm_remove(&mut self) {
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
                if process::was_cancelled() {
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
    pub fn clone_selected(&mut self) {
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
            self.prompt = Some(Purpose::Clone(remotes).prompt(&self.clone_dir));
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
        self.prompt = Some(Purpose::Clone(vec![remote]).prompt(&self.clone_dir));
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
        let job = gitprogress::job(title, names, 0);
        self.start_job(label, Some(job), move |tx| {
            let token = github::token();
            let single = jobs.len() == 1;
            let mut cloned = 0;
            let mut adopted = 0;
            // Local branches made for the other branches of the clones.
            let mut branches = 0;
            let mut failed = Vec::new();
            for (index, (owner, name, url)) in jobs.iter().enumerate() {
                if process::was_cancelled() {
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
                        if single || process::was_cancelled() {
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
    pub fn refresh(&mut self) {
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
            Queued::PullRequests(folders) => self.save_pull_requests(&folders),
            Queued::Forks(folders) => self.clone_forks(&folders),
            Queued::Move(repos, dir) => self.move_checkouts(repos, dir),
            Queued::Service(name, path, url) => self.push_to_service(name, path, url),
        }
    }

    /// Starts the oldest queued task once nothing else runs. A run that
    /// failed stays on screen first: the queue waits until its dialog is
    /// closed.
    pub fn start_queued(&mut self) -> bool {
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
    pub fn drop_queue(&mut self) -> usize {
        let dropped = self.queue.len();
        self.queue.clear();
        dropped
    }

    pub fn cancel_work(&mut self) {
        if self.busy || self.fetching {
            process::request_cancel();
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

    pub fn quit(&mut self) {
        // Stop git only if this app started some. The cancel flag is shared
        // by everything in the process, so it is not raised without need.
        if self.busy || self.fetching || self.status_pending > 0 {
            process::request_cancel();
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
        process::clear_cancel();
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
        self.worker.spawn(move |tx| {
            let result = work(tx);
            let _ = tx.send(Msg::JobFinished(result));
        });
    }

    pub fn reload_lists(&mut self, keep_status: bool, fetch: bool) {
        if self.busy || self.fetching {
            self.notice("Wait for the current Git command to finish.", true);
            return;
        }
        process::clear_cancel();
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
        self.reload_pull_counts();
    }

    /// True once: when the first local read after startup is done and
    /// nothing else is going on.
    fn startup_fetch_due(&mut self) -> bool {
        std::mem::take(&mut self.fetch_after_read)
            && !self.busy
            && !self.installed.is_empty()
            && !process::was_cancelled()
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

    /// The pull requests `repo` has on GitHub, once GitHub has answered.
    pub fn pull_counts(&self, repo: &LocalRepo) -> Option<github::PullCounts> {
        self.pulls.get(&pulls_key(&repo.owner, &repo.repo)).copied()
    }

    /// New and changed pull requests on the remote that `repo` has not
    /// saved, as the last fetch found them.
    pub fn pending_pulls(&self, repo: &LocalRepo) -> gitops::PendingPulls {
        self.pulls_pending
            .get(&repo.path)
            .copied()
            .unwrap_or_default()
    }

    /// Asks GitHub in the background how many pull requests every
    /// repository has, by state. Needs a sign-in; without one the column
    /// stays empty.
    fn reload_pull_counts(&mut self) {
        let mut repos: Vec<(String, String)> = Vec::new();
        for repo in &self.installed {
            if repo.owner.is_empty() || repo.repo.is_empty() {
                continue;
            }
            let key = pulls_key(&repo.owner, &repo.repo);
            if !repos
                .iter()
                .any(|(owner, name)| pulls_key(owner, name) == key)
            {
                repos.push((repo.owner.clone(), repo.repo.clone()));
            }
        }
        if repos.is_empty() {
            return;
        }
        self.pulls_generation += 1;
        let generation = self.pulls_generation;
        self.worker.spawn(move |tx| {
            let Some(token) = github::token() else {
                return;
            };
            let Ok(answers) = github::fetch_pull_counts(&repos, &token) else {
                return;
            };
            let counts = repos
                .iter()
                .zip(answers)
                .filter_map(|((owner, name), counts)| Some((pulls_key(owner, name), counts?)))
                .collect();
            let _ = tx.send(Msg::Pulls { generation, counts });
        });
    }

    fn reload_github_only(&mut self) {
        self.remote_generation += 1;
        self.remotes_state = RemoteState::Loading;
        let generation = self.remote_generation;
        let owners = self.owners.clone();
        let pinned = self.pinned.clone();
        self.worker.spawn(move |tx| {
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
        let pull_requests = self.pull_requests;
        let queue = std::sync::Arc::new(std::sync::Mutex::new(repos));
        let workers = (if fetch { 4 } else { 8 }).min(self.status_pending.max(1));
        for _ in 0..workers {
            let queue = queue.clone();
            let token = token.clone();
            self.worker.spawn(move |tx| {
                loop {
                    let next = queue.lock().ok().and_then(|mut queue| queue.pop());
                    let Some((folder, path)) = next else {
                        break;
                    };
                    let fetch_this = fetch && !process::was_cancelled();
                    let how = gitops::Fetch {
                        token: token.as_deref(),
                        pull_requests,
                    };
                    let check = gitops::check(&path, fetch_this, how);
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

    pub fn on_msg(&mut self, message: Msg) {
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
                    if let Some(pending) = check.pulls {
                        self.pulls_pending.insert(repo.path.clone(), pending);
                    }
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
                    if process::was_cancelled() {
                        process::clear_cancel();
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
            Msg::Watched { updated, failed } => self.on_watched(updated, failed),
            Msg::PullsSaved(path) => {
                self.pulls_pending.remove(&path);
            }
            Msg::Moved { from, to } => self.on_moved(&from, to),
            Msg::Pulls { generation, counts } => {
                if generation == self.pulls_generation {
                    self.pulls = counts.into_iter().collect();
                }
            }
            Msg::Forks { generation, result } => {
                if generation != self.forks_generation {
                    return;
                }
                if let Some(view) = self.forks.as_mut() {
                    view.loading = false;
                    match result {
                        Ok(list) => {
                            view.original = list.original;
                            view.dest = list.dest;
                            view.forks = list.forks;
                        }
                        Err(err) => view.error = Some(err),
                    }
                }
            }
            Msg::Cloned(path) => {
                if paths::push_path(&mut self.checkouts, path) {
                    self.save_config();
                }
            }
            Msg::JobFinished(result) => {
                self.busy = false;
                let cancelled = result.is_err() && process::was_cancelled();
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
                process::clear_cancel();
                self.generation += 1;
                self.fetching = false;
                self.settle_removed();
                self.load_rows();
                self.prune_marks();
                self.spawn_status(self.generation, false);
                // A run may have fetched the forks that are on screen.
                self.load_forks();
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

/// Lists the forks of one checkout on GitHub and collects them in the
/// folder beside it, which is handed back with what it holds. None when
/// the repository has no forks: no folder is made then.
fn forks_of(
    tx: &Sender<Msg>,
    path: &Path,
    original: &(String, String),
    origin: &str,
    token: &Option<String>,
) -> Result<Option<(PathBuf, gitops::ForksSaved)>, String> {
    let (owner, name) = original;
    if owner.is_empty() || name.is_empty() {
        return Err("is not a repository on GitHub".into());
    }
    let dest = gitops::forks_folder(path).ok_or("has no folder name")?;
    send_line(
        tx,
        &format!("Asking GitHub for the forks of {owner}/{name}."),
        false,
    );
    let listed = github::fetch_forks(owner, name, token.as_deref(), |line, progress| {
        send_line(tx, line, progress)
    })?;
    if let Some(warning) = &listed.warning {
        send_line(tx, &format!("The list is not complete: {warning}."), false);
    }
    if listed.forks.is_empty() {
        send_line(tx, "It has no forks.", false);
        return Ok(None);
    }
    send_line(
        tx,
        &format!("{} forks, into {}", listed.forks.len(), dest.display()),
        false,
    );
    let forks: Vec<gitops::ForkSource> = listed
        .forks
        .into_iter()
        .map(|fork| gitops::ForkSource {
            owner: fork.owner,
            name: fork.name,
            stamp: fork.stamp,
        })
        .collect();
    let url_of = |owner: &str, name: &str| gitops::fork_url(origin, owner, name);
    let saved = gitops::save_forks(
        path,
        &dest,
        original,
        &forks,
        &url_of,
        token.as_deref(),
        |line, progress| send_line(tx, line, progress),
    )?;
    Ok(Some((dest, saved)))
}

/// Updates one checkout on every branch. True means nothing moved.
/// `expected` is what the last check saw, shown until the fetch has
/// counted the branches again.
fn pull_one(
    tx: &Sender<Msg>,
    path: &Path,
    how: gitops::Fetch,
    expected: u32,
) -> Result<bool, String> {
    if expected > 0 {
        send(tx, JobEvent::Steps(0, expected));
    }
    let updated = gitops::update(
        path,
        how,
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

/// The name the pull request counts of a repository are kept under.
fn pulls_key(owner: &str, repo: &str) -> String {
    format!("{owner}/{repo}").to_lowercase()
}

fn count_label(count: usize) -> String {
    if count == 1 {
        "1 repository".to_string()
    } else {
        format!("{count} repositories")
    }
}

#[cfg(test)]
#[doc(hidden)]
pub mod testing {
    use super::*;

    pub fn test_settings() -> Settings {
        Settings {
            checkouts: Vec::new(),
            placement: Vec::new(),
            collapsed: Vec::new(),
            update_check: false,
            watched: Vec::new(),
            watch_interval: 5,
            pull_requests: false,
            update_check_now: false,
            shell: Default::default(),
            look: Default::default(),
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
                std::thread::current().id()
            )),
            first_run: false,
        }
    }

    pub fn installed(owner: &str, repo: &str) -> LocalRepo {
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
            pull_requests: 0,
            ready: true,
            fetched: false,
            error: None,
            warning: None,
            group: String::new(),
        }
    }

    pub fn remote(owner: &str, name: &str) -> RemoteRepo {
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

    /// Four repositories at paths of their own, so groups can be saved.
    pub fn grouped_app() -> Repos {
        let mut app = Repos::empty(test_settings());
        for name in ["api", "docs", "notes", "web"] {
            let mut repo = installed("acme", name);
            repo.path = PathBuf::from("/srv/code").join(name);
            app.checkouts.push(repo.path.clone());
            app.installed.push(repo);
        }
        app.selected = Some("api".into());
        app
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("devassist-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn add_list_offers_a_repository_outside_the_catalog() {
        let mut app = Repos::empty(test_settings());
        app.add_filter = "other/tool".into();
        let list = app.add_list();
        assert_eq!(list.len(), 1);
        assert!(list[0].direct);
        assert_eq!(list[0].id(), "other/tool");
    }

    #[test]
    fn add_list_keeps_an_installed_repository_visible() {
        let mut app = Repos::empty(test_settings());
        app.installed.push(installed("other", "tool"));
        app.add_filter = "other/tool".into();
        let list = app.add_list();
        assert_eq!(list.len(), 1);
        assert!(app.is_installed(&list[0]));
    }

    #[test]
    fn add_list_shows_installed_catalog_repositories_but_prefers_a_new_one() {
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
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
    fn a_default_account_is_not_looked_up_again() {
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
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
            std::thread::current().id()
        ));
        let mut settings = test_settings();
        settings.config_path = path.clone();
        let mut app = Repos::empty(settings);
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
    fn a_setting_left_alone_here_is_saved_as_the_file_has_it_now() {
        let mut app = Repos::empty(test_settings());
        let _ = std::fs::remove_file(&app.config_path);
        app.save_config();
        // Another DevAssist changes the interval and the look in the file.
        let other = std::fs::read_to_string(&app.config_path).unwrap()
            + "watch_interval = 30\ntheme = dark\n";
        std::fs::write(&app.config_path, other).unwrap();

        // A save from the list keeps both.
        app.clone_dir = std::env::temp_dir().join("devassist-elsewhere");
        app.save_config();
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("watch_interval = 30\n"), "{text}");
        assert!(text.contains("theme = dark\n"), "{text}");
        assert!(
            text.contains(&format!("clone_dir = {}\n", app.clone_dir.display())),
            "{text}"
        );

        // So does the settings dialog, which changes the title only.
        app.open_settings(false);
        app.form.as_mut().unwrap().title = "Desk".into();
        app.save_settings();
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        let _ = std::fs::remove_file(&app.config_path);
        assert!(text.contains("title = Desk\n"), "{text}");
        assert!(text.contains("watch_interval = 30\n"), "{text}");
        assert!(text.contains("theme = dark\n"), "{text}");
    }

    #[test]
    fn checkouts_taken_over_at_startup_are_written_whole_and_then_merged() {
        let api = std::env::temp_dir().join("devassist-taken-api");
        let web = std::env::temp_dir().join("devassist-taken-web");
        let mut settings = test_settings();
        // The file knows api; web was imported at this start.
        settings.checkouts = vec![api.clone(), web.clone()];
        settings.unsaved = true;
        let path = settings.config_path.clone();
        std::fs::write(&path, format!("checkout = {}\n", api.display())).unwrap();
        let mut app = Repos::empty(settings);

        // The first save writes the checkouts as the app has them.
        app.save_config();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains(&format!("checkout = {}\n", api.display())),
            "{text}"
        );
        assert!(
            text.contains(&format!("checkout = {}\n", web.display())),
            "{text}"
        );

        // A later save keeps what another DevAssist changed meanwhile.
        std::fs::write(&path, text + "watch_interval = 30\n").unwrap();
        app.clone_dir = std::env::temp_dir().join("devassist-taken-elsewhere");
        app.save_config();
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(text.contains("watch_interval = 30\n"), "{text}");
        assert!(
            text.contains(&format!("checkout = {}\n", web.display())),
            "{text}"
        );
        assert!(
            text.contains(&format!("clone_dir = {}\n", app.clone_dir.display())),
            "{text}"
        );
    }

    #[test]
    fn saving_keeps_the_look_the_settings_file_had() {
        let look = config::Look {
            theme: config::Theme::Dark,
            backdrop: "forest".into(),
            blur: false,
        };
        let mut settings = test_settings();
        settings.look = look.clone();
        let mut app = Repos::empty(settings);

        // Any save from the list writes it back.
        app.save_config();
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        let _ = std::fs::remove_file(&app.config_path);
        assert!(text.contains("theme = dark\n"), "{text}");
        assert!(text.contains("backdrop = forest\n"), "{text}");
        assert!(text.contains("blur = off\n"), "{text}");

        // So does the settings dialog, which does not edit them.
        app.open_settings(true);
        app.save_settings();
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        let _ = std::fs::remove_file(&app.config_path);
        assert!(text.contains("theme = dark\n"), "{text}");
        assert!(text.contains("backdrop = forest\n"), "{text}");
        assert!(text.contains("blur = off\n"), "{text}");
    }

    #[test]
    fn a_mistake_keeps_the_settings_dialog_open() {
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
        app.installed.push(installed("storytold", "photocraft"));
        app.remotes.push(remote("storytold", "photocraft"));
        app.remotes.push(remote("other", "photocraft"));
        assert_eq!(app.missing_count(), 1);
    }

    #[test]
    fn needs_update_sort_puts_behind_repositories_first() {
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
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
        let mut app = Repos::empty(test_settings());
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
    fn filter_matches_the_owner() {
        let mut app = Repos::empty(test_settings());
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
    fn pull_request_counts_from_github_land_on_their_repository() {
        let mut app = grouped_app();
        let api = app.installed[0].clone();
        assert_eq!(app.pull_counts(&api), None);
        let counts = github::PullCounts {
            open: 2,
            merged: 14,
            closed: 3,
        };
        app.pulls_generation = 4;
        // An answer to an older question is dropped.
        app.on_msg(Msg::Pulls {
            generation: 3,
            counts: vec![("acme/api".into(), counts)],
        });
        assert_eq!(app.pull_counts(&api), None);
        app.on_msg(Msg::Pulls {
            generation: 4,
            counts: vec![("acme/api".into(), counts)],
        });
        assert_eq!(app.pull_counts(&api), Some(counts));
        // Upper and lower case name the same repository.
        let mut shouted = api.clone();
        shouted.owner = "ACME".into();
        assert_eq!(app.pull_counts(&shouted), Some(counts));
        assert_eq!(app.pull_counts(&app.installed[1].clone()), None);
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn pull_requests_waiting_on_the_remote_are_kept_until_they_are_saved() {
        let mut app = grouped_app();
        let api = app.installed[0].clone();
        assert!(!app.pending_pulls(&api).any());
        let waiting = gitops::PendingPulls { new: 2, changed: 1 };
        let check = |pulls| gitops::RepoCheck {
            info: Err("not read".into()),
            warning: None,
            fetched: true,
            pulls,
        };
        app.status_pending = 2;
        app.on_msg(Msg::Status {
            generation: app.generation,
            folder: "api".into(),
            check: check(Some(waiting)),
        });
        assert_eq!(app.pending_pulls(&api), waiting);
        // A check that did not ask the remote leaves what is known.
        app.on_msg(Msg::Status {
            generation: app.generation,
            folder: "api".into(),
            check: check(None),
        });
        assert_eq!(app.pending_pulls(&api), waiting);
        // Saving them clears it.
        app.on_msg(Msg::PullsSaved(api.path.clone()));
        assert!(!app.pending_pulls(&api).any());
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn a_moved_checkout_keeps_its_place_on_the_list() {
        let mut app = grouped_app();
        let old = PathBuf::from("/srv/code/api");
        let new = PathBuf::from("/srv/elsewhere/api");
        app.watched = vec![old.clone()];
        app.placement = vec![(old.clone(), "Work".into())];
        let waiting = gitops::PendingPulls { new: 1, changed: 0 };
        app.pulls_pending.insert(old.clone(), waiting);

        app.on_msg(Msg::Moved {
            from: old.clone(),
            to: new.clone(),
        });
        assert!(app.checkouts.contains(&new));
        assert!(!app.checkouts.contains(&old));
        assert_eq!(app.watched, std::slice::from_ref(&new));
        assert_eq!(app.placement, [(new.clone(), "Work".to_string())]);
        assert_eq!(app.installed[0].path, new);
        assert_eq!(app.pending_pulls(&app.installed[0].clone()), waiting);
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("elsewhere"), "{text}");
        let _ = std::fs::remove_file(&app.config_path);
    }

    /// `cargo test -- --ignored live_forks`
    #[test]
    #[ignore = "asks GitHub and clones a small public repository"]
    fn live_forks_of_a_public_repository_are_collected_beside_it() {
        let root = std::env::temp_dir().join(format!("devassist-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let original = ("octocat".to_string(), "boysenberry-repo-1".to_string());
        let origin = gitops::fork_url("", &original.0, &original.1);
        let path = root.join("berry");
        let token = github::token();
        gitops::clone_repo(&origin, &path, token.as_deref(), |_, _| {}).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let (dest, saved) = forks_of(&tx, &path, &original, &origin, &token)
            .unwrap()
            .expect("it has forks");
        drop(tx);
        for msg in rx {
            if let Msg::Job(JobEvent::Line(line, false)) = msg {
                println!("{line}");
            }
        }
        println!("{saved:?}");
        assert_eq!(dest, root.join("berry-fork"));
        assert!(saved.created);
        assert!(saved.fetched > 1, "{saved:?}");
        assert!(saved.branches > saved.fetched, "{saved:?}");
        assert!(saved.pulls > 0, "{saved:?}");
        assert!(saved.folders > 0, "{saved:?}");
        let folders = std::fs::read_dir(&dest).unwrap().count();
        // The folders of the changed forks, and the repository itself.
        assert_eq!(folders, saved.folders as usize + 1);
        // Again at once: no fork has changed, so none is fetched.
        let (tx, _rx) = std::sync::mpsc::channel();
        let (_, again) = forks_of(&tx, &path, &original, &origin, &token)
            .unwrap()
            .expect("it has forks");
        println!("{again:?}");
        assert_eq!((again.fetched, again.unchanged), (0, saved.fetched));
        assert_eq!(again.folders, saved.folders);
        let _ = gitops::remove_checkout(&dest);
        let _ = gitops::remove_checkout(&path);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_automatic_pull_waits_for_its_turn() {
        let mut app = grouped_app();
        // Nothing is watched: nothing starts.
        assert!(!app.start_due_watch());
        app.watched = vec![PathBuf::from("/srv/code/api")];

        app.watch_due = Instant::now() + Duration::from_secs(60);
        assert!(!app.start_due_watch(), "not due yet");
        app.watch_due = Instant::now();
        app.busy = true;
        assert!(!app.start_due_watch(), "another task runs");
        app.busy = false;
        app.queue.push_back(Queued::Refresh);
        assert!(!app.start_due_watch(), "a task waits");
        app.queue.clear();
        app.status_pending = 1;
        assert!(!app.start_due_watch(), "the rows are being read");
        app.status_pending = 0;

        // A repository whose upstream is gone has nothing to pull.
        app.installed[0].upstream_gone = true;
        assert!(!app.start_due_watch());
        assert!(!app.busy);
        // The interval started over all the same.
        assert!(app.watch_due > Instant::now() + Duration::from_secs(200));
    }

    #[test]
    fn a_finished_automatic_pull_reports_on_the_status_line() {
        let mut app = Repos::empty(test_settings());
        let finish = |app: &mut Repos, updated: &[&str], failed: &[(&str, &str)]| {
            app.notice("7 installed.", false);
            app.watch_status = Some((app.status.clone(), false));
            app.notice(format!("{WATCH_LABEL} api…"), false);
            app.busy = true;
            app.on_watched(
                updated.iter().map(|name| name.to_string()).collect(),
                failed
                    .iter()
                    .map(|(name, err)| (name.to_string(), err.to_string()))
                    .collect(),
            );
            assert!(!app.busy);
        };
        // Nothing new: the line says what it said before.
        finish(&mut app, &[], &[]);
        assert_eq!(app.status, "7 installed.");
        finish(&mut app, &["api", "web"], &[]);
        assert_eq!(app.status, "Auto-pull updated api, web.");
        assert!(!app.status_error);
        finish(&mut app, &["web"], &[("api", "no network")]);
        assert_eq!(app.status, "Auto-pull of api failed: no network");
        assert!(app.status_error);
    }

    /// Runs git for a test, quietly and with a fixed author.
    fn git(dir: &Path, args: &[&str]) {
        let output = std::process::Command::new("git")
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

    #[test]
    fn a_watched_repository_is_pulled_when_its_time_has_come() {
        let root = scratch("watch");
        let origin = root.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        std::fs::write(origin.join("one"), "one").unwrap();
        git(&origin, &["add", "one"]);
        git(&origin, &["commit", "-q", "-m", "one"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), "tool"]);
        let clone = root.join("tool");
        // The origin moves on after the clone.
        std::fs::write(origin.join("two"), "two").unwrap();
        git(&origin, &["add", "two"]);
        git(&origin, &["commit", "-q", "-m", "two"]);
        assert!(!clone.join("two").exists());

        let mut app = Repos::empty(test_settings());
        let mut repo = installed("acme", "tool");
        repo.path = clone.clone();
        app.checkouts.push(clone.clone());
        app.installed.push(repo);
        app.watched = vec![clone.clone()];
        app.notice("1 installed.", false);

        let wait = |app: &mut Repos| {
            let until = Instant::now() + Duration::from_secs(60);
            while app.busy || app.status_pending > 0 {
                assert!(Instant::now() < until, "the pull did not finish");
                app.drain();
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        assert!(app.drain(), "the pull starts on the first idle moment");
        assert!(app.busy);
        // It stays out of the way: no dialog opens.
        assert!(app.mode == Mode::List);
        assert!(app.job.is_none());
        wait(&mut app);
        assert_eq!(app.status, "Auto-pull updated tool.");
        assert!(clone.join("two").exists());

        // The next one is an interval away. Brought forward, it finds
        // nothing and leaves the status line as it was.
        assert!(!app.start_due_watch());
        app.notice("1 installed.", false);
        app.watch_due = Instant::now();
        assert!(app.start_due_watch());
        wait(&mut app);
        assert_eq!(app.status, "1 installed.");
        let _ = std::fs::remove_dir_all(&root);
    }
}
