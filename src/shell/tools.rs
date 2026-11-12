//! The palette: Home and every tool in a list that narrows as you type.
//! It is the one way to go from one tool to another.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Act;
use crate::atoms::ui::dialog::{self, centered};
use crate::atoms::ui::hits::{Hits, contains};
use crate::atoms::ui::input::{self, Alt};
use crate::atoms::ui::text::fit;

/// A place the palette can go to.
pub struct Place<'a> {
    pub name: &'a str,
    pub about: &'a str,
}

/// What a key did to the palette.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Go to the place with this index.
    Pick(usize),
}

pub struct ToolMenu {
    filter: String,
    /// Which of the lines that match is highlighted.
    selected: usize,
}

impl ToolMenu {
    /// Opens with the place in use highlighted.
    pub fn new(current: usize) -> Self {
        Self {
            filter: String::new(),
            selected: current,
        }
    }

    /// Opens with `text` already typed and its first match highlighted.
    pub fn typed(text: &str) -> Self {
        Self {
            filter: text.to_string(),
            selected: 0,
        }
    }

    /// The places the typed text lets through, by index: those whose
    /// name holds it first, then those that only mention it.
    fn matches(&self, places: &[Place]) -> Vec<usize> {
        let wanted = self.filter.trim().to_lowercase();
        let has = |text: &str| text.to_lowercase().contains(&wanted);
        let by_name = (0..places.len()).filter(|index| has(places[*index].name));
        let by_about = (0..places.len())
            .filter(|index| !has(places[*index].name) && has(places[*index].about));
        by_name.chain(by_about).collect()
    }

    pub fn on_key(&mut self, key: KeyEvent, places: &[Place]) -> Outcome {
        let shown = self.matches(places);
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

    /// The place a click on the line named `menu:<n>` picks.
    pub fn clicked(&self, row: &str, places: &[Place]) -> Option<usize> {
        let line: usize = row.strip_prefix("menu:")?.parse().ok()?;
        self.matches(places).get(line).copied()
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        screen: Rect,
        places: &[Place],
        mouse: Option<(u16, u16)>,
        hits: &mut Hits<Act>,
    ) {
        let shown = self.matches(places);
        let height = (shown.len().max(1) as u16 + 6).min(screen.height.saturating_sub(2));
        let popup = centered(screen, 64.min(screen.width.saturating_sub(2)), height);
        let inner = dialog::window(frame, popup, " Tools ", Color::Cyan);
        let dim = Style::default().fg(Color::DarkGray);
        let field = Line::from(vec![
            Span::styled(" › ", Style::default().fg(Color::Cyan)),
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
            height: inner.height.saturating_sub(4),
        };
        if shown.is_empty() {
            frame.render_widget(
                Paragraph::new(Span::styled(" no tool by that name", dim)),
                list,
            );
        }
        let name_width = places
            .iter()
            .map(|place| place.name.chars().count())
            .max()
            .unwrap_or(0)
            + 3;
        let highlighted = self.selected.min(shown.len().saturating_sub(1));
        for (line, index) in shown.iter().enumerate() {
            let rect = Rect {
                x: list.x,
                y: list.y + line as u16,
                width: list.width,
                height: 1,
            };
            if rect.y >= list.y + list.height {
                break;
            }
            let place = &places[*index];
            let chosen = line == highlighted;
            let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
            let width = rect.width as usize;
            let text = fit(
                &format!(
                    "{} {}{}",
                    if chosen { "▌" } else { " " },
                    fit(place.name, name_width),
                    place.about
                ),
                width,
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
            } else {
                Style::default().fg(Color::Gray)
            };
            frame.render_widget(Paragraph::new(Span::styled(text, style)), rect);
            hits.rows.push((rect, format!("menu:{line}")));
        }
        let hint = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1.min(inner.height),
        };
        frame.render_widget(
            Paragraph::new(Span::styled(
                " type to search · enter goes there · esc closes",
                dim,
            )),
            hint,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn places() -> Vec<Place<'static>> {
        vec![
            Place {
                name: "Home",
                about: "the welcome screen",
            },
            Place {
                name: "Repos",
                about: "local clones of GitHub repositories",
            },
            Place {
                name: "Docker",
                about: "containers, Compose projects, images and volumes",
            },
        ]
    }

    fn press(menu: &mut ToolMenu, code: KeyCode) -> Outcome {
        menu.on_key(KeyEvent::new(code, KeyModifiers::NONE), &places())
    }

    #[test]
    fn it_opens_on_the_place_in_use_and_arrows_move() {
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
    fn typing_narrows_by_name_first_and_by_description_after() {
        let mut menu = ToolMenu::new(0);
        press(&mut menu, KeyCode::Char('o'));
        // Home, Repos and Docker have an o in their names.
        assert_eq!(menu.matches(&places()), [0, 1, 2]);
        press(&mut menu, KeyCode::Char('c'));
        press(&mut menu, KeyCode::Char('k'));
        assert_eq!(menu.matches(&places()), [2]);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Pick(2));

        // "git" is in no name: the description finds the tool.
        let mut menu = ToolMenu::typed("git");
        assert_eq!(menu.matches(&places()), [1]);
        // "com" is in a name nowhere and in two descriptions.
        let menu_com = ToolMenu::typed("com");
        assert_eq!(menu_com.matches(&places()), [0, 2]);
        // A name that matches comes before a description that does.
        let by_name = ToolMenu::typed("re");
        assert_eq!(by_name.matches(&places()), [1, 0]);

        // A digit is typed like any other character.
        press(&mut menu, KeyCode::Char('1'));
        assert_eq!(menu.matches(&places()), [] as [usize; 0]);
        assert_eq!(press(&mut menu, KeyCode::Enter), Outcome::Stay);
        assert_eq!(press(&mut menu, KeyCode::Esc), Outcome::Close);
    }

    #[test]
    fn a_click_picks_the_place_on_that_line() {
        let mut menu = ToolMenu::new(0);
        menu.paste("dock");
        assert_eq!(menu.clicked("menu:0", &places()), Some(2));
        assert_eq!(menu.clicked("menu:1", &places()), None);
        assert_eq!(menu.clicked("other", &places()), None);
    }

    #[test]
    fn every_place_is_listed_with_what_it_is() {
        let menu = ToolMenu::new(2);
        let mut hits = Hits::default();
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|frame| menu.draw(frame, frame.area(), &places(), None, &mut hits))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut screen = String::new();
        for y in 0..20 {
            for x in 0..80 {
                screen.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            screen.push('\n');
        }
        assert!(screen.contains("╭ Tools "));
        assert!(screen.contains("  Home     the welcome screen"));
        assert!(screen.contains("▌ Docker   containers, Compose projects"));
        assert_eq!(hits.rows.len(), 3);
    }
}
