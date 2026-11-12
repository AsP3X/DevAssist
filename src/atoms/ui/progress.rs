//! Marks for work that is running: a spinner, a state icon, a bar.

use ratatui::style::{Color, Style};
use ratatui::text::Span;

use crate::atoms::task::job::ItemState;

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// The spinner's frame at this tick. It turns every second tick.
pub fn spinner(tick: usize) -> &'static str {
    SPINNER[(tick / 2) % SPINNER.len()]
}

/// The mark and colour for an item in this state.
pub fn icon(state: ItemState, tick: usize) -> (&'static str, Style) {
    match state {
        ItemState::Waiting => ("·", Style::default().fg(Color::DarkGray)),
        ItemState::Running => (spinner(tick), Style::default().fg(Color::Cyan)),
        ItemState::Done => ("✓", Style::default().fg(Color::Green)),
        ItemState::Failed => ("✗", Style::default().fg(Color::Red)),
    }
}

/// A line of `width` cells, lit up to `fraction`. Without a fraction a
/// short block moves back and forth.
pub fn bar(width: usize, fraction: Option<f64>, color: Color, tick: usize) -> Vec<Span<'static>> {
    let width = width.max(1);
    let (from, to) = match fraction {
        Some(fraction) => (
            0,
            (fraction.clamp(0.0, 1.0) * width as f64).round() as usize,
        ),
        None => {
            let block = (width / 5).clamp(1, 12);
            let span = width - block;
            let step = if span == 0 { 0 } else { tick % (span * 2) };
            let at = if step > span { span * 2 - step } else { step };
            (at, at + block)
        }
    };
    let dim = Style::default().fg(Color::DarkGray);
    vec![
        Span::styled("━".repeat(from), dim),
        Span::styled("━".repeat(to - from), Style::default().fg(color)),
        Span::styled("━".repeat(width - to), dim),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lengths(spans: &[Span]) -> Vec<usize> {
        spans
            .iter()
            .map(|span| span.content.chars().count())
            .collect()
    }

    #[test]
    fn the_spinner_turns_every_second_tick_and_comes_round() {
        assert_eq!(spinner(0), "⠋");
        assert_eq!(spinner(1), "⠋");
        assert_eq!(spinner(2), "⠙");
        assert_eq!(spinner(20), "⠋");
    }

    #[test]
    fn every_state_has_its_icon() {
        assert_eq!(icon(ItemState::Waiting, 0).0, "·");
        assert_eq!(icon(ItemState::Done, 0).0, "✓");
        assert_eq!(icon(ItemState::Failed, 0).0, "✗");
        assert_eq!(icon(ItemState::Running, 2).0, spinner(2));
    }

    #[test]
    fn a_bar_fills_to_its_fraction_or_moves_a_block() {
        assert_eq!(lengths(&bar(10, Some(0.5), Color::Cyan, 0)), [0, 5, 5]);
        assert_eq!(lengths(&bar(10, Some(7.0), Color::Cyan, 0)), [0, 10, 0]);
        // No width still draws one cell.
        assert_eq!(lengths(&bar(0, Some(1.0), Color::Cyan, 0)), [0, 1, 0]);
        // Without a fraction a block of a fifth goes there and back.
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 0)), [0, 2, 8]);
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 8)), [8, 2, 0]);
        assert_eq!(lengths(&bar(10, None, Color::Cyan, 12)), [4, 2, 4]);
    }
}
