//! A window onto lines that keep arriving: it follows the newest, scrolls
//! back, and shows only the lines that match a filter.

use std::collections::VecDeque;
use std::iter::Peekable;
use std::str::Chars;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

use super::dialog::window;
use super::input::{self, Alt};
use super::text::fit;

/// How many lines are kept. The oldest make room for new ones.
const CAP: usize = 5_000;

/// What a key did to the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

/// The lines, and where the reader is in them.
pub struct LogView {
    title: String,
    lines: VecDeque<String>,
    /// The newest line stays on screen.
    follow: bool,
    /// The first line shown, counted among the lines that match.
    scroll: usize,
    filter: String,
    /// The filter field has the keyboard.
    typing: bool,
    /// Why no more lines will come, once that is so.
    ended: Option<String>,
    /// How many lines the last frame had room for.
    page: usize,
}

impl LogView {
    /// An empty view that follows what arrives.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            lines: VecDeque::new(),
            follow: true,
            scroll: 0,
            filter: String::new(),
            typing: false,
            ended: None,
            page: 0,
        }
    }

    /// Adds a line. Escape sequences, such as the ones that colour text,
    /// and other control characters are left out; a tab becomes four
    /// spaces.
    pub fn push(&mut self, line: &str) {
        if self.lines.len() >= CAP
            && let Some(dropped) = self.lines.pop_front()
            && !self.follow
            && matches(&dropped, &self.filter.to_lowercase())
        {
            // The lines on screen stay where they are.
            self.scroll = self.scroll.saturating_sub(1);
        }
        self.lines.push_back(clean(line));
    }

    /// The stream is over. The note, such as "ended" or an error, is
    /// shown in the title and before the hints.
    pub fn end(&mut self, note: impl Into<String>) {
        self.ended = Some(note.into());
    }

    /// True while the filter field has the keyboard.
    pub fn typing(&self) -> bool {
        self.typing
    }

    /// Handles a key. While the filter is typed, Enter keeps it and Esc
    /// drops it. Otherwise arrows, `j`, `k`, PgUp, PgDn and Home scroll,
    /// End and `f` follow the newest line, `/` starts a filter, Esc drops
    /// the filter or, without one, closes; `q` closes.
    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        if self.typing {
            match key.code {
                KeyCode::Enter => self.typing = false,
                KeyCode::Esc => {
                    self.typing = false;
                    self.set_filter(String::new());
                }
                _ => {
                    let mut filter = self.filter.clone();
                    if input::edit(&mut filter, key, Alt::Ignored) {
                        self.set_filter(filter);
                    }
                }
            }
            return Outcome::Stay;
        }
        let page = self.page.max(1) as isize;
        match key.code {
            KeyCode::Char('q') => return Outcome::Close,
            KeyCode::Esc if self.filter.is_empty() => return Outcome::Close,
            KeyCode::Esc => self.set_filter(String::new()),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-page),
            KeyCode::PageDown => self.scroll_by(page),
            KeyCode::Home => self.scroll_by(isize::MIN),
            KeyCode::End | KeyCode::Char('f') => self.follow = true,
            KeyCode::Char('/') => self.typing = true,
            _ => {}
        }
        Outcome::Stay
    }

    /// Adds pasted text to the filter while it is being typed.
    pub fn paste(&mut self, text: &str) {
        if !self.typing {
            return;
        }
        let mut filter = self.filter.clone();
        filter.extend(text.chars().filter(|ch| !ch.is_control()));
        self.set_filter(filter);
    }

    /// Scrolls `delta` lines; negative is up. Scrolling up stops
    /// following, and reaching the end follows again.
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.count().saturating_sub(self.page);
        let from = if self.follow {
            max
        } else {
            self.scroll.min(max)
        };
        let to = (from as isize).saturating_add(delta).clamp(0, max as isize) as usize;
        self.scroll = to;
        self.follow = to >= max;
    }

    /// Fills `area` with a titled window: the lines, cut to its width,
    /// and a last row that says what the view is doing and which keys it
    /// has, or holds the filter while it is typed.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.page = 0;
        if area.width == 0 || area.height == 0 {
            return;
        }
        let title = match &self.ended {
            Some(note) => format!(" {} · {note} ", self.title),
            None => format!(" {} ", self.title),
        };
        let inner = window(frame, area, &title, Color::Cyan);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let width = inner.width as usize;
        let rows = inner.height as usize - 1;
        self.page = rows;

        let needle = self.filter.to_lowercase();
        let shown: Vec<&String> = self
            .lines
            .iter()
            .filter(|line| matches(line, &needle))
            .collect();
        let max = shown.len().saturating_sub(rows);
        if self.follow || self.scroll >= max {
            self.scroll = max;
            self.follow = true;
        }
        for (index, line) in shown.iter().skip(self.scroll).take(rows).enumerate() {
            let rect = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            frame.render_widget(Paragraph::new(fit(line, width)), rect);
        }

        let (status, colour) = if self.typing {
            (
                format!("/{}▌  enter keep · esc clear", self.filter),
                Color::Yellow,
            )
        } else {
            (self.status(shown.len()), Color::DarkGray)
        };
        let rect = Rect::new(inner.x, inner.y + rows as u16, inner.width, 1);
        let style = Style::default().fg(colour);
        frame.render_widget(
            Paragraph::new(Span::styled(fit(&status, width), style)),
            rect,
        );
    }

    /// The last row while the filter is not being typed. `shown` is how
    /// many lines match.
    fn status(&self, shown: usize) -> String {
        let mut parts: Vec<String> = Vec::new();
        if let Some(note) = &self.ended {
            parts.push(note.clone());
        }
        parts.push(if self.follow {
            "following".to_string()
        } else {
            "paused · end follows".to_string()
        });
        let total = self.lines.len();
        let unit = if total == 1 { "line" } else { "lines" };
        if self.filter.is_empty() {
            parts.push(format!("{} {unit}", grouped(total)));
            parts.push("/ filter · esc close".to_string());
        } else {
            parts.push(format!("{} of {} {unit}", grouped(shown), grouped(total)));
            parts.push(format!("filter: {}", self.filter));
            parts.push("/ filter · esc clear".to_string());
        }
        parts.join(" · ")
    }

    /// How many lines match the filter.
    fn count(&self) -> usize {
        if self.filter.is_empty() {
            return self.lines.len();
        }
        let needle = self.filter.to_lowercase();
        self.lines
            .iter()
            .filter(|line| matches(line, &needle))
            .count()
    }

    /// Changes the filter and goes to the newest line that matches it.
    fn set_filter(&mut self, filter: String) {
        if filter != self.filter {
            self.filter = filter;
            self.follow = true;
        }
    }
}

