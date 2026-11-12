//! The shell's own line at the top of the screen: where you are, and a
//! note.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::atoms::ui::hits::contains;
use crate::atoms::ui::text::fit_end;

/// The program's name, as the top line shows it.
const TITLE: &str = "DevAssist";

/// How the note on the right is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Something happened, or something is on offer.
    News,
    Error,
    /// A standing hint.
    Hint,
}

/// Draws ` DevAssist › Docker ` from the left and `note` at the right
/// end. `tool` is the tool on screen; none on the welcome screen, where
/// the name is left out too. Returns where the program's name is: a
/// click there goes home.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    tool: Option<&str>,
    note: (&str, Tone),
    mouse: Option<(u16, u16)>,
) -> Rect {
    let end = area.x + area.width;
    let mut used = area.x + 1;
    let mut home = Rect::default();
    if let Some(tool) = tool {
        home = Rect {
            x: area.x + 1,
            y: area.y,
            width: (TITLE.len() as u16).min(area.width.saturating_sub(1)),
            height: 1,
        };
        let hovered = mouse.is_some_and(|(mx, my)| contains(home, mx, my));
        let title = if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };
        let line = Line::from(vec![
            Span::styled(TITLE, title),
            Span::styled(" › ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                tool.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let width = (line.width() as u16).min(area.width.saturating_sub(1));
        frame.render_widget(
            Paragraph::new(line),
            Rect {
                x: area.x + 1,
                y: area.y,
                width,
                height: 1,
            },
        );
        used += width;
    }

    let room = end.saturating_sub(used + 3) as usize;
    let text = fit_end(note.0, room);
    let width = UnicodeWidthStr::width(text.as_str()) as u16;
    if width > 0 {
        let style = match note.1 {
            Tone::News => Style::default().fg(Color::Cyan),
            Tone::Error => Style::default().fg(Color::Red),
            Tone::Hint => Style::default().fg(Color::DarkGray),
        };
        let rect = Rect {
            x: end - width - 1,
            y: area.y,
            width,
            height: 1,
        };
        frame.render_widget(Paragraph::new(Span::styled(text, style)), rect);
    }
    home
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn row(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, 0)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn it_says_which_tool_is_on_screen_and_the_note_sits_at_the_right() {
        let mut terminal = Terminal::new(TestBackend::new(50, 1)).unwrap();
        let mut home = Rect::default();
        terminal
            .draw(|frame| {
                home = draw(
                    frame,
                    frame.area(),
                    Some("Docker"),
                    ("ctrl-p tools", Tone::Hint),
                    None,
                );
            })
            .unwrap();
        let line = row(&terminal);
        assert!(line.starts_with(" DevAssist › Docker "), "{line:?}");
        assert!(line.ends_with("ctrl-p tools "), "{line:?}");
        // The program's name is what a click takes home.
        assert_eq!(home, Rect::new(1, 0, 9, 1));
    }

    #[test]
    fn the_welcome_screen_has_only_the_note() {
        let mut terminal = Terminal::new(TestBackend::new(50, 1)).unwrap();
        let mut home = Rect::new(1, 1, 1, 1);
        terminal
            .draw(|frame| {
                home = draw(frame, frame.area(), None, ("? help", Tone::Hint), None);
            })
            .unwrap();
        let line = row(&terminal);
        assert!(!line.contains("DevAssist"));
        assert!(line.ends_with("? help "), "{line:?}");
        assert_eq!(home, Rect::default());
    }

    #[test]
    fn a_long_note_keeps_its_end_and_leaves_the_place_alone() {
        let mut terminal = Terminal::new(TestBackend::new(40, 1)).unwrap();
        terminal
            .draw(|frame| {
                draw(
                    frame,
                    frame.area(),
                    Some("Docker"),
                    ("version 9.0.0 runs after a restart, press v", Tone::News),
                    None,
                );
            })
            .unwrap();
        let line = row(&terminal);
        assert!(line.starts_with(" DevAssist › Docker "), "{line:?}");
        assert!(line.trim_end().ends_with("press v"), "{line:?}");
    }
}
