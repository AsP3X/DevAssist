//! The frame every tool runs in: the terminal, the main loop, the list of
//! tools and what they share: the top line, switching, text selection,
//! help and the program's own update.

mod chrome;
pub mod config;
mod help;
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
use select::{Pass, Selection};
use tools::ToolMenu;
use updater::Updater;

/// One tool of the program, as the shell sees it.
pub trait Feature {
    /// The name in the top line and in the tool menu.
    fn name(&self) -> &'static str;
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

/// The shell's own dialog, over whatever tool is on screen.
enum Dialog {
    None,
    Help,
    /// The version dialog.
    Update,
    Tools(ToolMenu),
}

/// The smallest screen anything is drawn on.
const MIN_SIZE: (u16, u16) = (48, 13);

/// The tools, which one is on screen, and what they share.
struct Shell {
    features: Vec<Box<dyn Feature>>,
    active: usize,
    dialog: Dialog,
    selection: Selection,
    updater: Updater,
    mouse: Option<(u16, u16)>,
    /// Where the top line drew each tool's name.
    strip: Vec<Rect>,
    /// What the open dialog drew that can be clicked.
    hits: Hits<Act>,
    tick: usize,
    leave: Option<Leave>,
    /// The last frame had no room for anything but a message.
    cramped: bool,
}

impl Shell {
    fn new(features: Vec<Box<dyn Feature>>, updater: Updater) -> Self {
        let mut shell = Self {
            features,
            active: 0,
            dialog: Dialog::None,
            selection: Selection::default(),
            updater,
            mouse: None,
            strip: Vec::new(),
            hits: Hits::default(),
            tick: 0,
            leave: None,
            cramped: false,
        };
        if let Some(first) = shell.features.first_mut() {
            first.shown();
        }
        shell
    }

    fn names(&self) -> Vec<&'static str> {
        self.features.iter().map(|feature| feature.name()).collect()
    }

    fn switch_to(&mut self, index: usize) {
        if index >= self.features.len() {
            return;
        }
        self.dialog = Dialog::None;
        if index != self.active {
            self.active = index;
            self.features[index].shown();
        }
    }