/// Whether `line` holds `needle`, which is in lowercase, in any case.
fn matches(line: &str, needle: &str) -> bool {
    needle.is_empty() || line.to_lowercase().contains(needle)
}

/// `line` without escape sequences and control characters, and with four
/// spaces for each tab.
fn clean(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\x1b' => skip_escape(&mut chars),
            '\t' => out.push_str("    "),
            ch if ch.is_control() => {}
            ch => out.push(ch),
        }
    }
    out
}

/// Skips the rest of an escape sequence whose Esc was just read.
fn skip_escape(chars: &mut Peekable<Chars>) {
    match chars.next() {
        // A control sequence, such as a colour: up to its final letter.
        Some('[') => {
            for ch in chars.by_ref() {
                if ('@'..='~').contains(&ch) {
                    break;
                }
            }
        }
        // A string, such as a window title or a link: up to a bell or
        // to Esc and a backslash.
        Some(']' | 'P' | 'X' | '^' | '_') => {
            while let Some(ch) = chars.next() {
                if ch == '\x07' {
                    break;
                }
                if ch == '\x1b' {
                    if chars.peek() == Some(&'\\') {
                        chars.next();
                    }
                    break;
                }
            }
        }
        // Any other sequence: its in-between characters and one more.
        Some(ch) if (' '..='/').contains(&ch) => {
            for ch in chars.by_ref() {
                if !(' '..='/').contains(&ch) {
                    break;
                }
            }
        }
        _ => {}
    }
}

