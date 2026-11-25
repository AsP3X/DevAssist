//! The keys and the mouse of the repositories tool. What they act on is
//! in `state`.

use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};

use crate::atoms::task::job::Job;
use crate::atoms::ui::hits::Hits;
use crate::atoms::ui::input::{self, Alt};
use crate::atoms::ui::menu;
use crate::atoms::ui::path as pathprompt;
use crate::features::repos::settings::{self, Outcome};
use crate::features::repos::state::{Action, GROUP_KEY, MenuItem, MigrateItem, Mode, Repos};
use crate::shell::config::Settings;

/// The repositories tool in the terminal: its state, and what the screen
/// and the mouse add to it.
pub struct App {
    pub core: Repos,
    pub hits: Hits<Action>,
    pub mouse: Option<(u16, u16)>,
    last_click: Option<(String, Instant)>,
}

impl Deref for App {
    type Target = Repos;

    fn deref(&self) -> &Repos {
        &self.core
    }
}

impl DerefMut for App {
    fn deref_mut(&mut self) -> &mut Repos {
        &mut self.core
    }
}

impl App {
    pub fn new(settings: Settings) -> Self {
        Self::wrap(Repos::new(settings))
    }

    /// An app that reads no folder and calls no network, for drawing tests.
    #[cfg(test)]
    pub fn for_test(settings: Settings) -> Self {
        Self::wrap(Repos::offline(settings))
    }

    #[cfg(test)]
    fn empty(settings: Settings) -> Self {
        Self::wrap(Repos::empty(settings))
    }

    fn wrap(core: Repos) -> Self {
        Self {
            core,
            hits: Hits::default(),
            mouse: None,
            last_click: None,
        }
    }

    pub fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.on_key(key),
            Event::Paste(text) => self.paste(text),
            Event::Mouse(mouse) => {
                self.mouse = Some((mouse.column, mouse.row));
                match mouse.kind {
                    // The shell passes on a release only for a click.
                    MouseEventKind::Up(MouseButton::Left)
                    | MouseEventKind::Down(MouseButton::Right) => {
                        self.click(mouse.column, mouse.row);
                    }
                    MouseEventKind::ScrollUp => self.wheel(-3),
                    MouseEventKind::ScrollDown => self.wheel(3),
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            self.quit();
            return;
        }
        match self.mode.clone() {
            Mode::Settings => self.on_settings_key(key),
            Mode::Progress => self.on_progress_key(key),
            Mode::ConfirmRemove { .. } => self.on_confirm_key(key),
            Mode::Path => self.on_path_key(key),
            Mode::Group => self.on_group_key(key),
            Mode::Service => self.on_service_key(key),
            Mode::Menu => self.on_menu_key(key),
            Mode::Add => self.on_add_key(key, ctrl),
            Mode::Filter => self.on_filter_key(key),
            Mode::List if self.forks.is_some() => self.on_forks_key(key),
            Mode::List => self.on_list_key(key, ctrl),
        }
    }

