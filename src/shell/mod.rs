//! The frame every tool runs in: the terminal, the main loop, the list of
//! tools and what they share: the welcome screen, the palette that goes
//! from one to another, the top line, text selection, help and the
//! program's own update.

mod chrome;
pub mod config;
mod help;
mod home;
pub mod keys;
mod select;
mod terminal;
mod tools;
pub mod update;
mod updater;

use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::widgets::Paragraph;

use crate::atoms::ui::hits::{Hits, contains};
use chrome::Tone;
use home::{Card, Home};
use keys::KeyCombo;
use select::{Pass, Selection};
use tools::{Place, ToolMenu};
use updater::Updater;

/// One tool of the program, as the shell sees it.
pub trait Feature {
    /// The name in the top line, on the welcome screen and in the palette.
    fn name(&self) -> &'static str;
    /// What the tool is for, in a few words, for its card on the welcome
    /// screen and its line in the palette.
    fn about(&self) -> &'static str;
    /// What the tool has to report right now, such as "56 clones · 3
    /// behind", for its card. None when it has nothing to say yet.
    fn status(&self) -> Option<String>;
    /// True while a text field or a dialog has the keyboard, so the shell
    /// leaves its own keys to the tool.
    fn modal(&self) -> bool;
    /// The tool's keys, one `key   what it does` line each, for the help
    /// dialog.
    fn help(&self) -> &'static str;
    /// The tool came on screen.
    fn shown(&mut self);
    /// A key, a mouse action, a paste or a resize. A left click arrives
    /// as a press and a release in the same place; drags never arrive.
    fn on_event(&mut self, event: Event);
    /// Takes in what background work reported. True when something
    /// changed and the screen should be drawn again.
    fn drain(&mut self) -> bool;
    /// True while something is moving, so the screen is drawn at a
    /// steady pace without waiting for a key.
    fn waiting(&self) -> bool;
    /// Draws into `area`: the screen below the shell's line.
    fn draw(&mut self, frame: &mut Frame, area: Rect);
    /// What the tool asks of the program, if anything.
    fn request(&self) -> Option<Request>;
    /// The program is about to end, whoever asked for it: stop what was
    /// started.
    fn closing(&mut self);
}

/// What a tool can ask of the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Quit,
}

/// What the shell needs from outside to do its own work.
pub struct Options {
    /// Finds the GitHub token to ask for releases with, if there is one.
    pub token: fn() -> Option<String>,
    /// Look for a newer version right away.
    pub look_for_update: bool,
    /// The key that opens the palette.
    pub palette_key: KeyCombo,
    /// Where to start: `home`, or the name of a tool in lowercase.
    pub start: String,
}

/// How the program ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Leave {
    Quit,
    /// Quit and start the program again: an update was installed.
    Restart,
}

/// What a button in one of the shell's dialogs does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Act {
    Confirm,
    Close,
}

/// The shell's own dialog, over whatever is on screen.
enum Dialog {
    None,
    Help,
    /// The version dialog.
    Update,
    /// The palette.
    Tools(ToolMenu),
}

/// The smallest screen anything is drawn on.
const MIN_SIZE: (u16, u16) = (48, 13);

/// The first place in the palette: the welcome screen.
const HOME: Place = Place {
    name: "Home",
    about: "the welcome screen",
};

/// The keys of the welcome screen, for the help dialog.
const HOME_KEYS: &str = "\
arrows, tab    choose a tool
enter          open it
a letter       search the tools by name
q              quit";

/// The tools, what is on screen, and what they share.
struct Shell {
    features: Vec<Box<dyn Feature>>,
    /// The tool that is on screen, or was before the welcome screen.
    active: usize,
    /// The welcome screen is on screen, not a tool.
    at_home: bool,
    home: Home,
    palette_key: KeyCombo,
    dialog: Dialog,
    selection: Selection,
    updater: Updater,
    mouse: Option<(u16, u16)>,
    /// Where the top line drew the program's name: a click goes home.
    title: Rect,
    /// What the open dialog drew that can be clicked.
    hits: Hits<Act>,
    tick: usize,
    leave: Option<Leave>,
    /// The last frame had no room for anything but a message.
    cramped: bool,
}