    fn forward(&mut self, event: Event) {
        if let Some(feature) = self.features.get_mut(self.active) {
            feature.on_event(event);
        }
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
        let modal = self
            .features
            .get(self.active)
            .is_none_or(|feature| feature.modal());
        if !modal {
            let plain = !ctrl && !key.modifiers.contains(KeyModifiers::ALT);
            match key.code {
                KeyCode::Char('p') | KeyCode::Char('P') if ctrl => {
                    self.dialog = Dialog::Tools(ToolMenu::new(self.active));
                    return;
                }
                KeyCode::Char(digit @ '1'..='9') if plain => {
                    let index = digit as usize - '1' as usize;
                    if index < self.features.len() {
                        self.switch_to(index);
                        return;
                    }
                }
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
        let names = self.names();
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
            Dialog::Tools(menu) => match menu.on_key(key, &names) {
                tools::Outcome::Stay => {}
                tools::Outcome::Close => self.dialog = Dialog::None,
                tools::Outcome::Pick(index) => self.switch_to(index),
            },
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
        // The top line is the shell's: a click there never reaches the tool.
        if let Some(index) = self.strip.iter().position(|rect| contains(*rect, x, y)) {
            // As with the number keys: not while the tool has a dialog open.
            let modal = self
                .features
                .get(self.active)
                .is_none_or(|feature| feature.modal());
            if released && !modal {
                self.switch_to(index);
            }
            return;
        }
        self.forward(Event::Mouse(mouse));
    }

    fn click_dialog(&mut self, x: u16, y: u16) {
        let names = self.names();
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
            && let Some(index) = self
                .hits
                .row_at(x, y)
                .and_then(|row| menu.clicked(&row, &names))
        {
            self.switch_to(index);
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
        self.updater.waiting() || self.features.iter().any(|feature| feature.waiting())
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
        ("ctrl-p tools · ? help".to_string(), Tone::Hint)
    }

    fn draw(&mut self, frame: &mut Frame) {
        self.tick = self.tick.wrapping_add(1);
        self.hits.clear();
        let screen = frame.area();
        self.cramped = screen.width < MIN_SIZE.0 || screen.height < MIN_SIZE.1;
        if self.cramped {
            self.strip.clear();
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
        let names = self.names();
        let (note, tone) = self.note();
        self.strip = chrome::draw(frame, top, &names, self.active, (&note, tone), self.mouse);
        let (name, keys) = match self.features.get_mut(self.active) {
            Some(feature) => {
                feature.draw(frame, body);
                (feature.name(), feature.help())
            }
            None => ("", ""),
        };
        match &self.dialog {
            Dialog::None => {}
            Dialog::Help => help::draw(frame, screen, name, keys, self.mouse, &mut self.hits),
            Dialog::Update => {
                self.updater
                    .draw(frame, screen, self.tick, self.mouse, &mut self.hits);
            }
            Dialog::Tools(menu) => menu.draw(frame, screen, &names, self.mouse, &mut self.hits),
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
    let mut shell = Shell::new(make(), updater);
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

    fn shell_of(stubs: Vec<Stub>) -> Shell {
        Shell::new(
            stubs
                .into_iter()
                .map(|stub| Box::new(stub) as Box<dyn Feature>)
                .collect(),
            Updater::new(|| None),
        )
    }

    /// Two tools, with the log emptied of the first one's `shown`.
    fn two(log: &Log) -> Shell {
        let shell = shell_of(vec![stub("one", log), stub("two", log)]);
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
    fn an_event_goes_to_the_active_feature_only() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(key(KeyCode::Char('x')));
        shell.active = 1;
        shell.on_event(key(KeyCode::Char('x')));
        shell.on_event(Event::Paste("text".into()));
        assert_eq!(*log.borrow(), ["one key x", "two key x", "two paste text"]);
    }

    #[test]
    fn a_digit_switches_tools_and_tells_the_tool_it_is_shown() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(key(KeyCode::Char('2')));
        assert_eq!(shell.active, 1);
        // The tool that is already on screen is not shown again.
        shell.on_event(key(KeyCode::Char('2')));
        // There is no third tool: the key is the tool's.
        shell.on_event(key(KeyCode::Char('3')));
        shell.on_event(key(KeyCode::Char('1')));
        assert_eq!(shell.active, 0);
        assert_eq!(*log.borrow(), ["two shown", "two key 3", "one shown"]);
    }

    #[test]
    fn a_tool_that_has_the_keyboard_keeps_every_key() {
        let log = Log::default();
        let mut first = stub("one", &log);
        first.modal = true;
        let mut shell = shell_of(vec![first, stub("two", &log)]);
        log.borrow_mut().clear();
        shell.on_event(key(KeyCode::Char('2')));
        shell.on_event(key(KeyCode::Char('?')));
        shell.on_event(key(KeyCode::Char('v')));
        shell.on_event(ctrl('p'));
        assert_eq!(shell.active, 0);
        assert!(matches!(shell.dialog, Dialog::None));
        assert_eq!(
            *log.borrow(),
            ["one key 2", "one key ?", "one key v", "one key p"]
        );
    }

    #[test]
    fn the_top_line_lists_the_tools_and_the_tool_is_drawn_below_it() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.active = 1;
        let screen = render(&mut shell, 60, 14);
        let mut lines = screen.lines();
        let top = lines.next().unwrap();
        assert!(top.starts_with("  1 one   2 two "), "{top:?}");
        assert!(top.trim_end().ends_with("ctrl-p tools · ? help"), "{top:?}");
        assert_eq!(lines.next().unwrap().trim_end(), "body of two");
    }

    #[test]
    fn a_click_on_a_name_in_the_top_line_switches() {
        let log = Log::default();
        let mut shell = two(&log);
        render(&mut shell, 60, 14);
        click(&mut shell, 12, 0);
        assert_eq!(shell.active, 1);
        // A click below the top line is the tool's, press and release.
        click(&mut shell, 5, 3);
        assert_eq!(*log.borrow(), ["two shown", "two down 5,3", "two up 5,3"]);
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
    fn ctrl_p_opens_the_tool_menu_and_enter_goes_to_the_match() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(ctrl('p'));
        let screen = render(&mut shell, 60, 14);
        assert!(screen.contains("╭ Tools "));
        shell.on_event(key(KeyCode::Char('t')));
        shell.on_event(key(KeyCode::Char('w')));
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(shell.active, 1);
        assert!(matches!(shell.dialog, Dialog::None));
        assert_eq!(*log.borrow(), ["two shown"]);

        // A click on a line of the menu goes there too.
        shell.on_event(ctrl('p'));
        render(&mut shell, 60, 14);
        let (rect, _) = shell.hits.rows[0].clone();
        click(&mut shell, rect.x + 2, rect.y);
        assert_eq!(shell.active, 0);
    }

    #[test]
    fn help_shows_the_tools_keys_and_the_shells() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.on_event(key(KeyCode::Char('?')));
        let screen = render(&mut shell, 80, 20);
        assert!(screen.contains("╭ Keys · one "));
        assert!(screen.contains("does the stub's thing"));
        assert!(screen.contains("switch to that tool"));
        // The shell's keys come first, so a short screen cuts the tool's.
        assert!(screen.find("switch to that tool") < screen.find("does the stub's thing"));
        // While it is open, keys are not the tool's.
        shell.on_event(key(KeyCode::Char('x')));
        shell.on_event(key(KeyCode::Esc));
        assert!(matches!(shell.dialog, Dialog::None));
        assert!(log.borrow().is_empty());
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
    }

    #[test]
    fn every_feature_is_drained_even_when_it_is_not_on_screen() {
        let log = Log::default();
        let mut second = stub("two", &log);
        second.changed = true;
        let mut shell = shell_of(vec![stub("one", &log), second]);
        log.borrow_mut().clear();
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
    fn a_tool_that_is_not_on_screen_can_still_quit_the_program() {
        let log = Log::default();
        assert_eq!(shell_of(vec![stub("one", &log)]).leaving(), None);
        let mut quits = stub("two", &log);
        quits.request = Some(Request::Quit);
        assert_eq!(
            shell_of(vec![stub("one", &log), quits]).leaving(),
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
        shell.on_event(key(KeyCode::Char('2')));
        click(&mut shell, 5, 3);
        assert!(log.borrow().is_empty());
        assert_eq!(shell.active, 0);
        shell.on_event(ctrl('c'));
        assert_eq!(shell.leaving(), Some(Leave::Quit));

        let mut shell = two(&log);
        let screen = render(&mut shell, 48, 13);
        assert!(screen.contains("body of one"));
        shell.on_event(key(KeyCode::Enter));
        assert_eq!(*log.borrow(), ["one key Enter"]);
    }

    #[test]
    fn every_tool_is_told_when_the_program_ends() {
        let log = Log::default();
        let mut shell = two(&log);
        shell.close();
        assert_eq!(*log.borrow(), ["one closing", "two closing"]);
    }

    #[test]
    fn a_click_on_the_top_line_does_not_leave_a_tool_that_has_a_dialog_open() {
        let log = Log::default();
        let mut first = stub("one", &log);
        first.modal = true;
        let mut shell = shell_of(vec![first, stub("two", &log)]);
        render(&mut shell, 60, 14);
        click(&mut shell, 12, 0);
        assert_eq!(shell.active, 0);
    }
}
