//! The welcome screen: the logo and a card for every tool. The program
//! starts here, and the palette's "Home" comes back to it.

use std::time::{Duration, Instant};

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::update::VERSION;
use crate::atoms::ui::hits::contains;
use crate::atoms::ui::text::fit;

/// The name in block letters, six rows high.
const LOGO: [&str; 6] = [
    "██████╗ ███████╗██╗   ██╗ █████╗ ███████╗███████╗██╗███████╗████████╗",
    "██╔══██╗██╔════╝██║   ██║██╔══██╗██╔════╝██╔════╝██║██╔════╝╚══██╔══╝",
    "██║  ██║█████╗  ██║   ██║███████║███████╗███████╗██║███████╗   ██║   ",
    "██║  ██║██╔══╝  ╚██╗ ██╔╝██╔══██║╚════██║╚════██║██║╚════██║   ██║   ",
    "██████╔╝███████╗ ╚████╔╝ ██║  ██║███████║███████║██║███████║   ██║   ",
    "╚═════╝ ╚══════╝  ╚═══╝  ╚═╝  ╚═╝╚══════╝╚══════╝╚═╝╚══════╝   ╚═╝   ",
];

/// The name where the block letters do not fit.
const SMALL_LOGO: &str = "D e v A s s i s t";

const TAGLINE: &str = "your developer tools, one place";

/// The colours the logo runs through, left to right.
const FROM: (u8, u8, u8) = (64, 208, 224);
const TO: (u8, u8, u8) = (190, 120, 255);

/// How long the logo takes to draw itself at start.
const INTRO: Duration = Duration::from_millis(550);

const CARD_WIDTH: u16 = 36;
const CARD_HEIGHT: u16 = 6;
const CARD_GAP: u16 = 2;

/// What a tool shows on its card.
pub struct Card<'a> {
    pub name: &'a str,
    pub about: &'a str,
    /// What the tool has to report right now, if anything.
    pub status: Option<String>,
}

/// What a key did on the welcome screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    /// Open the tool with this index.
    Open(usize),
    /// Open the palette with this already typed.
    Search(String),
    Help,
    Version,
    Quit,
}

pub struct Home {
    /// The highlighted card.
    pub selected: usize,
    /// When the logo started drawing itself. None shows it whole.
    started: Option<Instant>,
    /// Where the last frame drew each card.
    cards: Vec<Rect>,
    /// How many cards the last frame put in a row.
    columns: usize,
}

impl Home {
    /// `animated` draws the logo column by column over the first moments.
    pub fn new(animated: bool) -> Self {
        Self {
            selected: 0,
            started: animated.then(Instant::now),
            cards: Vec::new(),
            columns: 1,
        }
    }

    /// True while the logo is still drawing itself.
    pub fn animating(&self) -> bool {
        self.started.is_some_and(|at| at.elapsed() < INTRO)
    }