    fn on_forks_key(&mut self, key: KeyEvent) {
        let Some(view) = self.forks.as_mut() else {
            return;
        };
        if view.filtering {
            match key.code {
                KeyCode::Esc => {
                    view.filter.clear();
                    view.filtering = false;
                }
                KeyCode::Enter | KeyCode::Down | KeyCode::Up => view.filtering = false,
                KeyCode::Backspace => {
                    view.filter.pop();
                }
                KeyCode::Char(ch) => view.filter.push(ch),
                _ => {}
            }
            view.scroll = 0;
            return;
        }
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.move_fork(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_fork(1),
            KeyCode::PageUp => self.move_fork(-10),
            KeyCode::PageDown => self.move_fork(10),
            KeyCode::Home => self.move_fork(isize::MIN / 2),
            KeyCode::End => self.move_fork(isize::MAX / 2),
            KeyCode::Enter | KeyCode::Char('o') | KeyCode::Char('O') => self.open_fork_folder(),
            KeyCode::Char('g') | KeyCode::Char('G') => self.open_fork_on_github(),
            KeyCode::Char('u') | KeyCode::Char('U') => self.update_shown_forks(),
            KeyCode::Char('r') | KeyCode::F(5) => self.load_forks(),
            KeyCode::Char('l') | KeyCode::Char('L') => self.show_job(),
            KeyCode::Char('s') | KeyCode::Char('S') => {
                view.sort = view.sort.next();
                view.scroll = 0;
            }
            KeyCode::Char('c') | KeyCode::Char('C') => {
                view.changed_only = !view.changed_only;
                view.scroll = 0;
            }
            KeyCode::Char('/') => {
                view.filter.clear();
                view.filtering = true;
            }
            KeyCode::Esc if !view.filter.is_empty() => view.filter.clear(),
            KeyCode::Esc | KeyCode::Left | KeyCode::Backspace | KeyCode::Char('q') => {
                self.close_forks()
            }
            _ => {}
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
            KeyCode::Char('f') | KeyCode::Char('F') => self.open_forks(),
            KeyCode::Char('a') => self.open_add(),
            KeyCode::Char('u') => self.update_chosen(),
            KeyCode::Char('U') => self.update_all(),
            KeyCode::Char('w') | KeyCode::Char('W') => self.toggle_watch(),
            KeyCode::Char('d') | KeyCode::Delete => self.begin_remove(),
            KeyCode::Char('r') | KeyCode::F(5) => self.refresh(),
            KeyCode::Char('s') | KeyCode::Char('S') => self.toggle_sort(),
            KeyCode::Char('o') | KeyCode::Char('O') => self.open_selected(),
            KeyCode::Char('i') | KeyCode::Char('I') => self.open_import(),
            KeyCode::Char('c') | KeyCode::Char(',') | KeyCode::F(2) => self.open_settings(false),
            KeyCode::Char('l') | KeyCode::Char('L') => self.show_job(),
            KeyCode::Char('q') => self.quit(),
            KeyCode::Char('/') => {
                self.mode = Mode::Filter;
                self.list_filter.clear();
            }
            KeyCode::Esc if self.busy || self.fetching => self.cancel_work(),
            KeyCode::Esc => self.marked.clear(),
            _ => {}
        }
    }

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Some(state) = self.menu.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        if let Some(selected) = state.migrate.as_mut() {
            // The submenu: Esc goes back to the menu it came from.
            let keys = MigrateItem::ALL.map(MigrateItem::key);
            match menu::on_key(selected, key, &keys) {
                menu::Outcome::Stay => {}
                menu::Outcome::Close => state.migrate = None,
                menu::Outcome::Run(index) => self.run_migrate(MigrateItem::ALL[index]),
            }
            return;
        }
        let keys = MenuItem::ALL.map(MenuItem::key);
        match menu::on_key(&mut state.selected, key, &keys) {
            menu::Outcome::Stay => {}
            menu::Outcome::Close => self.close_menu(),
            menu::Outcome::Run(index) => self.run_menu(MenuItem::ALL[index]),
        }
    }

    fn on_service_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.service_prompt.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match key.code {
            KeyCode::Esc => self.close_service(),
            KeyCode::Enter => self.accept_service(),
            _ => {
                if input::edit(&mut prompt.input, key, Alt::Types) {
                    prompt.error = None;
                }
            }
        }
    }

    fn on_group_key(&mut self, key: KeyEvent) {
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
            _ => {
                if input::edit(&mut prompt.input, key, Alt::Types) {
                    prompt.error = None;
                }
            }
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.list_filter.clear();
                self.list_scroll = 0;
            }
            KeyCode::Enter => self.enter_row(),
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            _ => {
                if input::edit(&mut self.list_filter, key, Alt::Ignored) {
                    self.list_scroll = 0;
                    self.snap_visible_selection();
                }
            }
        }
    }

    fn on_add_key(&mut self, key: KeyEvent, ctrl: bool) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::List,
            KeyCode::Enter => self.clone_selected(),
            KeyCode::Tab => self.list_typed_account(),
            KeyCode::Char(' ') if !ctrl => self.toggle_mark(),
            KeyCode::Up => self.move_by(-1),
            KeyCode::Down => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_to_end(true),
            KeyCode::End => self.move_to_end(false),
            _ => {
                if input::edit(&mut self.add_filter, key, Alt::Ignored) {
                    self.note_add_filter_changed();
                }
            }
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

    fn on_path_key(&mut self, key: KeyEvent) {
        let Some(prompt) = self.prompt.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match pathprompt::on_key(prompt, key) {
            pathprompt::Outcome::Stay => {}
            pathprompt::Outcome::Accept(folder) => self.use_folder(folder),
            pathprompt::Outcome::Close => self.close_path(),
        }
    }

    fn on_settings_key(&mut self, key: KeyEvent) {
        let Some(form) = self.form.as_mut() else {
            self.mode = Mode::List;
            return;
        };
        match settings::on_key(form, key) {
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

    fn wheel(&mut self, delta: isize) {
        if self.mode == Mode::Progress {
            self.scroll_job(-delta);
        } else if self.forks.is_some() && self.mode == Mode::List {
            self.move_fork(delta);
        } else {
            self.move_by(delta);
        }
    }

    fn click(&mut self, x: u16, y: u16) {
        if let Some(action) = self.hits.button_at(x, y) {
            self.run(action);
            return;
        }
        if let Some(name) = self.hits.row_at(x, y) {
            self.click_row(name);
        }
    }

    fn click_row(&mut self, name: String) {
        let now = Instant::now();
        let double = self.last_click.as_ref().is_some_and(|(previous, at)| {
            previous.eq_ignore_ascii_case(&name)
                && now.duration_since(*at) < Duration::from_millis(400)
        });
        self.last_click = Some((name.clone(), now));
        if self.mode == Mode::List
            && let Some(view) = self.forks.as_mut()
        {
            if let Some(owner) = name.strip_prefix("fork:") {
                view.selected = Some(owner.to_string());
                view.filtering = false;
                if double {
                    self.open_fork_folder();
                }
            }
            return;
        }
        if self.mode == Mode::Menu {
            let index = name
                .strip_prefix("menu:")
                .and_then(|index| index.parse::<usize>().ok());
            let submenu = self
                .menu
                .as_ref()
                .is_some_and(|menu| menu.migrate.is_some());
            if submenu {
                if let Some(item) = index.and_then(|index| MigrateItem::ALL.get(index).copied()) {
                    self.run_migrate(item);
                }
            } else if let Some(item) = index.and_then(|index| MenuItem::ALL.get(index).copied()) {
                self.run_menu(item);
            }
        } else if self.mode == Mode::Progress {
            if let Some(job) = self.core.job.as_mut()
                && let Some(index) = name
                    .strip_prefix("job:")
                    .and_then(|index| index.parse().ok())
            {
                self.core.job_close_at = None;
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
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::atoms::sys::paths;
    use crate::atoms::task::job::JobEvent;
    use crate::features::repos::github::RemoteRepo;
    use crate::features::repos::gitops;
    use crate::features::repos::state::testing::*;
    use crate::features::repos::state::*;

    fn grouped_app() -> App {
        App::wrap(crate::features::repos::state::testing::grouped_app())
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
        assert!(paths::same_path(&app.clone_dir, &root.join("second")));
        match app.queue.front() {
            Some(Queued::Clone(remotes, dir)) => {
                assert_eq!(remotes.len(), 2);
                assert!(paths::same_path(dir, &root.join("second")));
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
        app.click_row("menu:11".into());
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

    #[test]
    fn migrate_opens_a_submenu_that_moves_to_another_folder() {
        let mut app = grouped_app();
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        // The menu stays open, on its submenu.
        assert!(app.mode == Mode::Menu);
        assert_eq!(app.menu.as_ref().unwrap().migrate, Some(0));
        // Esc goes back to the menu, not out of it.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);
        assert_eq!(app.menu.as_ref().unwrap().migrate, None);
        // A key of the main menu does nothing in the submenu.
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('d'), KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);

        // Its line asks for the folder, starting where the repository is.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Path);
        assert!(app.menu.is_none());
        let prompt = app.prompt.as_mut().unwrap();
        assert!(matches!(&prompt.purpose, Purpose::Move(repos) if repos[0].0 == "api"));
        // It keeps its folder name inside the folder that is typed.
        let dir = std::env::temp_dir();
        prompt.input = dir.display().to_string();
        assert_eq!(
            clone_target(prompt).unwrap().file_name().unwrap(),
            std::ffi::OsStr::new("api")
        );
        // Esc there closes the dialog; nothing was moved.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);

        // A click on the submenu's line does the same, and accepting the
        // folder queues the move behind what is running.
        app.busy = true;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        app.click_row("menu:0".into());
        assert!(app.mode == Mode::Path);
        app.use_folder(PathBuf::from("/srv/elsewhere"));
        assert!(app.mode == Mode::List);
        assert_eq!(labels(&app), ["move api"]);
        app.drop_queue();
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn the_submenu_asks_where_on_another_service_a_repository_goes() {
        let mut app = grouped_app();
        // Several at once have no single address to go to.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.status.contains("one repository"), "{}", app.status);
        assert!(app.status_error);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.marked.is_empty());

        app.selected = Some("api".into());
        app.busy = true;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Service);
        assert_eq!(app.service_prompt.as_ref().unwrap().repo, "api");

        // Something that is no address stays in the dialog with a reason.
        for ch in "--upload-pack=x".chars() {
            press(&mut app, KeyCode::Char(ch), KeyModifiers::NONE);
        }
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Service);
        assert!(app.service_prompt.as_ref().unwrap().error.is_some());
        assert!(app.queue.is_empty());
        // Typing clears the reason.
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert!(app.service_prompt.as_ref().unwrap().error.is_none());

        // A pasted address is taken, and the push waits its turn.
        app.service_prompt.as_mut().unwrap().input.clear();
        app.paste(" https://gitlab.com/acme/api.git\n".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.service_prompt.is_none());
        assert_eq!(labels(&app), ["push api to another service"]);
        assert!(matches!(
            app.queue.front(),
            Some(Queued::Service(_, _, url)) if url == "https://gitlab.com/acme/api.git"
        ));
        app.drop_queue();

        // Esc closes the dialog without anything.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.queue.is_empty());
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn the_menu_clones_the_forks_of_the_selection() {
        let mut app = grouped_app();
        // Something else is running, so it waits in the queue.
        app.busy = true;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);
        press(&mut app, KeyCode::Char('n'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(labels(&app), ["fetch the forks of api"]);
        app.drop_queue();
    }

    fn fork(owner: &str, ahead: u32, date: i64, folder: bool) -> gitops::ForkInfo {
        gitops::ForkInfo {
            owner: owner.to_string(),
            name: "web".to_string(),
            branches: 2,
            pulls: 1,
            ahead: Some(ahead),
            date,
            folder,
            gone: false,
        }
    }

    #[test]
    fn f_opens_the_overview_of_the_forks_and_esc_goes_back() {
        let root = std::env::temp_dir().join(format!("devassist-forkview-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = grouped_app();
        app.selected = Some("web".into());
        // Nothing is cloned: there is nothing to show.
        press(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
        assert!(app.forks.is_none());
        assert!(app.status.contains("not cloned yet"), "{}", app.status);

        let web = app
            .installed
            .iter_mut()
            .find(|repo| repo.folder == "web")
            .unwrap();
        web.path = root.join("web");
        std::fs::create_dir_all(root.join("web-fork").join(".git")).unwrap();
        press(&mut app, KeyCode::Char('f'), KeyModifiers::NONE);
        let view = app.forks.as_ref().expect("the overview is open");
        assert!(view.loading);
        assert_eq!(view.repo, "web");
        // What an earlier read found is not taken.
        app.core.on_msg(Msg::Forks {
            generation: app.forks_generation - 1,
            result: Err("old".into()),
        });
        assert!(app.forks.as_ref().unwrap().loading);
        app.core.on_msg(Msg::Forks {
            generation: app.forks_generation,
            result: Ok(gitops::ForkList {
                original: "ada/web".into(),
                dest: root.join("web-fork"),
                forks: vec![
                    fork("amy", 0, 50, false),
                    fork("bob", 3, 10, true),
                    fork("cy", 9, 20, true),
                ],
            }),
        });
        let owners = |app: &App| -> Vec<String> {
            let view = app.forks.as_ref().unwrap();
            view.rows().iter().map(|fork| fork.owner.clone()).collect()
        };
        let chosen = |app: &App| app.forks.as_ref().unwrap().chosen().unwrap().owner.clone();
        assert!(!app.forks.as_ref().unwrap().loading);
        // The forks with changes come first, the one furthest ahead on top.
        assert_eq!(owners(&app), ["cy", "bob", "amy"]);
        assert_eq!(chosen(&app), "cy");
        press(&mut app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(chosen(&app), "bob");
        press(&mut app, KeyCode::End, KeyModifiers::NONE);
        assert_eq!(chosen(&app), "amy");
        // A fork without changes has no folder to open.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.status.contains("no folder"), "{}", app.status);
        press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(owners(&app), ["amy", "cy", "bob"]);
        press(&mut app, KeyCode::Char('s'), KeyModifiers::NONE);
        assert_eq!(owners(&app), ["amy", "bob", "cy"]);
        press(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
        assert_eq!(owners(&app), ["bob", "cy"]);
        press(&mut app, KeyCode::Char('/'), KeyModifiers::NONE);
        type_text(&mut app, "cy");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(owners(&app), ["cy"]);
        // Esc first clears the filter, then goes back to the repositories.
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(owners(&app), ["bob", "cy"]);
        // u fetches the forks of this repository, whatever is ticked.
        app.busy = true;
        press(&mut app, KeyCode::Char('u'), KeyModifiers::NONE);
        assert_eq!(labels(&app), ["fetch the forks of web"]);
        assert!(app.forks.is_some());
        app.drop_queue();
        press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
        assert!(app.forks.is_none());
        assert!(app.mode == Mode::List);
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn the_menu_updates_every_fork_folder_that_is_there() {
        let root = std::env::temp_dir().join(format!("devassist-allforks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = grouped_app();
        app.busy = true;
        // Nothing is cloned yet: there is nothing to update.
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(labels(&app).is_empty());

        // The forks of one repository are: it is taken, whatever is
        // highlighted or ticked.
        let web = app
            .installed
            .iter_mut()
            .find(|repo| repo.folder == "web")
            .unwrap();
        web.path = root.join("web");
        std::fs::create_dir_all(root.join("web-fork").join(".git")).unwrap();
        app.selected = Some("api".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(labels(&app), ["fetch the forks of web"]);
        app.drop_queue();
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn the_menu_fetches_the_pull_requests_of_the_selection() {
        let mut app = grouped_app();
        // Something else is running, so the fetch waits in the queue.
        app.busy = true;
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);
        press(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(labels(&app), ["fetch the pull requests of api"]);
        // It is an action, not the setting.
        assert!(!app.pull_requests);
        app.drop_queue();

        // With several ticked it takes them all, and clears the ticks.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('p'), KeyModifiers::NONE);
        assert_eq!(labels(&app), ["fetch the pull requests of 2 repositories"]);
        assert!(app.marked.is_empty());
        app.drop_queue();
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn w_turns_the_automatic_pull_on_and_off_and_saves_it() {
        let mut app = grouped_app();
        // Far in the future, so that only turning it on makes a pull due.
        app.watch_due = Instant::now() + Duration::from_secs(3600);
        press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
        assert_eq!(app.watched, [PathBuf::from("/srv/code/api")]);
        assert!(app.is_watched(Path::new("/srv/code/api")));
        assert!(app.status.contains("every 5 minutes"), "{}", app.status);
        assert!(app.watch_due <= Instant::now());
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.lines().any(|line| line.starts_with("watch = ")));

        // Ticking a watched and an unwatched one watches both.
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
        assert_eq!(app.marked, ["api", "docs"]);
        press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
        assert_eq!(app.watched.len(), 2);
        assert!(app.marked.is_empty());

        // On one that is watched, w stops it. The menu has the same line.
        app.selected = Some("api".into());
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::Menu);
        press(&mut app, KeyCode::Char('w'), KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert_eq!(app.watched, [PathBuf::from("/srv/code/docs")]);
        assert!(app.status.contains("off for api"), "{}", app.status);

        // A checkout that leaves the list is no longer watched.
        app.checkouts
            .retain(|path| path != Path::new("/srv/code/docs"));
        app.save_config();
        assert!(app.watched.is_empty());
        let _ = std::fs::remove_file(&app.config_path);
    }

    #[test]
    fn the_settings_dialog_switches_saving_pull_requests() {
        let mut app = App::empty(test_settings());
        assert!(!app.pull_requests);
        press(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(!app.form.as_ref().unwrap().pull_requests);
        app.form.as_mut().unwrap().click("pulls:on");
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.mode == Mode::List);
        assert!(app.pull_requests);
        assert!(app.status.contains("press r"), "{}", app.status);
        let text = std::fs::read_to_string(&app.config_path).unwrap();
        assert!(text.contains("pull_requests = on"));

        // The dialog opens on what is set, and saving again keeps it.
        press(&mut app, KeyCode::Char('c'), KeyModifiers::NONE);
        assert!(app.form.as_ref().unwrap().pull_requests);
        press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
        assert!(app.pull_requests);
        assert!(app.status.starts_with("Settings saved to"));
        let _ = std::fs::remove_file(&app.config_path);
    }
}
