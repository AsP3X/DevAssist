//! The keys and the mouse of the Docker tool. What they act on is in
//! `devassist_core::docker::state`.

use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;

use super::actions::{Choice, Verb};
use crate::atoms::ui::hits::{Hits, contains};
use crate::atoms::ui::input::{self, Alt};
use crate::atoms::ui::log::{self, LogView};
use crate::atoms::ui::menu;
use crate::atoms::ui::tabs;
pub use devassist_core::docker::state::*;
/// The Docker tool in the terminal: its state, and what the screen and
/// the mouse add to it.
pub struct Docker {
    pub core: DockerState,
    pub hits: Hits<Action>,
    /// Where the tab bar drew each tab.
    pub tab_rects: Vec<Rect>,
    pub mouse: Option<(u16, u16)>,
    last_click: Option<(String, Instant)>,
    /// What the screen adds to the core's open log. There is one exactly
    /// while the core has a log.
    pub log_view: Option<LogView>,
}

impl Deref for Docker {
    type Target = DockerState;

    fn deref(&self) -> &DockerState {
        &self.core
    }
}

impl DerefMut for Docker {
    fn deref_mut(&mut self) -> &mut DockerState {
        &mut self.core
    }
}

impl Default for Docker {
    fn default() -> Self {
        Self::new()
    }
}

impl Docker {
    /// The tool before it is first opened. It asks Docker nothing yet.
    pub fn new() -> Self {
        Self::wrap(DockerState::new())
    }

    /// A tool that holds these lists and never runs `docker`, for tests.
    #[cfg(test)]
    pub fn for_test(lists: super::model::Lists) -> Self {
        Self::wrap(DockerState::offline(lists))
    }

    fn wrap(core: DockerState) -> Self {
        Self {
            core,
            hits: Hits::default(),
            tab_rects: Vec::new(),
            mouse: None,
            last_click: None,
            log_view: None,
        }
    }

    /// Takes in what the threads reported, as the core does, and keeps
    /// the log view in step with the core's log.
    pub fn drain(&mut self) -> bool {
        let changed = self.core.drain();
        self.sync_log_view();
        changed
    }

    /// A log view is made when the core opens a log and dropped when the
    /// core closes it.
    fn sync_log_view(&mut self) {
        if self.core.log.is_none() {
            self.log_view = None;
        } else if self.log_view.is_none() {
            self.log_view = Some(LogView::new());
        }
    }

    pub fn on_event(&mut self, event: Event) {
        self.sync_log_view();
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
        self.sync_log_view();
    }

    fn wheel(&mut self, delta: isize) {
        if let Some(log) = self.core.log.as_mut() {
            let view = self.log_view.get_or_insert_with(LogView::new);
            view.scroll_by(log, delta);
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
        if let Some(log) = self.core.log.as_mut() {
            let view = self.log_view.get_or_insert_with(LogView::new);
            if view.on_key(log, key) == log::Outcome::Close {
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
            KeyCode::Char('/') => self.start_filter(),
            KeyCode::Esc => self.clear_ticks_or_filter(),
            _ => {}
        }
    }

    fn on_filter_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.drop_filter(),
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

    fn on_menu_key(&mut self, key: KeyEvent) {
        let Dialog::Menu { verbs, selected } = &mut self.core.dialog else {
            return;
        };
        let keys: Vec<char> = verbs.iter().map(|verb| verb.key()).collect();
        match menu::on_key(selected, key, &keys) {
            menu::Outcome::Stay => {}
            menu::Outcome::Close => self.dialog = Dialog::None,
            menu::Outcome::Run(index) => {
                let verb = verbs[index];
                self.run_from_menu(verb);
            }
        }
    }

    fn on_confirm_key(&mut self, key: KeyEvent) {
        let Dialog::Confirm(confirm) = &mut self.core.dialog else {
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
            self.switch_tab(index);
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
                self.run_from_menu(verb);
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
        self.select_row(name, double);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::docker::actions::Confirm;
    use crate::features::docker::model::samples;
    use devassist_core::docker::state::testing;

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

        testing::done(&mut docker, Ok(()));
        assert_eq!(docker.running.clone().unwrap().doing, "Stopping shop-web");
        testing::done(&mut docker, Err("no such container".into()));
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
        testing::done(&mut docker, Ok(()));

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
        testing::done(&mut docker, Ok(()));
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
        testing::loaded(&mut docker, 0, Ok(samples::lists()));
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
        testing::loaded(&mut docker, 0, Ok(lists));
        // The row that moved up into its place, not the project's header.
        assert_eq!(selected_name(&docker), "shop-web");
        assert_eq!(docker.selected_group(), None);
    }

    #[test]
    fn events_that_end_at_once_are_not_started_again_until_asked() {
        let mut docker = docker();
        testing::set_events_started(&mut docker, Instant::now());
        testing::events_ended(&mut docker);
        assert!(testing::events_refused(&docker));
        // One look to see whether the engine is still there, and no more.
        assert!(testing::stale(&docker));
        // `r` gives it another chance.
        press(&mut docker, KeyCode::Char('r'));
        assert!(!testing::events_refused(&docker));

        // A stream that ran for a while and ended is started again.
        testing::set_events_started(&mut docker, Instant::now() - testing::EVENTS_MIN_LIFE);
        testing::events_ended(&mut docker);
        assert!(!testing::events_refused(&docker));
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
        let id = testing::log_id(&docker);
        testing::log_line(&mut docker, id, "ready to accept connections".into());
        testing::log_line(&mut docker, id - 1, "from an older view".into());
        testing::log_ended(&mut docker, id, Ok(()));
        press(&mut docker, KeyCode::Esc);
        assert!(docker.log.is_none());
        // A late line for the closed view is dropped without harm.
        testing::log_line(&mut docker, id, "late".into());
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
        testing::loaded(&mut docker, 0, Ok(lists));
        assert!(docker.view().marked.is_empty());
        assert!(docker.view().selected.is_some());
        assert_ne!(selected_name(&docker), "backups");
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
