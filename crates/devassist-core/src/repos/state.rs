//! What the repositories tool knows and does. Its keys and its mouse are
//! the terminal app's, in `src/features/repos/app.rs`.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use crate::list;
use crate::prompt::{self as pathprompt, PathPrompt};
use crate::repos::forklines::{self, ForkProgress, ForkWatch};
use crate::repos::forks::{self, ForkCounts};
use crate::repos::form::Form;

use crate::config::{self, Settings};
use crate::repos::github::{self, AccountQuery, RemoteRepo};
use crate::repos::gitops::{self, LocalRepo};
use crate::repos::gitprogress;
use crate::sys::open;
use crate::sys::paths;
use crate::sys::process;
use crate::task::job::{Job, JobEvent};
use crate::task::worker::Worker;

/// How long the Add search waits after the last key before asking GitHub.
const LOOKUP_DELAY: Duration = Duration::from_millis(350);

/// Opening more folders than this at once is refused.
#[doc(hidden)]
pub const OPEN_LIMIT: usize = 8;
/// How long the progress dialog stays after a run that went well.
#[doc(hidden)]
pub const JOB_LINGER: Duration = Duration::from_millis(1200);
/// What the status line says when held work was asked for (see `Repos::hold_work`).
pub const HELD: &str = "This is a demo. Nothing is run or changed.";
/// What the status line says when a push to another service was cancelled.
const PUSH_CANCELLED: &str =
    "Cancelled. What was already sent stays on the other service; the address was not kept.";
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

/// One folder the remove dialog asks about.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RemoveLine {
    /// The folder's name on the list.
    pub name: String,
    /// Where it is on disk. Empty when it is no longer on the list.
    pub path: PathBuf,
    /// What is known of its uncommitted changes.
    pub state: RemoveState,
}

impl RemoveLine {
    /// It has uncommitted changes. False while that is not known: not
    /// read yet, the read failed, or no longer on the list.
    pub fn dirty(&self) -> bool {
        self.state == RemoveState::Dirty
    }
}

/// What the remove dialog knows of a folder's uncommitted changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemoveState {
    /// Read: it has none.
    Clean,
    /// It has some.
    Dirty,
    /// Not known: its state is not read yet, or the read failed.
    Unread,
    /// It is no longer on the list.
    Gone,
}

/// One checkout the move question asks about. A line that is `there` or
/// `taken` does not move: the dialog shows it as staying where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveLine {
    /// Its name on the list.
    pub name: String,
    /// Where it is now.
    pub from: PathBuf,
    /// Where the move puts it: the typed folder and its folder name. None
    /// while the typed folder is not one a move can use.
    pub to: Option<PathBuf>,
    /// It is in that folder already (`from` is `to`): the move leaves it
    /// where it is and says so.
    pub there: bool,
    /// The move of this one will be refused: something is at `to`
    /// already, an earlier line of the same move goes to the same `to`,
    /// or `to` lies inside the checkout itself. The others still move.
    pub taken: bool,
}

/// One thing a push to another service does or leaves, in a sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fact {
    /// It goes over or is done there; false for what stays as it is or is
    /// not done.
    pub goes: bool,
    /// One or two short sentences a person reads, as they are shown.
    pub words: String,
}

/// What Enter in Add asks for, before a folder is given.
enum AddChoice {
    /// Clone these.
    Clone(Vec<RemoteRepo>),
    /// Nothing is listed and a plain word is typed: list that account.
    ListAccount,
}

