//! What the Docker tool knows and what it does. The keys that drive it
//! are the interface's.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::actions::{self, Choice, Confirm, Request, Verb};
use super::cli::{self, Failure};
use super::logbuf::LogBuffer;
use super::model::{self, Container, Image, Lists, Project, Volume};
use crate::list;
use crate::sys::clipboard;
use crate::sys::process::Stream;
use crate::task::worker::Worker;

/// The tabs, in the order they are shown.
pub const TABS: [&str; 4] = ["Containers", "Compose", "Images", "Volumes"];
pub const CONTAINERS: usize = 0;
pub const COMPOSE: usize = 1;
pub const IMAGES: usize = 2;
pub const VOLUMES: usize = 3;

/// A row name that starts with this is the header of a Compose project
/// in the Containers tab. Nothing follows for containers in no project.
pub const GROUP_KEY: &str = "group::";

/// Events that come close together cause one reload, this long after the
/// last one.
const RELOAD_PAUSE: Duration = Duration::from_millis(500);

/// `docker events` that ends sooner than this after it started is not
/// started again by itself: something refuses it, and trying again would
/// only do the same twice a second.
const EVENTS_MIN_LIFE: Duration = Duration::from_secs(10);

/// A load that has taken this long may be given up for a new one with `r`.
const LOAD_PATIENCE: Duration = Duration::from_secs(5);

/// Whether Docker can be reached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The tool has not been opened yet: nothing was asked.
    Idle,
    /// The first answer is on its way.
    Loading,
    Ready,
    /// There is no `docker` program.
    Missing,
    /// Docker is installed and did not answer; this is what it said.
    Down(String),
}

/// One tab's list: where the highlight is, what is ticked, the filter.
#[derive(Default)]
pub struct View {
    pub selected: Option<String>,
    pub marked: Vec<String>,
    pub filter: String,
    pub scroll: usize,
}

/// What a button does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Answer(Choice),
    /// The tick box of a confirmation.
    Tick,
    CloseDialog,
    Toggle,
    Restart,
    Logs,
    Remove,
    Prune,
    Reload,
    Quit,
}

/// The dialog over the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    None,
    /// The actions for what is chosen, opened with Enter.
    Menu {
        verbs: Vec<Verb>,
        selected: usize,
    },
    Confirm(Confirm),
    /// Something failed: what was tried, and what Docker said.
    Error {
        title: String,
        text: String,
    },
}

