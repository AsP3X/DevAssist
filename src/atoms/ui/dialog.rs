//! The box a dialog is drawn in, and the row of buttons under it.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use super::hits::{Hits, contains};

/// A box of this size in the middle of `area`: at least 8 by 4, and never
/// larger than the area.
pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.max(8).min(area.width);
    let height = height.max(4).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Clears `popup`, draws a rounded border with `title` in the `accent`
/// colour, and returns the area inside the border.
pub fn window(frame: &mut Frame, popup: Rect, title: &str, accent: Color) -> Rect {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(accent))
        .title(Span::styled(
            title.to_string(),
            Style::default().fg(accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(Clear, popup);
    frame.render_widget(block, popup);
    inner
}

/// Draws the buttons from the left of `area`, records them in `hits`, and
/// returns the column after the last one. The button that does `primary`
/// stands out; the one under `mouse` is lit.
pub fn buttons<A: Copy + PartialEq>(
    frame: &mut Frame,
    area: Rect,
    buttons: &[(&str, A)],
    primary: A,
    mouse: Option<(u16, u16)>,
    hits: &mut Hits<A>,
) -> u16 {
    let mut x = area.x;
    for (label, action) in buttons {
        let text = format!(" {label} ");
        let width = text.len() as u16;
        if x + width > area.x + area.width {
            break;
        }
        let rect = Rect {
            x,
            y: area.y,
            width,
            height: 1,
        };
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if hovered {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if *action == primary {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        hits.buttons.push((rect, *action));
        x += width + 1;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn centered_sits_in_the_middle_and_stays_inside() {
        let screen = Rect::new(0, 0, 80, 24);
        assert_eq!(centered(screen, 40, 10), Rect::new(20, 7, 40, 10));
        // Never smaller than 8 by 4, never larger than the screen.
        assert_eq!(centered(screen, 2, 1), Rect::new(36, 10, 8, 4));
        assert_eq!(
            centered(Rect::new(0, 0, 6, 3), 40, 10),
            Rect::new(0, 0, 6, 3)
        );
    }

    #[test]
    fn a_window_has_a_rounded_titled_border_and_hands_back_its_inside() {
        let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
        let mut inner = Rect::default();
        terminal
            .draw(|frame| {
                inner = window(frame, Rect::new(0, 0, 20, 5), " Keys ", Color::Cyan);
            })
            .unwrap();
        assert_eq!(inner, Rect::new(1, 1, 18, 3));
        assert!(row(&terminal, 0).starts_with("╭ Keys ─"));
        assert!(row(&terminal, 4).starts_with("╰─"));
    }

    #[test]
    fn buttons_are_drawn_left_to_right_and_remembered() {
        let mut terminal = Terminal::new(TestBackend::new(20, 1)).unwrap();
        let mut hits: Hits<u8> = Hits::default();
        let mut end = 0;
        terminal
            .draw(|frame| {
                end = buttons(
                    frame,
                    Rect::new(0, 0, 20, 1),
                    &[("Save", 1), ("Cancel", 2)],
                    1,
                    None,
                    &mut hits,
                );
            })
            .unwrap();
        assert_eq!(row(&terminal, 0), " Save   Cancel      ");
        assert_eq!(end, 16);
        assert_eq!(
            hits.buttons,
            vec![(Rect::new(0, 0, 6, 1), 1), (Rect::new(7, 0, 8, 1), 2)]
        );

        // A button that does not fit is left out, with the ones after it.
        let mut hits: Hits<u8> = Hits::default();
        terminal
            .draw(|frame| {
                buttons(
                    frame,
                    Rect::new(0, 0, 10, 1),
                    &[("Save", 1), ("Cancel", 2)],
                    1,
                    None,
                    &mut hits,
                );
            })
            .unwrap();
        assert_eq!(hits.buttons.len(), 1);
    }
}
