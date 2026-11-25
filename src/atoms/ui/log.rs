//! A window onto lines that keep arriving: it follows the newest, scrolls
//! back, and shows only the lines that match a filter. The lines and the
//! reader's place in them are a `LogBuffer`.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;

pub use devassist_core::docker::logbuf::LogBuffer;

use super::dialog::window;
use super::input::{self, Alt};
use super::text::fit;

/// What a key did to the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
}

/// What the screen adds to a log: how many lines it has room for.
#[derive(Default)]
pub struct LogView {
    /// How many lines the last frame had room for.
    page: usize,
}

impl LogView {
    pub fn new() -> Self {
        Self { page: 0 }
    }

    /// Handles a key. While the filter is typed, Enter keeps it and Esc
    /// drops it. Otherwise arrows, `j`, `k`, PgUp, PgDn and Home scroll,
    /// End and `f` follow the newest line, `/` starts a filter, Esc drops
    /// the filter or, without one, closes; `q` closes.
    pub fn on_key(&mut self, buf: &mut LogBuffer, key: KeyEvent) -> Outcome {
        if buf.typing {
            match key.code {
                KeyCode::Enter => buf.typing = false,
                KeyCode::Esc => buf.drop_typed_filter(),
                _ => {
                    let mut filter = buf.filter.clone();
                    if input::edit(&mut filter, key, Alt::Ignored) {
                        buf.set_filter(filter);
                    }
                }
            }
            return Outcome::Stay;
        }
        let page = self.page.max(1) as isize;
        match key.code {
            KeyCode::Char('q') => return Outcome::Close,
            KeyCode::Esc if buf.filter.is_empty() => return Outcome::Close,
            KeyCode::Esc => buf.set_filter(String::new()),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(buf, -1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(buf, 1),
            KeyCode::PageUp => self.scroll_by(buf, -page),
            KeyCode::PageDown => self.scroll_by(buf, page),
            KeyCode::Home => self.scroll_by(buf, isize::MIN),
            KeyCode::End | KeyCode::Char('f') => buf.follow = true,
            KeyCode::Char('/') => buf.typing = true,
            _ => {}
        }
        Outcome::Stay
    }

    /// Scrolls `delta` lines; negative is up. Scrolling up stops
    /// following, and reaching the end follows again.
    pub fn scroll_by(&mut self, buf: &mut LogBuffer, delta: isize) {
        let max = buf.count().saturating_sub(self.page);
        let from = if buf.follow { max } else { buf.scroll.min(max) };
        let to = (from as isize).saturating_add(delta).clamp(0, max as isize) as usize;
        buf.scroll = to;
        buf.follow = to >= max;
    }

    /// Fills `area` with a titled window: the lines, cut to its width,
    /// and a last row that says what the view is doing and which keys it
    /// has, or holds the filter while it is typed.
    pub fn draw(&mut self, frame: &mut Frame, area: Rect, buf: &mut LogBuffer) {
        self.page = 0;
        if area.width == 0 || area.height == 0 {
            return;
        }
        let title = match &buf.ended {
            Some(note) => format!(" {} · {note} ", buf.title),
            None => format!(" {} ", buf.title),
        };
        let inner = window(frame, area, &title, Color::Cyan);
        if inner.width == 0 || inner.height == 0 {
            return;
        }
        let width = inner.width as usize;
        let rows = inner.height as usize - 1;
        self.page = rows;

        // The filter runs once: the lines it lets through give the count too.
        let shown: Vec<&str> = buf.shown().collect();
        let count = shown.len();
        let max = count.saturating_sub(rows);
        let follow = buf.follow || buf.scroll >= max;
        let scroll = if follow { max } else { buf.scroll };
        for (index, line) in shown.iter().skip(scroll).take(rows).enumerate() {
            let rect = Rect::new(inner.x, inner.y + index as u16, inner.width, 1);
            frame.render_widget(Paragraph::new(fit(line, width)), rect);
        }
        if follow {
            buf.scroll = max;
            buf.follow = true;
        }

        let (status, colour) = if buf.typing {
            (
                format!("/{}▌  enter keep · esc clear", buf.filter),
                Color::Yellow,
            )
        } else {
            (buf.status(count), Color::DarkGray)
        };
        let rect = Rect::new(inner.x, inner.y + rows as u16, inner.width, 1);
        let style = Style::default().fg(colour);
        frame.render_widget(
            Paragraph::new(Span::styled(fit(&status, width), style)),
            rect,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use devassist_core::docker::logbuf::CAP;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    /// A log and the view that shows it.
    struct Log {
        buf: LogBuffer,
        view: LogView,
    }

    impl Log {
        fn new(title: &str) -> Self {
            Self {
                buf: LogBuffer::new(title),
                view: LogView::new(),
            }
        }

        fn scroll_by(&mut self, delta: isize) {
            self.view.scroll_by(&mut self.buf, delta);
        }
    }

    fn press(log: &mut Log, code: KeyCode) -> Outcome {
        log.view
            .on_key(&mut log.buf, KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_text(log: &mut Log, text: &str) {
        for ch in text.chars() {
            press(log, KeyCode::Char(ch));
        }
    }

    /// A log holding `line 0` to `line <count - 1>`.
    fn numbered(count: usize) -> Log {
        let mut log = Log::new("logs api");
        for index in 0..count {
            log.buf.push(&format!("line {index}"));
        }
        log
    }

    /// Draws the view over a whole screen of this size and hands back its
    /// rows, without the spaces at their ends.
    fn screen(log: &mut Log, width: u16, height: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                log.view
                    .draw(frame, Rect::new(0, 0, width, height), &mut log.buf)
            })
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
    fn following_keeps_the_newest_line_on_screen() {
        // Six rows: two of border, one of hints, three of lines.
        let mut view = numbered(10);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 7");
        assert_eq!(inside(&rows, 3), "line 9");
        view.buf.push("line 10");
        view.buf.push("line 11");
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
        assert!(!view.buf.follow);
        view.buf.push("line 10");
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 5");
        assert_eq!(inside(&rows, 3), "line 7");
        assert!(inside(&rows, 4).starts_with("paused · end follows"));

        assert_eq!(press(&mut view, KeyCode::End), Outcome::Stay);
        assert!(view.buf.follow);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 3), "line 10");

        // The keys scroll too, and `f` follows again.
        press(&mut view, KeyCode::Up);
        press(&mut view, KeyCode::Char('k'));
        assert!(!view.buf.follow);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 3), "line 8");
        press(&mut view, KeyCode::Char('f'));
        assert!(view.buf.follow);

        // Scrolling down to the end follows again.
        view.scroll_by(-3);
        assert!(!view.buf.follow);
        view.scroll_by(1);
        assert!(!view.buf.follow);
        press(&mut view, KeyCode::Down);
        press(&mut view, KeyCode::Char('j'));
        assert!(view.buf.follow);
    }

