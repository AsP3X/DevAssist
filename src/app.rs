use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::config::Settings;
use crate::github::{self, RemoteRepo};
use crate::gitops::{self, LocalRepo};

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
}

#[derive(Clone, PartialEq, Eq)]
pub enum Mode {
    List,
    Filter,
    Add,
    ConfirmRemove { folder: String, dirty: bool },
    Help,
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

struct Planned {
    folder: String,
    path: PathBuf,
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
    Log(String),
    Progress(String),
    JobFinished(Result<String, String>),
}

#[derive(Clone, PartialEq, Eq)]
pub enum RemoteState {
    Loading,
    Ready,
    Failed(String),
}

pub struct App {
    pub root: PathBuf,
    pub title: String,
    pub owners: Vec<String>,
    pub pinned: Vec<github::RepoSpec>,
    pub protocol: gitops::Protocol,
    pub installed: Vec<LocalRepo>,
    pub remotes: Vec<RemoteRepo>,
    pub remotes_state: RemoteState,
    pub catalog_warning: Option<String>,
    pub selected: Option<String>,
    pub add_selected: Option<String>,
    pub mode: Mode,
    pub sort: SortMode,
    pub list_filter: String,
    pub add_filter: String,
    pub list_scroll: usize,
    pub add_scroll: usize,
    pub log: VecDeque<String>,
    pub progress: String,
    pub status: String,
    pub status_error: bool,
    pub busy: bool,
    pub fetching: bool,
    pub tick: u64,
    pub mouse: Option<(u16, u16)>,
    pub hits: Hits,
    pub should_quit: bool,
    generation: u64,
    remote_generation: u64,
    status_pending: usize,
    last_click: Option<(String, Instant)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
}

impl App {
    pub fn new(settings: Settings) -> Self {
        let mut app = Self::empty(settings);
        app.reload_lists(false, false);
        app
    }

