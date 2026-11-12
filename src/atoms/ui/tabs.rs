//! A row of tabs, one of them chosen.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::hits::contains;

/// Handles a key. Tab moves to the next of `count` tabs and Shift-Tab to
/// the previous one; both wrap. True when the key was one of these.
pub fn on_key(selected: &mut usize, key: KeyEvent, count: usize) -> bool {
    let forward = match key.code {
        KeyCode::Tab => true,
        KeyCode::BackTab => false,
        _ => return false,
    };
    if count == 0 {
        *selected = 0;
        return true;
    }
    let current = (*selected).min(count - 1);
    *selected = if forward {
        (current + 1) % count
    } else {
        (current + count - 1) % count
    };
    true
}

/// Draws the labels in the first row of `area`, each with a space on
/// either side and one between them. The selected one stands out; the one
/// under `mouse` is lit. Returns where each label was drawn, in order; a
/// label that does not fit, and every one after it, gets an empty
/// rectangle.
pub fn draw(
    frame: &mut Frame,
    area: Rect,
    labels: &[&str],
    selected: usize,
    mouse: Option<(u16, u16)>,
) -> Vec<Rect> {
    let end = area.x.saturating_add(area.width);
    let mut rects = Vec::with_capacity(labels.len());
    let mut x = area.x;
    let mut full = area.height == 0;
    for (index, label) in labels.iter().enumerate() {
        let text = format!(" {label} ");
        let width = UnicodeWidthStr::width(text.as_str()) as u16;
        if full || x.saturating_add(width) > end {
            full = true;
            rects.push(Rect::new(x.min(end), area.y, 0, 0));
            continue;
        }
        let rect = Rect::new(x, area.y, width, 1);
        let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
        let style = if index == selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if hovered {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Cyan)
        };
        frame.render_widget(Paragraph::new(text).style(style), rect);
        rects.push(rect);
        x = x.saturating_add(width + 1);
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    const LABELS: [&str; 3] = ["Containers", "Compose", "Images"];

    fn press(selected: &mut usize, code: KeyCode, count: usize) -> bool {
        let modifiers = if code == KeyCode::BackTab {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        on_key(selected, KeyEvent::new(code, modifiers), count)
    }

    fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer.cell((x, y)).unwrap().symbol().to_string())
            .collect()
    }

    #[test]
    fn tab_moves_to_the_next_tab_and_wraps() {
        let mut selected = 0;
        assert!(press(&mut selected, KeyCode::Tab, 3));
        assert_eq!(selected, 1);
        press(&mut selected, KeyCode::Tab, 3);
        assert_eq!(selected, 2);
        assert!(press(&mut selected, KeyCode::Tab, 3));
        assert_eq!(selected, 0);
    }

    #[test]
    fn shift_tab_moves_to_the_previous_tab_and_wraps() {
        let mut selected = 0;
        assert!(press(&mut selected, KeyCode::BackTab, 3));
        assert_eq!(selected, 2);
        assert!(press(&mut selected, KeyCode::BackTab, 3));
        assert_eq!(selected, 1);
    }

    #[test]
    fn another_key_is_not_handled_and_changes_nothing() {
        let mut selected = 1;
        for code in [KeyCode::Right, KeyCode::Enter, KeyCode::Char('x')] {
            assert!(!press(&mut selected, code, 3));
        }
        assert_eq!(selected, 1);
    }

    #[test]
    fn no_tabs_and_a_choice_past_the_end_do_not_panic() {
        let mut selected = 4;
        assert!(press(&mut selected, KeyCode::Tab, 0));
        assert_eq!(selected, 0);
        assert!(press(&mut selected, KeyCode::BackTab, 0));
        assert_eq!(selected, 0);
        // A choice past the end counts as the last tab.
        let mut selected = 9;
        press(&mut selected, KeyCode::Tab, 3);
        assert_eq!(selected, 0);
        let mut selected = 9;
        press(&mut selected, KeyCode::BackTab, 3);
        assert_eq!(selected, 1);
    }

    #[test]
    fn labels_are_drawn_in_one_row_and_their_places_handed_back() {
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap();
        let mut rects = Vec::new();
        terminal
            .draw(|frame| {
                rects = draw(frame, Rect::new(0, 0, 40, 2), &LABELS, 1, Some((3, 0)));
            })
            .unwrap();
        assert_eq!(
            row(&terminal, 0).trim_end(),
            " Containers   Compose   Images"
        );
        assert_eq!(row(&terminal, 1).trim_end(), "");
        assert_eq!(
            rects,
            vec![
                Rect::new(0, 0, 12, 1),
                Rect::new(13, 0, 9, 1),
                Rect::new(23, 0, 8, 1),
            ]
        );

        let buffer = terminal.backend().buffer();
        // The selected tab is black on cyan.
        let chosen = buffer.cell((14, 0)).unwrap();
        assert_eq!((chosen.fg, chosen.bg), (Color::Black, Color::Cyan));
        // The one under the mouse is underlined, the rest plain cyan.
        let hovered = buffer.cell((3, 0)).unwrap();
        assert_eq!(hovered.fg, Color::Cyan);
        assert!(hovered.modifier.contains(Modifier::UNDERLINED));
        let plain = buffer.cell((25, 0)).unwrap();
        assert_eq!(plain.fg, Color::Cyan);
        assert!(!plain.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn a_label_that_does_not_fit_gets_an_empty_place() {
        let mut terminal = Terminal::new(TestBackend::new(40, 2)).unwrap();
        let mut rects = Vec::new();
        terminal
            .draw(|frame| {
                rects = draw(frame, Rect::new(2, 1, 15, 1), &LABELS, 0, None);
            })
            .unwrap();
        assert_eq!(rects.len(), 3);
        assert_eq!(rects[0], Rect::new(2, 1, 12, 1));
        assert_eq!(rects[1].width, 0);
        assert_eq!(rects[2].width, 0);
        assert_eq!(row(&terminal, 1).trim_end(), "   Containers");

        // An area without a row draws nothing.
        terminal
            .draw(|frame| {
                rects = draw(frame, Rect::new(0, 0, 40, 0), &LABELS, 0, None);
            })
            .unwrap();
        assert!(rects.iter().all(|rect| rect.width == 0));
        assert_eq!(rects.len(), 3);
    }
}