impl Shell {
    /// `start` is `home` or a tool's name in lowercase; anything else
    /// starts at home. `animated` lets the logo draw itself.
    fn new(
        features: Vec<Box<dyn Feature>>,
        updater: Updater,
        palette_key: KeyCombo,
        start: &str,
        animated: bool,
    ) -> Self {
        let mut shell = Self {
            features,
            active: 0,
            at_home: true,
            home: Home::new(animated),
            palette_key,
            dialog: Dialog::None,
            selection: Selection::default(),
            updater,
            mouse: None,
            title: Rect::default(),
            hits: Hits::default(),
            tick: 0,
            leave: None,
            cramped: false,
        };
        let wanted = start.trim().to_lowercase();
        if let Some(index) = shell
            .features
            .iter()
            .position(|feature| feature.name().to_lowercase() == wanted)
        {
            shell.switch_to(index);
        }
        shell
    }

    /// Home, then every tool: what the palette lists.
    fn places(&self) -> Vec<Place<'static>> {
        let mut places = vec![HOME];
        places.extend(self.features.iter().map(|feature| Place {
            name: feature.name(),
            about: feature.about(),
        }));
        places
    }

    /// Goes to a place of the palette.
    fn go(&mut self, place: usize) {
        match place.checked_sub(1) {
            None => self.go_home(),
            Some(index) => self.switch_to(index),
        }
    }

    fn go_home(&mut self) {
        self.dialog = Dialog::None;
        self.at_home = true;
        self.home.selected = self.active;
    }

    fn switch_to(&mut self, index: usize) {
        if index >= self.features.len() {
            return;
        }
        self.dialog = Dialog::None;
        if self.at_home || index != self.active {
            self.at_home = false;
            self.active = index;
            self.features[index].shown();
        }
    }

    /// The tool on screen, if one is.
    fn tool(&self) -> Option<&dyn Feature> {
        if self.at_home {
            return None;
        }
        self.features.get(self.active).map(|feature| &**feature)
    }

    /// True while the tool on screen has the keyboard for itself. The
    /// welcome screen never has.
    fn modal(&self) -> bool {
        self.tool().is_some_and(|feature| feature.modal())
    }

    fn forward(&mut self, event: Event) {
        if self.at_home {
            return;
        }
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.on_event(event);
        }
    }

    fn open_palette(&mut self) {
        let current = if self.at_home { 0 } else { self.active + 1 };
        self.dialog = Dialog::Tools(ToolMenu::new(current));
    }

    fn on_event(&mut self, event: Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Release => {}
            Event::Key(key) => self.on_key(key),
            Event::Mouse(mouse) => self.on_mouse(mouse),
            Event::Paste(text) => match &mut self.dialog {
                Dialog::None => self.forward(Event::Paste(text)),
                Dialog::Tools(menu) => menu.paste(&text),
                Dialog::Help | Dialog::Update => {}
            },
            other => self.forward(other),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if self.selection.on_key(key) == Pass::Swallow {
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let ctrl_c = ctrl && matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'));
        if self.cramped {
            // Nothing is on screen to act on: only quitting is safe.
            if ctrl_c {
                self.leave = Some(Leave::Quit);
            }
            return;
        }
        if !matches!(self.dialog, Dialog::None) {
            if ctrl_c {
                self.leave = Some(Leave::Quit);
                return;
            }
            self.on_dialog_key(key);
            return;
        }
        if !self.modal() && self.palette_key.matches(key) {
            self.open_palette();
            return;
        }
        if self.at_home {
            if ctrl_c {
                self.leave = Some(Leave::Quit);
                return;
            }
            match self.home.on_key(key, self.features.len()) {
                home::Outcome::Stay => {}
                home::Outcome::Open(index) => self.switch_to(index),
                home::Outcome::Search(text) => {
                    self.dialog = Dialog::Tools(ToolMenu::typed(&text));
                }
                home::Outcome::Help => self.dialog = Dialog::Help,
                home::Outcome::Version => self.open_update(),
                home::Outcome::Quit => self.leave = Some(Leave::Quit),
            }
            return;
        }
        if !self.modal() {
            let plain = !ctrl && !key.modifiers.contains(KeyModifiers::ALT);
            match key.code {
                KeyCode::Char('?') if plain => {
                    self.dialog = Dialog::Help;
                    return;
                }
                KeyCode::Char('v') | KeyCode::Char('V') if plain => {
                    self.open_update();
                    return;
                }
                _ => {}
            }
        }
        self.forward(Event::Key(key));
    }

    fn open_update(&mut self) {
        self.dialog = Dialog::Update;
        self.updater.opened();
    }

    fn on_dialog_key(&mut self, key: KeyEvent) {
        let places = self.places();
        match &mut self.dialog {
            Dialog::None => {}
            Dialog::Help => {
                if matches!(
                    key.code,
                    KeyCode::Esc | KeyCode::Char('?') | KeyCode::Enter | KeyCode::Char('q')
                ) {
                    self.dialog = Dialog::None;
                }
            }
            Dialog::Update => match self.updater.on_key(key) {
                updater::Outcome::Stay => {}
                updater::Outcome::Close => self.dialog = Dialog::None,
                updater::Outcome::Restart => self.leave = Some(Leave::Restart),
            },
            Dialog::Tools(menu) => {
                // The key that opened the palette closes it again.
                if self.palette_key.matches(key) {
                    self.dialog = Dialog::None;
                    return;
                }
                match menu.on_key(key, &places) {
                    tools::Outcome::Stay => {}
                    tools::Outcome::Close => self.dialog = Dialog::None,
                    tools::Outcome::Pick(place) => self.go(place),
                }
            }
        }
    }

    fn on_mouse(&mut self, mut mouse: MouseEvent) {
        self.mouse = Some((mouse.column, mouse.row));
        if self.selection.on_mouse(&mut mouse) == Pass::Swallow {
            return;
        }
        if self.cramped {
            return;
        }
        let (x, y) = (mouse.column, mouse.row);
        let released = mouse.kind == MouseEventKind::Up(MouseButton::Left);
        if !matches!(self.dialog, Dialog::None) {
            if released {
                self.click_dialog(x, y);
            }
            return;
        }
        if self.at_home {
            if released && let Some(index) = self.home.card_at(x, y) {
                self.switch_to(index);
            }
            return;
        }
        // The program's name in the top line goes home, as the palette
        // does: not while the tool has a dialog open.
        if contains(self.title, x, y) {
            if released && !self.modal() {
                self.go_home();
            }
            return;
        }
        self.forward(Event::Mouse(mouse));
    }

    fn click_dialog(&mut self, x: u16, y: u16) {
        let places = self.places();
        if let Some(act) = self.hits.button_at(x, y) {
            match (&self.dialog, act) {
                (Dialog::Update, Act::Confirm) => {
                    if self.updater.enter() == updater::Outcome::Restart {
                        self.leave = Some(Leave::Restart);
                    }
                }
                _ => self.dialog = Dialog::None,
            }
            return;
        }
        if let Dialog::Tools(menu) = &self.dialog
            && let Some(place) = self
                .hits
                .row_at(x, y)
                .and_then(|row| menu.clicked(&row, &places))
        {
            self.go(place);
        }
    }

    /// Background work goes on in every tool, on screen or not.
    fn drain(&mut self) -> bool {
        let mut changed = self.updater.drain();
        for feature in &mut self.features {
            changed |= feature.drain();
        }
        changed
    }

    fn waiting(&self) -> bool {
        self.updater.waiting()
            || (self.at_home && self.home.animating())
            || self.features.iter().any(|feature| feature.waiting())
    }

    /// The note at the right of the top line: what a copy did, news about
    /// a version, or the standing hint.
    fn note(&self) -> (String, Tone) {
        if let Some((text, failed)) = self.selection.note() {
            let tone = if failed { Tone::Error } else { Tone::News };
            return (text.to_string(), tone);
        }
        if let Some(news) = self.updater.notice() {
            return (news, Tone::News);
        }
        (
            format!("{} tools · ? help", self.palette_key.label()),
            Tone::Hint,
        )
    }

    fn draw(&mut self, frame: &mut Frame) {
        self.tick = self.tick.wrapping_add(1);
        self.hits.clear();
        let screen = frame.area();
        self.cramped = screen.width < MIN_SIZE.0 || screen.height < MIN_SIZE.1;
        if self.cramped {
            self.title = Rect::default();
            frame.render_widget(Paragraph::new("Make the terminal a bit larger."), screen);
            self.selection.finish(frame.buffer_mut());
            return;
        }
        let top = Rect {
            height: 1,
            ..screen
        };
        let body = Rect {
            y: screen.y + 1,
            height: screen.height - 1,
            ..screen
        };
        let (note, tone) = self.note();
        let key = self.palette_key.label();
        let name = self.tool().map(|feature| feature.name());
        // Nothing under a dialog lights up under the mouse.
        let mouse = match self.dialog {
            Dialog::None => self.mouse,
            _ => None,
        };
        self.title = chrome::draw(frame, top, name, (&note, tone), mouse);
        let (name, keys) = if self.at_home {
            let cards: Vec<Card> = self
                .features
                .iter()
                .map(|feature| Card {
                    name: feature.name(),
                    about: feature.about(),
                    status: feature.status(),
                })
                .collect();
            self.home.draw(frame, body, &cards, &key, mouse);
            ("Home", HOME_KEYS)
        } else {
            match self.features.get_mut(self.active) {
                Some(feature) => {
                    feature.draw(frame, body);
                    (feature.name(), feature.help())
                }
                None => ("", ""),
            }
        };
        match &self.dialog {
            Dialog::None => {}
            Dialog::Help => help::draw(frame, screen, name, keys, &key, self.mouse, &mut self.hits),
            Dialog::Update => {
                self.updater
                    .draw(frame, screen, self.tick, self.mouse, &mut self.hits);
            }
            Dialog::Tools(menu) => {
                menu.draw(frame, screen, &self.places(), self.mouse, &mut self.hits);
            }
        }
        self.selection.finish(frame.buffer_mut());
    }

    /// Tells every tool the program is ending.
    fn close(&mut self) {
        for feature in &mut self.features {
            feature.closing();
        }
    }

    /// Whether the program should end, and how. A restart is the shell's
    /// own doing; a tool can only ask to quit.
    fn leaving(&self) -> Option<Leave> {
        if self.leave.is_some() {
            return self.leave;
        }
        self.features
            .iter()
            .any(|feature| feature.request() == Some(Request::Quit))
            .then_some(Leave::Quit)
    }
}

