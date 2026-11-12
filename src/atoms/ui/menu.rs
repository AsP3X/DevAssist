//! A menu of lines, each with a key that runs it.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::hits::{Hits, contains};
use super::text::fit;

/// What a key did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Run the line with this index.
    Run(usize),
}

/// One line of a menu.
pub struct Entry<'a> {
    pub label: &'a str,
    /// The key that runs it, in lowercase.
    pub key: char,
    /// Shown as a warning, for a line that removes something.
    pub warn: bool,
}

/// Handles a key. `keys` holds each line's key, in lowercase and in order.
/// Esc and `q` close; arrows, `j`, `k`, Home and End move the highlight;
/// Enter runs the highlighted line; a line's own key runs that line.
pub fn on_key(selected: &mut usize, key: KeyEvent, keys: &[char]) -> Outcome {
    let last = keys.len().saturating_sub(1);
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Outcome::Close,
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1).min(last),
        KeyCode::Home => *selected = 0,
        KeyCode::End => *selected = last,
        KeyCode::Enter if !keys.is_empty() => return Outcome::Run((*selected).min(last)),
        KeyCode::Char(ch) => {
            let wanted = ch.to_ascii_lowercase();
            if let Some(index) = keys.iter().position(|key| *key == wanted) {
                return Outcome::Run(index);
            }
        }
        _ => {}
    }
    Outcome::Stay
}

/// Draws one line per entry from the top of `area` and records each in
/// `hits` under the name `menu:<index>`. Lines that do not fit are left out.
pub fn draw<A>(
    frame: &mut Frame,
    area: Rect,
    entries: &[Entry],
    selected: usize,
    mouse: Option<(u16, u16)>,
    hits: &mut Hits<A>,
) {
    let width = area.width as usize;
    for (index, entry) in entries.iter().enumerate() {
        let y = area.y + index as u16;
        if y >= area.y + area.height {
            break;
        }
        let rect = Rect {
            x: area.x,
            y,
            width: area.width,
            height: 1,
        };
        let chosen = index == selected;
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let gap = width.saturating_sub(UnicodeWidthStr::width(entry.label) + 6);
        let line = format!(
            "{} {}{}{}  ",
            if chosen { "▌" } else { " " },
            entry.label,
            " ".repeat(gap),
            entry.key
        );
        let style = if chosen {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if entry.warn {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        frame.render_widget(Paragraph::new(Span::styled(fit(&line, width), style)), rect);
        hits.rows.push((rect, format!("menu:{index}")));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    const KEYS: [char; 3] = ['u', 'o', 'd'];

    fn press(selected: &mut usize, code: KeyCode) -> Outcome {
        on_key(selected, KeyEvent::new(code, KeyModifiers::NONE), &KEYS)
    }

    #[test]
    fn arrows_move_the_highlight_and_stop_at_the_ends() {
        let mut selected = 0;
        assert_eq!(press(&mut selected, KeyCode::Up), Outcome::Stay);
        assert_eq!(selected, 0);
        press(&mut selected, KeyCode::Down);
        press(&mut selected, KeyCode::Char('j'));
        press(&mut selected, KeyCode::Down);
        assert_eq!(selected, 2);
        press(&mut selected, KeyCode::Char('k'));
        assert_eq!(selected, 1);
        press(&mut selected, KeyCode::Home);
        assert_eq!(selected, 0);
        press(&mut selected, KeyCode::End);
        assert_eq!(selected, 2);
    }

    #[test]
    fn enter_runs_the_highlighted_line_and_a_key_runs_its_own() {
        let mut selected = 1;
        assert_eq!(press(&mut selected, KeyCode::Enter), Outcome::Run(1));
        assert_eq!(press(&mut selected, KeyCode::Char('d')), Outcome::Run(2));
        assert_eq!(press(&mut selected, KeyCode::Char('O')), Outcome::Run(1));
        assert_eq!(press(&mut selected, KeyCode::Char('x')), Outcome::Stay);
        // A highlight past the end runs the last line.
        let mut selected = 9;
        assert_eq!(press(&mut selected, KeyCode::Enter), Outcome::Run(2));
    }

    #[test]
    fn esc_and_q_close_and_an_empty_menu_runs_nothing() {
        let mut selected = 0;
        assert_eq!(press(&mut selected, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut selected, KeyCode::Char('q')), Outcome::Close);
        let none = on_key(
            &mut selected,
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &[],
        );
        assert_eq!(none, Outcome::Stay);
    }

    #[test]
    fn lines_are_drawn_with_their_key_and_remembered() {
        let entries = [
            Entry {
                label: "Update",
                key: 'u',
                warn: false,
            },
            Entry {
                label: "Remove…",
                key: 'd',
                warn: true,
            },
        ];
        let mut terminal = Terminal::new(TestBackend::new(24, 3)).unwrap();
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| draw(frame, Rect::new(0, 0, 24, 3), &entries, 1, None, &mut hits))
            .unwrap();
        let row = |y: u16| -> String {
            let buffer = terminal.backend().buffer();
            (0..24)
                .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
                .collect()
        };
        assert_eq!(row(0).trim_end(), format!("  Update{}u", " ".repeat(12)));
        assert_eq!(row(1).trim_end(), format!("▌ Remove…{}d", " ".repeat(11)));
        assert_eq!(
            hits.rows,
            vec![
                (Rect::new(0, 0, 24, 1), "menu:0".to_string()),
                (Rect::new(0, 1, 24, 1), "menu:1".to_string()),
            ]
        );

        // Lines that do not fit in the area are left out.
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| draw(frame, Rect::new(0, 0, 24, 1), &entries, 0, None, &mut hits))
            .unwrap();
        assert_eq!(hits.rows.len(), 1);
    }
}
