//! What the Docker tool knows and what its keys do.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use super::actions::{self, Choice, Confirm, Request, Verb};
use super::cli::{self, Failure};
use super::model::{self, Container, Image, Lists, Project, Volume};
use crate::atoms::sys::clipboard;
use crate::atoms::sys::process::Stream;
use crate::atoms::task::worker::Worker;
use crate::atoms::ui::hits::{Hits, contains};
use crate::atoms::ui::input::{self, Alt};
use crate::atoms::ui::list;
use crate::atoms::ui::log::{self, LogView};
use crate::atoms::ui::menu;
use crate::atoms::ui::tabs;

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

pub struct Docker {
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
    pub log: Option<LogView>,
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
    pub hits: Hits<Action>,
    /// Where the tab bar drew each tab.
    pub tab_rects: Vec<Rect>,
    pub mouse: Option<(u16, u16)>,
    pub tick: usize,
    last_click: Option<(String, Instant)>,
    pub should_quit: bool,
}

impl Default for Docker {
    fn default() -> Self {
        Self::new()
    }
}

impl Docker {
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
            hits: Hits::default(),
            tab_rects: Vec::new(),
            mouse: None,
            tick: 0,
            last_click: None,
            should_quit: false,
        }
    }

    /// A tool that holds these lists and never runs `docker`, for tests.
    #[cfg(test)]
    pub fn for_test(lists: Lists) -> Self {
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

    fn view_mut(&mut self) -> &mut View {
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

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.paste(&text),
            Event::Mouse(mouse) => {
                self.mouse = Some((mouse.column, mouse.row));
                match mouse.kind {
                    // The shell passes on a release only for a click.
                    MouseEventKind::Up(MouseButton::Left) => self.click(mouse.column, mouse.row),
                    MouseEventKind::ScrollUp => self.wheel(-3),
                    MouseEventKind::ScrollDown => self.wheel(3),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn paste(&mut self, text: &str) {
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

    fn wheel(&mut self, delta: isize) {
        if let Some(log) = self.log.as_mut() {
            log.scroll_by(delta);
        } else if self.dialog == Dialog::None {
            self.move_by(delta);
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.should_quit = true;
            return;
        }
        if let Some(log) = self.log.as_mut() {
            if log.on_key(key) == log::Outcome::Close {
                self.close_log();
            }
            return;
        }
        match &self.dialog {
            Dialog::Menu { .. } => self.on_menu_key(key),
            Dialog::Confirm(_) => self.on_confirm_key(key),
            Dialog::Error { .. } => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Char(' ')
                ) {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::None if self.filtering => self.on_filter_key(key),
            Dialog::None => self.on_list_key(key, ctrl),
        }
    }

    fn on_list_key(&mut self, key: KeyEvent, ctrl: bool) {
        if tabs::on_key(&mut self.tab, key, TABS.len()) {
            self.ensure_selection();
            return;
        }
        match key.code {
            KeyCode::Char('a') | KeyCode::Char('A') if ctrl => self.mark_all(),
            KeyCode::Char(' ') => self.toggle_mark(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            KeyCode::Enter => self.enter_row(),
            KeyCode::Left => self.fold_selected(true),
            KeyCode::Right => self.fold_selected(false),
            KeyCode::Char('s') | KeyCode::Char('S') => self.toggle(),
            KeyCode::Char('R') => self.run_verb(Verb::Restart),
            KeyCode::Char('l') | KeyCode::Char('L') => self.run_verb(Verb::Logs),
            KeyCode::Char('d') | KeyCode::Delete => self.remove(),
            KeyCode::Char('p') | KeyCode::Char('P') => self.prune(),
            KeyCode::Char('r') | KeyCode::F(5) => self.refresh(),
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('/') => {
                self.filtering = true;
                self.view_mut().filter.clear();
                self.filter_changed();
            }
            KeyCode::Esc => {
                let view = self.view_mut();
                if !view.marked.is_empty() {
                    view.marked.clear();
                } else if !view.filter.is_empty() {
                    view.filter.clear();
                    self.filter_changed();
                }
            }
            _ => {}
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.filtering = false;
                self.view_mut().filter.clear();
                self.filter_changed();
            }
            KeyCode::Enter => self.filtering = false,
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            _ => {
                if input::edit(&mut self.view_mut().filter, key, Alt::Ignored) {
                    self.filter_changed();
                }
            }
        }
    }

    fn filter_changed(&mut self) {
        self.view_mut().scroll = 0;
        self.ensure_selection();
    }

    fn move_by(&mut self, delta: isize) {
        let keys = self.keys_of(self.tab);
        list::move_by(&mut self.view_mut().selected, &keys, delta);
    }

    fn move_to_end(&mut self, start: bool) {
        let keys = self.keys_of(self.tab);
        list::move_to_end(&mut self.view_mut().selected, &keys, start);
    }

    /// Space: ticks the row and moves down. On a project's header, ticks
    /// or unticks all of its containers.
    fn toggle_mark(&mut self) {
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
    fn mark_all(&mut self) {
        let keys: Vec<String> = self
            .keys_of(self.tab)
            .into_iter()
            .filter(|key| !key.starts_with(GROUP_KEY))
            .collect();
        self.view_mut().marked = keys;
    }

    fn enter_row(&mut self) {
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

    fn fold_selected(&mut self, fold: bool) {
        if let Some(group) = self.selected_group().map(str::to_string)
            && self.is_collapsed(&group) != fold
        {
            self.toggle_group(&group);
        }
    }

    /// `r`: asks Docker again, and gives `docker events` another chance.
    /// A load that has hung is given up for a new one.
    fn refresh(&mut self) {
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

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Dialog::Menu { verbs, selected } = &mut self.dialog else {
            return;
        };
        let keys: Vec<char> = verbs.iter().map(|verb| verb.key()).collect();
        match menu::on_key(selected, key, &keys) {
            menu::Outcome::Stay => {}
            menu::Outcome::Close => self.dialog = Dialog::None,
            menu::Outcome::Run(index) => {
                let verb = verbs[index];
                self.dialog = Dialog::None;
                self.run_verb(verb);
            }
        }
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
    fn toggle(&mut self) {
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
    fn remove(&mut self) {
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
    fn prune(&mut self) {
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

    fn on_confirm_key(&mut self, key: KeyEvent) {
        let Dialog::Confirm(confirm) = &mut self.dialog else {
            return;
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                self.dialog = Dialog::None;
            }
            KeyCode::Enter | KeyCode::Char('y') | KeyCode::Char('Y') => {
                let choice = confirm.default_choice();
                self.answer(choice);
            }
            KeyCode::Char('f') | KeyCode::Char('F') => self.answer(Choice::Force),
            KeyCode::Char(' ') => confirm.toggle(),
            _ => {}
        }
    }

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
        self.log = Some(LogView::new(title));
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
    fn close_log(&mut self) {
        self.log = None;
        self.log_stream = None;
    }

    // ---- the mouse --------------------------------------------------

    fn click(&mut self, x: u16, y: u16) {
        if self.log.is_some() {
            return;
        }
        if let Some(action) = self.hits.button_at(x, y) {
            self.run(action);
            return;
        }
        if self.dialog == Dialog::None
            && let Some(index) = self.tab_rects.iter().position(|rect| contains(*rect, x, y))
        {
            self.tab = index;
            self.filtering = false;
            self.ensure_selection();
            return;
        }
        if let Some(name) = self.hits.row_at(x, y) {
            self.click_row(name);
        }
    }

    fn click_row(&mut self, name: String) {
        if let Dialog::Menu { verbs, .. } = &self.dialog {
            if let Some(verb) = name
                .strip_prefix("menu:")
                .and_then(|index| index.parse::<usize>().ok())
                .and_then(|index| verbs.get(index).copied())
            {
                self.dialog = Dialog::None;
                self.run_verb(verb);
            }
            return;
        }
        if self.dialog != Dialog::None {
            return;
        }
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|(previous, at)| {
            *previous == name && now.duration_since(*at) < Duration::from_millis(400)
        });
        self.last_click = Some((name.clone(), now));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::docker::model::samples;

    fn docker() -> Docker {
        Docker::for_test(samples::lists())
    }

    fn press(docker: &mut Docker, code: KeyCode) {
        docker.on_event(Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    }

    fn type_text(docker: &mut Docker, text: &str) {
        for ch in text.chars() {
            press(docker, KeyCode::Char(ch));
        }
    }

    fn labels(docker: &Docker) -> Vec<String> {
        docker
            .rows()
            .iter()
            .map(|row| match row {
                Row::Group { name, .. } => format!("[{name}]"),
                Row::Container(container) => container.name.clone(),
                Row::Project(project) => project.name.clone(),
                Row::Image(image) => image.name(),
                Row::Volume(volume) => volume.label(),
            })
            .collect()
    }

    fn selected_name(docker: &Docker) -> String {
        let key = docker.view().selected.clone().unwrap_or_default();
        docker
            .lists
            .containers
            .iter()
            .find(|c| c.id == key)
            .map_or(key, |c| c.name.clone())
    }

    #[test]
    fn nothing_is_asked_of_docker_until_the_tool_is_opened() {
        let docker = Docker::new();
        assert_eq!(docker.engine, Engine::Idle);
        assert!(!docker.waiting());
        assert!(!docker.modal());
    }

    #[test]
    fn containers_are_grouped_by_project_and_groups_fold() {
        let mut docker = docker();
        assert_eq!(
            labels(&docker),
            [
                "[shop]",
                "shop-admin",
                "shop-postgres",
                "shop-web",
                "[]",
                "backups"
            ]
        );
        // The highlight starts on the first project's header: Enter folds it.
        assert_eq!(docker.selected_group(), Some("shop"));
        press(&mut docker, KeyCode::Enter);
        assert_eq!(labels(&docker), ["[shop]", "[]", "backups"]);
        // A filter looks inside a folded project.
        press(&mut docker, KeyCode::Char('/'));
        type_text(&mut docker, "postgres");
        assert!(docker.modal());
        assert_eq!(labels(&docker), ["[shop]", "shop-postgres"]);
        press(&mut docker, KeyCode::Esc);
        assert!(!docker.modal());
        assert_eq!(labels(&docker), ["[shop]", "[]", "backups"]);
        press(&mut docker, KeyCode::Right);
        assert_eq!(labels(&docker).len(), 6);
    }

    #[test]
    fn tab_walks_the_four_lists() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Tab);
        assert_eq!(docker.tab, COMPOSE);
        assert_eq!(labels(&docker), ["blog", "shop"]);
        press(&mut docker, KeyCode::Tab);
        assert_eq!(docker.tab, IMAGES);
        assert_eq!(labels(&docker).len(), 4);
        press(&mut docker, KeyCode::Tab);
        assert_eq!(docker.tab, VOLUMES);
        assert_eq!(labels(&docker)[0], "old_cache");
        press(&mut docker, KeyCode::Tab);
        assert_eq!(docker.tab, CONTAINERS);
        press(&mut docker, KeyCode::BackTab);
        assert_eq!(docker.tab, VOLUMES);
    }

    #[test]
    fn s_starts_what_is_stopped_and_stops_what_runs() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        assert_eq!(selected_name(&docker), "shop-admin");
        press(&mut docker, KeyCode::Char('s'));
        let running = docker.running.clone().unwrap();
        assert_eq!(running.doing, "Starting shop-admin");
        assert_eq!(docker.status, "Starting shop-admin…");
        assert!(docker.waiting());

        // What is asked for meanwhile waits its turn.
        press(&mut docker, KeyCode::Down);
        press(&mut docker, KeyCode::Down);
        assert_eq!(selected_name(&docker), "shop-web");
        press(&mut docker, KeyCode::Char('s'));
        assert_eq!(docker.queue.len(), 1);
        assert_eq!(docker.queue[0].doing, "Stopping shop-web");

        docker.on_msg(Msg::Done(Ok(())));
        assert_eq!(docker.running.clone().unwrap().doing, "Stopping shop-web");
        docker.on_msg(Msg::Done(Err("no such container".into())));
        assert!(docker.running.is_none());
        assert!(docker.status_error);
        assert_eq!(
            docker.dialog,
            Dialog::Error {
                title: "Stopping shop-web".into(),
                text: "no such container".into()
            }
        );
        press(&mut docker, KeyCode::Enter);
        assert_eq!(docker.dialog, Dialog::None);
    }

    #[test]
    fn a_projects_header_stands_for_its_containers() {
        let mut docker = docker();
        assert_eq!(docker.chosen_count(), 3);
        press(&mut docker, KeyCode::Char('R'));
        assert_eq!(
            docker.running.clone().unwrap().doing,
            "Restarting 3 containers"
        );
    }

    #[test]
    fn removing_asks_first_and_force_is_its_own_key() {
        let mut docker = docker();
        // Tick everything: two run, two do not.
        docker.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        assert_eq!(docker.view().marked.len(), 4);
        press(&mut docker, KeyCode::Char('d'));
        assert!(matches!(
            docker.dialog,
            Dialog::Confirm(Confirm::Containers { .. })
        ));
        assert!(docker.running.is_none());

        // Esc leaves everything as it was.
        press(&mut docker, KeyCode::Esc);
        assert_eq!(docker.dialog, Dialog::None);
        assert!(docker.running.is_none());
        assert_eq!(docker.view().marked.len(), 4);

        // Enter removes the stopped ones only.
        press(&mut docker, KeyCode::Char('d'));
        press(&mut docker, KeyCode::Enter);
        let request = docker.running.clone().unwrap();
        assert_eq!(request.steps[0][0], "rm");
        assert_eq!(request.steps[0].len(), 3);
        assert!(docker.view().marked.is_empty());
        docker.on_msg(Msg::Done(Ok(())));

        // `f` stops and removes all of them.
        docker.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        press(&mut docker, KeyCode::Char('d'));
        press(&mut docker, KeyCode::Char('f'));
        let request = docker.running.clone().unwrap();
        assert_eq!(&request.steps[0][..2], ["rm", "--force"]);
    }

    #[test]
    fn enter_never_forces_and_f_does() {
        let mut docker = docker();
        // One running container: there is nothing to remove without force.
        press(&mut docker, KeyCode::End);
        press(&mut docker, KeyCode::Up);
        press(&mut docker, KeyCode::Up);
        assert_eq!(selected_name(&docker), "shop-web");
        press(&mut docker, KeyCode::Char('d'));
        assert!(matches!(docker.dialog, Dialog::Confirm(_)));
        press(&mut docker, KeyCode::Enter);
        assert_eq!(docker.dialog, Dialog::None);
        assert!(docker.running.is_none());
        press(&mut docker, KeyCode::Char('d'));
        press(&mut docker, KeyCode::Char('y'));
        assert!(docker.running.is_none());

        press(&mut docker, KeyCode::Char('d'));
        press(&mut docker, KeyCode::Char('f'));
        let request = docker.running.clone().unwrap();
        assert_eq!(&request.steps[0][..2], ["rm", "--force"]);
        assert_eq!(request.steps[0].len(), 3);
    }

    #[test]
    fn nothing_acts_on_what_the_filter_hides() {
        // The highlight is on a container; then a filter matches nothing.
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        assert_eq!(selected_name(&docker), "shop-admin");
        press(&mut docker, KeyCode::Char('/'));
        type_text(&mut docker, "zzz");
        press(&mut docker, KeyCode::Enter);
        assert!(labels(&docker).is_empty());
        press(&mut docker, KeyCode::Char('s'));
        press(&mut docker, KeyCode::Char('R'));
        assert!(docker.running.is_none());
        press(&mut docker, KeyCode::Char('d'));
        assert_eq!(docker.dialog, Dialog::None);
        press(&mut docker, KeyCode::Enter);
        assert_eq!(docker.dialog, Dialog::None);

        // A project's header under a filter stands for what is shown.
        let mut docker = self::docker();
        press(&mut docker, KeyCode::Char('/'));
        type_text(&mut docker, "web");
        press(&mut docker, KeyCode::Enter);
        assert_eq!(labels(&docker), ["[shop]", "shop-web"]);
        assert_eq!(docker.selected_group(), Some("shop"));
        assert_eq!(docker.chosen_count(), 1);
        press(&mut docker, KeyCode::Char('R'));
        assert_eq!(docker.running.clone().unwrap().doing, "Restarting shop-web");
        docker.on_msg(Msg::Done(Ok(())));
        // Space on that header ticks the one shown, not the hidden two.
        press(&mut docker, KeyCode::Char(' '));
        assert_eq!(docker.view().marked.len(), 1);

        // Ticks a later filter hides are kept, and are not acted on.
        let mut docker = self::docker();
        docker.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::CONTROL,
        )));
        press(&mut docker, KeyCode::Char('/'));
        type_text(&mut docker, "backups");
        press(&mut docker, KeyCode::Enter);
        assert_eq!(docker.view().marked.len(), 4);
        assert_eq!(docker.chosen_count(), 1);
        press(&mut docker, KeyCode::Char('d'));
        let Dialog::Confirm(Confirm::Containers { stopped, running }) = &docker.dialog else {
            panic!("no dialog");
        };
        assert_eq!(stopped.len(), 1);
        assert!(running.is_empty());
        // A reload does not drop the hidden ticks.
        press(&mut docker, KeyCode::Esc);
        docker.on_msg(Msg::Loaded(0, Ok(samples::lists())));
        assert_eq!(docker.view().marked.len(), 4);
    }

    #[test]
    fn the_highlight_stays_in_place_when_its_row_goes() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        press(&mut docker, KeyCode::Down);
        assert_eq!(selected_name(&docker), "shop-postgres");
        let mut lists = samples::lists();
        lists.containers.retain(|c| c.name != "shop-postgres");
        docker.on_msg(Msg::Loaded(0, Ok(lists)));
        // The row that moved up into its place, not the project's header.
        assert_eq!(selected_name(&docker), "shop-web");
        assert_eq!(docker.selected_group(), None);
    }

    #[test]
    fn events_that_end_at_once_are_not_started_again_until_asked() {
        let mut docker = docker();
        docker.events_started = Some(Instant::now());
        docker.on_msg(Msg::EventsEnded);
        assert!(docker.events_refused);
        // One look to see whether the engine is still there, and no more.
        assert!(docker.stale);
        // `r` gives it another chance.
        press(&mut docker, KeyCode::Char('r'));
        assert!(!docker.events_refused);

        // A stream that ran for a while and ended is started again.
        docker.events_started = Some(Instant::now() - EVENTS_MIN_LIFE);
        docker.on_msg(Msg::EventsEnded);
        assert!(!docker.events_refused);
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
    fn several_projects_are_not_taken_down_at_once() {
        let mut docker = docker();
        docker.tab = COMPOSE;
        docker.ensure_selection();
        press(&mut docker, KeyCode::Char(' '));
        press(&mut docker, KeyCode::Char(' '));
        assert_eq!(docker.view().marked.len(), 2);
        press(&mut docker, KeyCode::Char('d'));
        assert_eq!(docker.dialog, Dialog::None);
        assert!(
            docker
                .status
                .starts_with("Projects are taken down one at a time")
        );
        // `s` still acts on both: blog is started, shop is stopped.
        press(&mut docker, KeyCode::Char('s'));
        assert_eq!(
            docker.running.clone().unwrap().doing,
            "Starting project blog"
        );
        assert_eq!(docker.queue[0].doing, "Stopping project shop");
    }

    #[test]
    fn force_does_nothing_where_the_dialog_does_not_offer_it() {
        let mut docker = docker();
        docker.tab = VOLUMES;
        docker.ensure_selection();
        press(&mut docker, KeyCode::Char('d'));
        assert!(matches!(
            docker.dialog,
            Dialog::Confirm(Confirm::Volumes { .. })
        ));
        press(&mut docker, KeyCode::Char('f'));
        assert!(matches!(docker.dialog, Dialog::Confirm(_)));
        assert!(docker.running.is_none());
    }

    #[test]
    fn prune_lists_first_and_space_ticks_the_box() {
        let mut docker = docker();
        docker.tab = IMAGES;
        press(&mut docker, KeyCode::Char('p'));
        let Dialog::Confirm(confirm) = &docker.dialog else {
            panic!("no dialog");
        };
        assert!(!confirm.tick().unwrap().1);
        press(&mut docker, KeyCode::Char(' '));
        let Dialog::Confirm(confirm) = &docker.dialog else {
            panic!("no dialog");
        };
        assert!(confirm.tick().unwrap().1);
        press(&mut docker, KeyCode::Enter);
        assert_eq!(docker.running.clone().unwrap().steps[0].len(), 3);

        // Compose has nothing to prune.
        let mut docker = self::docker();
        docker.tab = COMPOSE;
        press(&mut docker, KeyCode::Char('p'));
        assert_eq!(docker.dialog, Dialog::None);
        assert!(docker.status.starts_with("There is nothing to prune"));
    }

    #[test]
    fn enter_opens_the_actions_and_a_key_runs_one() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        press(&mut docker, KeyCode::Enter);
        assert!(matches!(docker.dialog, Dialog::Menu { .. }));
        assert!(docker.modal());
        press(&mut docker, KeyCode::Char('l'));
        assert_eq!(docker.dialog, Dialog::None);
        // The log view is open and has the keyboard.
        assert!(docker.log.is_some());
        assert!(docker.modal());
        press(&mut docker, KeyCode::Char('q'));
        assert!(docker.log.is_none());
        assert!(!docker.should_quit);

        // In the Compose tab the menu is about the project.
        docker.tab = COMPOSE;
        docker.ensure_selection();
        press(&mut docker, KeyCode::Enter);
        press(&mut docker, KeyCode::Char('w'));
        assert!(matches!(
            docker.dialog,
            Dialog::Confirm(Confirm::Down { .. })
        ));
        press(&mut docker, KeyCode::Char(' '));
        press(&mut docker, KeyCode::Enter);
        let request = docker.running.clone().unwrap();
        assert_eq!(request.steps[0].join(" "), "compose -p blog down --volumes");
    }

    #[test]
    fn lines_reach_the_open_log_and_not_a_closed_one() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Down);
        press(&mut docker, KeyCode::Char('l'));
        let id = docker.log_id;
        docker.on_msg(Msg::Log(id, "ready to accept connections".into()));
        docker.on_msg(Msg::Log(id - 1, "from an older view".into()));
        docker.on_msg(Msg::LogEnded(id, Ok(())));
        press(&mut docker, KeyCode::Esc);
        assert!(docker.log.is_none());
        // A late line for the closed view is dropped without harm.
        docker.on_msg(Msg::Log(id, "late".into()));
    }

    #[test]
    fn a_reload_keeps_the_highlight_and_drops_ticks_on_what_is_gone() {
        let mut docker = docker();
        press(&mut docker, KeyCode::End);
        assert_eq!(selected_name(&docker), "backups");
        press(&mut docker, KeyCode::Char(' '));
        assert_eq!(docker.view().marked.len(), 1);

        let mut lists = samples::lists();
        lists.containers.retain(|c| c.name != "backups");
        docker.on_msg(Msg::Loaded(0, Ok(lists)));
        assert!(docker.view().marked.is_empty());
        assert!(docker.view().selected.is_some());
        assert_ne!(selected_name(&docker), "backups");
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

    #[test]
    fn q_and_ctrl_c_quit() {
        let mut docker = docker();
        press(&mut docker, KeyCode::Char('q'));
        assert!(docker.should_quit);
        let mut docker = self::docker();
        docker.on_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(docker.should_quit);
    }
}
