//! The tool menu: every tool in a list that narrows as you type.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Act;
use crate::atoms::ui::dialog::{self, centered};
use crate::atoms::ui::hits::Hits;
use crate::atoms::ui::input::{self, Alt};
use crate::atoms::ui::menu;

/// What a key did to the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Switch to the tool with this index.
    Pick(usize),
}

pub struct ToolMenu {
    filter: String,
    /// Which of the lines that match is highlighted.
    selected: usize,
}

impl ToolMenu {
    /// Opens with the tool on screen highlighted.
    pub fn new(active: usize) -> Self {
        Self {
            filter: String::new(),
            selected: active,
        }
    }

    /// The tools the typed text lets through, by index.
    fn matches(&self, names: &[&str]) -> Vec<usize> {
        let wanted = self.filter.trim().to_lowercase();
        names
            .iter()
            .enumerate()
            .filter(|(_, name)| name.to_lowercase().contains(&wanted))
            .map(|(index, _)| index)
            .collect()
    }

    pub fn on_key(&mut self, key: KeyEvent, names: &[&str]) -> Outcome {
        let shown = self.matches(names);
        let last = shown.len().saturating_sub(1);
        match key.code {
            KeyCode::Esc => return Outcome::Close,
            KeyCode::Enter => {
                return match shown.get(self.selected.min(last)) {
                    Some(index) => Outcome::Pick(*index),
                    None => Outcome::Stay,
                };
            }
            KeyCode::Up => self.selected = self.selected.min(last).saturating_sub(1),
            KeyCode::Down => self.selected = (self.selected + 1).min(last),
            _ => {
                if input::edit(&mut self.filter, key, Alt::Ignored) {
                    self.selected = 0;
                }
            }
        }
        Outcome::Stay
    }

    pub fn paste(&mut self, text: &str) {
        self.filter
            .extend(text.chars().filter(|ch| !ch.is_control()));
        self.selected = 0;
    }

    /// The tool a click on the line named `menu:<n>` picks.
    pub fn clicked(&self, row: &str, names: &[&str]) -> Option<usize> {
        let line: usize = row.strip_prefix("menu:")?.parse().ok()?;
        self.matches(names).get(line).copied()
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        screen: Rect,
        names: &[&str],
        mouse: Option<(u16, u16)>,
        hits: &mut Hits<Act>,
    ) {
        let shown = self.matches(names);
        let height = (shown.len().max(1) as u16 + 5).min(screen.height.saturating_sub(2));
        let popup = centered(screen, 44.min(screen.width.saturating_sub(2)), height);
        let inner = dialog::window(frame, popup, " Tools ", Color::Cyan);
        let field = Line::from(vec![
            Span::styled(" go to ", Style::default().fg(Color::DarkGray)),
            Span::styled(self.filter.as_str(), Style::default().fg(Color::White)),
            Span::styled("▌", Style::default().fg(Color::Cyan)),
        ]);
        frame.render_widget(
            Paragraph::new(field),
            Rect {
                height: 1.min(inner.height),
                ..inner
            },
        );
        let list = Rect {
            x: inner.x,
            y: inner.y + 2,
            width: inner.width,
            height: inner.height.saturating_sub(3),
        };
        if shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(
                    " no tool by that name",
                    Style::default().fg(Color::DarkGray),
                )),
                list,
            );
        }
        let keys: Vec<char> = shown
            .iter()
            .map(|index| char::from_digit(*index as u32 + 1, 10).unwrap_or(' '))
            .collect();
        let entries: Vec<menu::Entry> = shown
            .iter()
            .zip(&keys)
            .map(|(index, key)| menu::Entry {
                label: names[*index],
                key: *key,
                warn: false,
            })
            .collect();
        let last = shown.len().saturating_sub(1);
        menu::draw(frame, list, &entries, self.selected.min(last), mouse, hits);
        let hint = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1.min(inner.height),
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                " type to narrow · enter goes there · esc closes",
                Style::default().fg(Color::DarkGray),
            )),
            hint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyModifiers;

    const NAMES: [&str; 3] = ["Repos", "Docker", "Orbit"];

    fn press(menu: &mut ToolMenu, code: KeyCode) -> Outcome {
        menu.on_key(KeyEvent::new(code, KeyModifiers::NONE), &NAMES)
    }

    #[test]
    fn it_opens_on_the_tool_in_use_and_arrows_move() {
        let mut menu = ToolMenu::new(1);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(1));
        press(&mut menu, KeyCode::Down);
        press(&mut menu, KeyCode::Down);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(2));
        press(&mut menu, KeyCode::Up);
        press(&mut menu, KeyCode::Up);
        press(&mut menu, KeyCode::Up);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(0));
    }

    #[test]
    fn typing_narrows_the_list_and_enter_takes_the_first_match() {
        let mut menu = ToolMenu::new(0);
        press(&mut menu, KeyCode::Char('o'));
        // Repos, Docker and Orbit all have an o.
        assert_eq!(menu.matches(&NAMES), [0, 1, 2]);
        press(&mut menu, KeyCode::Char('r'));
        assert_eq!(menu.matches(&NAMES), [2]);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(2));
        // A digit is typed like any other character.
        press(&mut menu, KeyCode::Char('1'));
        assert_eq!(menu.matches(&NAMES), [] as [usize; 0]);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Esc), Outcome::Close);
    }

    #[test]
    fn a_click_picks_the_tool_on_that_line() {
        let mut menu = ToolMenu::new(0);
        menu.paste("or");
        assert_eq!(menu.clicked("menu:0", &NAMES), Some(2));
        assert_eq!(menu.clicked("menu:1", &NAMES), None);
        assert_eq!(menu.clicked("other", &NAMES), None);
    }
}
