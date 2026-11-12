//! The frame every feature runs in: the terminal, the main loop, and the
//! list of features.

mod terminal;

use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{self, Event};

/// One thing the program can do, as the shell sees it.
pub trait Feature {
    /// A key, a mouse action, a paste or a resize.
    fn on_event(&mut self, event: Event);
    /// Takes in what background work reported. True when something
    /// changed and the screen should be drawn again.
    fn drain(&mut self) -> bool;
    /// True while something is moving, so the screen is drawn at a
    /// steady pace without waiting for a key.
    fn waiting(&self) -> bool;
    fn draw(&mut self, frame: &mut Frame);
    /// What the feature asks of the program, if anything.
    fn request(&self) -> Option<Request>;
}

/// What a feature can ask of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Quit,
    /// Quit and start the program again: an update was installed.
    Restart,
}

/// The features, and which one is on screen.
struct Shell {
    features: Vec<Box<dyn Feature>>,
    active: usize,
}

impl Shell {
    fn new(features: Vec<Box<dyn Feature>>) -> Self {
        Self {
            features,
            active: 0,
        }
    }

    fn on_event(&mut self, event: Event) {
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.on_event(event);
        }
    }

    /// Background work goes on in every feature, on screen or not.
    fn drain(&mut self) -> bool {
        let mut changed = false;
        for feature in &mut self.features {
            changed |= feature.drain();
        }
        changed
    }

    fn waiting(&self) -> bool {
        self.features.iter().any(|feature| feature.waiting())
    }

    fn draw(&mut self, frame: &mut Frame) {
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.draw(frame);
        }
    }

    fn request(&self) -> Option<Request> {
        let mut asked = None;
        for feature in &self.features {
            match feature.request() {
                Some(Request::Restart) => return Some(Request::Restart),
                Some(Request::Quit) => asked = Some(Request::Quit),
                None => {}
            }
        }
        asked
    }
}

/// Runs the program until a feature asks to quit. `make` builds the
/// features once the console is ready for them.
pub fn run(make: impl FnOnce() -> Vec<Box<dyn Feature>>) -> Result<(), Box<dyn std::error::Error>> {
    terminal::enable_utf8();
    let mut shell = Shell::new(make());
    let mut screen = terminal::setup()?;
    let mut dirty = true;
    let result = loop {
        if dirty || shell.waiting() {
            screen.draw(|frame| shell.draw(frame))?;
            dirty = false;
        }
        if shell.request().is_some() {
            break Ok(());
        }
        let wait = if shell.waiting() { 40 } else { 200 };
        if event::poll(Duration::from_millis(wait))? {
            while event::poll(Duration::from_millis(0))? {
                shell.on_event(event::read()?);
            }
            dirty = true;
        }
        if shell.drain() {
            dirty = true;
        }
    };
    terminal::restore(&mut screen)?;
    if shell.request() == Some(Request::Restart) {
        // An update was installed: hand the terminal to the new version.
        let exe = std::env::current_exe()?;
        let status = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .status()?;
        std::process::exit(status.code().unwrap_or(0));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::widgets::Paragraph;
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<String>>>;

    struct Stub {
        name: &'static str,
        log: Log,
        changed: bool,
        waiting: bool,
        request: Option<Request>,
    }

    impl Feature for Stub {
        fn on_event(&mut self, _: Event) {
            self.log.borrow_mut().push(format!("{} event", self.name));
        }
        fn drain(&mut self) -> bool {
            self.log.borrow_mut().push(format!("{} drain", self.name));
            self.changed
        }
        fn waiting(&self) -> bool {
            self.waiting
        }
        fn draw(&mut self, frame: &mut Frame) {
            frame.render_widget(Paragraph::new(self.name), frame.area());
        }
        fn request(&self) -> Option<Request> {
            self.request
        }
    }

    fn stub(name: &'static str, log: &Log) -> Stub {
        Stub {
            name,
            log: log.clone(),
            changed: false,
            waiting: false,
            request: None,
        }
    }

    fn shell_of(stubs: Vec<Stub>) -> Shell {
        Shell::new(
            stubs
                .into_iter()
                .map(|stub| Box::new(stub) as Box<dyn Feature>)
                .collect(),
        )
    }

    fn a_key() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
    }

    #[test]
    fn an_event_goes_to_the_active_feature_only() {
        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log), stub("two", &log)]);
        shell.on_event(a_key());
        shell.active = 1;
        shell.on_event(a_key());
        assert_eq!(*log.borrow(), ["one event", "two event"]);
    }

    #[test]
    fn every_feature_is_drained_even_when_it_is_not_on_screen() {
        let log = Log::default();
        let mut second = stub("two", &log);
        second.changed = true;
        let mut shell = shell_of(vec![stub("one", &log), second]);
        assert!(shell.drain());
        assert_eq!(*log.borrow(), ["one drain", "two drain"]);

        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log)]);
        assert!(!shell.drain());
    }

    #[test]
    fn the_shell_waits_while_any_feature_does() {
        let log = Log::default();
        let mut second = stub("two", &log);
        assert!(!shell_of(vec![stub("one", &log), stub("two", &log)]).waiting());
        second.waiting = true;
        assert!(shell_of(vec![stub("one", &log), second]).waiting());
    }

    #[test]
    fn a_restart_asked_by_any_feature_wins_over_a_quit() {
        let log = Log::default();
        assert_eq!(shell_of(vec![stub("one", &log)]).request(), None);
        let mut quits = stub("one", &log);
        quits.request = Some(Request::Quit);
        let mut restarts = stub("two", &log);
        restarts.request = Some(Request::Restart);
        assert_eq!(
            shell_of(vec![quits, restarts]).request(),
            Some(Request::Restart)
        );
        let mut quits = stub("one", &log);
        quits.request = Some(Request::Quit);
        assert_eq!(
            shell_of(vec![quits, stub("two", &log)]).request(),
            Some(Request::Quit)
        );
    }

    #[test]
    fn only_the_active_feature_is_drawn() {
        let log = Log::default();
        let mut shell = shell_of(vec![stub("one", &log), stub("two", &log)]);
        shell.active = 1;
        let mut terminal = Terminal::new(TestBackend::new(8, 1)).unwrap();
        terminal.draw(|frame| shell.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..8)
            .map(|x| buffer.cell((x, 0)).unwrap().symbol().to_string())
            .collect();
        assert_eq!(row.trim_end(), "two");
    }
}