/// One line of a list.
pub enum Row<'a> {
    /// The header of a Compose project in the Containers tab.
    Group {
        name: &'a str,
        running: usize,
        total: usize,
        collapsed: bool,
    },
    Container(&'a Container),
    Project(&'a Project),
    Image(&'a Image),
    Volume(&'a Volume),
}

impl Row<'_> {
    pub fn key(&self) -> String {
        match self {
            Row::Group { name, .. } => format!("{GROUP_KEY}{name}"),
            Row::Container(container) => container.id.clone(),
            Row::Project(project) => project.name.clone(),
            Row::Image(image) => image.key(),
            Row::Volume(volume) => volume.name.clone(),
        }
    }
}

enum Msg {
    /// The answer to the load with this number.
    Loaded(u64, Result<Lists, Failure>),
    /// Docker reported a change: the lists are out of date.
    Stale,
    EventsEnded,
    /// The running action is over.
    Done(Result<(), String>),
    /// A line for the log view with this number.
    Log(u64, String),
    LogEnded(u64, Result<(), String>),
}

/// What the Docker tool knows, and what it does, whatever shows it.
pub struct DockerState {
    pub engine: Engine,
    pub lists: Lists,
    pub tab: usize,
    pub views: [View; 4],
    /// Projects folded shut in the Containers tab. Empty is "no project".
    pub collapsed: Vec<String>,
    /// The filter field has the keyboard.
    pub filtering: bool,
    pub dialog: Dialog,
    /// The open log view: a container's log, or an action's output.
    pub log: Option<LogBuffer>,
    /// The number of the open log view. Lines for an older one are dropped.
    log_id: u64,
    /// Feeds a container's or project's log. Dropping it stops `docker logs`.
    log_stream: Option<Stream>,
    /// Feeds the output of the running action. It outlives its log view.
    action_stream: Option<Stream>,
    /// The number of the log view that shows the running action's output.
    action_log: Option<u64>,
    /// What waits for the running action to finish, oldest first.
    pub queue: VecDeque<Request>,
    pub running: Option<Request>,
    pub status: String,
    pub status_error: bool,
    /// `docker events`, which says when the lists are out of date.
    events: Option<Stream>,
    events_started: Option<Instant>,
    /// `docker events` ended right after it started. The lists then only
    /// change when asked: after an action, on `r`, on coming on screen.
    events_refused: bool,
    loading: bool,
    /// The number of the load whose answer is awaited. An older answer is
    /// dropped.
    load_id: u64,
    loading_since: Option<Instant>,
    stale: bool,
    loaded_at: Option<Instant>,
    /// False in tests: nothing is started and nothing is copied.
    live: bool,
    worker: Worker<Msg>,
    pub tick: usize,
    pub should_quit: bool,
}

impl Default for DockerState {
    fn default() -> Self {
        Self::new()
    }
}

impl DockerState {
    /// The tool before it is first opened. It asks Docker nothing yet.
    pub fn new() -> Self {
        Self {
            engine: Engine::Idle,
            lists: Lists::default(),
            tab: CONTAINERS,
            views: Default::default(),
            collapsed: Vec::new(),
            filtering: false,
            dialog: Dialog::None,
            log: None,
            log_id: 0,
            log_stream: None,
            action_stream: None,
            action_log: None,
            queue: VecDeque::new(),
            running: None,
            status: String::new(),
            status_error: false,
            events: None,
            events_started: None,
            events_refused: false,
            loading: false,
            load_id: 0,
            loading_since: None,
            stale: false,
            loaded_at: None,
            live: true,
            worker: Worker::default(),
            tick: 0,
            should_quit: false,
        }
    }

    /// A tool that holds these lists and never runs `docker`, for tests.
    pub fn offline(lists: Lists) -> Self {
        let mut docker = Self::new();
        docker.live = false;
        docker.engine = Engine::Ready;
        docker.lists = lists;
        docker.ensure_selection();
        docker
    }

    /// The tool came on screen: load for the first time, or look again.
    pub fn shown(&mut self) {
        self.reload();
    }

    /// True while a dialog, the log view or the filter has the keyboard.
    pub fn modal(&self) -> bool {
        self.log.is_some() || self.dialog != Dialog::None || self.filtering
    }

    pub fn waiting(&self) -> bool {
        self.loading || self.stale || self.running.is_some()
    }

    // ---- loading ----------------------------------------------------

    /// Asks Docker for all four lists, on a thread.
    fn reload(&mut self) {
        if !self.live {
            return;
        }
        if self.loading {
            self.stale = true;
            return;
        }
        self.loading = true;
        self.loading_since = Some(Instant::now());
        self.load_id += 1;
        self.stale = false;
        if self.engine != Engine::Ready {
            self.engine = Engine::Loading;
        }
        let id = self.load_id;
        self.worker.spawn(move |tx| {
            let lists = cli::output(&cli::ps()).map(|containers| {
                // Without containers there is no engine. The other lists
                // may be missing, as Compose is when it is not installed.
                let read = |arguments: Vec<String>| cli::output(&arguments).unwrap_or_default();
                Lists {
                    containers: model::parse_containers(&containers),
                    projects: model::parse_projects(&read(cli::projects())),
                    images: model::parse_images(&read(cli::images())),
                    volumes: model::parse_volumes(&read(cli::volumes())),
                }
            });
            let _ = tx.send(Msg::Loaded(id, lists));
        });
    }

    fn watch_events(&mut self) {
        if self.events.is_some() || self.events_refused || !self.live {
            return;
        }
        self.events_started = Some(Instant::now());
        let lines = self.worker.sender();
        let end = self.worker.sender();
        self.events = Stream::start(
            cli::command(&cli::events()),
            move |line| {
                if model::event_matters(line) {
                    let _ = lines.send(Msg::Stale);
                }
            },
            move |_| {
                let _ = end.send(Msg::EventsEnded);
            },
        )
        .ok();
    }

    /// Takes in what the threads reported and starts what is due. True
    /// when something changed.
    pub fn drain(&mut self) -> bool {
        let messages = self.worker.drain();
        let mut changed = !messages.is_empty();
        for message in messages {
            self.on_msg(message);
        }
        let rested = self.loaded_at.is_none_or(|at| at.elapsed() >= RELOAD_PAUSE);
        if self.stale && !self.loading && rested {
            self.reload();
            changed = true;
        }
        changed
    }

    fn on_msg(&mut self, message: Msg) {
        match message {
            Msg::Loaded(id, result) => {
                if id != self.load_id {
                    // A load that was given up for a newer one.
                    return;
                }
                self.loading = false;
                self.loading_since = None;
                self.loaded_at = Some(Instant::now());
                match result {
                    Ok(lists) => {
                        let before = self.shown_keys();
                        self.lists = lists;
                        self.engine = Engine::Ready;
                        self.settle(&before);
                        self.watch_events();
                    }
                    Err(Failure::Missing) => self.unreachable(Engine::Missing),
                    Err(Failure::Failed(text)) => self.unreachable(Engine::Down(text)),
                }
            }
            Msg::Stale => self.stale = true,
            Msg::EventsEnded => {
                // The engine stopped or restarted: find out which. If the
                // stream had only just started, it is not started again.
                self.events = None;
                self.events_refused = self
                    .events_started
                    .is_some_and(|at| at.elapsed() < EVENTS_MIN_LIFE);
                self.stale = true;
            }
            Msg::Done(result) => self.finished(result),
            Msg::Log(id, line) => {
                if id == self.log_id
                    && let Some(log) = self.log.as_mut()
                {
                    log.push(&line);
                }
            }
            Msg::LogEnded(id, result) => {
                if id == self.log_id
                    && let Some(log) = self.log.as_mut()
                {
                    log.end(match result {
                        Ok(()) => "ended".to_string(),
                        Err(text) => text,
                    });
                }
            }
        }
    }

    fn unreachable(&mut self, engine: Engine) {
        self.engine = engine;
        self.lists = Lists::default();
        self.events = None;
        self.stale = false;
        let before = self.shown_keys();
        self.settle(&before);
    }

    /// The keys of every tab's rows, as they are shown now.
    fn shown_keys(&self) -> Vec<Vec<String>> {
        (0..TABS.len()).map(|tab| self.keys_of(tab)).collect()
    }

    /// After the lists changed: forget ticks on what is gone, and keep
    /// the highlight where it was. When its row is gone, the row that
    /// took its place gets it, not the top of the list. `before` holds
    /// each tab's rows from before the change.
    fn settle(&mut self, before: &[Vec<String>]) {
        for tab in 0..TABS.len() {
            let existing = self.all_keys(tab);
            let keys = self.keys_of(tab);
            let view = &mut self.views[tab];
            view.marked.retain(|key| existing.contains(key));
            let kept = view
                .selected
                .as_ref()
                .is_some_and(|selected| keys.contains(selected));
            if kept {
                continue;
            }
            let place = view
                .selected
                .as_ref()
                .and_then(|selected| before.get(tab)?.iter().position(|key| key == selected))
                .unwrap_or(0);
            view.selected = keys.get(place.min(keys.len().saturating_sub(1))).cloned();
        }
    }

    pub fn ensure_selection(&mut self) {
        let keys = self.keys_of(self.tab);
        list::snap(&mut self.views[self.tab].selected, &keys);
    }

    // ---- rows -------------------------------------------------------

    pub fn view(&self) -> &View {
        &self.views[self.tab]
    }

    pub fn view_mut(&mut self) -> &mut View {
        &mut self.views[self.tab]
    }

    /// The lines of the tab on screen.
    pub fn rows(&self) -> Vec<Row<'_>> {
        self.rows_of(self.tab)
    }

    fn keys_of(&self, tab: usize) -> Vec<String> {
        self.rows_of(tab).iter().map(Row::key).collect()
    }

    /// Whether the tab's filter lets through something with these fields.
    fn lets(&self, tab: usize, fields: &[&str]) -> bool {
        let filter = self.views[tab].filter.trim().to_lowercase();
        filter.is_empty()
            || fields
                .iter()
                .any(|field| field.to_lowercase().contains(&filter))
    }

    /// The containers the filter lets through, folded away or not.
    fn reachable_containers(&self) -> Vec<&Container> {
        self.lists
            .containers
            .iter()
            .filter(|c| {
                self.lets(
                    CONTAINERS,
                    &[&c.name, &c.image, &c.project, &c.status, &c.id],
                )
            })
            .collect()
    }

    /// The containers of a project that the filter lets through. A
    /// project's header stands for these, never for hidden ones.
    fn members(&self, project: &str) -> Vec<String> {
        self.reachable_containers()
            .into_iter()
            .filter(|c| c.project == project)
            .map(|c| c.id.clone())
            .collect()
    }

    /// Every key the tab's filter lets through. In the Containers tab
    /// that includes containers of a folded project.
    fn reachable_keys(&self, tab: usize) -> Vec<String> {
        if tab == CONTAINERS {
            self.reachable_containers()
                .into_iter()
                .map(|c| c.id.clone())
                .collect()
        } else {
            self.keys_of(tab)
        }
    }

    /// Every key the tab has, whatever the filter.
    fn all_keys(&self, tab: usize) -> Vec<String> {
        let lists = &self.lists;
        match tab {
            CONTAINERS => lists.containers.iter().map(|c| c.id.clone()).collect(),
            COMPOSE => lists.projects.iter().map(|p| p.name.clone()).collect(),
            IMAGES => lists.images.iter().map(Image::key).collect(),
            _ => lists.volumes.iter().map(|v| v.name.clone()).collect(),
        }
    }

    fn rows_of(&self, tab: usize) -> Vec<Row<'_>> {
        let filter = self.views[tab].filter.trim().to_lowercase();
        let lets = |fields: &[&str]| self.lets(tab, fields);
        match tab {
            CONTAINERS => {
                let shown = self.reachable_containers();
                let grouped = shown.iter().any(|c| !c.project.is_empty());
                let mut rows = Vec::new();
                let mut current: Option<&str> = None;
                for container in shown.iter().copied() {
                    let project = container.project.as_str();
                    if grouped && current != Some(project) {
                        current = Some(project);
                        let members = || shown.iter().filter(|c| c.project == project);
                        rows.push(Row::Group {
                            name: project,
                            running: members().filter(|c| c.running()).count(),
                            total: members().count(),
                            // A filter looks inside folded projects too.
                            collapsed: filter.is_empty() && self.is_collapsed(project),
                        });
                    }
                    if filter.is_empty() && grouped && self.is_collapsed(project) {
                        continue;
                    }
                    rows.push(Row::Container(container));
                }
                rows
            }
            COMPOSE => self
                .lists
                .projects
                .iter()
                .filter(|p| lets(&[&p.name, &p.status]))
                .map(Row::Project)
                .collect(),
            IMAGES => self
                .lists
                .images
                .iter()
                .filter(|i| lets(&[&i.name(), &i.id]))
                .map(Row::Image)
                .collect(),
            _ => self
                .lists
                .volumes
                .iter()
                .filter(|v| lets(&[&v.name, &v.driver]))
                .map(Row::Volume)
                .collect(),
        }
    }

    fn is_collapsed(&self, project: &str) -> bool {
        self.collapsed.iter().any(|name| name == project)
    }

    /// The project whose header is highlighted, if one is.
    pub fn selected_group(&self) -> Option<&str> {
        if self.tab != CONTAINERS {
            return None;
        }
        self.view().selected.as_deref()?.strip_prefix(GROUP_KEY)
    }

    /// What an action is about: the ticked rows, or else the highlighted
    /// one. A project's header stands for its containers. Only what the
    /// filter lets through counts: nothing acts on a row that cannot be
    /// seen.
    fn chosen_keys(&self) -> Vec<String> {
        let view = self.view();
        if !view.marked.is_empty() {
            let reachable = self.reachable_keys(self.tab);
            return view
                .marked
                .iter()
                .filter(|key| reachable.contains(key))
                .cloned()
                .collect();
        }
        let Some(selected) = view.selected.as_ref() else {
            return Vec::new();
        };
        // The highlight can be left on a row a filter has since hidden.
        if !self.keys_of(self.tab).contains(selected) {
            return Vec::new();
        }
        match self.selected_group() {
            Some(group) => self.members(group),
            None => vec![selected.clone()],
        }
    }

    pub fn chosen_count(&self) -> usize {
        self.chosen_keys().len()
    }

    fn chosen_containers(&self) -> Vec<&Container> {
        let keys = self.chosen_keys();
        self.lists
            .containers
            .iter()
            .filter(|c| keys.contains(&c.id))
            .collect()
    }

    fn chosen_projects(&self) -> Vec<&Project> {
        let keys = self.chosen_keys();
        self.lists
            .projects
            .iter()
            .filter(|p| keys.contains(&p.name))
            .collect()
    }

    fn chosen_images(&self) -> Vec<&Image> {
        let keys = self.chosen_keys();
        self.lists
            .images
            .iter()
            .filter(|i| keys.contains(&i.key()))
            .collect()
    }

    fn chosen_volumes(&self) -> Vec<&Volume> {
        let keys = self.chosen_keys();
        self.lists
            .volumes
            .iter()
            .filter(|v| keys.contains(&v.name))
            .collect()
    }

    // ---- events -----------------------------------------------------

    pub fn paste(&mut self, text: &str) {
        let text: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if let Some(log) = self.log.as_mut() {
            if log.typing() {
                log.paste(&text);
            }
        } else if self.filtering {
            self.view_mut().filter.push_str(&text);
            self.filter_changed();
        }
    }

    /// `/`: opens the filter field, empty.
    pub fn start_filter(&mut self) {
        self.filtering = true;
        self.view_mut().filter.clear();
        self.filter_changed();
    }

    /// Esc in the list: drops the ticks, or else the filter.
    pub fn clear_ticks_or_filter(&mut self) {
        let view = self.view_mut();
        if !view.marked.is_empty() {
            view.marked.clear();
        } else if !view.filter.is_empty() {
            view.filter.clear();
            self.filter_changed();
        }
    }

    /// Esc in the filter field: leaves it and drops the filter.
    pub fn drop_filter(&mut self) {
        self.filtering = false;
        self.view_mut().filter.clear();
        self.filter_changed();
    }

    pub fn filter_changed(&mut self) {
        self.view_mut().scroll = 0;
        self.ensure_selection();
    }

    pub fn move_by(&mut self, delta: isize) {
        let keys = self.keys_of(self.tab);
        list::move_by(&mut self.view_mut().selected, &keys, delta);
    }

    pub fn move_to_end(&mut self, start: bool) {
        let keys = self.keys_of(self.tab);
        list::move_to_end(&mut self.view_mut().selected, &keys, start);
    }

    /// Space: ticks the row and moves down. On a project's header, ticks
    /// or unticks all of its containers.
    pub fn toggle_mark(&mut self) {
        if let Some(group) = self.selected_group().map(str::to_string) {
            let members = self.members(&group);
            let view = self.view_mut();
            if members.iter().all(|id| view.marked.contains(id)) {
                view.marked.retain(|id| !members.contains(id));
            } else {
                for id in members {
                    if !view.marked.contains(&id) {
                        view.marked.push(id);
                    }
                }
            }
            return;
        }
        let Some(key) = self.view().selected.clone() else {
            return;
        };
        let view = self.view_mut();
        match view.marked.iter().position(|marked| *marked == key) {
            Some(index) => {
                view.marked.remove(index);
            }
            None => view.marked.push(key),
        }
        self.move_by(1);
    }

    /// Ctrl-A: ticks everything the list shows.
    pub fn mark_all(&mut self) {
        let keys: Vec<String> = self
            .keys_of(self.tab)
            .into_iter()
            .filter(|key| !key.starts_with(GROUP_KEY))
            .collect();
        self.view_mut().marked = keys;
    }

    pub fn enter_row(&mut self) {
        match self.selected_group().map(str::to_string) {
            Some(group) if self.view().marked.is_empty() => self.toggle_group(&group),
            _ => self.open_menu(),
        }
    }

    fn toggle_group(&mut self, group: &str) {
        match self.collapsed.iter().position(|name| name == group) {
            Some(index) => {
                self.collapsed.remove(index);
            }
            None => self.collapsed.push(group.to_string()),
        }
    }

    pub fn fold_selected(&mut self, fold: bool) {
        if let Some(group) = self.selected_group().map(str::to_string)
            && self.is_collapsed(&group) != fold
        {
            self.toggle_group(&group);
        }
    }

    /// `r`: asks Docker again, and gives `docker events` another chance.
    /// A load that has hung is given up for a new one.
    pub fn refresh(&mut self) {
        self.notice("Asking Docker…", false);
        self.events_refused = false;
        if self.loading
            && self
                .loading_since
                .is_some_and(|since| since.elapsed() >= LOAD_PATIENCE)
        {
            self.loading = false;
        }
        self.reload();
    }

    fn notice(&mut self, text: impl Into<String>, error: bool) {
        self.status = text.into();
        self.status_error = error;
    }

    // ---- the actions menu -------------------------------------------

    fn open_menu(&mut self) {
        if self.chosen_keys().is_empty() {
            return;
        }
        let verbs: &[Verb] = match self.tab {
            CONTAINERS => &actions::CONTAINER_MENU,
            COMPOSE => &actions::PROJECT_MENU,
            _ => &actions::OBJECT_MENU,
        };
        self.dialog = Dialog::Menu {
            verbs: verbs.to_vec(),
            selected: 0,
        };
    }

    /// A line of the menu was chosen: closes the menu and does what it says.
    pub fn run_from_menu(&mut self, verb: Verb) {
        self.dialog = Dialog::None;
        self.run_verb(verb);
    }

    /// Does what a line of the menu, or its key in the list, says.
    pub fn run_verb(&mut self, verb: Verb) {
        match (self.tab, verb) {
            (CONTAINERS, Verb::Start) => {
                let request = actions::start(&self.chosen_containers());
                self.submit_or(request, "Nothing to start: it is running.");
            }
            (CONTAINERS, Verb::Stop) => {
                let request = actions::stop(&self.chosen_containers());
                self.submit_or(request, "Nothing to stop: it is not running.");
            }
            (CONTAINERS, Verb::Restart) => {
                let request = actions::restart(&self.chosen_containers());
                self.submit_or(request, "Nothing is selected.");
            }
            (CONTAINERS, Verb::Logs) => {
                let found = self
                    .chosen_containers()
                    .first()
                    .map(|c| (format!("logs · {}", c.name), cli::logs(&c.id)));
                if let Some((title, arguments)) = found {
                    self.open_log(title, arguments);
                }
            }
            (COMPOSE, Verb::Start) => {
                let requests: Vec<Request> = self
                    .chosen_projects()
                    .into_iter()
                    .map(actions::project_start)
                    .collect();
                self.submit_all(requests);
            }
            (COMPOSE, Verb::Stop) => {
                let requests: Vec<Request> = self
                    .chosen_projects()
                    .into_iter()
                    .map(actions::project_stop)
                    .collect();
                self.submit_all(requests);
            }
            (COMPOSE, Verb::Restart) => {
                let requests: Vec<Request> = self
                    .chosen_projects()
                    .into_iter()
                    .map(actions::project_restart)
                    .collect();
                self.submit_all(requests);
            }
            (COMPOSE, Verb::Logs) => {
                let found = self.chosen_projects().first().map(|p| {
                    (
                        format!("logs · project {}", p.name),
                        cli::project_logs(&p.name),
                    )
                });
                if let Some((title, arguments)) = found {
                    self.open_log(title, arguments);
                }
            }
            (COMPOSE, Verb::Down) => self.remove(),
            (_, Verb::Remove) => self.remove(),
            (_, Verb::CopyName) => self.copy_names(),
            _ => {}
        }
    }

    /// `s`: starts what is stopped, or stops what runs.
    pub fn toggle(&mut self) {
        match self.tab {
            CONTAINERS => {
                let request = actions::toggle(&self.chosen_containers());
                self.submit_or(request, "Nothing is selected.");
            }
            COMPOSE => {
                let requests: Vec<Request> = self
                    .chosen_projects()
                    .into_iter()
                    .map(actions::project_toggle)
                    .collect();
                self.submit_all(requests);
            }
            _ => self.notice("Images and volumes are not started or stopped.", false),
        }
    }

    /// `d`: asks before removing what is chosen.
    pub fn remove(&mut self) {
        if self.chosen_keys().is_empty() {
            return;
        }
        let confirm = match self.tab {
            CONTAINERS => Confirm::containers(&self.chosen_containers()),
            COMPOSE => {
                if self.chosen_projects().len() > 1 {
                    self.notice(
                        "Projects are taken down one at a time: clear the selection, then d.",
                        false,
                    );
                    return;
                }
                let down = self
                    .chosen_projects()
                    .first()
                    .map(|project| Confirm::down(project, &self.lists.containers));
                match down {
                    Some(confirm) => confirm,
                    None => return,
                }
            }
            IMAGES => Confirm::images(&self.chosen_images(), &self.lists.containers),
            _ => Confirm::volumes(&self.chosen_volumes(), &self.lists.containers),
        };
        self.dialog = Dialog::Confirm(confirm);
    }

    /// `p`: asks before removing everything unused in this tab.
    pub fn prune(&mut self) {
        let confirm = match self.tab {
            CONTAINERS => Confirm::prune_containers(&self.lists.containers),
            IMAGES => Confirm::prune_images(&self.lists.images, &self.lists.containers),
            VOLUMES => Confirm::prune_volumes(&self.lists.volumes, &self.lists.containers),
            _ => {
                self.notice(
                    "There is nothing to prune in Compose. d takes a project down.",
                    false,
                );
                return;
            }
        };
        self.dialog = Dialog::Confirm(confirm);
    }

    fn copy_names(&mut self) {
        let names: Vec<String> = match self.tab {
            CONTAINERS => self
                .chosen_containers()
                .iter()
                .map(|c| c.name.clone())
                .collect(),
            COMPOSE => self
                .chosen_projects()
                .iter()
                .map(|p| p.name.clone())
                .collect(),
            IMAGES => self.chosen_images().iter().map(|i| i.reference()).collect(),
            _ => self
                .chosen_volumes()
                .iter()
                .map(|v| v.name.clone())
                .collect(),
        };
        if names.is_empty() {
            return;
        }
        let copied = if self.live {
            clipboard::set_text(&names.join("\n"))
        } else {
            Ok(())
        };
        match copied {
            Ok(()) => self.notice(
                format!("Copied {}", actions::count(names.len(), "name")),
                false,
            ),
            Err(err) => self.notice(err, true),
        }
    }

    // ---- confirmations ----------------------------------------------

    /// A button of the confirmation was chosen.
    pub fn answer(&mut self, choice: Choice) {
        let Dialog::Confirm(confirm) = &self.dialog else {
            return;
        };
        if choice == Choice::Cancel {
            self.dialog = Dialog::None;
            return;
        }
        // A button the dialog does not have does nothing.
        if let Some(request) = confirm.request(choice) {
            self.dialog = Dialog::None;
            self.view_mut().marked.clear();
            self.submit(request);
        }
    }

    // ---- running things ---------------------------------------------

    fn submit_or(&mut self, request: Option<Request>, otherwise: &str) {
        match request {
            Some(request) => {
                self.view_mut().marked.clear();
                self.submit(request);
            }
            None => self.notice(otherwise, false),
        }
    }

    fn submit_all(&mut self, requests: Vec<Request>) {
        if !requests.is_empty() {
            self.view_mut().marked.clear();
        }
        for request in requests {
            self.submit(request);
        }
    }

    /// Runs the request now, or after what is running.
    fn submit(&mut self, request: Request) {
        self.queue.push_back(request);
        self.start_next();
    }

    fn start_next(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Some(request) = self.queue.pop_front() else {
            return;
        };
        self.notice(format!("{}…", request.doing), false);
        self.running = Some(request.clone());
        if !self.live {
            return;
        }
        if request.show_output {
            // Long output: show it as it comes. Closing the view leaves
            // the command running. A view or a dialog that is already open
            // is not pushed aside: the output is then not shown.
            let id = if self.log.is_none() && self.dialog == Dialog::None {
                self.new_log(request.doing.clone())
            } else {
                u64::MAX
            };
            self.action_log = Some(id);
            let lines = self.worker.sender();
            let end = self.worker.sender();
            let started = Stream::start(
                cli::command(&request.steps[0]),
                move |line| {
                    let _ = lines.send(Msg::Log(id, line.to_string()));
                },
                move |result| {
                    let _ = end.send(Msg::Done(result));
                },
            );
            match started {
                Ok(stream) => self.action_stream = Some(stream),
                Err(err) => self.finished(Err(Failure::from_run(err).text())),
            }
            return;
        }
        self.worker.spawn(move |tx| {
            let mut result = Ok(());
            for step in &request.steps {
                if let Err(failure) = cli::output(step) {
                    result = Err(failure.text());
                    break;
                }
            }
            let _ = tx.send(Msg::Done(result));
        });
    }

    /// The running action is over: say how it went and run the next.
    fn finished(&mut self, result: Result<(), String>) {
        self.action_stream = None;
        let Some(request) = self.running.take() else {
            return;
        };
        let watched = self.action_log.take() == Some(self.log_id) && self.log.is_some();
        match result {
            Ok(()) => {
                self.notice(request.done.clone(), false);
                if watched && let Some(log) = self.log.as_mut() {
                    log.end("done");
                }
            }
            Err(text) => {
                self.notice(format!("{} did not work: {text}", request.doing), true);
                if watched && let Some(log) = self.log.as_mut() {
                    log.end(text);
                } else {
                    self.dialog = Dialog::Error {
                        title: request.doing,
                        text,
                    };
                }
            }
        }
        self.start_next();
        self.reload();
    }

    // ---- logs -------------------------------------------------------

    /// Opens an empty log view and returns its number.
    fn new_log(&mut self, title: String) -> u64 {
        self.log_stream = None;
        self.log_id += 1;
        self.log = Some(LogBuffer::new(title));
        self.log_id
    }

    /// Opens the log view on a `docker` command that keeps printing.
    fn open_log(&mut self, title: String, arguments: Vec<String>) {
        let id = self.new_log(title);
        if !self.live {
            return;
        }
        let lines = self.worker.sender();
        let end = self.worker.sender();
        let started = Stream::start(
            cli::command(&arguments),
            move |line| {
                let _ = lines.send(Msg::Log(id, line.to_string()));
            },
            move |result| {
                let _ = end.send(Msg::LogEnded(id, result));
            },
        );
        match started {
            Ok(stream) => self.log_stream = Some(stream),
            Err(err) => {
                if let Some(log) = self.log.as_mut() {
                    log.end(Failure::from_run(err).text());
                }
            }
        }
    }

    /// Closes the log view and stops `docker logs`. An action whose
    /// output was shown goes on.
    pub fn close_log(&mut self) {
        self.log = None;
        self.log_stream = None;
    }

    // ---- the mouse --------------------------------------------------

    /// Goes to the tab with this number, out of the filter field.
    pub fn switch_tab(&mut self, index: usize) {
        self.tab = index;
        self.filtering = false;
        self.ensure_selection();
    }

    /// Highlights the row with this key. On a project's header it folds or
    /// unfolds the project; `double`, on any other row, opens the actions.
    pub fn select_row(&mut self, name: String, double: bool) {
        self.view_mut().selected = Some(name.clone());
        // A click on a project's header folds or unfolds it.
        if let Some(group) = name.strip_prefix(GROUP_KEY) {
            self.toggle_group(group);
        } else if double {
            self.open_menu();
        }
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Answer(choice) => self.answer(choice),
            Action::Tick => {
                if let Dialog::Confirm(confirm) = &mut self.dialog {
                    confirm.toggle();
                }
            }
            Action::CloseDialog => self.dialog = Dialog::None,
            Action::Toggle => self.toggle(),
            Action::Restart => self.run_verb(Verb::Restart),
            Action::Logs => self.run_verb(Verb::Logs),
            Action::Remove => self.remove(),
            Action::Prune => self.prune(),
            Action::Reload => self.refresh(),
            Action::Quit => self.should_quit = true,
        }
    }
}

