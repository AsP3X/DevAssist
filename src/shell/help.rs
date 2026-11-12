//! The help dialog: the keys of the tool on screen, then the shell's own.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Act;
use crate::atoms::ui::dialog::{self, centered};
use crate::atoms::ui::hits::Hits;

/// The keys that work in every tool.
const SHELL_KEYS: &str = "\
1-9            switch to that tool · ctrl-p lists the tools
v              version: look for an update and install it
drag           select text · releasing the mouse copies it
ctrl-c         copy the selection, or quit";

pub fn draw(
    frame: &mut Frame,
    screen: Rect,
    name: &str,
    tool_keys: &str,
    mouse: Option<(u16, u16)>,
    hits: &mut Hits<Act>,
) {
    let mut lines: Vec<Line> = tool_keys.lines().map(Line::from).collect();
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Everywhere",
        Style::default().fg(Color::DarkGray),
    )));
    lines.extend(SHELL_KEYS.lines().map(Line::from));
    let popup = centered(
        screen,
        72.min(screen.width.saturating_sub(2)),
        (lines.len() as u16 + 3).min(screen.height.saturating_sub(2)),
    );
    let inner = dialog::window(frame, popup, &format!(" Keys · {name} "), Color::Cyan);
    let text = Rect {
        height: inner.height.saturating_sub(1),
        ..inner
    };
    frame.render_widget(Paragraph::new(lines), text);
    let buttons = Rect {
        x: inner.x,
        y: inner.y + inner.height.saturating_sub(1),
        width: inner.width,
        height: 1,
    };
    dialog::buttons(
        frame,
        buttons,
        &[("Close", Act::Close)],
        Act::Confirm,
        mouse,
        hits,
    );
}