    /// How much of the logo is shown, from 0 to 1.
    fn revealed(&self) -> f64 {
        match self.started {
            Some(at) => (at.elapsed().as_secs_f64() / INTRO.as_secs_f64()).min(1.0),
            None => 1.0,
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, tools: usize) -> Outcome {
        let last = tools.saturating_sub(1);
        let columns = self.columns.max(1);
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        self.selected = self.selected.min(last);
        match key.code {
            KeyCode::Left => self.selected = self.selected.saturating_sub(1),
            KeyCode::Right => self.selected = (self.selected + 1).min(last),
            KeyCode::Up => self.selected = self.selected.saturating_sub(columns),
            KeyCode::Down if self.selected + columns <= last => self.selected += columns,
            KeyCode::Tab if tools > 0 => self.selected = (self.selected + 1) % tools,
            KeyCode::BackTab if tools > 0 => self.selected = (self.selected + tools - 1) % tools,
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.selected = last,
            KeyCode::Enter if tools > 0 => return Outcome::Open(self.selected),
            KeyCode::Char('?') if plain => return Outcome::Help,
            KeyCode::Char('q') if plain => return Outcome::Quit,
            KeyCode::Char('v') | KeyCode::Char('V') if plain => return Outcome::Version,
            KeyCode::Char(ch) if plain && ch.is_alphanumeric() => {
                return Outcome::Search(ch.to_string());
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// Where the last frame drew the card with this index.
    #[cfg(test)]
    pub fn card_at_index(&self, index: usize) -> Rect {
        self.cards[index]
    }

    /// The tool whose card is at this cell.
    pub fn card_at(&self, x: u16, y: u16) -> Option<usize> {
        self.cards.iter().position(|rect| contains(*rect, x, y))
    }

    pub fn draw(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        cards: &[Card],
        palette_key: &str,
        mouse: Option<(u16, u16)>,
    ) {
        self.selected = self.selected.min(cards.len().saturating_sub(1));
        let big = area.width >= 80 && area.height >= 24;
        let logo_height: u16 = if big { LOGO.len() as u16 } else { 1 };
        // As many cards in a row as fit, and never more rows than fit.
        let fitting = ((area.width.saturating_sub(4) + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).max(1);
        let columns = (fitting as usize).min(cards.len().max(1));
        let rows = cards.len().div_ceil(columns) as u16;
        self.columns = columns;
        let cards_height = rows * CARD_HEIGHT + rows.saturating_sub(1);
        let wanted = logo_height + 3 + cards_height + 3;
        let mut y = area.y + area.height.saturating_sub(wanted) / 2;
        let bottom = area.y + area.height;

        if big {
            self.draw_logo(frame, area, y);
        } else {
            let title = Span::styled(
                SMALL_LOGO,
                Style::default().fg(rgb(0.0)).add_modifier(Modifier::BOLD),
            );
            centred(frame, area, y, Line::from(title));
        }
        y += logo_height;
        let tagline = Line::from(vec![
            Span::styled(TAGLINE, Style::default().fg(Color::Gray)),
            Span::styled(
                format!("  ·  v{VERSION}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]);
        centred(frame, area, y + 1, tagline);
        y += 3;

        let row_width = columns as u16 * CARD_WIDTH + (columns as u16).saturating_sub(1) * CARD_GAP;
        let left = area.x + area.width.saturating_sub(row_width) / 2;
        self.cards.clear();
        for (index, card) in cards.iter().enumerate() {
            let column = (index % columns) as u16;
            let row = (index / columns) as u16;
            let rect = Rect {
                x: left + column * (CARD_WIDTH + CARD_GAP),
                y: y + row * (CARD_HEIGHT + 1),
                width: CARD_WIDTH.min(area.width),
                height: CARD_HEIGHT,
            };
            if rect.y + rect.height > bottom || rect.x + rect.width > area.x + area.width {
                self.cards.push(Rect::default());
                continue;
            }
            let hovered = mouse.is_some_and(|(mx, my)| contains(rect, mx, my));
            draw_card(frame, rect, card, index == self.selected, hovered);
            self.cards.push(rect);
        }
        y += cards_height + 1;

        let dim = Style::default().fg(Color::DarkGray);
        let hints = [
            Line::from(Span::styled(
                "arrows choose · enter opens · or type a tool's name",
                Style::default().fg(Color::Gray),
            )),
            Line::from(Span::styled(
                format!("{palette_key} tools from anywhere · ? help · v version · q quit"),
                dim,
            )),
        ];
        for (offset, hint) in hints.into_iter().enumerate() {
            centred(frame, area, y + offset as u16, hint);
        }
    }

    /// The block letters, each column in its place on the gradient, as
    /// far as the intro has come.
    fn draw_logo(&self, frame: &mut Frame, area: Rect, y: u16) {
        let width = LOGO[0].chars().count();
        let shown = (self.revealed() * width as f64).ceil() as usize;
        for (row, text) in LOGO.iter().enumerate() {
            let spans: Vec<Span> = text
                .chars()
                .enumerate()
                .map(|(column, ch)| {
                    if column >= shown {
                        return Span::raw(" ");
                    }
                    let place = column as f64 / (width - 1) as f64;
                    Span::styled(ch.to_string(), Style::default().fg(rgb(place)))
                })
                .collect();
            centred(frame, area, y + row as u16, Line::from(spans));
        }
    }
}

/// The gradient's colour at `place`, from 0 at the left to 1 at the right.
fn rgb(place: f64) -> Color {
    let mix = |from: u8, to: u8| (from as f64 + (to as f64 - from as f64) * place).round() as u8;
    Color::Rgb(mix(FROM.0, TO.0), mix(FROM.1, TO.1), mix(FROM.2, TO.2))
}

/// Draws one line in the middle of row `y`, if that row is inside `area`.
fn centred(frame: &mut Frame, area: Rect, y: u16, line: Line) {
    if y >= area.y + area.height {
        return;
    }
    let width = (line.width() as u16).min(area.width);
    let rect = Rect {
        x: area.x + (area.width - width) / 2,
        y,
        width,
        height: 1,
    };
    frame.render_widget(Paragraph::new(line), rect);
}

fn draw_card(frame: &mut Frame, rect: Rect, card: &Card, selected: bool, hovered: bool) {
    let (border, title) = if selected {
        (
            Style::default().fg(rgb(0.0)),
            Style::default().fg(rgb(0.0)).add_modifier(Modifier::BOLD),
        )
    } else if hovered {
        (
            Style::default().fg(Color::Gray),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        (
            Style::default().fg(Color::DarkGray),
            Style::default().fg(Color::Gray),
        )
    };
    let marker = if selected { "▌ " } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(border)
        .title(Span::styled(format!(" {marker}{} ", card.name), title));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let width = inner.width.saturating_sub(2) as usize;
    let text = Style::default().fg(if selected { Color::White } else { Color::Gray });
    let mut lines: Vec<Line> = wrap(card.about, width)
        .into_iter()
        .take(2)
        .map(|line| Line::from(Span::styled(format!(" {line}"), text)))
        .collect();
    while lines.len() < 3 {
        lines.push(Line::from(""));
    }
    lines.push(match &card.status {
        Some(status) => Line::from(vec![
            Span::styled(" ● ", Style::default().fg(Color::Green)),
            Span::styled(fit(status, width.saturating_sub(2)), text),
        ]),
        None => Line::from(Span::styled(
            " ○ opens when you choose it",
            Style::default().fg(Color::DarkGray),
        )),
    });
    frame.render_widget(Paragraph::new(lines), inner);
}

/// Breaks `text` into lines of at most `width` cells, at spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let longer = UnicodeWidthStr::width(current.as_str()) + 1 + UnicodeWidthStr::width(word);
        if !current.is_empty() && longer > width {
            lines.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn cards() -> Vec<Card<'static>> {
        vec![
            Card {
                name: "Repos",
                about: "Local clones of GitHub repositories: add, update, group, remove",
                status: Some("56 clones · 3 behind".to_string()),
            },
            Card {
                name: "Docker",
                about: "Containers, Compose projects, images and volumes, with live logs",
                status: None,
            },
            Card {
                name: "Orbit",
                about: "A third tool",
                status: None,
            },
        ]
    }

    fn press(home: &mut Home, code: KeyCode) -> Outcome {
        home.on_key(KeyEvent::new(code, KeyModifiers::NONE), 3)
    }

    fn render(home: &mut Home, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| home.draw(frame, frame.area(), &cards(), "ctrl-p", None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn a_wide_screen_shows_the_block_letters_and_a_card_for_every_tool() {
        let mut home = Home::new(false);
        let screen = render(&mut home, 120, 34);
        assert!(screen.contains(LOGO[0]));
        assert!(screen.contains(LOGO[5]));
        assert!(screen.contains(TAGLINE));
        assert!(screen.contains(&format!("v{VERSION}")));
        assert!(screen.contains("╭ ▌ Repos "));
        assert!(screen.contains("╭ Docker "));
        assert!(screen.contains("Local clones of GitHub"));
        assert!(screen.contains("● 56 clones · 3 behind"));
        assert!(screen.contains("○ opens when you choose it"));
        assert!(screen.contains("ctrl-p tools from anywhere"));
        // Three cards fit side by side.
        assert_eq!(home.columns, 3);
        assert!(home.cards.iter().all(|rect| rect.width > 0));
        assert_eq!(
            home.card_at(home.cards[1].x + 1, home.cards[1].y + 1),
            Some(1)
        );
        assert_eq!(home.card_at(0, 0), None);
    }

    #[test]
    fn the_logo_runs_through_a_gradient() {
        let mut home = Home::new(false);
        let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
        terminal
            .draw(|frame| home.draw(frame, frame.area(), &cards(), "ctrl-p", None))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let row = (0..34)
            .find(|y| buffer.cell((30, *y)).unwrap().symbol() == "█")
            .unwrap();
        let first = (0..120)
            .find(|x| buffer.cell((*x, row)).unwrap().symbol() == "█")
            .unwrap();
        let last = (0..120)
            .rev()
            .find(|x| buffer.cell((*x, row)).unwrap().symbol() == "█")
            .unwrap();
        assert_eq!(buffer.cell((first, row)).unwrap().fg, rgb(0.0));
        assert_ne!(
            buffer.cell((first, row)).unwrap().fg,
            buffer.cell((last, row)).unwrap().fg
        );
    }

    #[test]
    fn a_small_screen_gets_the_name_in_one_line_and_stacks_the_cards() {
        let mut home = Home::new(false);
        let screen = render(&mut home, 48, 26);
        assert!(!screen.contains("██████"));
        assert!(screen.contains(SMALL_LOGO));
        assert_eq!(home.columns, 1);
        assert!(screen.contains("╭ ▌ Repos "));
        assert!(screen.contains("╭ Docker "));
        // The smallest screen the shell draws on must not panic.
        render(&mut home, 48, 12);
        render(&mut home, 10, 3);
    }

    #[test]
    fn arrows_move_between_cards_and_enter_opens_one() {
        let mut home = Home::new(false);
        render(&mut home, 120, 34);
        assert_eq!(press(&mut home, KeyCode::Left), Outcome::Stay);
        assert_eq!(home.selected, 0);
        press(&mut home, KeyCode::Right);
        assert_eq!(press(&mut home, KeyCode::Enter), Outcome::Open(1));
        press(&mut home, KeyCode::Right);
        press(&mut home, KeyCode::Right);
        assert_eq!(home.selected, 2);
        press(&mut home, KeyCode::Tab);
        assert_eq!(home.selected, 0);
        press(&mut home, KeyCode::BackTab);
        assert_eq!(home.selected, 2);

        // Stacked in one column, up and down walk the cards.
        render(&mut home, 48, 30);
        press(&mut home, KeyCode::Up);
        assert_eq!(home.selected, 1);
        press(&mut home, KeyCode::Down);
        press(&mut home, KeyCode::Down);
        assert_eq!(home.selected, 2);
    }

    #[test]
    fn a_letter_starts_a_search_and_the_shells_keys_are_passed_on() {
        let mut home = Home::new(false);
        assert_eq!(
            press(&mut home, KeyCode::Char('d')),
            Outcome::Search("d".to_string())
        );
        assert_eq!(press(&mut home, KeyCode::Char('?')), Outcome::Help);
        assert_eq!(press(&mut home, KeyCode::Char('v')), Outcome::Version);
        assert_eq!(press(&mut home, KeyCode::Char('q')), Outcome::Quit);
        // With a modifier it is not text.
        let ctrl_d = KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(home.on_key(ctrl_d, 3), Outcome::Stay);
    }

    #[test]
    fn the_logo_draws_itself_at_start_and_is_whole_afterwards() {
        let mut home = Home::new(true);
        assert!(home.animating());
        home.started = Some(Instant::now() - INTRO);
        assert!(!home.animating());
        let screen = render(&mut home, 120, 34);
        assert!(screen.contains(LOGO[0]));
        // At the very start nothing of it is there yet.
        home.started = Some(Instant::now() + Duration::from_secs(60));
        assert!((home.revealed() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn text_wraps_at_spaces() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("", 7), [] as [&str; 0]);
        assert_eq!(wrap("longerthanwidth x", 5), ["longerthanwidth", "x"]);
    }
}