/// Runs the program until a tool asks to quit. `make` builds the tools
/// once the console is ready for them.
pub fn run(
    options: Options,
    make: impl FnOnce() -> Vec<Box<dyn Feature>>,
) -> Result<(), Box<dyn std::error::Error>> {
    terminal::enable_utf8();
    update::clean_up();
    let mut updater = Updater::new(options.token);
    if options.look_for_update {
        updater.look(true);
    }
    let mut shell = Shell::new(make(), updater, options.palette_key, &options.start, true);
    let mut screen = terminal::setup()?;
    let mut dirty = true;
    let result = loop {
        if dirty || shell.waiting() {
            screen.draw(|frame| shell.draw(frame))?;
            dirty = false;
        }
        if shell.leaving().is_some() {
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
    let leave = shell.leaving();
    // Let the tools stop what they started before another copy runs.
    shell.close();
    drop(shell);
    if leave == Some(Leave::Restart) {
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
    use std::cell::RefCell;
    use std::rc::Rc;

    type Log = Rc<RefCell<Vec<String>>>;

    struct Stub {
        name: &'static str,
        log: Log,
        changed: bool,
        waiting: bool,
        modal: bool,
        request: Option<Request>,
    }

    impl Feature for Stub {
        fn name(&self) -> &'static str {
            self.name
        }
        fn about(&self) -> &'static str {
            match self.name {
                "one" => "the first stub, for apples",
                _ => "the second stub, for pears",
            }
        }
        fn status(&self) -> Option<String> {
            (self.name == "one").then(|| "3 apples".to_string())
        }
        fn modal(&self) -> bool {
            self.modal
        }
        fn help(&self) -> &'static str {
            "x              does the stub's thing"
        }
        fn shown(&mut self) {
            self.log.borrow_mut().push(format!("{} shown", self.name));
        }
        fn on_event(&mut self, event: Event) {
            let what = match event {
                Event::Key(key) => format!("key {}", key.code),
                Event::Mouse(mouse) => match mouse.kind {
                    MouseEventKind::Down(_) => format!("down {},{}", mouse.column, mouse.row),
                    MouseEventKind::Up(_) => format!("up {},{}", mouse.column, mouse.row),
                    _ => "mouse".to_string(),
                },
                Event::Paste(text) => format!("paste {text}"),
                _ => "event".to_string(),
            };
            self.log.borrow_mut().push(format!("{} {what}", self.name));
        }
        fn drain(&mut self) -> bool {
            self.log.borrow_mut().push(format!("{} drain", self.name));
            self.changed
        }
        fn waiting(&self) -> bool {
            self.waiting
        }
        fn draw(&mut self, frame: &mut Frame, area: Rect) {
            frame.render_widget(Paragraph::new(format!("body of {}", self.name)), area);
        }
        fn request(&self) -> Option<Request> {
            self.request
        }
        fn closing(&mut self) {
            self.log.borrow_mut().push(format!("{} closing", self.name));
        }
    }

    fn stub(name: &'static str, log: &Log) -> Stub {
        Stub {
            name,
            log: log.clone(),
            changed: false,
            waiting: false,
            modal: false,
            request: None,
        }
    }

    /// A shell that starts at `start`, with the palette on ctrl-p.
    fn shell_at(stubs: Vec<Stub>, start: &str) -> Shell {
        Shell::new(
            stubs
                .into_iter()
                .map(|stub| Box::new(stub) as Box<dyn Feature>)
                .collect(),
            Updater::new(|| None),
            KeyCombo::default(),
            start,
            false,
        )
    }

    /// Two tools, on the welcome screen.
    fn at_home(log: &Log) -> Shell {
        let shell = shell_at(vec![stub("one", log), stub("two", log)], "home");
        assert!(log.borrow().is_empty());
        shell
    }

    /// Two tools, with the first on screen and the log emptied of that.
    fn two(log: &Log) -> Shell {
        let shell = shell_at(vec![stub("one", log), stub("two", log)], "one");
        assert_eq!(*log.borrow(), ["one shown"]);
        log.borrow_mut().clear();
        shell
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn ctrl(ch: char) -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::CONTROL))
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        })
    }

    fn click(shell: &mut Shell, column: u16, row: u16) {
        shell.on_event(mouse(MouseEventKind::Down(MouseButton::Left), column, row));
        shell.on_event(mouse(MouseEventKind::Up(MouseButton::Left), column, row));
    }

    fn type_text(shell: &mut Shell, text: &str) {
        for ch in text.chars() {
            shell.on_event(key(KeyCode::Char(ch)));
        }
    }

    fn render(shell: &mut Shell, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| shell.draw(frame)).unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn the_program_starts_on_the_welcome_screen_with_a_card_per_tool() {
        let log = Log::default();
        let mut shell = at_home(&log);
        let screen = render(&mut shell, 100, 30);
        assert!(screen.contains("██████"));
        assert!(screen.contains("╭ ▌ one "));
        assert!(screen.contains("the first stub, for apples"));
        assert!(screen.contains("● 3 apples"));
        assert!(screen.contains("╭ two "));
        assert!(screen.contains("○ opens when you choose it"));
        // No tool is on screen: the top line names none.
        let top = screen.lines().next().unwrap();
        assert!(!top.contains("DevAssist"));
        assert!(top.trim_end().ends_with("ctrl-p tools · ? help"), "{top:?}");
        assert!(!screen.contains("body of"));
    }

    #[test]
    fn enter_or_a_click_on_a_card_opens_its_tool() {
        let log = Log::default();
        let mut shell = at_home(&log);
        render(&mut shell, 100, 30);
        shell.on_event(key(KeyCode::Right));
        shell.on_event(key(KeyCode::Enter));
        assert!(!shell.at_home);
        assert_eq!(shell.active, 1);
        assert_eq!(*log.borrow(), ["two shown"]);
        let screen = render(&mut shell, 100, 30);
        let mut lines = screen.lines();
        assert!(lines.next().unwrap().starts_with(" DevAssist › two "));
        assert_eq!(lines.next().unwrap().trim_end(), "body of two");

        let log = Log::default();
        let mut shell = at_home(&log);
        render(&mut shell, 100, 30);
        let card = shell.home.card_at_index(1);
        click(&mut shell, card.x + 2, card.y + 2);
        assert_eq!(shell.active, 1);
        assert_eq!(*log.borrow(), ["two shown"]);
    }

    #[test]
    fn the_start_setting_opens_a_tool_directly() {
        let log = Log::default();
        let shell = shell_at(vec![stub("one", &log), stub("two", &log)], "Two");
        assert!(!shell.at_home);
        assert_eq!(shell.active, 1);
        assert_eq!(*log.borrow(), ["two shown"]);
        // A name no tool has starts at home.
        let log = Log::default();
        let shell = shell_at(vec![stub("one", &log)], "orbit");
        assert!(shell.at_home);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn the_palette_goes_to_any_tool_and_back_home() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(ctrl('p'));
        let screen = render(&mut shell, 80, 20);
        assert!(screen.contains("╭ Tools "));
        assert!(screen.contains("Home"));
        assert!(screen.contains("the second stub, for pears"));
        type_text(&mut shell, "tw");
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(shell.active, 1);
        assert!(matches!(shell.dialog, Dialog::None));
        assert_eq!(*log.borrow(), ["two shown"]);

        // A tool is found by what it is for, too.
        shell.on_event(ctrl('p'));
        type_text(&mut shell, "apples");
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(shell.active, 0);

        // Home is the first place; from there a tool is shown anew.
        shell.on_event(ctrl('p'));
        type_text(&mut shell, "home");
        shell.on_event(key(KeyCode::Enter));
        assert!(shell.at_home);
        assert_eq!(shell.home.selected, 0);
        log.borrow_mut().clear();
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(*log.borrow(), ["one shown"]);

        // A click on a line of the palette goes there too.
        shell.on_event(ctrl('p'));
        render(&mut shell, 80, 20);
        let (rect, _) = shell.hits.rows[2].clone();
        click(&mut shell, rect.x + 2, rect.y);
        assert_eq!(shell.active, 1);

        // The key that opens the palette closes it.
        shell.on_event(ctrl('p'));
        shell.on_event(ctrl('p'));
        assert!(matches!(shell.dialog, Dialog::None));
    }

    #[test]
    fn typing_on_the_welcome_screen_searches_the_tools() {
        let log = Log::default();
        let mut shell = at_home(&log);
        type_text(&mut shell, "tw");
        let screen = render(&mut shell, 80, 20);
        assert!(screen.contains("› tw▌"), "{screen}");
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(shell.active, 1);
        assert!(!shell.at_home);
    }

    #[test]
    fn the_palette_key_can_be_another_one() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.palette_key = KeyCombo::parse("ctrl-k").unwrap();
        shell.on_event(ctrl('p'));
        assert!(matches!(shell.dialog, Dialog::None));
        assert_eq!(*log.borrow(), ["one key p"]);
        shell.on_event(ctrl('k'));
        assert!(matches!(shell.dialog, Dialog::Tools(_)));
        shell.on_event(key(KeyCode::Esc));
        let screen = render(&mut shell, 80, 20);
        assert!(screen.lines().next().unwrap().contains("ctrl-k tools"));
        shell.on_event(key(KeyCode::Char('?')));
        let screen = render(&mut shell, 80, 24);
        assert!(screen.contains("ctrl-k         the tools: search them and go to one"));
    }

    #[test]
    fn an_event_goes_to_the_tool_on_screen_only() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(key(KeyCode::Char('x')));
        // The number keys are the tool's own.
        shell.on_event(key(KeyCode::Char('2')));
        shell.active = 1;
        shell.on_event(key(KeyCode::Char('x')));
        shell.on_event(Event::Paste("text".into()));
        assert_eq!(
            *log.borrow(),
            ["one key x", "one key 2", "two key x", "two paste text"]
        );
        // On the welcome screen no tool hears anything.
        let log = Log::default();
        let mut shell = at_home(&log);
        shell.on_event(key(KeyCode::Down));
        shell.on_event(Event::Paste("text".into()));
        click(&mut shell, 5, 3);
        assert!(log.borrow().is_empty());
    }

    #[test]
    fn a_tool_that_has_the_keyboard_keeps_every_key() {
        let log = Log::default();
        let mut first = stub("one", &log);
        first.modal = true;
        let mut shell = shell_at(vec![first, stub("two", &log)], "one");
        log.borrow_mut().clear();
        shell.on_event(key(KeyCode::Char('?')));
        shell.on_event(key(KeyCode::Char('v')));
        shell.on_event(ctrl('p'));
        assert_eq!(shell.active, 0);
        assert!(matches!(shell.dialog, Dialog::None));
        assert_eq!(*log.borrow(), ["one key ?", "one key v", "one key p"]);
        // Nor does a click on the program's name take it away.
        render(&mut shell, 60, 14);
        click(&mut shell, 3, 0);
        assert!(!shell.at_home);
    }

    #[test]
    fn a_click_on_the_programs_name_goes_home_and_other_clicks_are_the_tools() {
        let log = Log::default();
        let mut shell = two(&log);
        render(&mut shell, 60, 14);
        click(&mut shell, 5, 3);
        assert_eq!(*log.borrow(), ["one down 5,3", "one up 5,3"]);
        click(&mut shell, 3, 0);
        assert!(shell.at_home);
    }

    #[test]
    fn a_drag_selects_text_and_never_reaches_the_tool() {
        let log = Log::default();
        let mut shell = two(&log);
        render(&mut shell, 60, 14);
        let left = MouseButton::Left;
        shell.on_event(mouse(MouseEventKind::Down(left), 0, 1));
        shell.on_event(mouse(MouseEventKind::Drag(left), 3, 1));
        let screen = render(&mut shell, 60, 14);
        assert!(screen.lines().next().unwrap().contains("release to copy"));
        shell.on_event(mouse(MouseEventKind::Up(left), 3, 1));
        assert_eq!(shell.selection.copied, ["body"]);
        let screen = render(&mut shell, 60, 14);
        assert!(
            screen
                .lines()
                .next()
                .unwrap()
                .contains("Copied to the clipboard")
        );
        assert_eq!(*log.borrow(), ["one down 0,1"]);
    }

    #[test]
    fn help_shows_the_shells_keys_and_those_of_what_is_on_screen() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(key(KeyCode::Char('?')));
        let screen = render(&mut shell, 80, 20);
        assert!(screen.contains("╭ Keys · one "));
        assert!(screen.contains("does the stub's thing"));
        assert!(screen.contains("ctrl-p         the tools: search them and go to one"));
        // The shell's keys come first, so a short screen cuts the tool's.
        assert!(screen.find("the tools: search") < screen.find("does the stub's thing"));
        // While it is open, keys are not the tool's.
        shell.on_event(key(KeyCode::Char('x')));
        shell.on_event(key(KeyCode::Esc));
        assert!(matches!(shell.dialog, Dialog::None));
        assert!(log.borrow().is_empty());

        // The welcome screen has its own few keys.
        let mut shell = at_home(&log);
        shell.on_event(key(KeyCode::Char('?')));
        let screen = render(&mut shell, 80, 24);
        assert!(screen.contains("╭ Keys · Home "));
        assert!(screen.contains("search the tools by name"));
    }

    #[test]
    fn v_opens_the_version_dialog_and_a_restart_ends_the_program() {
        let log = Log::default();
        let mut shell = two(&log);
        // Nothing is looked up in a test: an update is already in place.
        shell.updater.state = updater::UpdateState::Installed("9.0.0".into());
        let screen = render(&mut shell, 80, 20);
        assert!(
            screen
                .lines()
                .next()
                .unwrap()
                .contains("version 9.0.0 runs after a restart, press v")
        );
        shell.on_event(key(KeyCode::Char('v')));
        let screen = render(&mut shell, 80, 20);
        assert!(screen.contains("✓ Version 9.0.0 is installed."));
        assert_eq!(shell.leaving(), None);
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(shell.leaving(), Some(Leave::Restart));

        // From the welcome screen too.
        let mut shell = at_home(&log);
        shell.updater.state = updater::UpdateState::Installed("9.0.0".into());
        shell.on_event(key(KeyCode::Char('v')));
        assert!(matches!(shell.dialog, Dialog::Update));
    }

    #[test]
    fn q_and_ctrl_c_quit_from_the_welcome_screen() {
        let log = Log::default();
        let mut shell = at_home(&log);
        shell.on_event(key(KeyCode::Char('q')));
        assert_eq!(shell.leaving(), Some(Leave::Quit));
        let mut shell = at_home(&log);
        shell.on_event(ctrl('c'));
        assert_eq!(shell.leaving(), Some(Leave::Quit));
    }

    #[test]
    fn every_feature_is_drained_even_when_it_is_not_on_screen() {
        let log = Log::default();
        let mut second = stub("two", &log);
        second.changed = true;
        let mut shell = shell_at(vec![stub("one", &log), second], "home");
        assert!(shell.drain());
        assert_eq!(*log.borrow(), ["one drain", "two drain"]);

        let log = Log::default();
        let mut shell = shell_at(vec![stub("one", &log)], "home");
        assert!(!shell.drain());
    }

    #[test]
    fn the_shell_waits_while_any_feature_does() {
        let log = Log::default();
        let mut second = stub("two", &log);
        assert!(!at_home(&log).waiting());
        second.waiting = true;
        assert!(shell_at(vec![stub("one", &log), second], "home").waiting());
    }

    #[test]
    fn a_tool_that_is_not_on_screen_can_still_quit_the_program() {
        let log = Log::default();
        assert_eq!(at_home(&log).leaving(), None);
        let mut quits = stub("two", &log);
        quits.request = Some(Request::Quit);
        assert_eq!(
            shell_at(vec![stub("one", &log), quits], "home").leaving(),
            Some(Leave::Quit)
        );
    }

    #[test]
    fn a_screen_that_is_too_small_says_so_and_takes_no_keys() {
        let log = Log::default();
        let mut shell = two(&log);
        let screen = render(&mut shell, 47, 13);
        assert!(screen.contains("Make the terminal a bit larger."));
        // What the keys would act on cannot be seen: they do nothing.
        shell.on_event(key(KeyCode::Enter));
        shell.on_event(ctrl('p'));
        click(&mut shell, 5, 3);
        assert!(log.borrow().is_empty());
        assert!(matches!(shell.dialog, Dialog::None));
        shell.on_event(ctrl('c'));
        assert_eq!(shell.leaving(), Some(Leave::Quit));

        let mut shell = two(&log);
        let screen = render(&mut shell, 48, 13);
        assert!(screen.contains("body of one"));
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(*log.borrow(), ["one key Enter"]);
        // The welcome screen fits the smallest screen too.
        log.borrow_mut().clear();
        let mut shell = at_home(&log);
        let screen = render(&mut shell, 48, 13);
        assert!(screen.contains("D e v A s s i s t"));
    }

    #[test]
    fn every_tool_is_told_when_the_program_ends() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.close();
        assert_eq!(*log.borrow(), ["one closing", "two closing"]);
    }
}