/// A number with a comma between its thousands.
fn grouped(number: usize) -> String {
    let digits = number.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn press(view: &mut LogView, code: KeyCode) -> Outcome {
        view.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(view: &mut LogView, text: &str) {
        for ch in text.chars() {
            press(view, KeyCode::Char(ch));
        }
    }

    /// A view holding `line 0` to `line <count - 1>`.
    fn numbered(count: usize) -> LogView {
        let mut view = LogView::new("logs api");
        for index in 0..count {
            view.push(&format!("line {index}"));
        }
        view
    }

    /// Draws the view over a whole screen of this size and hands back its
    /// rows, without the spaces at their ends.
    fn screen(view: &mut LogView, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| view.draw(frame, Rect::new(0, 0, width, height)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| {
                let row: String = (0..width)
                    .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                    .collect();
                row.trim_end().to_string()
            })
            .collect()
    }

    /// The text between the borders of row `y`.
    fn inside(rows: &[String], y: usize) -> String {
        rows[y]
            .trim_start_matches('│')
            .trim_end_matches('│')
            .trim_end()
            .to_string()
    }

    #[test]
    fn the_cap_drops_the_oldest_lines() {
        let view = numbered(CAP + 10);
        assert_eq!(view.lines.len(), CAP);
        assert_eq!(view.lines.front().map(String::as_str), Some("line 10"));
        let last = format!("line {}", CAP + 9);
        assert_eq!(view.lines.back(), Some(&last));
    }

    #[test]
    fn colour_codes_and_control_characters_are_stripped() {
        let mut view = LogView::new("logs");
        view.push("\x1b[31mred\x1b[0m");
        view.push("\x1b[1;32mok\x1b[0m done\r");
        view.push("\x1b]0;window title\x07shown");
        view.push("\x1b]8;;http://example.test\x1b\\link\x1b]8;;\x1b\\ after");
        view.push("a\tb\x07\x08");
        view.push("\x1b(Bplain\x1b");
        let lines: Vec<&str> = view.lines.iter().map(String::as_str).collect();
        assert_eq!(
            lines,
            vec!["red", "ok done", "shown", "link after", "a    b", "plain"]
        );
    }

    #[test]
    fn following_keeps_the_newest_line_on_screen() {
        // Six rows: two of border, one of hints, three of lines.
        let mut view = numbered(10);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 7");
        assert_eq!(inside(&rows, 3), "line 9");
        view.push("line 10");
        view.push("line 11");
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 9");
        assert_eq!(inside(&rows, 3), "line 11");
        assert!(inside(&rows, 4).starts_with("following"));
    }

    #[test]
    fn scrolling_up_stops_following_and_end_resumes() {
        let mut view = numbered(10);
        screen(&mut view, 40, 6);
        view.scroll_by(-2);
        assert!(!view.follow);
        view.push("line 10");
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 5");
        assert_eq!(inside(&rows, 3), "line 7");
        assert!(inside(&rows, 4).starts_with("paused · end follows"));

        assert_eq!(press(&mut view, KeyCode::End), Outcome::Stay);
        assert!(view.follow);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 3), "line 10");

        // The keys scroll too, and `f` follows again.
        press(&mut view, KeyCode::Up);
        press(&mut view, KeyCode::Char('k'));
        assert!(!view.follow);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 3), "line 8");
        press(&mut view, KeyCode::Char('f'));
        assert!(view.follow);

        // Scrolling down to the end follows again.
        view.scroll_by(-3);
        assert!(!view.follow);
        view.scroll_by(1);
        assert!(!view.follow);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Char('j'));
        assert!(view.follow);
    }

    #[test]
    fn page_keys_move_by_what_the_last_frame_showed() {
        let mut view = numbered(20);
        screen(&mut view, 40, 6);
        press(&mut view, KeyCode::PageUp);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 14");
        press(&mut view, KeyCode::Home);
        assert!(!view.follow);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 0");
        press(&mut view, KeyCode::PageDown);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 3");
        // Nothing to scroll: the view keeps following.
        let mut view = numbered(2);
        screen(&mut view, 40, 6);
        press(&mut view, KeyCode::Home);
        press(&mut view, KeyCode::PageUp);
        assert!(view.follow);
    }

    #[test]
    fn a_paused_view_stays_on_its_lines_when_old_ones_are_dropped() {
        let mut view = numbered(CAP);
        screen(&mut view, 40, 6);
        view.scroll_by(-10);
        let before = screen(&mut view, 40, 6);
        view.push("one more");
        view.push("and another");
        let after = screen(&mut view, 40, 6);
        assert_eq!(inside(&before, 1), inside(&after, 1));
        assert_eq!(view.lines.len(), CAP);
    }

    #[test]
    fn the_filter_shows_only_matching_lines() {
        let mut view = LogView::new("logs");
        for line in ["GET /health", "error: boom", "GET /users", "ERROR again"] {
            view.push(line);
        }
        assert!(!view.typing());
        assert_eq!(press(&mut view, KeyCode::Char('/')), Outcome::Stay);
        assert!(view.typing());
        // Letters that are keys elsewhere are typed here.
        type_text(&mut view, "errq");
        press(&mut view, KeyCode::Backspace);
        view.paste("o\nr");
        assert_eq!(view.filter, "error");
        let rows = screen(&mut view, 40, 7);
        assert_eq!(inside(&rows, 1), "error: boom");
        assert_eq!(inside(&rows, 2), "ERROR again");
        assert_eq!(inside(&rows, 3), "");
        assert!(inside(&rows, 5).starts_with("/error▌"));

        assert_eq!(press(&mut view, KeyCode::Enter), Outcome::Stay);
        assert!(!view.typing());
        assert_eq!(view.filter, "error");
        let rows = screen(&mut view, 60, 7);
        assert_eq!(inside(&rows, 2), "ERROR again");
        assert!(inside(&rows, 5).contains("2 of 4 lines"));
        assert!(inside(&rows, 5).contains("filter: error"));

        // A paste goes nowhere unless the filter is being typed.
        view.paste("x");
        assert_eq!(view.filter, "error");
    }

    #[test]
    fn esc_closes_the_filter_first_and_the_view_second() {
        let mut view = numbered(3);
        press(&mut view, KeyCode::Char('/'));
        type_text(&mut view, "line");
        // While typing, Esc drops the filter and leaves the field.
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Stay);
        assert!(!view.typing());
        assert_eq!(view.filter, "");

        press(&mut view, KeyCode::Char('/'));
        type_text(&mut view, "line");
        press(&mut view, KeyCode::Enter);
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Stay);
        assert_eq!(view.filter, "");
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut view, KeyCode::Char('q')), Outcome::Close);
    }

    #[test]
    fn the_title_the_lines_and_the_hint_row_are_drawn() {
        let mut view = LogView::new("logs api");
        view.push("listening on :8080");
        view.push("a line that is far too long to fit into the window it is shown in");
        let rows = screen(&mut view, 50, 6);
        assert!(rows[0].starts_with("╭ logs api ─"));
        assert_eq!(inside(&rows, 1), "listening on :8080");
        assert_eq!(
            inside(&rows, 2),
            "a line that is far too long to fit into the win…"
        );
        assert_eq!(
            inside(&rows, 4),
            "following · 2 lines · / filter · esc close"
        );
        assert!(rows[5].starts_with("╰─"));

        view.end("ended");
        let rows = screen(&mut view, 50, 6);
        assert!(rows[0].starts_with("╭ logs api · ended ─"));
        assert!(inside(&rows, 4).starts_with("ended · following"));
    }

    #[test]
    fn large_counts_are_grouped_in_thousands() {
        let mut view = numbered(1234);
        let rows = screen(&mut view, 60, 6);
        assert!(inside(&rows, 4).contains("1,234 lines"));
        let mut view = numbered(1);
        let rows = screen(&mut view, 60, 6);
        assert!(inside(&rows, 4).contains("· 1 line ·"));
    }

    #[test]
    fn a_tiny_area_does_not_panic() {
        for (width, height) in [(10, 3), (2, 2), (1, 1), (3, 1), (1, 5)] {
            let mut view = numbered(30);
            view.end("ended");
            screen(&mut view, width, height);
            press(&mut view, KeyCode::PageUp);
            press(&mut view, KeyCode::Char('/'));
            screen(&mut view, width, height);
        }
        let mut view = numbered(30);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| view.draw(frame, Rect::new(0, 0, 0, 0)))
            .unwrap();
        view.scroll_by(-1);
        view.scroll_by(isize::MAX);
        view.scroll_by(isize::MIN);
    }
}