    #[test]
    fn page_keys_move_by_what_the_last_frame_showed() {
        let mut view = numbered(20);
        screen(&mut view, 40, 6);
        press(&mut view, KeyCode::PageUp);
        let rows = screen(&mut view, 40, 6);
        assert_eq!(inside(&rows, 1), "line 14");
        press(&mut view, KeyCode::Home);
        assert!(!view.buf.follow);
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
        assert!(view.buf.follow);
    }

    #[test]
    fn a_paused_view_stays_on_its_lines_when_old_ones_are_dropped() {
        let mut view = numbered(CAP);
        screen(&mut view, 40, 6);
        view.scroll_by(-10);
        let before = screen(&mut view, 40, 6);
        view.buf.push("one more");
        view.buf.push("and another");
        let after = screen(&mut view, 40, 6);
        assert_eq!(inside(&before, 1), inside(&after, 1));
        assert_eq!(view.buf.count(), CAP);
    }

    #[test]
    fn the_filter_shows_only_matching_lines() {
        let mut view = Log::new("logs");
        for line in ["GET /health", "error: boom", "GET /users", "ERROR again"] {
            view.buf.push(line);
        }
        assert!(!view.buf.typing());
        assert_eq!(press(&mut view, KeyCode::Char('/')), Outcome::Stay);
        assert!(view.buf.typing());
        // Letters that are keys elsewhere are typed here.
        type_text(&mut view, "errq");
        press(&mut view, KeyCode::Backspace);
        view.buf.paste("o\nr");
        assert_eq!(view.buf.filter, "error");
        let rows = screen(&mut view, 40, 7);
        assert_eq!(inside(&rows, 1), "error: boom");
        assert_eq!(inside(&rows, 2), "ERROR again");
        assert_eq!(inside(&rows, 3), "");
        assert!(inside(&rows, 5).starts_with("/error▌"));

        assert_eq!(press(&mut view, KeyCode::Enter), Outcome::Stay);
        assert!(!view.buf.typing());
        assert_eq!(view.buf.filter, "error");
        let rows = screen(&mut view, 60, 7);
        assert_eq!(inside(&rows, 2), "ERROR again");
        assert!(inside(&rows, 5).contains("2 of 4 lines"));
        assert!(inside(&rows, 5).contains("filter: error"));

        // A paste goes nowhere unless the filter is being typed.
        view.buf.paste("x");
        assert_eq!(view.buf.filter, "error");
    }

    #[test]
    fn esc_closes_the_filter_first_and_the_view_second() {
        let mut view = numbered(3);
        press(&mut view, KeyCode::Char('/'));
        type_text(&mut view, "line");
        // While typing, Esc drops the filter and leaves the field.
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Stay);
        assert!(!view.buf.typing());
        assert_eq!(view.buf.filter, "");

        press(&mut view, KeyCode::Char('/'));
        type_text(&mut view, "line");
        press(&mut view, KeyCode::Enter);
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Stay);
        assert_eq!(view.buf.filter, "");
        assert_eq!(press(&mut view, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut view, KeyCode::Char('q')), Outcome::Close);
    }

    #[test]
    fn the_title_the_lines_and_the_hint_row_are_drawn() {
        let mut view = Log::new("logs api");
        view.buf.push("listening on :8080");
        view.buf
            .push("a line that is far too long to fit into the window it is shown in");
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

        view.buf.end("ended");
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
            view.buf.end("ended");
            screen(&mut view, width, height);
            press(&mut view, KeyCode::PageUp);
            press(&mut view, KeyCode::Char('/'));
            screen(&mut view, width, height);
        }
        let mut view = numbered(30);
        let mut terminal = Terminal::new(TestBackend::new(10, 3)).unwrap();
        terminal
            .draw(|frame| view.view.draw(frame, Rect::new(0, 0, 0, 0), &mut view.buf))
            .unwrap();
        view.scroll_by(-1);
        view.scroll_by(isize::MAX);
        view.scroll_by(isize::MIN);
    }
}