    fn empty(settings: Settings) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            root: settings.root,
            title: settings.title,
            owners: settings.owners,
            pinned: settings.pinned,
            protocol: settings.protocol,
            installed: Vec::new(),
            remotes: Vec::new(),
            remotes_state: RemoteState::Loading,
            catalog_warning: None,
            selected: None,
            add_selected: None,
            mode: Mode::List,
            sort: SortMode::Name,
            list_filter: String::new(),
            add_filter: String::new(),
            list_scroll: 0,
            add_scroll: 0,
            log: VecDeque::new(),
            progress: String::new(),
            status: "Looking for repositories…".to_string(),
            status_error: false,
            busy: false,
            fetching: false,
            tick: 0,
            mouse: None,
            hits: Hits::default(),
            should_quit: false,
            generation: 0,
            remote_generation: 0,
            status_pending: 0,
            last_click: None,
            tx,
            rx,
        }
    }

    pub fn drain(&mut self) -> bool {
        let mut messages = Vec::new();
        while let Ok(message) = self.rx.try_recv() {
            messages.push(message);
        }
        let arrived = !messages.is_empty();
        for message in messages {
            self.on_msg(message);
        }
        arrived
    }

    pub fn waiting(&self) -> bool {
        self.busy
            || self.fetching
            || self.status_pending > 0
            || self.remotes_state == RemoteState::Loading
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.paste(text),
            Event::Mouse(mouse) => {
                self.mouse = Some((mouse.column, mouse.row));
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left)
                    | MouseEventKind::Down(MouseButton::Right) => {
                        self.click(mouse.column, mouse.row);
                    }
                    MouseEventKind::ScrollUp => self.move_by(-3),
                    MouseEventKind::ScrollDown => self.move_by(3),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    pub fn source_label(&self) -> String {
        if self.owners.is_empty() {
            if self.pinned.is_empty() {
                "any repository".to_string()
            } else {
                "pinned".to_string()
            }
        } else {
            self.owners.join(", ")
        }
    }

    pub fn mixed_owners(&self) -> bool {
        let mut seen: Option<&str> = None;
        for repo in &self.installed {
            match seen {
                None => seen = Some(repo.owner.as_str()),
                Some(owner) if owner.eq_ignore_ascii_case(&repo.owner) => {}
                Some(_) => return true,
            }
        }
        false
    }

    pub fn visible_installed(&self) -> Vec<&LocalRepo> {
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

    pub fn add_list(&self) -> Vec<RemoteRepo> {
        let mut remotes: Vec<RemoteRepo> = self
            .remotes
            .iter()
            .filter(|repo| !self.is_installed(repo) && remote_matches(repo, &self.add_filter))
            .cloned()
            .collect();
        remotes.sort_by(|a, b| {
            a.archived.cmp(&b.archived).then(
                a.name
                    .to_lowercase()
                    .cmp(&b.name.to_lowercase())
                    .then(a.owner.to_lowercase().cmp(&b.owner.to_lowercase())),
            )
        });
        if let Some(spec) = github::parse_repo_spec(&self.add_filter) {
            let installed = self.installed.iter().any(|repo| {
                repo.owner.eq_ignore_ascii_case(&spec.owner)
                    && repo.repo.eq_ignore_ascii_case(&spec.name)
            });
            let listed = remotes.iter().any(|repo| repo.matches_spec(&spec));
            if !installed && !listed {
                remotes.insert(0, RemoteRepo::direct(spec));
            }
        }
        remotes
    }

    pub fn add_empty_message(&self) -> String {
        if let Some(spec) = github::parse_repo_spec(&self.add_filter) {
            let installed = self.installed.iter().any(|repo| {
                repo.owner.eq_ignore_ascii_case(&spec.owner)
                    && repo.repo.eq_ignore_ascii_case(&spec.name)
            });
            if installed {
                return format!("{} is already in this folder.", spec.id());
            }
        }
        match &self.remotes_state {
            RemoteState::Loading => "Loading repositories from GitHub…".to_string(),
            RemoteState::Failed(err) => err.clone(),
            RemoteState::Ready
                if self.add_filter.is_empty()
                    && self.owners.is_empty()
                    && self.pinned.is_empty() =>
            {
                "Type owner/repo or paste a GitHub URL.".to_string()
            }
            RemoteState::Ready if self.add_filter.is_empty() => {
                "Every listed repository is already here. Type owner/repo to clone another."
                    .to_string()
            }
            RemoteState::Ready => "Nothing matches that search.".to_string(),
        }
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
        if self.mixed_owners() {
            format!("{}/{}", repo.owner, repo.repo)
        } else {
            String::new()
        }
    }

    fn is_installed(&self, remote: &RemoteRepo) -> bool {
        self.installed.iter().any(|local| {
            local.owner.eq_ignore_ascii_case(&remote.owner)
                && local.repo.eq_ignore_ascii_case(&remote.name)
        })
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.quit();
            return;
        }
        match self.mode.clone() {
            Mode::Help => self.on_help_key(key),
            Mode::ConfirmRemove { .. } => self.on_confirm_key(key),
            Mode::Add => self.on_add_key(key, ctrl),
            Mode::Filter => self.on_filter_key(key, ctrl),
            Mode::List => self.on_list_key(key, ctrl),
        }
    }

    fn on_list_key(&mut self, key: KeyEvent, _ctrl: bool) {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            KeyCode::Enter => self.update_one(),
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('u') => self.update_one(),
            KeyCode::Char('U') => self.update_all(),
            KeyCode::Char('d') | KeyCode::Delete => self.begin_remove(),
            KeyCode::Char('r') | KeyCode::F(5) => self.reload_lists(false, true),
            KeyCode::Char('s') | KeyCode::Char('S') => self.toggle_sort(),
            KeyCode::Char('o') | KeyCode::Char('O') => self.open_selected(),
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('?') => self.mode = Mode::Help,
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.list_filter.clear();
            }
            KeyCode::Esc if self.busy || self.fetching => self.cancel_work(),
            KeyCode::Esc if !self.log.is_empty() => self.log.clear(),
            _ => {}
        }
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
            KeyCode::Enter => self.update_one(),
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
            _ => {}
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
            _ => {}
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
        if self.mode == Mode::Add {
            self.add_selected = Some(name);
            if double {
                self.clone_selected();
            }
        } else if matches!(self.mode, Mode::List | Mode::Filter) {
            self.selected = Some(name);
            if double {
                self.update_one();
            }
        }
    }

    pub fn run(&mut self, action: Action) {
        match action {
            Action::Add => self.open_add(),
            Action::UpdateOne => self.update_one(),
            Action::UpdateAll => self.update_all(),
            Action::Remove => self.begin_remove(),
            Action::Refresh | Action::Retry => {
                if self.mode == Mode::Add && matches!(action, Action::Retry) {
                    self.reload_github_only();
                } else {
                    self.reload_lists(false, true);
                }
            }
            Action::Quit => self.quit(),
            Action::Confirm => self.confirm_remove(),
            Action::Cancel => self.mode = Mode::List,
            Action::Clone => self.clone_selected(),
        }
    }

    fn move_by(&mut self, delta: isize) {
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
            self.visible_installed()
                .into_iter()
                .map(|repo| repo.folder.clone())
                .collect()
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
        let still_here = self
            .selected
            .as_ref()
            .is_some_and(|name| self.installed.iter().any(|repo| &repo.folder == name));
        if !still_here {
            self.selected = self.installed.first().map(|repo| repo.folder.clone());
        }
    }

    fn preferred_add_id(&self) -> Option<String> {
        let list = self.add_list();
        list.iter()
            .find(|repo| !repo.direct)
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
        let Some(spec) = github::parse_repo_spec(&self.add_filter) else {
            return;
        };
        let list = self.add_list();
        let id = spec.id();
        let present = list.iter().any(|repo| repo.id().eq_ignore_ascii_case(&id));
        if !present {
            return;
        }
        let exact = self.remotes.iter().any(|repo| repo.matches_spec(&spec));
        let url = self.add_filter.to_ascii_lowercase().contains("github.com");
        if exact || url || list.len() == 1 {
            self.add_selected = Some(id);
        }
    }

    fn open_add(&mut self) {
        if self.busy {
            self.notice("Wait for the current Git command to finish.", true);
            return;
        }
        self.mode = Mode::Add;
        self.add_filter.clear();
        self.add_scroll = 0;
        self.add_selected = None;
        if matches!(self.remotes_state, RemoteState::Failed(_)) {
            self.reload_github_only();
        }
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
        let Some(folder) = self.selected.clone() else {
            self.notice("Select a repository first.", true);
            return;
        };
        let Some(repo) = self.installed.iter().find(|repo| repo.folder == folder) else {
            return;
        };
        match gitops::open_folder(&repo.path) {
            Ok(()) => self.notice(format!("Opened {folder}."), false),
            Err(err) => self.notice(err, true),
        }
    }

    fn update_one(&mut self) {
        if !self.guard_idle() {
            return;
        }
        let Some(folder) = self.selected.clone() else {
            self.notice("Select a repository first.", true);
            return;
        };
        let Some(repo) = self.installed.iter().find(|repo| repo.folder == folder) else {
            return;
        };
        let path = repo.path.clone();
        self.mode = Mode::List;
        self.start_job(format!("Updating {folder}…"), move |tx| {
            let token = github::token();
            gitops::pull(&path, token.as_deref(), |line, progress| {
                send_line(tx, &folder, line, progress)
            })?;
            Ok(format!("Updated {folder}."))
        });
    }

    fn update_all(&mut self) {
        if !self.guard_idle() {
            return;
        }
        let repos: Vec<Planned> = self
            .installed
            .iter()
            .map(|repo| {
                let gone = repo.ready && repo.error.is_none() && repo.upstream_gone;
                let skip = !gone
                    && repo.ready
                    && repo.fetched
                    && repo.error.is_none()
                    && repo.warning.is_none()
                    && repo.behind == Some(0);
                Planned {
                    folder: repo.folder.clone(),
                    path: repo.path.clone(),
                    skip,
                    gone,
                }
            })
            .collect();
        if repos.is_empty() {
            self.notice("There are no repositories to update.", true);
            return;
        }
        let count = repos.len();
        self.mode = Mode::List;
        self.start_job(format!("Updating {count} repositories…"), move |tx| {
            let token = github::token();
            let mut updated = 0;
            let mut skipped = 0;
            let mut failed = Vec::new();
            for (index, item) in repos.iter().enumerate() {
                if gitops::was_cancelled() {
                    return Err("cancelled".into());
                }
                if item.skip {
                    skipped += 1;
                    continue;
                }
                let _ = tx.send(Msg::Log(format!(
                    "[{}/{}] {}",
                    index + 1,
                    repos.len(),
                    item.folder
                )));
                if item.gone {
                    failed.push(item.folder.clone());
                    let _ = tx.send(Msg::Log(format!("{}: upstream is gone", item.folder)));
                    continue;
                }
                match gitops::pull(&item.path, token.as_deref(), |line, progress| {
                    send_line(tx, &item.folder, line, progress)
                }) {
                    Ok(()) => updated += 1,
                    Err(err) => {
                        if gitops::was_cancelled() {
                            return Err(err);
                        }
                        failed.push(item.folder.clone());
                        let _ = tx.send(Msg::Log(format!("{} failed: {err}", item.folder)));
                    }
                }
            }
            if failed.is_empty() {
                if updated == 0 {
                    Ok(format!("All {count} repositories are up to date."))
                } else if skipped == 0 {
                    Ok(format!("Updated {updated}."))
                } else {
                    Ok(format!("Updated {updated}. {skipped} already up to date."))
                }
            } else {
                Err(format!(
                    "Updated {updated}, skipped {skipped}, {} failed: {}",
                    failed.len(),
                    failed.join(", ")
                ))
            }
        });
    }

    fn begin_remove(&mut self) {
        if !self.guard_idle() {
            return;
        }
        let Some(folder) = self.selected.clone() else {
            self.notice("Select a repository to remove.", true);
            return;
        };
        let dirty = self
            .installed
            .iter()
            .find(|repo| repo.folder == folder)
            .is_some_and(|repo| repo.dirty);
        self.mode = Mode::ConfirmRemove { folder, dirty };
    }

    fn confirm_remove(&mut self) {
        let Mode::ConfirmRemove { folder, .. } = self.mode.clone() else {
            return;
        };
        if !self.guard_idle() {
            return;
        }
        let root = self.root.clone();
        self.mode = Mode::List;
        self.start_job(format!("Removing {folder}…"), move |_tx| {
            gitops::remove_repo(&root, &folder)?;
            Ok(format!("Removed {folder}."))
        });
    }

    fn clone_selected(&mut self) {
        if !self.guard_idle() {
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
            self.notice(format!("{} is already in this folder.", remote.id()), true);
            return;
        }
        let dest = match gitops::clone_destination(&self.root, &remote.owner, &remote.name) {
            Ok(dest) => dest,
            Err(err) => {
                self.notice(err, true);
                return;
            }
        };
        let url = gitops::clone_url(
            &remote.owner,
            &remote.name,
            self.protocol,
            &self.installed,
            &remote.clone_url,
        );
        let folder = dest
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| remote.name.clone());
        self.mode = Mode::List;
        self.start_job(format!("Cloning {folder}…"), move |tx| {
            let token = github::token();
            gitops::clone_repo(&url, &dest, token.as_deref(), |line, progress| {
                send_line(tx, &folder, line, progress)
            })?;
            Ok(format!("Cloned {folder}."))
        });
    }

    fn guard_idle(&mut self) -> bool {
        if self.busy || self.fetching {
            self.notice("Wait for the current Git command to finish.", true);
            false
        } else {
            true
        }
    }

    fn cancel_work(&mut self) {
        if self.busy || self.fetching {
            gitops::request_cancel();
            self.notice("Cancelling…", false);
        }
    }

    fn quit(&mut self) {
        gitops::request_cancel();
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
            .filter(|repo| repo.behind.unwrap_or(0) > 0)
            .count();
        if self.installed.is_empty() {
            "No repositories installed.".to_string()
        } else if behind == 0 {
            format!("{} installed.", self.installed.len())
        } else if behind == 1 {
            format!("{} installed. 1 is behind.", self.installed.len())
        } else {
            format!("{} installed. {behind} are behind.", self.installed.len())
        }
    }

    fn start_job<F>(&mut self, label: String, work: F)
    where
        F: FnOnce(&Sender<Msg>) -> Result<String, String> + Send + 'static,
    {
        gitops::clear_cancel();
        self.busy = true;
        self.progress.clear();
        self.log.clear();
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
        self.installed = gitops::scan(&self.root);
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
                            repo.branch = info.branch;
                            repo.dirty = info.dirty;
                            repo.ahead = info.ahead;
                            repo.behind = info.behind;
                            repo.upstream_gone = info.upstream_gone;
                            repo.error = None;
                            repo.ready = true;
                        }
                        Err(err) => {
                            repo.ready = true;
                            repo.error = Some(err);
                        }
                    }
                }
                self.status_pending = self.status_pending.saturating_sub(1);
                if self.status_pending == 0 {
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
            Msg::Log(line) => {
                self.log.push_back(line);
                while self.log.len() > 200 {
                    self.log.pop_front();
                }
            }
            Msg::Progress(line) => self.progress = line,
            Msg::JobFinished(result) => {
                self.busy = false;
                self.progress.clear();
                match result {
                    Err(_) if gitops::was_cancelled() => {
                        self.notice("Cancelled.".to_string(), false)
                    }
                    Ok(message) => self.notice(message, false),
                    Err(err) => self.notice(err, true),
                }
                gitops::clear_cancel();
                self.generation += 1;
                self.fetching = false;
                self.installed = gitops::scan(&self.root);
                self.spawn_status(self.generation, false);
            }
        }
    }
}

fn send_line(tx: &Sender<Msg>, folder: &str, line: &str, progress: bool) {
    let _ = if progress {
        tx.send(Msg::Progress(format!("{folder}: {line}")))
    } else {
        tx.send(Msg::Log(format!("{folder}: {line}")))
    };
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

fn delete_word(text: &mut String) {
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
            root: PathBuf::from("."),
            title: "Test".into(),
            owners: vec!["storytold".into()],
            pinned: Vec::new(),
            protocol: gitops::Protocol::Https,
            doctor: false,
        }
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
            ready: true,
            fetched: false,
            error: None,
            warning: None,
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
    fn add_list_hides_a_repository_that_is_already_installed() {
        let mut app = App::empty(test_settings());
        app.installed.push(installed("other", "tool"));
        app.add_filter = "other/tool".into();
        assert!(app.add_list().is_empty());
        assert!(app.add_empty_message().contains("already"));
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
