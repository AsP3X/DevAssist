//! The shell's own line at the top of the screen: the tools, and a note.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::atoms::ui::hits::contains;
use crate::atoms::ui::text::fit_end;

/// How the note on the right is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Something happened, or something is on offer.
    News,
    Error,
    /// A standing hint.
    Hint,
}

/// Draws ` 1 Repos  2 Docker ` from the left and `note` at the right end.
/// Returns where each tool's name is, for clicks; a name that does not fit
/// gets an empty rectangle.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    names: &[&str],
    active: usize,
    note: (&str, Tone),
    mouse: Option<(u16, u16)>,
) -> Vec<Rect> {
    let end = area.x + area.width;
    let mut rects = Vec::with_capacity(names.len());
    let mut x = area.x + 1;
    for (index, name) in names.iter().enumerate() {
        let text = format!(" {} {name} ", index + 1);
        let width = UnicodeWidthStr::width(text.as_str()) as u16;
        if x + width > end {
            rects.push(Rect::default());
            continue;
        }
        let rect = Rect {
            x,
            y: area.y,
            width,
            height: 1,
        };
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if index == active {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Gray)
        };
        frame.render_widget(Paragraph::new(Span::styled(text, style)), rect);
        rects.push(rect);
        x += width + 1;
    }

    let room = end.saturating_sub(x + 2) as usize;
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
    rects
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
    fn the_tools_are_numbered_and_the_note_sits_at_the_right() {
        let mut terminal = Terminal::new(TestBackend::new(50, 1)).unwrap();
        let mut rects = Vec::new();
        terminal
            .draw(|frame| {
                rects = draw(
                    frame,
                    frame.area(),
                    &["Repos", "Docker"],
                    1,
                    ("? help", Tone::Hint),
                    None,
                );
            })
            .unwrap();
        let line = row(&terminal);
        assert!(line.starts_with("  1 Repos   2 Docker "), "{line:?}");
        assert!(line.ends_with("? help "), "{line:?}");
        assert_eq!(rects, [Rect::new(1, 0, 9, 1), Rect::new(11, 0, 10, 1)]);
        // The active tool stands out.
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer.cell((12, 0)).unwrap().bg, Color::Cyan);
        assert_ne!(buffer.cell((2, 0)).unwrap().bg, Color::Cyan);
    }

    #[test]
    fn a_tool_that_does_not_fit_is_left_out() {
        let mut terminal = Terminal::new(TestBackend::new(14, 1)).unwrap();
        let mut rects = Vec::new();
        terminal
            .draw(|frame| {
                rects = draw(
                    frame,
                    frame.area(),
                    &["Repos", "Docker"],
                    0,
                    ("", Tone::Hint),
                    None,
                );
            })
            .unwrap();
        assert_eq!(rects[1], Rect::default());
        assert!(!row(&terminal).contains("Docker"));
    }
}