/// Where a move into `dir` puts the checkout at `from`: under the folder
/// name it has now. None for a path without one.
fn move_target(dir: &Path, from: &Path) -> Option<PathBuf> {
    Some(dir.join(from.file_name()?))
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
        Purpose::Move(repos) if repos.len() == 1 => {
            move_target(&pathprompt::resolve(&prompt.input).ok()?, &repos[0].1)
        }
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
            (MenuItem::AllForks, _) => "Update the forks of all repositories",
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

    fn next(self) -> Self {
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
    /// What "ahead" is counted against, as the collection says it.
    base: Option<String>,
    /// When the forks were last fetched, in seconds since 1970.
    fetched: Option<i64>,
}

impl ForksView {
    /// The numbers of the overview: of every fork, and of the ones shown.
    pub fn counts(&self) -> ForkCounts {
        ForkCounts {
            all: self.forks.len(),
            shown: self.rows().len(),
            changed: self.forks.iter().filter(|fork| fork.folder).count(),
            gone: self.forks.iter().filter(|fork| fork.gone).count(),
            branches: self.forks.iter().map(|fork| fork.branches).sum(),
            pulls: self.forks.iter().map(|fork| fork.pulls).sum(),
        }
    }

    /// The sentence that sums up the forks, as "38 forks. 6 have changes
    /// and a folder. 1 is gone from GitHub. 61 branches and 9 pull
    /// requests are saved.": the parts that are zero are left out.
    pub fn summary(&self) -> String {
        forks::summary(&self.counts())
    }

    /// Why the list shows no fork, or None when it shows some.
    pub fn empty_message(&self) -> Option<String> {
        if !self.rows().is_empty() {
            return None;
        }
        let text = if self.loading {
            "Reading the forks…"
        } else if self.error.is_some() {
            "The forks could not be read. r tries again."
        } else if self.forks.is_empty() {
            "No fork is in the folder yet. u fetches them."
        } else {
            "No fork matches. esc clears the filter, c shows the forks without changes."
        };
        Some(text.to_string())
    }

    /// What "ahead" is counted against, as `AsP3X/main`, when the
    /// collection says it.
    pub fn base(&self) -> Option<&str> {
        self.base.as_deref()
    }

    /// When the forks were last fetched, in seconds since 1970.
    pub fn fetched(&self) -> Option<i64> {
        self.fetched
    }

    /// Takes what a read of the collection found.
    fn take(&mut self, list: gitops::ForkList) {
        self.loading = false;
        self.error = None;
        self.original = list.original;
        self.dest = list.dest;
        self.forks = list.forks;
        self.base = list.base;
        self.fetched = list.fetched;
    }

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
    /// Every repository it is about has its forks cloned beside it.
    pub forks_cloned: bool,
}

impl Menu {
    /// What a line says in this menu.
    pub fn label(&self, item: MenuItem) -> &'static str {
        match item {
            // Choosing it again fetches what is new, so it says so.
            MenuItem::Forks if self.forks_cloned => "Update the forks",
            item => item.label(self.repos.len() > 1),
        }
    }
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

#[doc(hidden)]
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
    #[doc(hidden)]
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
    #[doc(hidden)]
    pub forks_generation: u64,
    /// Made-up forks a held session shows, by repository (`hold_forks`).
    held_forks: HashMap<String, gitops::ForkList>,
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
    /// The job is a fetch of forks: what the lines of its task running now
    /// said. None for every other job.
    fork_watch: Option<ForkWatch>,
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
    #[doc(hidden)]
    pub watch_due: Instant,
    /// Nothing runs and nothing is written: set by `hold_work`, and by
    /// `reading` once its first read has started.
    work_held: bool,
    /// GitHub is not asked for the catalog or the pull request counts,
    /// while local work runs as always: set by `testing::hold_github`, for
    /// the tests that run real work on checkouts whose origin names GitHub.
    github_held: bool,
    /// Where a token for GitHub comes from: `github::token`, or a test's own
    /// (`testing::token_source`). Never asked while GitHub is held.
    token_source: fn() -> Option<String>,
    /// The remotes of the checkout the service dialog is open for, with
    /// their addresses: read once when it opens.
    service_remotes: Vec<(String, String)>,
    /// The run is a push to another service: a cancel of it says what
    /// stays there.
    push_running: bool,
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

    /// Reads the checkouts as they are on disk and changes none of them:
    /// no fetch, no automatic pull, no settings dialog, and the settings
    /// file is not written. For a run that only looks, such as a picture
    /// of the window.
    pub fn reading(settings: Settings) -> Self {
        let mut app = Self::empty(settings);
        // The first read runs as it always did, then everything is held.
        app.reload_lists(false, false);
        app.hold_work();
        app
    }

    /// Holds all work from now on, for a run that only shows: nothing is
    /// started or queued, nothing is fetched or pulled, GitHub is not
    /// asked, no folder, page or clipboard is touched, and the settings
    /// file is not written. What was asked for says `HELD` on the status
    /// line. Menus, dialogs, ticks, the highlight, sort, filter, folding
    /// and groups work as always, and the local read of the rows goes on.
    pub fn hold_work(&mut self) {
        self.work_held = true;
        self.queue.clear();
        self.lookup_due = None;
        self.fetch_after_read = false;
        // No catalog read was asked for, and none will be: nothing is
        // loading. One already asked for (by `reading`) is left to land.
        if self.remote_generation == 0 && self.remotes_state == RemoteState::Loading {
            self.remotes_state = RemoteState::Ready;
        }
    }

    /// Whether `hold_work` holds all work.
    pub fn work_held(&self) -> bool {
        self.work_held
    }

    /// When work is held, says so on the status line and returns true:
    /// the caller then does nothing.
    fn held(&mut self) -> bool {
        if self.work_held {
            self.notice(HELD, false);
        }
        self.work_held
    }

    /// A tool that reads no folder and calls no network.
    pub fn offline(settings: Settings) -> Self {
        let first_run = settings.first_run;
        let mut app = Self::empty(settings);
        app.remotes_state = RemoteState::Ready;
        if first_run {
            app.open_settings(true);
        }
        app
    }

    #[doc(hidden)]
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
            held_forks: HashMap::new(),
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
            fork_watch: None,
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
            work_held: false,
            github_held: false,
            token_source: github::token,
            service_remotes: Vec::new(),
            push_running: false,
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
        self.add_status_with(true)
    }

    /// `add_status` without its hint at a key: where Tab lists the
    /// typed word as an account, that part is left out. For a window,
    /// where Tab moves the focus.
    pub fn add_status_plain(&self) -> AddStatus {
        self.add_status_with(false)
    }

    /// Add's status line, with or without the hint that Tab lists the
    /// typed word.
    fn add_status_with(&self, tab_hint: bool) -> AddStatus {
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
                // Held, no lookup is started: nothing is loading. One that
                // was already running is left to land.
                None if self.work_held
                    && self.lookup_running.as_deref()
                        != Some(query.owner.to_ascii_lowercase().as_str()) =>
                {
                    plain(HELD.to_string())
                }
                None => loading(format!("Loading {site}…")),
            };
        }
        match &self.remotes_state {
            RemoteState::Loading => loading("Loading repositories from GitHub…".to_string()),
            RemoteState::Failed(err) => failed(err.clone()),
            RemoteState::Ready => {
                let other = match self.add_word() {
                    Some(word) if tab_hint => Some(format!("tab lists github.com/{word}")),
                    Some(_) => None,
                    None => Some("type owner/ or paste a link to list any account".to_string()),
                };
                let other = other.map(|other| format!(" · {other}")).unwrap_or_default();
                if self.owners.is_empty() && self.pinned.is_empty() {
                    return plain(format!("No default account{other}"));
                }
                let cloned = self
                    .remotes
                    .iter()
                    .filter(|repo| self.is_installed(repo))
                    .count();
                plain(format!(
                    "{} · {cloned} cloned{other}",
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

    /// Where Add looks, for its title: the account the search names, the
    /// one default account, or GitHub.
    pub fn add_source(&self) -> String {
        if let Some(owner) = self.add_account() {
            return format!("github.com/{owner}");
        }
        match self.owners.as_slice() {
            [owner] => format!("github.com/{owner}"),
            _ => "GitHub".to_string(),
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

    /// Highlights the found repository `id` ("owner/name"). Nothing when
    /// Add is closed or `id` is not on its list.
    pub fn pick_add(&mut self, id: &str) {
        if self.mode != Mode::Add {
            return;
        }
        if let Some(remote) = self
            .add_list()
            .into_iter()
            .find(|repo| repo.id().eq_ignore_ascii_case(id))
        {
            self.add_selected = Some(remote.id());
        }
    }

    /// Ticks or unticks the found repository `id` ("owner/name"), and
    /// leaves the highlight where it is. One already cloned is not ticked.
    /// Nothing when Add is closed or `id` is not on its list.
    pub fn toggle_add_mark_of(&mut self, id: &str) {
        if self.mode != Mode::Add {
            return;
        }
        let Some(remote) = self
            .add_list()
            .into_iter()
            .find(|repo| repo.id().eq_ignore_ascii_case(id))
        else {
            return;
        };
        if let Some(index) = self
            .add_marked
            .iter()
            .position(|marked| marked.id().eq_ignore_ascii_case(id))
        {
            self.add_marked.remove(index);
        } else if !self.is_installed(&remote) {
            self.add_marked.push(remote);
        }
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
        self.open_forks_with(true);
    }

    /// `open_forks`, whose notice for forks that are not cloned names no
    /// key. For a window.
    pub fn open_forks_plain(&mut self) {
        self.open_forks_with(false);
    }

    /// Opens the fork overview; `key_hint` says whether the notice for
    /// forks that are not cloned names the terminal's keys. Held, the
    /// forks are the ones `hold_forks` gave, and no folder is looked at.
    fn open_forks_with(&mut self, key_hint: bool) {
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
        let cloned = if self.work_held {
            self.held_forks.contains_key(&repo.folder)
        } else {
            gitops::forks_cloned(&repo.path)
        };
        if !cloned {
            let how = if key_hint {
                "Enter, then n clones them."
            } else {
                "Clone the forks does it."
            };
            self.notice(
                format!("The forks of {} are not cloned yet. {how}", repo.folder),
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
            base: None,
            fetched: None,
        });
        self.load_forks();
    }

    /// Gives a held session the forks it shows for the repository `repo`
    /// (by row name), in place of a collection on disk: story data for a
    /// demo. A later list for the same repository replaces this one. A
    /// live session never looks at them.
    pub fn hold_forks(&mut self, repo: &str, list: gitops::ForkList) {
        self.held_forks.insert(repo.to_string(), list);
    }

    /// Filters the fork overview by `text`, from the top of the list. A
    /// highlighted fork the filter hides gives way as `ForksView::chosen`
    /// says. Nothing happens when the text is the same or no overview is
    /// open.
    pub fn set_fork_filter(&mut self, text: &str) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        if view.filter == text {
            return;
        }
        view.filter = text.to_string();
        view.scroll = 0;
    }

    /// Empties the fork overview's filter and ends its typing.
    pub fn clear_fork_filter(&mut self) {
        self.set_fork_filter("");
        if let Some(view) = self.forks.as_mut() {
            view.filtering = false;
        }
    }

    /// Sorts the fork overview by `sort`, from the top of the list.
    pub fn set_fork_sort(&mut self, sort: ForkSort) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        if view.sort != sort {
            view.sort = sort;
            view.scroll = 0;
        }
    }

    /// Shows only the forks with commits of their own, or all of them,
    /// from the top of the list.
    pub fn set_fork_changed_only(&mut self, on: bool) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        if view.changed_only != on {
            view.changed_only = on;
            view.scroll = 0;
        }
    }

    /// Reads the fork folder of the open overview again, in the background.
    /// Held, it takes the forks `hold_forks` gave at once.
    pub fn load_forks(&mut self) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        if self.work_held {
            self.forks_generation += 1;
            match self.held_forks.get(&view.repo) {
                Some(list) => view.take(list.clone()),
                None => {
                    view.loading = false;
                    self.notice(HELD, false);
                }
            }
            return;
        }
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
        if self.held() {
            return;
        }
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
        if self.held() {
            return;
        }
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

    /// Sorts the fork overview the next way.
    pub fn cycle_fork_sort(&mut self) {
        if let Some(sort) = self.forks.as_ref().map(|view| view.sort.next()) {
            self.set_fork_sort(sort);
        }
    }

    /// Shows only the forks with commits of their own, or all of them.
    pub fn toggle_fork_changed_only(&mut self) {
        if let Some(on) = self.forks.as_ref().map(|view| !view.changed_only) {
            self.set_fork_changed_only(on);
        }
    }

    /// Starts typing a new filter for the fork overview.
    pub fn start_fork_filter(&mut self) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        view.filter.clear();
        view.filtering = true;
    }

    /// Highlights a fork in the overview, by owner, and ends the typing
    /// of its filter.
    pub fn pick_fork(&mut self, owner: &str) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        view.selected = Some(owner.to_string());
        view.filtering = false;
    }

    pub fn is_marked_local(&self, folder: &str) -> bool {
        self.marked.iter().any(|marked| marked == folder)
    }

    /// Ticks or unticks the highlighted repository, then moves down so
    /// Space can be pressed again. On a group's header it ticks the whole
    /// group, or unticks it when all of it is ticked.
    pub fn toggle_list_mark(&mut self) {
        let names = self.list_names();
        let Some(row) = self
            .selected
            .clone()
            .filter(|row| names.contains(row))
            .or_else(|| names.first().cloned())
        else {
            return;
        };
        self.toggle_mark_of(&row);
        self.selected = Some(row);
        // The overview's highlight moves, also while Add is open over it.
        if matches!(self.mode, Mode::List | Mode::Filter | Mode::Add) {
            list::move_by(&mut self.selected, &names, 1);
        }
    }

    /// Ticks or unticks the row with this key (a repository's folder or a
    /// group's key) without moving the highlight. A group's key ticks the
    /// whole group, or unticks it when all of it is ticked. A key that is
    /// not a row changes nothing.
    pub fn toggle_mark_of(&mut self, row: &str) {
        if !self.list_names().iter().any(|name| name == row) {
            return;
        }
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
        } else if let Some(index) = self.marked.iter().position(|marked| marked == row) {
            self.marked.remove(index);
        } else {
            self.marked.push(row.to_string());
        }
    }

    /// Unticks everything.
    pub fn clear_marks(&mut self) {
        self.marked.clear();
    }

    /// Highlights the row with this key (a repository's folder or a
    /// group's key). Only a row the list shows can be picked: an unknown
    /// key, or one the filter or a folded group hides, changes nothing.
    pub fn pick(&mut self, key: &str) {
        if self.list_names().iter().any(|name| name == key) {
            self.selected = Some(key.to_string());
        }
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
        // Held (a demo), no folder is looked for: the forks are cloned
        // where the story gave some (`hold_forks`), for every name chosen.
        let forks_cloned = if self.work_held {
            repos.iter().all(|name| self.held_forks.contains_key(name))
        } else {
            self.installed
                .iter()
                .filter(|repo| repos.contains(&repo.folder))
                .all(|repo| gitops::forks_cloned(&repo.path))
        };
        self.menu = Some(Menu {
            repos,
            selected: 0,
            migrate: None,
            forks_cloned,
        });
        self.mode = Mode::Menu;
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
        self.mode = Mode::List;
    }

    /// The menu's line that is highlighted, while the menu is open.
    pub fn menu_highlight(&self) -> Option<MenuItem> {
        let menu = self.menu.as_ref()?;
        MenuItem::ALL.get(menu.selected).copied()
    }

    /// Highlights the menu's line `item`, for a menu that shows only some
    /// of its lines. Nothing while the menu is closed.
    pub fn highlight_menu(&mut self, item: MenuItem) {
        if let Some(menu) = self.menu.as_mut()
            && let Some(index) = MenuItem::ALL.iter().position(|known| *known == item)
        {
            menu.selected = index;
        }
    }

    /// The Migrate submenu's line that is highlighted, while it is open.
    pub fn migrate_highlight(&self) -> Option<MigrateItem> {
        let index = self.menu.as_ref()?.migrate?;
        MigrateItem::ALL.get(index).copied()
    }

    /// Highlights the Migrate submenu's line `item`. Nothing while the
    /// submenu is closed.
    pub fn highlight_migrate(&mut self, item: MigrateItem) {
        if let Some(selected) = self.menu.as_mut().and_then(|menu| menu.migrate.as_mut())
            && let Some(index) = MigrateItem::ALL.iter().position(|known| *known == item)
        {
            *selected = index;
        }
    }

    /// Closes the Migrate submenu and goes back to the menu it came from.
    /// Nothing while the submenu is closed.
    pub fn close_migrate(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.migrate = None;
        }
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
        let repos: Vec<(String, PathBuf, String)> = self
            .chosen_repos()
            .iter()
            .map(|repo| (repo.folder.clone(), repo.path.clone(), repo.origin.clone()))
            .collect();
        match repos.as_slice() {
            [] => self.notice("Select a repository first.", true),
            [(repo, path, origin)] => {
                // Held (a demo), the row's origin stands for its remotes:
                // no git is run.
                self.service_remotes = if self.work_held {
                    vec![("origin".to_string(), origin.clone())]
                } else {
                    gitops::remote_urls(path)
                };
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

    /// The service dialog's address becomes `text`, whole; what went wrong
    /// with the last one is forgotten. Nothing while the dialog is closed
    /// or when the text is the same. The address is checked by
    /// `accept_service`.
    pub fn set_service_input(&mut self, text: &str) {
        if let Some(prompt) = self.service_prompt.as_mut()
            && prompt.input != text
        {
            text.clone_into(&mut prompt.input);
            prompt.error = None;
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
        // Held, the text alone is checked: no folder it names is looked for.
        let checked = if self.work_held {
            gitops::push_address_text(&prompt.input, &prompt.path)
        } else {
            gitops::push_address(&prompt.input, &prompt.path)
        };
        let url = match checked {
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
        // Held, the dialog closes and the ticks stay.
        if self.held() {
            return;
        }
        self.marked.clear();
        self.submit(Queued::Service(prompt.repo, prompt.path, url));
    }

    /// What a push to another service takes over or does (`goes`) and what
    /// it leaves as it is or does not do, as `gitops::migrate_to` works.
    /// The fourth names the remote the address is kept as once the service
    /// dialog has an address a push can use, and says when a remote of
    /// that name this checkout has already is pointed at it. Told by the
    /// text alone (`gitops::push_address_text`): it reads nothing, so it
    /// can be asked on every keystroke. Whether a folder named is there is
    /// looked at by Push (`accept_service`) and by the push itself.
    pub fn service_facts(&self) -> Vec<Fact> {
        let url = self
            .service_prompt
            .as_ref()
            .and_then(|prompt| gitops::push_address_text(&prompt.input, &prompt.path).ok());
        let kept = url.as_ref().map_or_else(
            || "This checkout keeps its files and origin, and keeps the address as one more remote, named after its host.".to_string(),
            |url| {
                let name = gitops::remote_name_for(url);
                let repointed = self
                    .service_remotes
                    .iter()
                    .any(|(known, address)| *known == name && address.trim() != url);
                let mut words = format!(
                    "This checkout keeps its files and origin, and keeps the address as the remote {name}."
                );
                if repointed {
                    words.push_str(
                        " A remote of that name that this checkout already has is pointed at the new address.",
                    );
                }
                words
            },
        );
        let fact = |goes: bool, words: &str| Fact {
            goes,
            words: words.to_string(),
        };
        vec![
            fact(
                true,
                "Every branch and tag goes over; a branch goes as origin has it where the one here is behind it or missing, otherwise as it is here.",
            ),
            fact(
                true,
                "Origin, if there is one, is fetched first; if it cannot be reached, what is saved here goes.",
            ),
            fact(
                true,
                "Every GitHub pull request, open, merged or closed, goes over as a branch pull/<number> (or under another name if pull is taken here), replaced on every push.",
            ),
            fact(true, &kept),
            fact(
                false,
                "Apart from those pull-request branches, nothing there is overwritten or deleted: a branch there only moves forward, so no commit there is lost.",
            ),
            fact(
                false,
                "A branch there that has commits the pushed branch lacks, or a tag there that differs from the pushed one, stops the push part-way with an error, before the address is kept.",
            ),
            fact(
                false,
                "No pull request is opened there, and no issue, comment or release goes over.",
            ),
        ]
    }

    /// Where a token for GitHub comes from now: nowhere while GitHub is
    /// held (`testing::hold_github`), so a test never asks `gh` for one.
    fn token_getter(&self) -> fn() -> Option<String> {
        if self.github_held {
            || None
        } else {
            self.token_source
        }
    }

    /// Pushes every branch, tag and pull request of one checkout to
    /// another service, in the progress dialog, after fetching what origin
    /// has now. The checkout keeps its files and its origin.
    fn push_to_service(&mut self, name: String, path: PathBuf, url: String) {
        let host = gitops::address_host(&url).unwrap_or_else(|| url.clone());
        let job = gitprogress::job(format!("Pushing {name} to {host}"), vec![name.clone()], 0);
        let token = self.token_getter();
        self.push_running = true;
        self.start_job(
            format!("Pushing {name} to {host}…"),
            Some(job),
            move |tx| {
                send(tx, JobEvent::Start(0));
                let token = token();
                let done = gitops::migrate_to(&path, &url, token.as_deref(), |line, progress| {
                    send_line(tx, line, progress)
                })?;
                // The fetch before the push saved the pull requests too.
                if done.stale.is_none() {
                    let _ = tx.send(Msg::PullsSaved(path.clone()));
                }
                Ok(service_summary(&name, &host, &done))
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
            // What was left on the disk that should not have been.
            let mut left: Vec<String> = Vec::new();
            for (index, (name, from)) in repos.iter().enumerate() {
                if process::was_cancelled() {
                    return Err(cancelled_leaving(&left));
                }
                send(tx, JobEvent::Start(index));
                let Some(to) = move_target(&dir, from) else {
                    failed.push(name.clone());
                    send(tx, JobEvent::Failed(index, "has no folder name".into()));
                    continue;
                };
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
                        let note = match warning {
                            Some(warning) => {
                                send_line(tx, &warning, false);
                                left.push(warning);
                                "moved, the old folder stayed"
                            }
                            None => "moved",
                        };
                        send(tx, JobEvent::Done(index, note.into()));
                    }
                    Err(err) => {
                        if let Some((_, rest)) = err.split_once(". The partial copy at ") {
                            left.push(format!("The partial copy at {rest}"));
                        }
                        failed.push(name.clone());
                        send(tx, JobEvent::Failed(index, err));
                    }
                }
            }
            if process::was_cancelled() {
                return Err(cancelled_leaving(&left));
            }
            moved_words(&named, &dir, (moved, stayed), &failed, &left)
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
        if self.held() {
            return;
        }
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
        if self.held() {
            return;
        }
        let paths: Vec<String> = self
            .chosen_repos()
            .into_iter()
            .map(|repo| repo.path.display().to_string())
            .collect();
        if paths.is_empty() {
            self.notice("Select a repository first.", true);
            return;
        }
        match crate::sys::clipboard::set_text(&paths.join("\n")) {
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

    /// Up or Down in the group dialog: walks through the groups that exist.
    pub fn step_group(&mut self, down: bool) {
        let groups = self.groups();
        let Some(prompt) = self.group_prompt.as_mut() else {
            return;
        };
        if groups.is_empty() {
            return;
        }
        let at = groups
            .iter()
            .position(|group| group.eq_ignore_ascii_case(prompt.input.trim()));
        let last = groups.len() - 1;
        let next = match (at, down) {
            (Some(index), true) if index < last => index + 1,
            (Some(index), false) if index > 0 => index - 1,
            (_, true) => 0,
            (_, false) => last,
        };
        prompt.input = groups[next].clone();
        prompt.error = None;
    }

    /// The group dialog's name becomes `name`, as typed or picked; what
    /// went wrong with the last one is forgotten. Nothing while the dialog
    /// is closed. The name is tidied and checked by `accept_group`.
    pub fn set_group_name(&mut self, name: &str) {
        if let Some(prompt) = self.group_prompt.as_mut()
            && prompt.input != name
        {
            name.clone_into(&mut prompt.input);
            prompt.error = None;
        }
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

    /// Starts filtering the installed list, with an empty filter.
    pub fn open_filter(&mut self) {
        self.mode = Mode::Filter;
        self.list_filter.clear();
    }

    /// Ends filtering the installed list and shows all of it again.
    pub fn close_filter(&mut self) {
        self.mode = Mode::List;
        self.list_filter.clear();
        self.list_scroll = 0;
    }

    /// The filter of the installed list was typed into.
    pub fn note_list_filter_changed(&mut self) {
        self.list_scroll = 0;
        self.snap_visible_selection();
    }

    /// Sets the filter of the installed list to `text` in one step, without
    /// opening the filter mode, and does what typing into it does. The same
    /// text again does nothing.
    pub fn set_list_filter(&mut self, text: &str) {
        if self.list_filter == text {
            return;
        }
        self.list_filter = text.to_string();
        self.note_list_filter_changed();
    }

    /// The folder dialog's folder becomes `text`, whole; what went wrong
    /// with the last one and what Tab found are forgotten. Nothing while
    /// the dialog is closed or when the text is the same.
    pub fn set_path_input(&mut self, text: &str) {
        if let Some(prompt) = self.prompt.as_mut() {
            prompt.set_input(text);
        }
    }

    pub fn open_import(&mut self) {
        self.prompt = Some(Purpose::Import.prompt(&self.clone_dir));
        self.mode = Mode::Path;
    }

    /// The Clone or Import button of the folder dialog. Held, only the
    /// text is checked: no folder is looked at, and the held notice says
    /// that nothing was done.
    fn accept_path(&mut self) {
        let held = self.work_held;
        let Some(prompt) = self.prompt.as_mut() else {
            return;
        };
        let checked = if held {
            pathprompt::resolve_text(&prompt.input)
        } else {
            prompt.folder()
        };
        match checked {
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
        self.answer_path(prompt.purpose, folder);
    }

    /// Does what the folder question asked for with `folder`, a folder
    /// that passed its checks, and goes back to the list.
    fn answer_path(&mut self, purpose: Purpose, folder: PathBuf) {
        self.mode = Mode::List;
        // Held, the dialog closes and the ticks and the clone folder stay.
        // Nothing is made on disk either: that happens when the task runs.
        if self.held() {
            return;
        }
        match purpose {
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

    /// The folders that fit what the open folder question has typed, as
    /// `PathPrompt::suggestions` finds them; empty while none is open. It
    /// reads the disk on every call: ask again only when the text changed.
    pub fn path_suggestions(&self) -> Vec<String> {
        self.prompt
            .as_ref()
            .map(PathPrompt::suggestions)
            .unwrap_or_default()
    }

    /// Each checkout of the open move question, with where a move into the
    /// typed folder puts it and whether that move will be refused, as the
    /// move itself works that out (`move_checkouts`, `gitops::move_checkout`).
    /// Empty while no move question is open. It looks at the disk on every
    /// call: ask again only when the text changed.
    ///
    /// While work is held (`hold_work`) it reads nothing: where each goes is
    /// told by the text alone, and only what the disk would tell is left
    /// out (a file at the typed folder, something at a checkout's target).
    pub fn move_lines(&self) -> Vec<MoveLine> {
        let Some(prompt) = &self.prompt else {
            return Vec::new();
        };
        let Purpose::Move(repos) = &prompt.purpose else {
            return Vec::new();
        };
        let held = self.work_held;
        let dir = if held {
            pathprompt::resolve_text(&prompt.input).ok()
        } else {
            prompt.folder().ok()
        };
        let mut lines: Vec<MoveLine> = Vec::with_capacity(repos.len());
        for (name, from) in repos {
            let to = dir.as_deref().and_then(|dir| move_target(dir, from));
            let there = to.as_deref().is_some_and(|to| paths::same_path(from, to));
            let taken = !there
                && to.as_deref().is_some_and(|to| {
                    // The moves run in this order: an earlier one that
                    // moves takes the place first.
                    let earlier = lines.iter().any(|line| {
                        !line.there
                            && !line.taken
                            && line
                                .to
                                .as_deref()
                                .is_some_and(|other| paths::same_path(other, to))
                    });
                    (!held && to.exists()) || earlier || paths::is_within(to, from)
                });
            lines.push(MoveLine {
                name: name.clone(),
                from: from.clone(),
                to,
                there,
                taken,
            });
        }
        lines
    }

    /// Whether the open move question would take a checkout onto another
    /// volume, where a move copies everything and then deletes the old
    /// folder. True when one is known to; None while no move question is
    /// open, its folder is not usable, or a checkout's volume cannot be
    /// told. Checkouts that do not move (`there` or `taken`) do not count.
    /// While work is held it reads nothing: the drives or roots the paths
    /// name are compared (`gitops::volume_root`).
    pub fn move_crosses_volumes(&self) -> Option<bool> {
        let lines = self.move_lines();
        if lines.is_empty() {
            return None;
        }
        let same = |from: &Path, to: &Path| {
            if self.work_held {
                let root = |path: &Path| gitops::volume_root(path).map(|root| root.to_lowercase());
                Some(root(from)? == root(to)?)
            } else {
                gitops::same_volume(from, to)
            }
        };
        let mut known = true;
        for line in lines.iter().filter(|line| !line.there && !line.taken) {
            match line.to.as_deref().and_then(|to| same(&line.from, to)) {
                Some(false) => return Some(true),
                Some(true) => {}
                None => known = false,
            }
        }
        known.then_some(false)
    }

    /// Why the open folder question would refuse its text, as `Confirm`
    /// would say it, without submitting it: None while nothing is typed,
    /// while the folder is usable, or while no question is open. While
    /// work is held it reads nothing: only what the text tells. It looks
    /// at the disk otherwise: ask again only when the text changed.
    pub fn path_reason(&self) -> Option<String> {
        let prompt = self.prompt.as_ref()?;
        if prompt.input.trim().is_empty() {
            return None;
        }
        let checked = if self.work_held {
            pathprompt::resolve_text(&prompt.input)
        } else {
            prompt.folder()
        };
        checked.err()
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
        // Held, what changed stays in memory; nobody asked for this write.
        if self.work_held {
            return;
        }
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

    /// How many output lines of the shown repository the front end draws,
    /// so scrolling back stops at the first line. Does nothing without a
    /// run.
    pub fn set_job_view(&mut self, lines: usize) {
        if let Some(job) = self.job.as_mut() {
            job.view = lines;
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

    /// Shows the repository at this place in the progress dialog.
    pub fn pick_job_at(&mut self, index: usize) {
        if let Some(job) = self.job.as_mut() {
            self.job_close_at = None;
            job.pick_index(index);
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
            Action::Cancel if matches!(self.mode, Mode::ConfirmRemove { .. }) => {
                self.cancel_remove();
            }
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

    /// The keys of the list that is on screen: Add's ids, or the
    /// overview's rows.
    fn current_names(&self) -> Vec<String> {
        if self.mode == Mode::Add {
            self.add_list().into_iter().map(|repo| repo.id()).collect()
        } else {
            self.list_names()
        }
    }

    /// The keys of the overview's rows, whatever is open over it.
    fn list_names(&self) -> Vec<String> {
        self.rows().iter().map(Row::key).collect()
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
            self.selected = self.list_names().into_iter().next();
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
        let names = self.list_names();
        list::snap(&mut self.selected, &names);
    }

    /// Sets Add's search to `text` in one step and does what typing into
    /// it does. The same text again does nothing, and so does a closed Add.
    pub fn set_add_filter(&mut self, text: &str) {
        if self.mode != Mode::Add || self.add_filter == text {
            return;
        }
        text.clone_into(&mut self.add_filter);
        self.note_add_filter_changed();
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
        if self.work_held {
            return;
        }
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
        if self.work_held {
            return;
        }
        let key = owner.to_ascii_lowercase();
        self.lookup_running = Some(key.clone());
        self.worker.spawn(move |tx| {
            let result = github::fetch_account(&owner);
            let _ = tx.send(Msg::Account { key, result });
        });
    }

    fn retry_add(&mut self) {
        if self.held() {
            return;
        }
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
        if self.held() {
            return;
        }
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
        if self.held() {
            return;
        }
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
        if self.held() {
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
        if self.held() {
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
        if self.held() {
            return;
        }
        self.marked.clear();
        self.submit(Queued::Forks(folders));
    }

    /// Brings the forks of every repository up to date that has them
    /// cloned beside it, one repository after another in the progress
    /// dialog: "Update the forks of all repositories". The selection and
    /// the ticks play no part.
    pub fn update_all_forks(&mut self) {
        // Held (a demo), no folder is looked for: the forks are cloned
        // where the story gave some (`hold_forks`).
        let held = self.work_held;
        let folders: Vec<String> = self
            .installed
            .iter()
            .filter(|repo| {
                if held {
                    self.held_forks.contains_key(&repo.folder)
                } else {
                    gitops::forks_cloned(&repo.path)
                }
            })
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
        if self.held() {
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
        self.start_fork_job(label, job, move |tx| {
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

    /// How many repositories are watched, for the automatic pull.
    pub fn watched_count(&self) -> usize {
        self.watched.len()
    }

    /// For a repository the running job has an item for: how far it is, from
    /// 0 to 1, and its state in a few words, as the progress dialog shows
    /// them. None when no job is running (a finished one is not) or the job
    /// has no item for the repository.
    pub fn job_share_of(&self, folder: &str) -> Option<(f32, String)> {
        let job = self.job.as_ref().filter(|job| job.running())?;
        let item = job.items.iter().find(|item| item.name == folder)?;
        let share = item.bar_fill().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
        Some((share, item.short_state()))
    }

    fn watch_every(&self) -> Duration {
        Duration::from_secs(u64::from(self.watch_interval.max(1)) * 60)
    }

    /// Starts or stops the automatic pull for the chosen repositories.
    /// When all of them are watched already it stops; otherwise the rest
    /// join, and everything watched is pulled right away.
    pub fn toggle_watch(&mut self) {
        if self.held() {
            return;
        }
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
            || self.work_held
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

    /// Says that repositories must be picked before anything is removed,
    /// and changes nothing else. The window calls it for `d` on a group's
    /// header with nothing ticked, where [`Repos::begin_remove`] would
    /// dissolve the group without asking.
    pub fn ask_for_repositories_to_remove(&mut self) {
        self.notice("Pick repositories to remove.", true);
    }

    /// The folders the remove dialog asks about, in its order. Empty when
    /// the dialog is not open.
    pub fn remove_lines(&self) -> Vec<RemoveLine> {
        let Mode::ConfirmRemove { folders } = &self.mode else {
            return Vec::new();
        };
        folders
            .iter()
            .map(|folder| {
                let repo = self.installed.iter().find(|repo| &repo.folder == folder);
                let state = match repo {
                    None => RemoveState::Gone,
                    Some(repo) if repo.dirty => RemoveState::Dirty,
                    Some(repo) if !repo.ready || repo.error.is_some() => RemoveState::Unread,
                    Some(_) => RemoveState::Clean,
                };
                RemoveLine {
                    name: folder.clone(),
                    path: repo.map(|repo| repo.path.clone()).unwrap_or_default(),
                    state,
                }
            })
            .collect()
    }

    /// Closes the remove dialog and goes back to the list. Nothing is
    /// removed and the ticks stay.
    pub fn cancel_remove(&mut self) {
        if matches!(self.mode, Mode::ConfirmRemove { .. }) {
            self.mode = Mode::List;
        }
    }

    pub fn confirm_remove(&mut self) {
        let Mode::ConfirmRemove { folders } = self.mode.clone() else {
            return;
        };
        self.mode = Mode::List;
        // Held, the dialog closes and the ticks stay.
        if self.held() {
            return;
        }
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
        match self.add_choice() {
            Ok(AddChoice::Clone(remotes)) => {
                self.prompt = Some(Purpose::Clone(remotes).prompt(&self.clone_dir));
                self.mode = Mode::Path;
            }
            Ok(AddChoice::ListAccount) => self.list_typed_account(),
            Err(err) => self.notice(err, true),
        }
    }

    /// Enter in Add and then in the folder question, in one step, with
    /// `folder` as the question's text: the ticked repositories, or the
    /// highlighted one when none is ticked, are cloned into it. The
    /// question is never open.
    ///
    /// `Ok` when the clone was submitted: the ticks are gone, `clone_dir`
    /// is the folder (saved when it changed) and the mode is what the
    /// question's Enter leaves: the list, under the run's progress dialog
    /// when the clone starts at once. The folder is made when it runs.
    /// `Ok` too when nothing is listed and a plain word is typed: that
    /// account is listed, as Enter in Add does, and the mode stays Add.
    /// Held, it is `Ok` with the mode the list, `HELD` on the status line,
    /// the ticks and `clone_dir` kept, and nothing made or started; the
    /// folder is then checked by its text alone, not looked at.
    /// `Err` holds Add's words or the question's; nothing changed and the
    /// mode is still Add. Outside Add it is `Err("Add is not open.")` and
    /// changes nothing, whatever else is open.
    pub fn clone_marked_into(&mut self, folder: &str) -> Result<(), String> {
        if self.mode != Mode::Add {
            return Err("Add is not open.".into());
        }
        let remotes = match self.add_choice()? {
            AddChoice::Clone(remotes) => remotes,
            AddChoice::ListAccount => {
                self.list_typed_account();
                return Ok(());
            }
        };
        let mut prompt = Purpose::Clone(remotes).prompt(&self.clone_dir);
        prompt.set_input(folder);
        // Held, the text alone: no folder is looked at.
        let folder = if self.work_held {
            pathprompt::resolve_text(&prompt.input)?
        } else {
            prompt.folder()?
        };
        self.answer_path(prompt.purpose, folder);
        Ok(())
    }

    /// What Enter in Add asks for: the ticked repositories that are not on
    /// the list yet, or else the highlighted one. `Err` says why there is
    /// nothing to clone.
    fn add_choice(&self) -> Result<AddChoice, String> {
        if !self.add_marked.is_empty() {
            let remotes: Vec<RemoteRepo> = self
                .add_marked
                .iter()
                .filter(|remote| !self.is_installed(remote))
                .cloned()
                .collect();
            if remotes.is_empty() {
                return Err("Those repositories are on the list already.".into());
            }
            return Ok(AddChoice::Clone(remotes));
        }
        if self.add_list().is_empty() && self.add_word().is_some() {
            return Ok(AddChoice::ListAccount);
        }
        let missing = || "Select a repository to add.".to_string();
        let id = self.add_selected.as_ref().ok_or_else(missing)?;
        let remote = self
            .add_list()
            .into_iter()
            .find(|repo| repo.id().eq_ignore_ascii_case(id))
            .ok_or_else(missing)?;
        if self.is_installed(&remote) {
            return Err(format!("{} is on the list already.", remote.id()));
        }
        Ok(AddChoice::Clone(vec![remote]))
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
        if self.held() {
            return;
        }
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
        if self.work_held || self.busy || self.fetching || self.queue.is_empty() {
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
        // Held, a run on screen is the demo's: nothing runs to stop, and
        // the status line says so, as for every other held action.
        if self.work_held && self.job.as_ref().is_some_and(Job::running) {
            self.held();
            return;
        }
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
        self.fork_watch = None;
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

    /// Runs a fetch of forks as `start_job` runs a job, and marks the job
    /// as one, so that its lines are read for `fork_progress`.
    fn start_fork_job<F>(&mut self, label: String, job: Job, work: F)
    where
        F: FnOnce(&Sender<Msg>) -> Result<String, String> + Send + 'static,
    {
        self.start_job(label, Some(job), work);
        self.fork_watch = Some(ForkWatch::default());
    }

    /// How far the fetch of forks that runs now is: the task running now,
    /// once its lines have said how many forks there are. None while no
    /// job runs, for a job that is not a fetch of forks, before the total
    /// is known, and once the job has ended.
    pub fn fork_progress(&self) -> Option<ForkProgress> {
        self.running_fork_watch()?.progress()
    }

    /// The collection folder the fetch of forks that runs now fetches
    /// into, for the task running now, once its lines have said it.
    pub fn fork_job_into(&self) -> Option<PathBuf> {
        self.running_fork_watch()?.into().map(Path::to_path_buf)
    }

    /// What the lines of the fetch of forks said, while one runs.
    fn running_fork_watch(&self) -> Option<&ForkWatch> {
        self.job.as_ref().filter(|job| job.running())?;
        self.fork_watch.as_ref()
    }

    /// Gives a held session a fetch of forks that runs for `repo` (by row
    /// name) and has said `lines` (each with whether it is a progress
    /// line), and shows it in the progress dialog: story data for a demo.
    /// No worker runs it, so it never ends by itself; Cancel says the held
    /// notice and Hide hides it. A live session gets nothing here.
    pub fn hold_fork_job(&mut self, repo: &str, lines: &[(&str, bool)]) {
        if !self.work_held {
            return;
        }
        let title = format!("Updating the forks of {repo}");
        self.notice(format!("{title}…"), false);
        self.job = Some(gitprogress::job(title, vec![repo.to_string()], 0));
        self.fork_watch = Some(ForkWatch::default());
        self.job_close_at = None;
        self.dialog_hidden = false;
        if matches!(self.mode, Mode::List | Mode::Filter | Mode::Progress) {
            self.mode = Mode::Progress;
        }
        self.on_msg(Msg::Job(JobEvent::Start(0)));
        for (line, progress) in lines {
            self.on_msg(Msg::Job(JobEvent::Line((*line).to_string(), *progress)));
        }
    }

    pub fn reload_lists(&mut self, keep_status: bool, fetch: bool) {
        // Held, it only reads what is on disk.
        let fetch = fetch && !self.work_held;
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
            && !self.work_held
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
        if self.work_held || self.github_held {
            return;
        }
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
        if self.work_held || self.github_held {
            return;
        }
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
        // Held, every status read is a local one.
        let fetch = fetch && !self.work_held;
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
        let token = if fetch { self.token_getter()() } else { None };
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

    #[doc(hidden)]
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
                    // What an ended job hears is not heard here either.
                    if job.running()
                        && let Some(watch) = self.fork_watch.as_mut()
                    {
                        watch.on(&event);
                    }
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
                    match result {
                        Ok(list) => view.take(list),
                        Err(err) => {
                            view.loading = false;
                            view.error = Some(err);
                        }
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
                let push = std::mem::take(&mut self.push_running);
                let (words, error) = finished_words(result, cancelled, push);
                self.notice(words, error);
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

/// How a move of `named` into `dir` ended: `moved` and `stayed` count
/// the checkouts moved and those that were there already. With checkouts
/// that `failed`, or with something `left` on the disk that should not be
/// there (an old folder that could not be deleted, a partial copy that
/// could not be removed), it is an error, which the progress dialog keeps
/// open and the status line shows whole.
fn moved_words(
    named: &str,
    dir: &Path,
    (moved, stayed): (usize, usize),
    failed: &[String],
    left: &[String],
) -> Result<String, String> {
    let mut words = if failed.is_empty() {
        match (moved, stayed) {
            (0, _) => format!("{named} is in {} already.", dir.display()),
            (_, 0) => format!("Moved {named} to {}.", dir.display()),
            _ => format!(
                "Moved {moved} to {}. {stayed} already there.",
                dir.display()
            ),
        }
    } else {
        format!(
            "Moved {moved}, {} failed: {}",
            failed.len(),
            failed.join(", ")
        )
    };
    for sentence in left {
        if !words.ends_with('.') {
            words.push('.');
        }
        words.push(' ');
        words.push_str(sentence);
    }
    if failed.is_empty() && left.is_empty() {
        Ok(words)
    } else {
        Err(words)
    }
}

/// The error of a run that was cancelled with something `left` on the
/// disk: the status line says "Cancelled." and then what was left.
fn cancelled_leaving(left: &[String]) -> String {
    if left.is_empty() {
        "cancelled".to_string()
    } else {
        format!("{CANCELLED_LEAVING}{}", left.join(" "))
    }
}

/// How the error of a cancelled run starts that has more to say.
const CANCELLED_LEAVING: &str = "cancelled. ";

/// What the status line says when a run ends with `result`: "Cancelled."
/// when it was `cancelled`, and for a `push` to another service also what
/// stays there; else its message, or its error (true).
fn finished_words(result: Result<String, String>, cancelled: bool, push: bool) -> (String, bool) {
    match result {
        Err(_) if cancelled && push => (PUSH_CANCELLED.to_string(), false),
        Err(err) if cancelled => match err.strip_prefix(CANCELLED_LEAVING) {
            Some(left) => (format!("Cancelled. {left}"), true),
            None => ("Cancelled.".to_string(), false),
        },
        Ok(message) => (message, false),
        Err(err) => (err, true),
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
    send_line(tx, &forklines::total(listed.forks.len(), &dest), false);
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
        #[cfg(feature = "testing")]
        crate::sys::note(crate::sys::Reach::Disk);
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

/// The line that says what a push to another service took over.
fn service_summary(name: &str, host: &str, done: &gitops::Migrated) -> String {
    let count = |count: u32, one: &str, many: &str| match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    };
    let pulls = match done.pulls {
        0 => "no pull requests".to_string(),
        pulls => format!(
            "{} as branches {}/<number>",
            count(pulls, "pull request", "pull requests"),
            done.pull_prefix
        ),
    };
    let mut text = format!(
        "Pushed {name} to {host}: {}, {} and {pulls}. The address is kept as the remote {}.",
        count(done.branches, "branch", "branches"),
        count(done.tags, "tag", "tags"),
        done.remote
    );
    if let Some(why) = &done.stale {
        text.push_str(&format!(
            " Origin could not be fetched first ({why}), so this is what was saved here."
        ));
    }
    text
}

fn count_label(count: usize) -> String {
    if count == 1 {
        "1 repository".to_string()
    } else {
        format!("{count} repositories")
    }
}

#[cfg(any(test, feature = "testing"))]
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

    /// Keeps `repos` from asking GitHub (or `gh` for a token) for its
    /// catalog and its pull request counts from now on, as a reload after
    /// an import would; local work (reading, importing, saving) runs as
    /// always. For tests whose checkouts' origins name GitHub.
    pub fn hold_github(repos: &mut Repos) {
        repos.github_held = true;
    }

    /// Holds all work in `repos` and gives it a fetch of forks for `repo`
    /// that runs, without a worker, and has said `lines`: for the window's
    /// tests of the fetch dialog (`Repos::hold_fork_job`).
    pub fn fork_job(repos: &mut Repos, repo: &str, lines: &[(&str, bool)]) {
        repos.hold_work();
        repos.hold_fork_job(repo, lines);
    }

    /// Makes `source` where `repos` gets a token for GitHub from, in place
    /// of `gh` and the environment: for a test that counts the asks.
    pub fn token_source(repos: &mut Repos, source: fn() -> Option<String>) {
        repos.token_source = source;
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

    #[test]
    fn a_reload_held_from_github_asks_github_nothing() {
        let mut app = grouped_app();
        app.remotes_state = RemoteState::Ready;
        hold_github(&mut app);
        let (remotes, pulls) = (app.remote_generation, app.pulls_generation);
        // What a reload after an import asks of GitHub: its catalog, and the pull request counts of the rows.
        app.reload_github_only();
        app.reload_pull_counts();
        assert_eq!(
            (app.remote_generation, app.pulls_generation),
            (remotes, pulls),
            "GitHub was asked"
        );
        assert!(app.remotes_state == RemoteState::Ready);
        assert!(!app.work_held(), "local work is not held");
    }

    #[test]
    fn the_line_after_a_push_to_a_service_counts_the_pull_requests() {
        let mut done = gitops::Migrated {
            remote: "gitlab".into(),
            branches: 3,
            tags: 1,
            pulls: 19,
            pull_prefix: "pull".into(),
            stale: None,
        };
        assert_eq!(
            service_summary("api", "gitlab.com", &done),
            "Pushed api to gitlab.com: 3 branches, 1 tag and 19 pull requests as branches pull/<number>. The address is kept as the remote gitlab."
        );
        done.pulls = 0;
        done.stale = Some("could not reach github.com".into());
        let text = service_summary("api", "gitlab.com", &done);
        assert!(text.contains("1 tag and no pull requests."), "{text}");
        assert!(
            text.ends_with("(could not reach github.com), so this is what was saved here."),
            "{text}"
        );
    }

    fn scratch(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("devassist-app-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn the_plain_add_status_names_no_tab() {
        let mut app = Repos::empty(test_settings());
        app.remotes_state = RemoteState::Ready;
        app.remotes = vec![remote("storytold", "alpha"), remote("storytold", "beta")];
        app.mode = Mode::Add;
        // The terminal app's words are as they were.
        assert_eq!(
            app.add_status().text,
            "2 repositories · 0 cloned · type owner/ or paste a link to list any account"
        );
        assert_eq!(app.add_status_plain().text, app.add_status().text);
        app.add_filter = "someone".into();
        assert_eq!(
            app.add_status().text,
            "2 repositories · 0 cloned · tab lists github.com/someone"
        );
        assert_eq!(app.add_status_plain().text, "2 repositories · 0 cloned");
        app.owners.clear();
        assert_eq!(
            app.add_status().text,
            "No default account · tab lists github.com/someone"
        );
        assert_eq!(app.add_status_plain().text, "No default account");
        app.add_filter.clear();
        assert_eq!(
            app.add_status_plain().text,
            "No default account · type owner/ or paste a link to list any account"
        );
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

    /// `cargo test --workspace -- --ignored live_forks`
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

    #[test]
    fn a_reading_start_fetches_nothing_pulls_nothing_and_writes_nothing() {
        let root = scratch("reading");
        let origin = root.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        std::fs::write(origin.join("one"), "one").unwrap();
        git(&origin, &["add", "one"]);
        git(&origin, &["commit", "-q", "-m", "one"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), "tool"]);
        let clone = root.join("tool");
        // The origin moves on after the clone: a fetch or a pull would show.
        std::fs::write(origin.join("two"), "two").unwrap();
        git(&origin, &["add", "two"]);
        git(&origin, &["commit", "-q", "-m", "two"]);
        let tip = |dir: &Path, rev: &str| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", rev])
                .output()
                .expect("git runs");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let known = tip(&clone, "origin/main");

        let settings = Settings {
            checkouts: vec![clone.clone()],
            watched: vec![clone.clone()],
            owners: Vec::new(),
            // Checkouts taken over at startup would be written by a normal start.
            unsaved: true,
            first_run: true,
            config_path: root.join("devassist.toml"),
            ..test_settings()
        };
        let mut app = Repos::reading(settings);
        assert!(app.form.is_none(), "no settings dialog");
        let until = Instant::now() + Duration::from_secs(60);
        while app.waiting() {
            assert!(!app.fetching, "nothing is fetched");
            assert!(Instant::now() < until, "the read did not finish");
            app.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
        // Idle now: a normal start would fetch, then pull the watched one.
        for _ in 0..20 {
            app.drain();
            assert!(
                !app.busy && !app.fetching,
                "nothing starts once the read is done"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(app.installed.len(), 1);
        assert_eq!(app.installed[0].branch, "main");
        assert_eq!(app.watched_count(), 1);
        assert_eq!(tip(&clone, "origin/main"), known, "not fetched");
        assert!(!clone.join("two").exists(), "not pulled");
        assert!(
            !root.join("devassist.toml").exists(),
            "the settings file is not written"
        );
        app.quit();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_row_is_picked_by_its_key_and_an_unknown_key_changes_nothing() {
        let mut app = grouped_app();
        app.pick("docs");
        assert_eq!(app.selected.as_deref(), Some("docs"));
        app.pick("nope");
        assert_eq!(app.selected.as_deref(), Some("docs"));
        // A repository the filter hides is not a row, so it is not picked.
        app.list_filter = "web".into();
        app.pick("api");
        assert_eq!(app.selected.as_deref(), Some("docs"));
        app.pick("web");
        assert_eq!(app.selected.as_deref(), Some("web"));
    }

    #[test]
    fn a_groups_header_is_picked_and_a_member_of_a_folded_group_is_not() {
        let mut app = grouped_app();
        app.placement = vec![
            (PathBuf::from("/srv/code/api"), "Work".into()),
            (PathBuf::from("/srv/code/web"), "Work".into()),
        ];
        app.assign_groups();
        app.pick("group::Work");
        assert_eq!(app.selected.as_deref(), Some("group::Work"));
        app.pick("web");
        assert_eq!(app.selected.as_deref(), Some("web"));
        // Folded, the group hides its members: they are no rows to pick.
        app.collapsed = vec!["Work".into()];
        app.pick("group::Work");
        app.pick("api");
        assert_eq!(app.selected.as_deref(), Some("group::Work"));
        app.pick("docs");
        assert_eq!(app.selected.as_deref(), Some("docs"));
    }

    #[test]
    fn a_repository_is_ticked_without_moving_the_highlight() {
        let mut app = grouped_app();
        app.toggle_mark_of("docs");
        assert_eq!(app.selected.as_deref(), Some("api"));
        assert_eq!(app.marked, ["docs"]);
        app.toggle_mark_of("docs");
        assert_eq!(app.selected.as_deref(), Some("api"));
        assert!(app.marked.is_empty());
        // What is not a row is not ticked.
        app.toggle_mark_of("nope");
        assert!(app.marked.is_empty());
    }

    #[test]
    fn ticking_a_group_ticks_its_repositories() {
        let mut app = grouped_app();
        app.installed[0].group = "work".into();
        app.installed[1].group = "work".into();
        app.toggle_mark_of(&group_key("work"));
        assert_eq!(app.marked, ["api", "docs"]);
        assert_eq!(app.selected.as_deref(), Some("api"));
        app.toggle_mark_of(&group_key("work"));
        assert!(app.marked.is_empty());
    }

    #[test]
    fn clearing_the_ticks_unticks_everything() {
        let mut app = grouped_app();
        app.toggle_mark_of("api");
        app.toggle_mark_of("web");
        assert_eq!(app.marked.len(), 2);
        app.clear_marks();
        assert!(app.marked.is_empty());
    }

    #[test]
    fn the_filter_is_set_in_one_step_and_the_highlight_stays_on_a_shown_row() {
        let mut app = Repos::empty(test_settings());
        for name in ["api", "portal", "poll"] {
            app.installed.push(installed("acme", name));
        }
        app.selected = Some("api".into());
        app.set_list_filter("po");
        assert!(app.mode == Mode::List, "no filter mode is opened");
        assert_eq!(app.list_filter, "po");
        let keys: Vec<String> = app.rows().iter().map(Row::key).collect();
        assert_eq!(keys, ["portal", "poll"]);
        assert!(keys.contains(app.selected.as_ref().unwrap()));
        // The same text again does nothing, not even the scroll.
        app.list_scroll = 3;
        app.set_list_filter("po");
        assert_eq!(app.list_scroll, 3);
        app.set_list_filter("");
        assert_eq!(app.rows().len(), 3);
    }

    #[test]
    fn a_running_job_says_how_far_each_repository_is() {
        let mut app = grouped_app();
        assert_eq!(app.job_share_of("api"), None, "no job");
        app.job = Some(Job::new("Updating", vec!["api".into(), "docs".into()], 0));
        let job = app.job.as_mut().unwrap();
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Steps(3, 4));
        let (share, text) = app.job_share_of("api").unwrap();
        assert!((share - 0.75).abs() < 1e-6);
        assert_eq!(text, "3/4");
        // Waiting is at the start; a repository outside the job has nothing.
        let (share, text) = app.job_share_of("docs").unwrap();
        assert!(share.abs() < 1e-6);
        assert_eq!(text, "waiting");
        assert_eq!(app.job_share_of("web"), None);
        // A finished run is not busy any more.
        app.job.as_mut().unwrap().finish(Ok("done".into()), false);
        assert_eq!(app.job_share_of("api"), None);
    }

    #[test]
    fn the_overview_keeps_its_own_list_while_add_is_open() {
        let mut app = Repos::empty(test_settings());
        for name in ["api", "web"] {
            app.installed.push(installed("acme", name));
        }
        // Add lists ids of its own: "acme/web" is no row of the overview.
        app.remotes = vec![remote("acme", "web"), remote("acme", "api")];
        app.remotes_state = RemoteState::Ready;
        app.selected = Some("api".into());
        app.open_add();
        assert!(app.mode == Mode::Add);
        app.pick("web");
        assert_eq!(app.selected.as_deref(), Some("web"));
        app.toggle_mark_of("api");
        assert!(app.is_marked_local("api"));
        app.pick("api");
        app.set_list_filter("we");
        assert_eq!(
            app.selected.as_deref(),
            Some("web"),
            "a row key, never an id"
        );
        // Add's own highlight is Add's.
        app.pick("acme/web");
        assert_eq!(app.selected.as_deref(), Some("web"));
    }

    /// A temporary folder that stands for a checkout, and a settings file
    /// beside it that must not be written.
    fn held_app(name: &str) -> (Repos, PathBuf) {
        let root = scratch(name);
        let path = root.join("tool");
        std::fs::create_dir_all(&path).unwrap();
        let settings = Settings {
            config_path: root.join("devassist.toml"),
            ..test_settings()
        };
        let mut app = Repos::empty(settings);
        let mut repo = installed("acme", "tool");
        repo.path = path.clone();
        app.checkouts.push(path.clone());
        app.installed.push(repo);
        app.watched = vec![path.clone()];
        app.watch_due = Instant::now();
        app.selected = Some("tool".into());
        app.hold_work();
        (app, root)
    }

    #[test]
    fn asking_for_repositories_to_remove_says_so_and_changes_nothing_else() {
        let mut app = grouped_app();
        app.pick("api");
        app.status = "Sorted by name.".into();
        let (mode, marked, selected) = (app.mode.clone(), app.marked.clone(), app.selected.clone());
        app.ask_for_repositories_to_remove();
        assert_eq!(app.status, "Pick repositories to remove.");
        assert!(app.status_error);
        assert!(app.mode == mode);
        assert_eq!(
            (app.marked.clone(), app.selected.clone()),
            (marked, selected)
        );
    }

    #[test]
    fn remove_lines_name_each_folder_and_what_is_known_of_its_changes() {
        let mut app = grouped_app();
        app.installed[1].dirty = true;
        // Its state is not read yet: nobody knows whether it is clean.
        app.installed[3].ready = false;
        assert!(app.remove_lines().is_empty(), "no dialog, no lines");
        app.marked = vec!["api".into(), "docs".into(), "web".into()];
        app.begin_remove();
        let lines = app.remove_lines();
        let line = |name: &str, state: RemoveState| RemoveLine {
            name: name.into(),
            path: PathBuf::from("/srv/code").join(name),
            state,
        };
        assert_eq!(app.installed[3].folder, "web");
        assert_eq!(
            lines,
            [
                line("api", RemoveState::Clean),
                line("docs", RemoveState::Dirty),
                line("web", RemoveState::Unread)
            ]
        );
        assert_eq!(
            lines.iter().map(RemoveLine::dirty).collect::<Vec<_>>(),
            [false, true, false],
            "only what has changes counts as dirty"
        );
        // Read, but the read failed: not known either.
        app.installed[3].ready = true;
        app.installed[3].error = Some("could not read".into());
        assert_eq!(app.remove_lines()[2].state, RemoveState::Unread);
        app.installed[3].error = None;
        assert_eq!(app.remove_lines()[2].state, RemoveState::Clean);
        // A folder that left the list meanwhile has no path and says so.
        app.mode = Mode::ConfirmRemove {
            folders: vec!["gone".into()],
        };
        assert_eq!(
            app.remove_lines(),
            [RemoveLine {
                name: "gone".into(),
                path: PathBuf::new(),
                state: RemoveState::Gone,
            }]
        );
        assert!(!app.remove_lines()[0].dirty());
        // Held, it reads the same.
        app.hold_work();
        app.mode = Mode::ConfirmRemove {
            folders: vec!["docs".into()],
        };
        assert_eq!(app.remove_lines(), [line("docs", RemoveState::Dirty)]);
        app.mode = Mode::List;
        assert!(app.remove_lines().is_empty());
    }

    #[test]
    fn cancelling_a_removal_goes_back_to_the_list_and_removes_nothing() {
        for held in [false, true] {
            let (mut app, root) = held_app(if held { "cancel-held" } else { "cancel" });
            if !held {
                app.work_held = false;
            }
            app.marked = vec!["tool".into()];
            app.begin_remove();
            assert!(matches!(app.mode, Mode::ConfirmRemove { .. }));
            app.notice("before", false);
            app.cancel_remove();
            assert!(app.mode == Mode::List, "held {held}");
            assert_eq!(app.marked, ["tool"], "the tick stays");
            assert_eq!(app.installed.len(), 1, "still on the list");
            assert_eq!(app.checkouts.len(), 1);
            assert!(app.installed[0].path.exists(), "the folder is still there");
            assert!(app.job.is_none() && app.queue.is_empty() && !app.busy);
            assert_eq!(app.status, "before", "nothing to say");
            // Outside the dialog it does nothing.
            app.mode = Mode::Menu;
            app.cancel_remove();
            assert!(app.mode == Mode::Menu);
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn the_front_end_says_how_many_output_lines_it_shows() {
        let mut app = Repos::empty(test_settings());
        app.hold_work();
        // No run: nothing to set, and nothing breaks.
        app.set_job_view(7);
        assert!(app.job.is_none());
        let mut job = Job::new("Updating", vec!["api".into()], 0);
        job.on(JobEvent::Start(0));
        for index in 0..10 {
            job.on(JobEvent::Line(format!("line {index}"), false));
        }
        app.job = Some(job);
        app.set_job_view(4);
        assert_eq!(app.job.as_ref().unwrap().view, 4);
        // Scrolling stops where the shown lines reach the first one.
        app.job.as_mut().unwrap().scroll_by(100);
        assert_eq!(app.job.as_ref().unwrap().scroll, 6);
    }

    #[test]
    fn a_held_repos_that_asked_github_nothing_waits_for_nothing() {
        let mut app = Repos::empty(test_settings());
        assert!(app.waiting(), "a fresh one waits for the catalog");
        app.hold_work();
        assert!(!app.waiting(), "held, no catalog is coming");
        assert!(app.remotes_state == RemoteState::Ready);
        // A catalog read already asked for is left to land.
        let mut app = Repos::empty(test_settings());
        app.remote_generation = 1;
        app.hold_work();
        assert!(app.remotes_state == RemoteState::Loading);
    }

    #[test]
    fn space_in_add_moves_the_overviews_highlight_not_adds() {
        let mut app = Repos::empty(test_settings());
        for name in ["api", "web"] {
            app.installed.push(installed("acme", name));
        }
        app.remotes = vec![remote("acme", "one"), remote("acme", "two")];
        app.remotes_state = RemoteState::Ready;
        app.selected = Some("api".into());
        app.open_add();
        app.add_selected = Some("acme/one".into());
        app.toggle_list_mark();
        assert_eq!(app.marked, ["api"]);
        assert_eq!(app.selected.as_deref(), Some("web"));
        assert_eq!(app.add_selected.as_deref(), Some("acme/one"));
    }

    #[test]
    fn held_work_starts_nothing_and_says_so() {
        let (mut app, root) = held_app("held");
        assert!(app.work_held());
        assert!(!Repos::empty(test_settings()).work_held());
        let idle = |app: &Repos, what: &str| {
            assert!(!app.busy, "{what}: busy");
            assert!(app.queue.is_empty(), "{what}: queued");
            assert!(!app.fetching, "{what}: fetching");
            assert!(app.lookup_due.is_none() && app.lookup_running.is_none());
            assert!(!app.waiting(), "{what}: waiting");
        };
        /// What a person asks for, by name.
        type Call = (&'static str, fn(&mut Repos));
        let calls: Vec<Call> = vec![
            ("update_all", Repos::update_all),
            ("update_chosen", Repos::update_chosen),
            ("update_one", Repos::update_one),
            ("refresh", Repos::refresh),
            ("toggle_watch", Repos::toggle_watch),
            ("open_selected", Repos::open_selected),
            ("copy_paths", |app| app.run_menu(MenuItem::CopyPath)),
            ("open_on_github", |app| app.run_menu(MenuItem::Github)),
            ("pull requests", |app| app.run_menu(MenuItem::PullRequests)),
            ("forks", |app| app.run_menu(MenuItem::Forks)),
            ("remove", |app| {
                app.begin_remove();
                assert!(matches!(app.mode, Mode::ConfirmRemove { .. }));
                app.confirm_remove();
            }),
            ("import", |app| {
                app.open_import();
                let dir = app.clone_dir.clone();
                app.use_folder(dir);
            }),
            ("move", |app| {
                app.run_migrate(MigrateItem::Folder);
                assert!(app.mode == Mode::Path);
                app.use_folder(std::env::temp_dir());
            }),
            ("clone", |app| {
                app.prompt =
                    Some(Purpose::Clone(vec![remote("acme", "new")]).prompt(&app.clone_dir));
                app.use_folder(std::env::temp_dir().join("devassist-held-clone"));
            }),
            ("clone from add", |app| {
                app.mode = Mode::Add;
                let folder = std::env::temp_dir()
                    .join(format!("devassist-held-add-{}", std::process::id()))
                    .join("new");
                assert_eq!(app.clone_marked_into(&folder.display().to_string()), Ok(()));
                assert!(
                    !folder.exists() && !folder.parent().unwrap().exists(),
                    "no folder is made"
                );
            }),
            ("service", |app| {
                app.run_migrate(MigrateItem::Service);
                assert!(app.mode == Mode::Service);
                app.service_prompt.as_mut().unwrap().input =
                    "https://example.invalid/acme/tool.git".into();
                app.accept_service();
            }),
            ("save_settings", |app| {
                app.open_settings(false);
                app.save_settings();
                assert!(app.form.is_some(), "the dialog stays open");
                app.close_settings();
            }),
            ("run Refresh", |app| app.run(Action::Refresh)),
        ];
        let clone_dir = app.clone_dir.clone();
        for (what, call) in calls {
            app.notice("before", false);
            app.marked = vec!["tool".into()];
            app.add_marked = vec![remote("acme", "new")];
            call(&mut app);
            assert_eq!(app.status, HELD, "{what}");
            assert!(!app.status_error, "{what}");
            idle(&app, what);
            // A refused action leaves what a person sees as it was.
            assert_eq!(app.marked, ["tool"], "{what}: the tick is kept");
            assert_eq!(app.add_marked.len(), 1, "{what}: Add's tick is kept");
            assert_eq!(app.clone_dir, clone_dir, "{what}: the clone folder");
            assert!(app.mode == Mode::List, "{what}: back on the list");
            assert!(app.prompt.is_none() && app.service_prompt.is_none());
        }
        // The watch is as it was, and nothing pulls it, though it is due.
        assert_eq!(app.watched_count(), 1);
        app.notice("before", false);
        assert!(!app.drain());
        assert!(!app.start_due_watch());
        assert_eq!(app.status, "before", "the held watch says nothing");
        // Typing in Add asks GitHub nothing.
        app.open_add();
        app.add_filter = "someone/".into();
        app.note_add_filter_changed();
        // Retry is refused as every held action is, and says so.
        app.notice("before", false);
        app.retry_add();
        assert_eq!(app.status, HELD, "retry");
        assert!(!app.status_error, "retry");
        idle(&app, "add");
        assert!(app.installed[0].path.exists(), "the folder is still there");
        assert!(app.job.is_none());
        assert!(
            !root.join("devassist.toml").exists(),
            "the settings file is not written"
        );
        // A run shown while held (a demo's) is cancelled by nobody, and
        // its Cancel says so rather than nothing.
        let mut job = Job::new("Updating", vec!["tool".into()], 0);
        job.on(JobEvent::Start(0));
        app.job = Some(job);
        app.notice("before", false);
        app.cancel_work();
        assert_eq!(app.status, HELD, "cancel");
        assert!(
            !app.job.as_ref().unwrap().cancelling,
            "nothing was asked to stop"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_group_name_is_set_whole_and_forgets_the_last_error() {
        let (mut app, root) = held_app("group-name");
        app.set_group_name("Tools");
        assert!(
            app.group_prompt.is_none(),
            "nothing while the dialog is closed"
        );
        app.open_group_prompt();
        app.set_group_name(&"x".repeat(41));
        app.accept_group();
        assert!(app.mode == Mode::Group, "too long a name is refused");
        assert!(app.group_prompt.as_ref().unwrap().error.is_some());
        app.set_group_name("Tools");
        let prompt = app.group_prompt.as_ref().unwrap();
        assert_eq!(
            (prompt.input.as_str(), prompt.error.as_deref()),
            ("Tools", None)
        );
        app.accept_group();
        assert_eq!(app.installed[0].group, "Tools");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_menu_offers_to_update_the_forks_of_a_repository_that_has_them_cloned() {
        let (mut app, root) = held_app("menu-forks");
        app.enter_row();
        let menu = app.menu.as_ref().unwrap();
        assert_eq!(menu.label(MenuItem::Forks), "Clone the forks");
        let folder = menu.repos[0].clone();
        app.close_menu();

        // Its forks are cloned beside it: the same line now updates them.
        let path = app
            .installed
            .iter()
            .find(|repo| repo.folder == folder)
            .unwrap()
            .path
            .clone();
        let forks = gitops::forks_folder(&path).unwrap();
        std::fs::create_dir_all(forks.join(".git")).unwrap();
        // Held (a demo), the menu looks for no folder: the forks count as not cloned.
        app.enter_row();
        assert_eq!(
            app.menu.as_ref().unwrap().label(MenuItem::Forks),
            "Clone the forks"
        );
        app.close_menu();
        // Not held, it looks; opening the menu starts no work.
        app.work_held = false;
        app.enter_row();
        assert!(!app.busy && app.queue.is_empty());
        let menu = app.menu.as_ref().unwrap();
        assert_eq!(menu.label(MenuItem::Forks), "Update the forks");
        assert_eq!(
            menu.label(MenuItem::AllForks),
            "Update the forks of all repositories"
        );
        assert_eq!(menu.label(MenuItem::Open), "Open the folder");
        let _ = std::fs::remove_dir_all(&forks);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_menu_highlights_the_line_asked_for_and_nothing_while_it_is_closed() {
        let (mut app, root) = held_app("menu-highlight");
        app.highlight_menu(MenuItem::Remove);
        assert!(app.menu.is_none() && app.menu_highlight().is_none());
        app.enter_row();
        assert_eq!(
            app.menu_highlight(),
            Some(MenuItem::Update),
            "the first line"
        );
        app.highlight_menu(MenuItem::Remove);
        assert_eq!(app.menu_highlight(), Some(MenuItem::Remove));
        assert_eq!(app.menu.as_ref().unwrap().selected, MenuItem::ALL.len() - 1);
        app.highlight_menu(MenuItem::Github);
        assert_eq!(app.menu_highlight(), Some(MenuItem::Github));
        app.close_menu();
        assert!(app.menu_highlight().is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_add_search_is_set_and_plans_a_lookup_once() {
        let mut app = Repos::empty(test_settings());
        app.remotes_state = RemoteState::Ready;
        app.remotes = vec![remote("storytold", "alpha"), remote("storytold", "beta")];
        assert_eq!(app.add_source(), "github.com/storytold");
        app.set_add_filter("someone/");
        assert!(app.add_filter.is_empty(), "nothing while Add is closed");
        assert!(app.lookup_due.is_none());

        app.open_add();
        app.add_scroll = 4;
        app.set_add_filter("bet");
        assert_eq!(app.add_filter, "bet");
        assert_eq!(app.add_scroll, 0, "the list starts at the top again");
        assert_eq!(app.add_selected.as_deref(), Some("storytold/beta"));
        assert!(app.lookup_due.is_none(), "no account is named");

        app.set_add_filter("someone/");
        assert_eq!(app.add_source(), "github.com/someone");
        let (owner, at) = app.lookup_due.clone().expect("a lookup is planned");
        assert_eq!(owner, "someone");
        app.add_scroll = 2;
        app.set_add_filter("someone/");
        assert_eq!(
            app.lookup_due.as_ref().map(|(_, due)| *due),
            Some(at),
            "the same text plans nothing new"
        );
        assert_eq!(app.add_scroll, 2, "the same text changes nothing");

        // Several default accounts, or none: GitHub.
        app.set_add_filter("");
        app.owners = vec!["storytold".into(), "acme".into()];
        assert_eq!(app.add_source(), "GitHub");
        app.owners.clear();
        assert_eq!(app.add_source(), "GitHub");
    }

    #[test]
    fn a_found_repository_is_picked_and_ticked_by_its_id() {
        let mut app = Repos::empty(test_settings());
        app.remotes_state = RemoteState::Ready;
        app.installed.push(installed("storytold", "alpha"));
        app.remotes = vec![
            remote("storytold", "alpha"),
            remote("storytold", "beta"),
            remote("storytold", "gamma"),
        ];
        app.pick_add("storytold/beta");
        app.toggle_add_mark_of("storytold/beta");
        assert!(app.add_selected.is_none() && app.add_marked.is_empty());
        assert!(app.mode == Mode::List, "nothing while Add is closed");

        app.open_add();
        app.pick_add("Storytold/Gamma");
        assert_eq!(app.add_selected.as_deref(), Some("storytold/gamma"));
        app.pick_add("storytold/nowhere");
        assert_eq!(
            app.add_selected.as_deref(),
            Some("storytold/gamma"),
            "an unknown id does nothing"
        );

        app.toggle_add_mark_of("storytold/beta");
        assert!(app.is_marked(&remote("storytold", "beta")));
        assert_eq!(
            app.add_selected.as_deref(),
            Some("storytold/gamma"),
            "the highlight stays"
        );
        app.toggle_add_mark_of("storytold/alpha");
        assert!(
            !app.is_marked(&remote("storytold", "alpha")),
            "an installed one is not ticked"
        );
        app.toggle_add_mark_of("storytold/nowhere");
        assert_eq!(app.add_marked.len(), 1, "an unknown id does nothing");
        app.toggle_add_mark_of("STORYTOLD/BETA");
        assert!(app.add_marked.is_empty(), "a second time unticks");
    }

    #[test]
    fn a_question_takes_its_text_and_forgets_its_error() {
        let (mut app, root) = held_app("question-text");
        app.set_path_input("anything");
        app.set_service_input("anything");
        assert!(app.prompt.is_none() && app.service_prompt.is_none());
        assert!(app.mode == Mode::List, "nothing while no question is open");

        // The folder question.
        app.open_import();
        let prompt = app.prompt.as_mut().unwrap();
        prompt.error = Some("That is not a folder.".into());
        prompt.matches = vec!["api".into()];
        let before = prompt.input.clone();
        app.set_path_input(&before);
        let prompt = app.prompt.as_ref().unwrap();
        assert!(
            prompt.error.is_some() && !prompt.matches.is_empty(),
            "the same text changes nothing"
        );
        let typed = root.join("elsewhere").display().to_string();
        app.set_path_input(&typed);
        let prompt = app.prompt.as_ref().unwrap();
        assert_eq!(prompt.input, typed);
        assert!(prompt.error.is_none() && prompt.matches.is_empty());
        app.close_path();

        // The service question.
        app.run_migrate(MigrateItem::Service);
        assert!(app.mode == Mode::Service);
        app.accept_service();
        assert!(app.service_prompt.as_ref().unwrap().error.is_some());
        app.set_service_input("https://gitlab.com/acme/tool.git");
        let prompt = app.service_prompt.as_ref().unwrap();
        assert_eq!(prompt.input, "https://gitlab.com/acme/tool.git");
        assert!(prompt.error.is_none());
        app.set_service_input("");
        assert!(app.service_prompt.as_ref().unwrap().input.is_empty());
        app.close_service();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_migrate_menu_is_stepped_and_closed_through_methods() {
        let (mut app, root) = held_app("migrate-menu");
        app.highlight_migrate(MigrateItem::Service);
        app.close_migrate();
        assert!(app.menu.is_none() && app.migrate_highlight().is_none());
        app.enter_row();
        app.highlight_migrate(MigrateItem::Service);
        assert!(
            app.migrate_highlight().is_none(),
            "nothing while the submenu is closed"
        );
        assert_eq!(app.menu.as_ref().unwrap().migrate, None);
        app.highlight_menu(MenuItem::Migrate);
        app.run_menu(MenuItem::Migrate);
        assert_eq!(app.migrate_highlight(), Some(MigrateItem::Folder));
        app.highlight_migrate(MigrateItem::Service);
        assert_eq!(app.migrate_highlight(), Some(MigrateItem::Service));
        assert_eq!(app.menu.as_ref().unwrap().migrate, Some(1));
        app.highlight_migrate(MigrateItem::Folder);
        assert_eq!(app.migrate_highlight(), Some(MigrateItem::Folder));
        app.close_migrate();
        assert!(app.migrate_highlight().is_none());
        assert!(app.mode == Mode::Menu, "back on the menu it came from");
        assert_eq!(app.menu_highlight(), Some(MenuItem::Migrate));
        app.close_menu();
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_held_add_says_so_instead_of_loading_for_ever() {
        let (mut app, root) = held_app("held-add");
        app.remotes_state = RemoteState::Ready;
        app.open_add();
        app.set_add_filter("someone/");
        let status = app.add_status();
        assert_eq!(status.text, HELD);
        assert!(!status.busy && !status.error);
        assert!(app.lookup_due.is_none());
        assert!(!app.waiting(), "no lookup is coming");
        assert!(!app.drain());
        assert!(!app.waiting());
        // One that was running when work was held is left to land.
        app.lookup_running = Some("someone".into());
        assert!(app.add_status().busy);
        app.lookup_running = None;
        // Not held, it loads.
        app.work_held = false;
        app.set_add_filter("other/");
        let status = app.add_status();
        assert_eq!(status.text, "Loading github.com/other…");
        assert!(status.busy);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn held_work_still_opens_and_closes_what_a_person_looks_at() {
        let (mut app, root) = held_app("held-looks");
        app.enter_row();
        assert!(app.mode == Mode::Menu);
        app.close_menu();
        assert!(app.mode == Mode::List);
        app.begin_remove();
        assert!(matches!(app.mode, Mode::ConfirmRemove { .. }));
        app.run(Action::Cancel);
        assert!(app.mode == Mode::List);
        app.toggle_mark_of("tool");
        assert!(app.is_marked_local("tool"));
        app.clear_marks();
        app.toggle_sort();
        app.open_group_prompt();
        assert!(app.mode == Mode::Group);
        app.group_prompt.as_mut().unwrap().input = "Tools".into();
        app.accept_group();
        assert_eq!(app.installed[0].group, "Tools");
        assert_eq!(app.status, "tool is in Tools now.");
        app.toggle_group("Tools");
        assert_eq!(app.rows().len(), 1, "folded");
        app.open_settings(false);
        assert!(app.mode == Mode::Settings);
        app.close_settings();
        app.open_add();
        assert!(app.mode == Mode::Add);
        app.run(Action::Cancel);
        assert!(app.mode == Mode::List);
        assert!(
            !root.join("devassist.toml").exists(),
            "the settings file is not written"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// A scratch folder that is deleted when the test ends, also when it
    /// panics. Git's read-only files go too.
    struct Temp(PathBuf);

    impl Temp {
        fn new(name: &str) -> Self {
            Self(scratch(name))
        }
    }

    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = gitops::remove_tree(&self.0);
        }
    }

    /// A file URL Git clones from, for a folder on this disk.
    fn file_url(path: &Path) -> String {
        let text = path.display().to_string().replace('\\', "/");
        if text.starts_with('/') {
            format!("file://{text}")
        } else {
            format!("file:///{text}")
        }
    }

    /// A bare repository with one commit, standing in for one on GitHub.
    fn bare_origin(root: &Path, name: &str) -> PathBuf {
        let work = root.join(format!("{name}-work"));
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        std::fs::write(work.join("readme"), name).unwrap();
        git(&work, &["add", "readme"]);
        git(&work, &["commit", "-q", "-m", "one"]);
        let bare = root.join(format!("{name}.git"));
        git(
            root,
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        bare
    }

    /// Waits for the running task to end and its rows to be read.
    fn finish(app: &mut Repos) {
        let until = Instant::now() + Duration::from_secs(60);
        while app.busy || app.status_pending > 0 {
            assert!(Instant::now() < until, "the task did not finish");
            app.drain();
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn cloning_into_a_folder_from_add_is_one_step() {
        let temp = Temp::new("clone-from-add");
        let root = temp.0.clone();
        let settings = Settings {
            config_path: root.join("devassist.toml"),
            ..test_settings()
        };
        let mut app = Repos::empty(settings);
        app.remotes_state = RemoteState::Ready;
        let mut tool = remote("storytold", "tool");
        tool.clone_url = file_url(&bare_origin(&root, "tool"));
        let mut other = remote("storytold", "other");
        other.clone_url = file_url(&bare_origin(&root, "other"));
        app.remotes = vec![tool, other.clone()];
        app.open_add();
        app.add_selected = Some("storytold/tool".into());
        let code = root.join("code");

        // The highlighted one, nothing ticked.
        assert_eq!(app.clone_marked_into(&code.display().to_string()), Ok(()));
        // As after the question's Enter: the list, under the run's dialog.
        assert!(app.busy && app.mode == Mode::Progress, "the run is shown");
        assert!(app.prompt.is_none(), "the folder question never opened");
        assert_eq!(app.clone_dir, code, "the next clone is offered it");
        let saved = std::fs::read_to_string(root.join("devassist.toml")).unwrap();
        assert!(
            saved.contains(&format!("clone_dir = {}\n", code.display())),
            "{saved}"
        );
        finish(&mut app);
        assert!(!app.status_error, "{}", app.status);
        assert!(app.status.starts_with("Cloned tool into"), "{}", app.status);
        assert!(code.join("tool").join(".git").is_dir());
        assert!(
            app.checkouts
                .iter()
                .any(|path| paths::same_path(path, &code.join("tool")))
        );

        // The ticked ones, whatever is highlighted; the ticks go.
        app.open_add();
        app.add_selected = Some("storytold/tool".into());
        app.add_marked = vec![other];
        assert_eq!(app.clone_marked_into(&code.display().to_string()), Ok(()));
        assert!(app.add_marked.is_empty());
        finish(&mut app);
        assert!(
            app.status.starts_with("Cloned other into"),
            "{}",
            app.status
        );
        assert!(code.join("other").join(".git").is_dir());
    }

    #[test]
    fn a_bad_folder_leaves_add_as_it_was() {
        let temp = Temp::new("bad-add-folder");
        let root = temp.0.clone();
        let mut app = Repos::empty(test_settings());
        app.remotes_state = RemoteState::Ready;
        app.remotes = vec![remote("storytold", "tool"), remote("storytold", "web")];

        // Only from Add: another question stays as it is.
        app.add_selected = Some("storytold/tool".into());
        app.notice("before", false);
        let folder = root.display().to_string();
        assert_eq!(
            app.clone_marked_into(&folder),
            Err("Add is not open.".into())
        );
        assert!(app.mode == Mode::List && app.status == "before");
        app.open_import();
        assert_eq!(
            app.clone_marked_into(&folder),
            Err("Add is not open.".into())
        );
        assert!(app.mode == Mode::Path);
        assert!(matches!(
            app.prompt.as_ref().map(|prompt| &prompt.purpose),
            Some(Purpose::Import)
        ));
        assert!(!app.busy && app.queue.is_empty() && app.status == "before");
        app.close_path();

        app.open_add();
        app.add_selected = Some("storytold/tool".into());
        app.add_marked = vec![remote("storytold", "web")];
        let clone_dir = app.clone_dir.clone();
        let file = root.join("file.txt");
        std::fs::write(&file, "x").unwrap();
        let unchanged = |app: &Repos, what: &str| {
            assert!(app.mode == Mode::Add, "{what}: still Add");
            assert!(app.prompt.is_none(), "{what}: no question");
            assert_eq!(app.add_marked.len(), 1, "{what}: the tick stays");
            assert_eq!(
                app.add_selected.as_deref(),
                Some("storytold/tool"),
                "{what}"
            );
            assert_eq!(app.clone_dir, clone_dir, "{what}: the clone folder");
            assert_eq!(app.status, "before", "{what}: the status line");
            assert!(!app.busy && app.queue.is_empty(), "{what}: nothing runs");
        };

        // The question's own words.
        let file_text = file.display().to_string();
        assert_eq!(
            app.clone_marked_into(&file_text),
            Err(format!("{file_text} is a file, not a folder."))
        );
        unchanged(&app, "a file");
        let relative = app.clone_marked_into("dev").unwrap_err();
        assert!(
            relative.starts_with("Use a full path, such as "),
            "{relative}"
        );
        assert_eq!(Err(relative), pathprompt::resolve("dev"));
        unchanged(&app, "a relative path");
        assert_eq!(app.clone_marked_into("  "), Err("Type the folder.".into()));
        unchanged(&app, "nothing typed");

        // Add's own words, before the folder is looked at.
        app.installed.push(installed("storytold", "web"));
        assert_eq!(
            app.clone_marked_into(&folder),
            Err("Those repositories are on the list already.".into())
        );
        unchanged(&app, "ticked ones on the list");
        app.add_marked.clear();
        app.installed.push(installed("storytold", "tool"));
        assert_eq!(
            app.clone_marked_into(&folder),
            Err("storytold/tool is on the list already.".into())
        );
        app.add_selected = None;
        assert_eq!(
            app.clone_marked_into(&folder),
            Err("Select a repository to add.".into())
        );
        assert!(app.mode == Mode::Add && app.status == "before");

        // An empty list and a plain word: that account is listed, and
        // Add stays open. Held, so nobody asks GitHub.
        app.hold_work();
        app.add_filter = "someone".into();
        assert!(app.add_list().is_empty());
        assert_eq!(app.clone_marked_into(&folder), Ok(()));
        assert!(app.mode == Mode::Add);
        assert_eq!(app.add_filter, "someone/");
        assert_eq!(app.clone_dir, clone_dir);
        assert!(!app.busy && app.queue.is_empty());
    }

    #[test]
    fn move_lines_say_from_and_to_and_what_is_there_already() {
        let temp = Temp::new("move-lines");
        let root = temp.0.clone();
        let here = root.join("here");
        let there = root.join("there");
        let busy = root.join("busy");
        std::fs::create_dir_all(&there).unwrap();
        // Something called docs sits in busy already.
        std::fs::create_dir_all(busy.join("docs")).unwrap();
        let mut app = Repos::empty(test_settings());
        // api-copy is a second checkout in a folder called api.
        for (name, path) in [
            ("api", here.join("api")),
            ("docs", here.join("docs")),
            ("web", here.join("web")),
            ("api-copy", root.join("else").join("api")),
        ] {
            let mut repo = installed("acme", name);
            repo.folder = name.into();
            repo.path = path;
            std::fs::create_dir_all(&repo.path).unwrap();
            app.checkouts.push(repo.path.clone());
            app.installed.push(repo);
        }
        assert!(app.move_lines().is_empty(), "no question is open");
        assert_eq!(app.move_crosses_volumes(), None);
        app.selected = Some("api".into());
        app.marked = vec!["api".into(), "docs".into()];
        app.run_migrate(MigrateItem::Folder);
        assert!(app.mode == Mode::Path);
        let line =
            |name: &str, from: &Path, to: Option<PathBuf>, there: bool, taken: bool| MoveLine {
                name: name.into(),
                from: from.to_path_buf(),
                to,
                there,
                taken,
            };
        let api = here.join("api");
        let docs = here.join("docs");

        // It starts where the first one is: both are there already.
        assert_eq!(
            app.move_lines(),
            [
                line("api", &api, Some(api.clone()), true, false),
                line("docs", &docs, Some(docs.clone()), true, false),
            ]
        );
        assert_eq!(app.move_crosses_volumes(), Some(false), "nothing moves");
        app.set_path_input(&there.display().to_string());
        assert_eq!(
            app.move_lines(),
            [
                line("api", &api, Some(there.join("api")), false, false),
                line("docs", &docs, Some(there.join("docs")), false, false),
            ]
        );
        assert_eq!(app.move_crosses_volumes(), Some(false));
        // A folder that is not there yet is made by the move.
        let later = root.join("later");
        app.set_path_input(&later.display().to_string());
        assert_eq!(app.move_lines()[0].to, Some(later.join("api")));
        assert_eq!(app.move_crosses_volumes(), Some(false));

        // Refused: something is at the target already.
        app.set_path_input(&busy.display().to_string());
        assert_eq!(
            app.move_lines(),
            [
                line("api", &api, Some(busy.join("api")), false, false),
                line("docs", &docs, Some(busy.join("docs")), false, true),
            ]
        );
        // Refused: the target is inside the checkout itself.
        app.set_path_input(&api.display().to_string());
        assert_eq!(
            app.move_lines(),
            [
                line("api", &api, Some(api.join("api")), false, true),
                line("docs", &docs, Some(api.join("docs")), false, false),
            ]
        );

        // Not a usable folder: nowhere to go yet.
        for bad in ["", "dev"] {
            app.set_path_input(bad);
            assert_eq!(
                app.move_lines(),
                [
                    line("api", &api, None, false, false),
                    line("docs", &docs, None, false, false)
                ],
                "{bad:?}"
            );
            assert_eq!(app.move_crosses_volumes(), None, "{bad:?}");
        }

        // The question's folders, and the text stays.
        let typed = format!("{}{}th", root.display(), std::path::MAIN_SEPARATOR);
        app.set_path_input(&typed);
        assert_eq!(app.path_suggestions(), [there.display().to_string()]);
        assert_eq!(app.prompt.as_ref().unwrap().input, typed);
        app.close_path();
        assert!(app.path_suggestions().is_empty());

        // Refused: an earlier one of the same move goes to the same place.
        let copy = root.join("else").join("api");
        app.marked = vec!["api".into(), "api-copy".into()];
        app.run_migrate(MigrateItem::Folder);
        app.set_path_input(&there.display().to_string());
        assert_eq!(
            app.move_lines(),
            [
                line("api", &api, Some(there.join("api")), false, false),
                line("api-copy", &copy, Some(there.join("api")), false, true),
            ]
        );
    }

    #[test]
    fn a_move_that_leaves_the_old_folder_says_so_and_stays_on_screen() {
        let temp = Temp::new("move-left");
        let root = temp.0.clone();
        let old = root.join("here").join("api");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("readme"), "api").unwrap();
        let there = root.join("there");
        let settings = Settings {
            config_path: root.join("devassist.toml"),
            ..test_settings()
        };
        let mut app = Repos::empty(settings);
        let mut repo = installed("acme", "api");
        repo.path = old.clone();
        app.checkouts.push(old.clone());
        app.installed.push(repo);
        gitops::seams::as_if_another_drive(&old);
        gitops::seams::refuse(&old);

        app.move_checkouts(vec![("api".into(), old.clone())], there.clone());
        finish(&mut app);
        let words = format!(
            "Moved api to {}. The old folder {} could not be deleted: refused by the test.",
            there.display(),
            old.display()
        );
        assert_eq!(app.status, words);
        assert!(app.status_error, "it is said as a fault");
        let job = app.job.as_ref().unwrap();
        assert_eq!(job.result, Some(Err(words)));
        assert!(app.job_close_at.is_none(), "the dialog waits to be closed");
        // The copy is complete: it is the repository now.
        assert!(app.checkouts.contains(&there.join("api")));
        assert!(!app.checkouts.contains(&old));
        assert!(there.join("api").join("readme").is_file());
    }

    /// The dialogs' checks of a held session look at no folder; live, the
    /// same checks do, and are counted.
    #[cfg(feature = "testing")]
    #[test]
    fn a_held_session_checks_its_dialogs_by_the_text_alone() {
        let reads = crate::sys::disk_reads_here;
        // A full path that is nowhere: live, nothing is pushed to it.
        let folder = std::env::temp_dir()
            .join(format!("devassist-nowhere-{}", std::process::id()))
            .display()
            .to_string();
        let walk = |app: &mut Repos| {
            app.selected = Some("api".into());
            app.enter_row();
            assert!(app.menu.is_some());
            app.close_menu();
            app.run_migrate(MigrateItem::Service);
            app.set_service_input(&folder);
            let facts = app.service_facts();
            assert!(
                facts[3].words.contains("the remote mirror"),
                "{}",
                facts[3].words
            );
            app.accept_service();
            app.close_service();
            app.open_import();
            app.set_path_input(&folder);
            app.run(Action::Confirm);
            app.close_path();
            app.run_migrate(MigrateItem::Folder);
            app.set_path_input(&folder);
            app.run(Action::Confirm);
            app.close_path();
            app.remotes_state = RemoteState::Ready;
            app.remotes = vec![remote("storytold", "tool")];
            app.open_add();
            app.add_selected = Some("storytold/tool".into());
            let _ = app.clone_marked_into(&folder);
        };
        let mut held = grouped_app();
        held.hold_work();
        let before = reads();
        walk(&mut held);
        assert_eq!(reads(), before, "the held session looked at the disk");
        assert_eq!(held.status, HELD);
        assert!(held.queue.is_empty() && !held.busy);

        // Live, the facts still read nothing; the other checks look.
        let mut live = grouped_app();
        testing::hold_github(&mut live);
        live.run_migrate(MigrateItem::Service);
        live.set_service_input(&folder);
        let before = reads();
        let _ = live.service_facts();
        assert_eq!(reads(), before, "the facts looked at the disk");
        live.accept_service();
        assert!(reads() > before, "Push did not look");
        live.close_service();
        let before = reads();
        live.selected = Some("api".into());
        live.enter_row();
        assert!(reads() > before, "the menu did not look for the forks");
    }

    fn a_fork(
        owner: &str,
        branches: u32,
        pulls: u32,
        folder: bool,
        gone: bool,
    ) -> gitops::ForkInfo {
        gitops::ForkInfo {
            owner: owner.into(),
            name: "api".into(),
            branches,
            pulls,
            ahead: Some(2),
            date: 1_795_392_000,
            folder,
            gone,
        }
    }

    /// An open overview of these forks of acme/api.
    fn fork_view(forks: Vec<gitops::ForkInfo>) -> ForksView {
        ForksView {
            repo: "api".into(),
            path: PathBuf::from("api"),
            original: "acme/api".into(),
            dest: PathBuf::from("api-fork"),
            forks,
            loading: false,
            error: None,
            selected: None,
            scroll: 0,
            sort: ForkSort::Changed,
            filter: String::new(),
            filtering: false,
            changed_only: false,
            base: None,
            fetched: None,
        }
    }

    #[test]
    fn the_overview_counts_every_fork_and_sums_them_up() {
        let mut view = fork_view(vec![
            a_fork("amy", 4, 1, true, false),
            a_fork("bob", 1, 0, false, false),
            a_fork("cy", 2, 3, false, true),
        ]);
        assert_eq!(
            view.counts(),
            ForkCounts {
                all: 3,
                shown: 3,
                changed: 1,
                gone: 1,
                branches: 7,
                pulls: 4,
            }
        );
        let sentence = "3 forks. 1 has changes and a folder. 1 is gone from GitHub. 7 branches and 4 pull requests are saved.";
        assert_eq!(view.summary(), sentence);
        // The filter changes what is shown, not what there is.
        view.filter = "amy".into();
        assert_eq!(view.counts().shown, 1);
        assert_eq!(view.counts().all, 3);
        assert_eq!(view.summary(), sentence);
        assert_eq!(fork_view(Vec::new()).summary(), "No forks.");
    }

    #[test]
    fn the_empty_message_says_why_nothing_shows() {
        let mut view = fork_view(Vec::new());
        view.loading = true;
        view.error = Some("broken".into());
        assert_eq!(view.empty_message().as_deref(), Some("Reading the forks…"));
        view.loading = false;
        assert_eq!(
            view.empty_message().as_deref(),
            Some("The forks could not be read. r tries again.")
        );
        view.error = None;
        assert_eq!(
            view.empty_message().as_deref(),
            Some("No fork is in the folder yet. u fetches them.")
        );
        view.forks = vec![a_fork("bob", 1, 0, false, false)];
        assert_eq!(view.empty_message(), None, "a fork shows");
        // Shown forks win over a read that is running again.
        view.loading = true;
        assert_eq!(view.empty_message(), None);
        view.loading = false;
        view.changed_only = true;
        assert_eq!(
            view.empty_message().as_deref(),
            Some("No fork matches. esc clears the filter, c shows the forks without changes.")
        );
        view.changed_only = false;
        view.filter = "nobody".into();
        assert_eq!(
            view.empty_message().as_deref(),
            Some("No fork matches. esc clears the filter, c shows the forks without changes.")
        );
    }

    #[test]
    fn the_fork_filter_is_set_and_cleared_through_methods() {
        let mut app = grouped_app();
        // With no overview open, nothing happens.
        app.set_fork_filter("cy");
        app.clear_fork_filter();
        app.set_fork_sort(ForkSort::Name);
        app.set_fork_changed_only(true);
        assert!(app.forks.is_none());

        let mut view = fork_view(vec![
            a_fork("bob", 1, 0, true, false),
            a_fork("cy", 1, 0, false, false),
        ]);
        view.selected = Some("bob".into());
        view.scroll = 5;
        view.filtering = true;
        app.forks = Some(view);
        fn open(app: &Repos) -> &ForksView {
            app.forks.as_ref().unwrap()
        }
        app.set_fork_filter("cy");
        assert_eq!(open(&app).filter, "cy");
        assert_eq!(open(&app).scroll, 0, "the list starts at the top");
        assert!(open(&app).filtering, "the typing goes on");
        // The highlighted fork is filtered out: the first one shown is it.
        assert_eq!(open(&app).chosen().unwrap().owner, "cy");
        // The same text changes nothing.
        app.forks.as_mut().unwrap().scroll = 3;
        app.set_fork_filter("cy");
        assert_eq!(open(&app).scroll, 3);
        app.clear_fork_filter();
        assert_eq!(open(&app).filter, "");
        assert!(!open(&app).filtering);
        assert_eq!(open(&app).scroll, 0);

        app.forks.as_mut().unwrap().scroll = 3;
        app.set_fork_sort(ForkSort::Changed);
        assert_eq!(open(&app).scroll, 3, "the sort it has already");
        app.set_fork_sort(ForkSort::Name);
        assert_eq!(open(&app).sort, ForkSort::Name);
        assert_eq!(open(&app).scroll, 0);
        app.forks.as_mut().unwrap().scroll = 3;
        app.set_fork_changed_only(false);
        assert_eq!(open(&app).scroll, 3, "all are shown already");
        app.set_fork_changed_only(true);
        assert!(open(&app).changed_only);
        assert_eq!(open(&app).scroll, 0);
        // The cycling and the toggling neighbours do the same.
        app.cycle_fork_sort();
        assert_eq!(open(&app).sort, ForkSort::Changed);
        app.toggle_fork_changed_only();
        assert!(!open(&app).changed_only);
    }

    #[test]
    fn a_held_session_shows_the_forks_it_was_given_and_reads_no_disk() {
        let (mut app, root) = held_app("forks-held");
        let _temp = Temp(root.clone());
        let mut other = installed("acme", "other");
        other.path = root.join("other");
        app.installed.push(other);
        let list = gitops::ForkList {
            original: "acme/tool".into(),
            dest: PathBuf::from("story").join("tool-fork"),
            forks: vec![
                a_fork("bob", 3, 1, true, false),
                a_fork("cy", 1, 0, false, false),
            ],
            base: Some("acme/main".into()),
            fetched: Some(1_795_392_000),
        };
        app.hold_forks("tool", gitops::ForkList::default());
        // A later list replaces the first.
        app.hold_forks("tool", list.clone());
        #[cfg(feature = "testing")]
        let before = crate::sys::disk_reads_here();

        // The menu knows the forks are there.
        app.enter_row();
        assert_eq!(
            app.menu.as_ref().unwrap().label(MenuItem::Forks),
            "Update the forks"
        );
        app.close_menu();
        app.open_forks();
        let view = app.forks.as_ref().expect("the overview is open");
        assert!(!view.loading, "the forks are there at once");
        assert_eq!(view.forks, list.forks);
        assert_eq!(view.original, "acme/tool");
        assert_eq!(view.dest, list.dest);
        assert_eq!(view.base(), Some("acme/main"));
        assert_eq!(view.fetched(), Some(1_795_392_000));
        // Reading again takes the same list, at once.
        app.forks.as_mut().unwrap().forks.clear();
        app.load_forks();
        let view = app.forks.as_ref().unwrap();
        assert!(!view.loading);
        assert_eq!(view.forks, list.forks);
        app.close_forks();
        app.open_forks_plain();
        assert_eq!(app.forks.as_ref().unwrap().forks, list.forks);

        // What would run or open something says the demo's words.
        app.status.clear();
        app.update_shown_forks();
        assert_eq!(app.status, HELD);
        assert!(app.queue.is_empty() && !app.busy && app.job.is_none());
        app.status.clear();
        app.open_fork_folder();
        assert_eq!(app.status, HELD);
        app.status.clear();
        app.open_fork_on_github();
        assert_eq!(app.status, HELD);
        app.close_forks();

        // A repository without made-up forks has none cloned.
        app.selected = Some("other".into());
        app.enter_row();
        assert_eq!(
            app.menu.as_ref().unwrap().label(MenuItem::Forks),
            "Clone the forks"
        );
        app.close_menu();
        app.open_forks();
        assert!(app.forks.is_none());
        assert_eq!(
            app.status,
            "The forks of other are not cloned yet. Enter, then n clones them."
        );
        app.open_forks_plain();
        assert_eq!(
            app.status,
            "The forks of other are not cloned yet. Clone the forks does it."
        );
        #[cfg(feature = "testing")]
        assert_eq!(
            crate::sys::disk_reads_here(),
            before,
            "the held session looked at the disk"
        );
        // No read was sent to the worker.
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            app.worker.drain().is_empty(),
            "a read ran in the background"
        );
        assert!(!root.join("tool-fork").exists());
    }

    #[test]
    fn a_held_session_updates_all_forks_from_its_story_alone() {
        let (mut app, root) = held_app("forks-all-held");
        let _temp = Temp(root.clone());
        #[cfg(feature = "testing")]
        let before = crate::sys::disk_reads_here();
        // No story forks: none are cloned.
        app.update_all_forks();
        assert_eq!(
            app.status,
            "No forks are cloned yet. Clone the forks of a repository first."
        );
        assert!(app.status_error);
        // With them, it is the demo's notice, and nothing runs.
        app.hold_forks("tool", gitops::ForkList::default());
        app.status.clear();
        app.update_all_forks();
        assert_eq!(app.status, HELD);
        assert!(app.queue.is_empty() && !app.busy && app.job.is_none());
        assert!(app.mode == Mode::List);
        #[cfg(feature = "testing")]
        assert_eq!(
            crate::sys::disk_reads_here(),
            before,
            "the held session looked at the disk"
        );
    }

    #[test]
    fn a_held_menu_counts_the_forks_of_every_chosen_name() {
        let (mut app, root) = held_app("forks-menu-held");
        let _temp = Temp(root.clone());
        app.hold_forks("tool", gitops::ForkList::default());
        app.enter_row();
        assert_eq!(
            app.menu.as_ref().unwrap().label(MenuItem::Forks),
            "Update the forks"
        );
        app.close_menu();
        // A chosen name that is no row has no forks in the story.
        app.selected = Some("ghost".into());
        app.enter_row();
        assert_eq!(
            app.menu.as_ref().unwrap().label(MenuItem::Forks),
            "Clone the forks"
        );
    }

    /// What a worker would send of a job, sent by hand.
    fn job_says(app: &mut Repos, event: JobEvent) {
        app.on_msg(Msg::Job(event));
    }

    fn job_line(app: &mut Repos, line: &str, progress: bool) {
        job_says(app, JobEvent::Line(line.to_string(), progress));
    }

    fn fork_fetch(done: u32, total: u32, pause: Option<&str>) -> Option<ForkProgress> {
        Some(ForkProgress {
            done,
            total,
            at_a_time: 8,
            pause: pause.map(String::from),
        })
    }

    #[test]
    fn the_progress_of_a_fetch_follows_its_lines() {
        let mut app = grouped_app();
        assert_eq!(app.fork_progress(), None, "nothing runs");
        let job = gitprogress::job(
            "Updating the forks of 2 repositories",
            vec!["api".into(), "docs".into()],
            0,
        );
        // The worker ends at once; what it says is sent by hand below.
        app.start_fork_job("Updating the forks…".into(), job, |_| Ok("Done.".into()));
        job_says(&mut app, JobEvent::Start(0));
        let dest = PathBuf::from("/srv/code").join("api-fork");
        job_line(&mut app, "Asking GitHub for the forks of acme/api.", false);
        job_line(&mut app, "Listing forks: 38", true);
        assert_eq!(app.fork_progress(), None, "no total yet");
        assert_eq!(app.fork_job_into(), None);
        // The drawing's five lines.
        job_line(
            &mut app,
            &format!("38 forks, into {}", dest.display()),
            false,
        );
        assert_eq!(app.fork_progress(), fork_fetch(0, 38, None));
        assert_eq!(app.fork_job_into(), Some(dest.clone()));
        job_line(&mut app, "Fetching acme/api, the original.", false);
        job_line(&mut app, "Fetching forks:  63% (24/38)", true);
        assert_eq!(app.fork_progress(), fork_fetch(24, 38, None));
        job_line(&mut app, "The fetch timed out. Paused for 0:30.", false);
        assert_eq!(
            app.fork_progress(),
            fork_fetch(
                24,
                38,
                Some("The fetch timed out. Paused, going on in 0:30.")
            )
        );
        job_line(&mut app, "Paused, going on in 0:12", true);
        assert_eq!(
            app.fork_progress(),
            fork_fetch(
                24,
                38,
                Some("The fetch timed out. Paused, going on in 0:12.")
            )
        );
        // The next repository starts from nothing.
        job_says(&mut app, JobEvent::Done(0, "38 forks".into()));
        job_says(&mut app, JobEvent::Start(1));
        assert_eq!(app.fork_progress(), None);
        assert_eq!(app.fork_job_into(), None);
        let docs = PathBuf::from("/srv/code").join("docs-fork");
        job_line(
            &mut app,
            &format!("3 forks, into {}", docs.display()),
            false,
        );
        job_line(&mut app, "Fetching forks:  33% (1/3)", true);
        assert_eq!(app.fork_progress(), fork_fetch(1, 3, None));
        assert_eq!(app.fork_job_into(), Some(docs));
        // A run that has ended says nothing of it.
        app.job.as_mut().unwrap().finish(Ok("Done.".into()), false);
        assert_eq!(app.fork_progress(), None);
        assert_eq!(app.fork_job_into(), None);
    }

    #[test]
    fn a_pause_ends_when_the_fetch_goes_on() {
        let total = "38 forks, into api-fork";
        let mut app = grouped_app();
        testing::fork_job(
            &mut app,
            "api",
            &[
                (total, false),
                ("Fetching forks:  63% (24/38)", true),
                ("GitHub's limit is reached. Paused for 4:12.", false),
                ("Paused, going on in 4:10", true),
            ],
        );
        assert_eq!(
            app.fork_progress(),
            fork_fetch(
                24,
                38,
                Some("GitHub's limit is reached. Paused, going on in 4:10.")
            )
        );
        job_line(&mut app, "Going on.", false);
        assert_eq!(app.fork_progress(), fork_fetch(24, 38, None));
        // A fork that pauses while the others go on: the next count ends
        // what it says.
        let mut app = grouped_app();
        testing::fork_job(
            &mut app,
            "api",
            &[
                (total, false),
                ("The fetch timed out. Paused for 0:10.", false),
            ],
        );
        assert_eq!(
            app.fork_progress(),
            fork_fetch(
                0,
                38,
                Some("The fetch timed out. Paused, going on in 0:10.")
            )
        );
        job_line(&mut app, "Fetching forks:  65% (25/38)", true);
        assert_eq!(app.fork_progress(), fork_fetch(25, 38, None));
    }

    #[test]
    fn a_held_fetch_of_forks_stays_until_it_is_hidden() {
        // Live, there is no story to show.
        let mut app = grouped_app();
        app.hold_fork_job("api", &[("38 forks, into api-fork", false)]);
        assert!(app.job.is_none());
        assert!(app.mode == Mode::List);
        assert_eq!(app.fork_progress(), None);

        app.hold_work();
        app.hold_fork_job("api", &[("38 forks, into api-fork", false)]);
        assert!(app.mode == Mode::Progress);
        assert_eq!(app.fork_progress(), fork_fetch(0, 38, None));
        assert_eq!(app.fork_job_into(), Some(PathBuf::from("api-fork")));
        let job = app.job.as_ref().unwrap();
        assert!(job.running());
        assert_eq!(job.items.len(), 1);
        assert_eq!(job.items[0].name, "api");
        assert!(!app.busy, "no worker runs it");
        // It never ends, nor closes by itself.
        for _ in 0..3 {
            app.drain();
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(app.job.as_ref().unwrap().running());
        assert!(app.mode == Mode::Progress);
        assert!(app.job_close_at.is_none());
        // Cancel only says the demo's words.
        app.status.clear();
        app.cancel_work();
        assert_eq!(app.status, HELD);
        let job = app.job.as_ref().unwrap();
        assert!(job.running() && !job.cancelling);
        assert_eq!(app.fork_progress(), fork_fetch(0, 38, None));
        // Hide hides it; it is still there to show again.
        app.hide_job();
        assert!(app.mode == Mode::List);
        assert!(app.job.as_ref().unwrap().running());
        app.show_job();
        assert!(app.mode == Mode::Progress);
        assert_eq!(app.fork_progress(), fork_fetch(0, 38, None));
    }

    #[test]
    fn a_run_that_is_not_a_fetch_of_forks_has_no_fork_progress() {
        let lines = [
            ("38 forks, into api-fork", false),
            ("Fetching forks:  63% (24/38)", true),
            ("The fetch timed out. Paused for 0:30.", false),
        ];
        let mut app = grouped_app();
        let job = Job::new("Updating", vec!["api".into()], 0);
        app.start_job("Updating api…".into(), Some(job), |_| Ok("Done.".into()));
        job_says(&mut app, JobEvent::Start(0));
        for (line, progress) in lines {
            job_line(&mut app, line, progress);
        }
        assert_eq!(app.fork_progress(), None);
        assert_eq!(app.fork_job_into(), None);
        // A run that follows a fetch of forks is not one either.
        let mut app = grouped_app();
        let forks = gitprogress::job("Updating the forks of api", vec!["api".into()], 0);
        app.start_fork_job("Updating the forks of api…".into(), forks, |_| {
            Ok("Done.".into())
        });
        job_says(&mut app, JobEvent::Start(0));
        job_line(&mut app, lines[0].0, false);
        assert_eq!(app.fork_progress(), fork_fetch(0, 38, None));
        app.job.as_mut().unwrap().finish(Ok("Done.".into()), false);
        app.busy = false;
        let job = Job::new("Updating", vec!["api".into()], 0);
        app.start_job("Updating api…".into(), Some(job), |_| Ok("Done.".into()));
        job_says(&mut app, JobEvent::Start(0));
        for (line, progress) in lines {
            job_line(&mut app, line, progress);
        }
        assert_eq!(app.fork_progress(), None);
        assert_eq!(app.fork_job_into(), None);
    }

    #[test]
    fn show_the_forks_of_a_repository_without_them_says_so_plainly() {
        let mut app = grouped_app();
        app.selected = Some("api".into());
        app.open_forks_plain();
        assert!(app.forks.is_none());
        assert_eq!(
            app.status,
            "The forks of api are not cloned yet. Clone the forks does it."
        );
        assert!(app.status_error);
        // The terminal's words still name its keys.
        app.open_forks();
        assert_eq!(
            app.status,
            "The forks of api are not cloned yet. Enter, then n clones them."
        );
        app.marked = vec!["api".into(), "web".into()];
        app.open_forks_plain();
        assert_eq!(app.status, "Highlight one repository to see its forks.");
    }

    #[test]
    fn what_a_move_left_is_said_after_how_it_went() {
        let dir = Path::new("/srv/new");
        let old = "The old folder /srv/old/api could not be deleted: busy.".to_string();
        assert_eq!(
            moved_words("api", dir, (1, 0), &[], &[]),
            Ok("Moved api to /srv/new.".into())
        );
        assert_eq!(
            moved_words(
                "2 repositories",
                dir,
                (2, 0),
                &[],
                std::slice::from_ref(&old)
            ),
            Err(format!("Moved 2 repositories to /srv/new. {old}"))
        );
        assert_eq!(
            moved_words(
                "2 repositories",
                dir,
                (1, 0),
                &["web".into()],
                std::slice::from_ref(&old)
            ),
            Err(format!("Moved 1, 1 failed: web. {old}"))
        );
        let partial = "The partial copy at /srv/new/web could not be removed: busy.";
        assert_eq!(
            finished_words(Err(cancelled_leaving(&[partial.into()])), true, false),
            (format!("Cancelled. {partial}"), true)
        );
        assert_eq!(
            finished_words(Err(cancelled_leaving(&[])), true, false),
            ("Cancelled.".into(), false)
        );
    }

    #[test]
    fn a_held_move_question_says_where_each_goes_from_the_text_alone() {
        let temp = Temp::new("move-held");
        let root = temp.0.clone();
        // Something sits where api would go: a live question calls it taken,
        // a held one does not look.
        std::fs::create_dir_all(root.join("there").join("api")).unwrap();
        let (here, elsewhere) = if cfg!(windows) {
            (PathBuf::from(r"D:\code\work"), PathBuf::from(r"E:\work"))
        } else {
            (PathBuf::from("/srv/code/work"), PathBuf::from("/mnt/work"))
        };
        let mut app = Repos::empty(test_settings());
        app.hold_work();
        for name in ["api", "web", "docs"] {
            let mut repo = installed("acme", name);
            repo.folder = name.into();
            repo.path = here.join(name);
            app.checkouts.push(repo.path.clone());
            app.installed.push(repo);
        }
        app.marked = vec!["api".into(), "web".into()];
        app.run_migrate(MigrateItem::Folder);
        assert!(app.mode == Mode::Path);
        let to = |dir: &Path, name: &str| Some(dir.join(name));
        // It starts where they are: both are there already, by their text.
        let lines = app.move_lines();
        assert_eq!(
            lines.iter().map(|line| line.there).collect::<Vec<_>>(),
            [true, true]
        );
        assert_eq!(app.move_crosses_volumes(), Some(false), "nothing moves");
        // Another drive, told by the text.
        app.set_path_input(&elsewhere.display().to_string());
        let lines = app.move_lines();
        assert_eq!(
            lines.iter().map(|line| line.to.clone()).collect::<Vec<_>>(),
            [to(&elsewhere, "api"), to(&elsewhere, "web")]
        );
        assert!(lines.iter().all(|line| !line.there && !line.taken));
        // Told by the drive in the text on Windows; elsewhere a mount point
        // cannot be told from it, and nothing is said.
        let crosses = cfg!(windows).then_some(true);
        assert_eq!(app.move_crosses_volumes(), crosses);
        // What is on the disk is not looked at.
        let there = root.join("there");
        app.set_path_input(&there.display().to_string());
        let lines = app.move_lines();
        assert_eq!(lines[0].to, to(&there, "api"));
        assert!(!lines[0].taken, "a held question looked at the disk");
        assert_eq!(
            app.move_crosses_volumes(),
            crosses,
            "{} is on another drive",
            there.display()
        );
        // Still refused by the text: into the checkout itself.
        app.set_path_input(&here.join("api").display().to_string());
        assert!(app.move_lines()[0].taken);
        // A text that is no folder: nowhere to go yet.
        app.set_path_input("dev");
        assert!(app.move_lines().iter().all(|line| line.to.is_none()));
        assert_eq!(app.move_crosses_volumes(), None);
    }

    #[test]
    fn the_folder_questions_reason_is_told_while_typing_without_submitting() {
        let temp = Temp::new("path-reason");
        let file = temp.0.join("notes.txt");
        std::fs::create_dir_all(&temp.0).unwrap();
        std::fs::write(&file, "notes").unwrap();
        let mut app = Repos::empty(test_settings());
        assert_eq!(app.path_reason(), None, "no question is open");
        app.open_import();
        let says = |app: &mut Repos, text: &str| {
            app.set_path_input(text);
            app.path_reason()
        };
        assert_eq!(says(&mut app, ""), None, "nothing typed yet");
        assert_eq!(says(&mut app, "   "), None, "nothing typed yet");
        assert!(says(&mut app, "dev").is_some_and(|reason| reason.starts_with("Use a full path")));
        assert_eq!(
            says(&mut app, &file.display().to_string()),
            Some(format!("{} is a file, not a folder.", file.display()))
        );
        assert_eq!(says(&mut app, &temp.0.display().to_string()), None);
        // Nothing was submitted: the question is open and has no error.
        assert!(app.mode == Mode::Path && app.prompt.as_ref().unwrap().error.is_none());
        // Held: only what the text tells.
        app.hold_work();
        assert_eq!(
            says(&mut app, &file.display().to_string()),
            None,
            "a held question looked at the disk"
        );
        assert!(says(&mut app, "dev").is_some());
    }

    #[test]
    fn a_target_inside_the_checkout_by_another_case_is_refused_where_case_does_not_count() {
        let temp = Temp::new("move-case");
        let api = temp.0.join("code").join("api");
        std::fs::create_dir_all(&api).unwrap();
        let mut app = Repos::empty(test_settings());
        let mut repo = installed("acme", "api");
        repo.folder = "api".into();
        repo.path = api.clone();
        app.checkouts.push(api.clone());
        app.installed.push(repo);
        app.marked = vec!["api".into()];
        app.run_migrate(MigrateItem::Folder);
        let inside = PathBuf::from(api.display().to_string().to_uppercase()).join("sub");
        app.set_path_input(&inside.display().to_string());
        assert_eq!(
            app.move_lines()[0].taken,
            cfg!(windows),
            "{}",
            inside.display()
        );
    }

    /// How often [`counted_token`] was asked for a token.
    static TOKENS_ASKED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    /// A token source that counts its asks and gives none.
    fn counted_token() -> Option<String> {
        TOKENS_ASKED.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        None
    }

    #[test]
    fn a_push_while_github_is_held_asks_for_no_token() {
        let temp = Temp::new("push-token");
        let root = temp.0.clone();
        let tool = root.join("tool");
        std::fs::create_dir_all(&tool).unwrap();
        git(&tool, &["init", "-q", "-b", "main"]);
        std::fs::write(tool.join("one"), "one").unwrap();
        git(&tool, &["add", "one"]);
        git(&tool, &["commit", "-q", "-m", "one"]);
        git(&root, &["init", "-q", "--bare", "target.git"]);
        let address = root.join("target.git").display().to_string();
        let settings = Settings {
            config_path: root.join("devassist.toml"),
            ..test_settings()
        };
        let mut app = Repos::empty(settings);
        let mut repo = installed("acme", "tool");
        repo.path = tool.clone();
        app.checkouts.push(tool.clone());
        app.installed.push(repo);
        testing::token_source(&mut app, counted_token);
        let push = |app: &mut Repos| {
            app.clear_marks();
            app.pick("tool");
            app.run_migrate(MigrateItem::Service);
            assert!(app.mode == Mode::Service, "{}", app.status);
            app.set_service_input(&address);
            app.accept_service();
            finish(app);
            assert!(
                matches!(
                    app.job.as_ref().and_then(|job| job.result.clone()),
                    Some(Ok(_))
                ),
                "{}",
                app.status
            );
        };
        testing::hold_github(&mut app);
        push(&mut app);
        assert_eq!(
            TOKENS_ASKED.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "held, a token was asked for"
        );
        // Not held, the same push asks the source: it is the one the push uses.
        app.github_held = false;
        push(&mut app);
        assert_eq!(TOKENS_ASKED.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn a_cancelled_push_says_what_stays_there_and_other_runs_do_not() {
        let cancelled = Err("cancelled".to_string());
        assert_eq!(
            finished_words(cancelled.clone(), true, true),
            (
                "Cancelled. What was already sent stays on the other service; the address was not kept.".to_string(),
                false
            )
        );
        assert_eq!(
            finished_words(cancelled.clone(), true, false),
            ("Cancelled.".to_string(), false)
        );
        // A push that failed, or went well, says so as every run does.
        assert_eq!(
            finished_words(Err("rejected".into()), false, true),
            ("rejected".to_string(), true)
        );
        assert_eq!(
            finished_words(Ok("Pushed.".into()), false, true),
            ("Pushed.".to_string(), false)
        );
        // Only a push's run is marked as one, and the mark goes with it.
        let mut app = Repos::empty(test_settings());
        app.busy = true;
        app.push_running = true;
        app.on_msg(Msg::JobFinished(Ok("Pushed.".into())));
        assert!(!app.push_running);
    }

    #[test]
    fn the_push_facts_match_what_a_push_does() {
        let temp = Temp::new("push-facts");
        let root = temp.0.clone();
        let upstream = root.join("upstream");
        let clone = root.join("clone");
        let target = root.join("target.git");
        let commit = |dir: &Path, name: &str| {
            std::fs::write(dir.join(name), name).unwrap();
            git(dir, &["add", name]);
            git(dir, &["commit", "-q", "-m", name]);
        };
        let tip = |dir: &Path, name: &str| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["rev-parse", "--verify", "-q", name])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };
        let refs = |dir: &Path| {
            let output = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["for-each-ref", "--format=%(refname)"])
                .output()
                .unwrap();
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::to_string)
                .collect::<Vec<_>>()
        };
        let config = || std::fs::read_to_string(clone.join(".git").join("config")).unwrap();
        std::fs::create_dir_all(&upstream).unwrap();
        git(&upstream, &["init", "-q", "-b", "main"]);
        commit(&upstream, "one");
        git(&upstream, &["tag", "v1"]);
        git(&upstream, &["checkout", "-q", "-b", "dev"]);
        commit(&upstream, "dev-work");
        git(&upstream, &["checkout", "-q", "-b", "old"]);
        git(&upstream, &["checkout", "-q", "-b", "split", "main"]);
        commit(&upstream, "split-work");
        git(&upstream, &["checkout", "-q", "main"]);
        git(&root, &["clone", "-q", "upstream", "clone"]);
        git(&root, &["init", "-q", "--bare", "target.git"]);
        // Here: main has a commit of its own, old is behind origin's, mine
        // is here only. Origin gets a branch and two pull requests after
        // the clone: 1 is open, 2 was closed and its branch deleted, as
        // GitHub keeps the ref of a closed or merged one.
        commit(&clone, "local");
        git(&clone, &["branch", "-q", "old", "origin/old~1"]);
        git(&clone, &["branch", "-q", "mine"]);
        // split here went another way from origin's.
        git(&clone, &["checkout", "-q", "-b", "split", "origin/split~1"]);
        commit(&clone, "split-here");
        git(&clone, &["checkout", "-q", "main"]);
        git(&upstream, &["checkout", "-q", "-b", "late"]);
        commit(&upstream, "late-work");
        git(&upstream, &["checkout", "-q", "--detach", "main"]);
        commit(&upstream, "pull-1");
        git(&upstream, &["update-ref", "refs/pull/1/head", "HEAD"]);
        git(&upstream, &["checkout", "-q", "-b", "fix", "main"]);
        commit(&upstream, "pull-2");
        git(&upstream, &["update-ref", "refs/pull/2/head", "HEAD"]);
        git(&upstream, &["checkout", "-q", "main"]);
        git(&upstream, &["branch", "-q", "-D", "fix"]);
        // The other service has a branch of its own, and a pull/1 that
        // went another way.
        let address = target.display().to_string();
        git(
            &upstream,
            &["push", "-q", &address, "main:refs/heads/theirs"],
        );
        git(
            &upstream,
            &["push", "-q", &address, "dev:refs/heads/pull/1"],
        );
        let theirs = tip(&target, "refs/heads/theirs");

        // Here, a remote of the name the address gets, at another address.
        git(
            &clone,
            &["remote", "add", "mirror", "https://example.invalid/old.git"],
        );

        let mut app = Repos::empty(test_settings());
        app.service_prompt = Some(ServicePrompt {
            repo: "tool".into(),
            path: clone.clone(),
            input: address.clone(),
            error: None,
        });
        // As the dialog reads them when it opens.
        app.service_remotes = gitops::remote_urls(&clone);
        let facts = app.service_facts();
        let words: Vec<(bool, &str)> = facts
            .iter()
            .map(|fact| (fact.goes, fact.words.as_str()))
            .collect();
        assert_eq!(
            words,
            [
                (
                    true,
                    "Every branch and tag goes over; a branch goes as origin has it where the one here is behind it or missing, otherwise as it is here."
                ),
                (
                    true,
                    "Origin, if there is one, is fetched first; if it cannot be reached, what is saved here goes."
                ),
                (
                    true,
                    "Every GitHub pull request, open, merged or closed, goes over as a branch pull/<number> (or under another name if pull is taken here), replaced on every push."
                ),
                (
                    true,
                    "This checkout keeps its files and origin, and keeps the address as the remote mirror. A remote of that name that this checkout already has is pointed at the new address."
                ),
                (
                    false,
                    "Apart from those pull-request branches, nothing there is overwritten or deleted: a branch there only moves forward, so no commit there is lost."
                ),
                (
                    false,
                    "A branch there that has commits the pushed branch lacks, or a tag there that differs from the pushed one, stops the push part-way with an error, before the address is kept."
                ),
                (
                    false,
                    "No pull request is opened there, and no issue, comment or release goes over."
                ),
            ]
        );
        let kept = |app: &Repos| app.service_facts()[3].words.clone();
        // No remote of that name: the second sentence is left out.
        let remotes = std::mem::take(&mut app.service_remotes);
        assert_eq!(
            kept(&app),
            "This checkout keeps its files and origin, and keeps the address as the remote mirror."
        );
        app.service_remotes = remotes;
        // Without a usable address, the remote is not named yet.
        app.service_prompt.as_mut().unwrap().input = "nowhere".into();
        assert_eq!(
            kept(&app),
            "This checkout keeps its files and origin, and keeps the address as one more remote, named after its host."
        );
        app.service_prompt.as_mut().unwrap().input = address.clone();

        // The push the dialog's Enter runs.
        let done = gitops::migrate_to(&clone, &address, None, |_, _| {}).unwrap();
        // 1. Every branch and tag; origin's where the one here is behind it or
        // missing, else the one here: ahead (main) or gone another way (split).
        assert_eq!(
            tip(&target, "refs/heads/main"),
            tip(&clone, "refs/heads/main")
        );
        assert_eq!(
            tip(&target, "refs/heads/split"),
            tip(&clone, "refs/heads/split")
        );
        assert_ne!(
            tip(&clone, "refs/heads/split"),
            tip(&clone, "refs/remotes/origin/split")
        );
        assert_eq!(
            tip(&target, "refs/heads/mine"),
            tip(&clone, "refs/heads/mine")
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
        // 2. Origin was fetched first: what it got after the clone went too.
        assert!(done.stale.is_none());
        assert_eq!(
            tip(&target, "refs/heads/late"),
            tip(&upstream, "refs/heads/late")
        );
        // 3. Open and closed pull requests as branches pull/<number>; the
        // pull/1 that was there is replaced.
        assert_eq!(done.pull_prefix, "pull");
        assert_eq!(done.pulls, 2);
        for number in ["1", "2"] {
            assert_eq!(
                tip(&target, &format!("refs/heads/pull/{number}")),
                tip(&upstream, &format!("refs/pull/{number}/head")),
                "pull {number}"
            );
        }
        // 4. Nothing else there was overwritten or deleted.
        assert_eq!(tip(&target, "refs/heads/theirs"), theirs);
        // 6. Only branches and tags went over; here, the files, the branch,
        // origin, and the address as "mirror".
        for name in refs(&target) {
            assert!(
                name.starts_with("refs/heads/") || name.starts_with("refs/tags/"),
                "{name}"
            );
        }
        assert!(clone.join("local").is_file() && clone.join("one").is_file());
        assert_eq!(tip(&clone, "HEAD"), tip(&clone, "refs/heads/main"));
        let saved = config();
        assert!(
            saved.contains("[remote \"origin\"]") && saved.contains("upstream"),
            "{saved}"
        );
        assert_eq!(done.remote, "mirror");
        assert!(saved.contains("[remote \"mirror\"]"), "{saved}");
        // 4. The remote of that name that was here points at the address now;
        // it was not added twice.
        let remotes = gitops::remote_urls(&clone);
        assert!(
            remotes.contains(&("mirror".to_string(), address.clone())),
            "{remotes:?}"
        );
        assert_eq!(saved.matches("[remote \"mirror\"]").count(), 1, "{saved}");
        app.service_remotes = remotes;
        assert_eq!(
            kept(&app),
            "This checkout keeps its files and origin, and keeps the address as the remote mirror.",
            "a remote at the address already is not repointed"
        );

        // 3. Replaced on every push, though that is no fast-forward.
        git(&upstream, &["checkout", "-q", "--detach", "main"]);
        commit(&upstream, "pull-1-again");
        git(&upstream, &["update-ref", "refs/pull/1/head", "HEAD"]);
        git(&upstream, &["checkout", "-q", "main"]);
        gitops::migrate_to(&clone, &address, None, |_, _| {}).unwrap();
        assert_eq!(
            tip(&target, "refs/heads/pull/1"),
            tip(&upstream, "refs/pull/1/head")
        );

        // 2. Origin cannot be reached: what is saved here goes.
        gitops::remove_tree(&upstream).unwrap();
        commit(&clone, "offline");
        let done = gitops::migrate_to(&clone, &address, None, |_, _| {}).unwrap();
        assert!(done.stale.is_some());
        assert_eq!(
            tip(&target, "refs/heads/main"),
            tip(&clone, "refs/heads/main")
        );

        // 5. A tag there that went another way stops the push, and stays.
        let main = tip(&clone, "refs/heads/main");
        git(
            &clone,
            &["push", "-q", "--force", &address, "main:refs/tags/v1"],
        );
        assert!(gitops::migrate_to(&clone, &address, None, |_, _| {}).is_err());
        assert_eq!(tip(&target, "refs/tags/v1^{commit}"), main);
        git(
            &clone,
            &[
                "push",
                "-q",
                "--force",
                &address,
                "refs/tags/v1:refs/tags/v1",
            ],
        );

        // 5. So does a branch there that went another way; it stays as it
        // was, and the address is not kept.
        git(&clone, &["remote", "remove", "mirror"]);
        git(&clone, &["checkout", "-q", "-b", "side", "v1"]);
        commit(&clone, "elsewhere");
        git(
            &clone,
            &["push", "-q", "--force", &address, "side:refs/heads/dev"],
        );
        git(&clone, &["checkout", "-q", "main"]);
        git(&clone, &["branch", "-q", "-D", "side"]);
        let moved = tip(&target, "refs/heads/dev");
        assert!(gitops::migrate_to(&clone, &address, None, |_, _| {}).is_err());
        assert_eq!(tip(&target, "refs/heads/dev"), moved);
        assert!(!config().contains("[remote \"mirror\"]"), "{}", config());

        // 4, 5. A branch there that is only ahead of the pushed one stops the
        // push too; it only moves forward, so it keeps its commits.
        git(
            &clone,
            &[
                "push",
                "-q",
                "--force",
                &address,
                "refs/remotes/origin/dev:refs/heads/dev",
            ],
        );
        git(&clone, &["checkout", "-q", "-b", "further", "main"]);
        commit(&clone, "there-only");
        git(&clone, &["push", "-q", &address, "further:refs/heads/main"]);
        git(&clone, &["checkout", "-q", "main"]);
        git(&clone, &["branch", "-q", "-D", "further"]);
        let ahead = tip(&target, "refs/heads/main");
        assert_ne!(ahead, tip(&clone, "refs/heads/main"));
        assert!(gitops::migrate_to(&clone, &address, None, |_, _| {}).is_err());
        assert_eq!(tip(&target, "refs/heads/main"), ahead);
        assert!(!config().contains("[remote \"mirror\"]"), "{}", config());
    }
}