/// What tests in both crates use to feed the tool what its threads would
/// report, and to look at what it keeps to itself.
#[doc(hidden)]
pub mod testing {
    use super::*;

    pub const EVENTS_MIN_LIFE: Duration = super::EVENTS_MIN_LIFE;

    /// The running action is over.
    pub fn done(docker: &mut DockerState, result: Result<(), String>) {
        docker.on_msg(Msg::Done(result));
    }

    /// The answer to the load with this number.
    pub fn loaded(docker: &mut DockerState, id: u64, result: Result<Lists, Failure>) {
        docker.on_msg(Msg::Loaded(id, result));
    }

    /// `docker events` ended.
    pub fn events_ended(docker: &mut DockerState) {
        docker.on_msg(Msg::EventsEnded);
    }

    /// A line for the log view with this number.
    pub fn log_line(docker: &mut DockerState, id: u64, line: String) {
        docker.on_msg(Msg::Log(id, line));
    }

    /// The stream of the log view with this number ended.
    pub fn log_ended(docker: &mut DockerState, id: u64, result: Result<(), String>) {
        docker.on_msg(Msg::LogEnded(id, result));
    }

    /// The number of the open log view.
    pub fn log_id(docker: &DockerState) -> u64 {
        docker.log_id
    }

    pub fn set_events_started(docker: &mut DockerState, at: Instant) {
        docker.events_started = Some(at);
    }

    pub fn events_refused(docker: &DockerState) -> bool {
        docker.events_refused
    }

    pub fn stale(docker: &DockerState) -> bool {
        docker.stale
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::model::samples;

    fn docker() -> DockerState {
        DockerState::offline(samples::lists())
    }

    #[test]
    fn nothing_is_asked_of_docker_until_the_tool_is_opened() {
        let docker = DockerState::new();
        assert_eq!(docker.engine, Engine::Idle);
        assert!(!docker.waiting());
        assert!(!docker.modal());
    }

    #[test]
    fn an_answer_to_a_load_that_was_given_up_is_dropped() {
        let mut docker = docker();
        docker.load_id = 7;
        docker.loading = true;
        docker.on_msg(Msg::Loaded(6, Err(Failure::Missing)));
        assert_eq!(docker.engine, Engine::Ready);
        assert!(docker.loading);
        docker.on_msg(Msg::Loaded(7, Err(Failure::Missing)));
        assert_eq!(docker.engine, Engine::Missing);
        assert!(!docker.loading);
    }

    #[test]
    fn docker_missing_or_down_is_told_apart() {
        let mut docker = docker();
        docker.on_msg(Msg::Loaded(0, Err(Failure::Missing)));
        assert_eq!(docker.engine, Engine::Missing);
        assert!(docker.lists.containers.is_empty());
        docker.on_msg(Msg::Loaded(
            0,
            Err(Failure::Failed("pipe not found".into())),
        ));
        assert_eq!(docker.engine, Engine::Down("pipe not found".into()));
        docker.on_msg(Msg::Loaded(0, Ok(samples::lists())));
        assert_eq!(docker.engine, Engine::Ready);
        assert_eq!(docker.lists.containers.len(), 4);
    }
}
