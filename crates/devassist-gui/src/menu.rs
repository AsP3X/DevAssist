//! The row's menu: what can be done to the chosen repositories (the ticked ones, else the highlighted one), opened
//! by a row's "…" or by Enter on a row (`repos_screen`), drawn on the overlay ([`crate::overlay`]) while the core's
//! mode is `Mode::Menu`, through the same bridge as the dialogs (`dialogs::Which::Menu`).
//!
//! It shows every line of the core's, in the core's words and with the core's keys (`MenuItem::label`,
//! `MenuItem::key`, and the core's `Menu::label` for the forks' line, "Clone the forks" or "Update the forks"), in the
//! drawing's blocks set apart by rules: the work on the repositories, the forks' three, Migrate, Remove. It is the
//! theme's width, or as wide as its longest line needs ([`width`]). A menu has a shield but no scrim; a click outside
//! it or Esc closes it, and the click goes no further. It is not walked with Tab: as in the terminal app, Up and Down
//! (and `k` and `j`) move the highlight and stop at the ends, Home and End go to them, Enter runs the highlighted
//! line, a line's letter runs that line, and `q` closes. The pointer, when it moves over a line, highlights it, so
//! the mouse and the keys never show two highlights; a pointer that only rests where the menu opens chooses nothing.
//! A line that opens a dialog (Update, Move to a group, Remove, the forks' runs) hands the overlay over to it in the
//! next frame, and the key or click that chose the line does nothing in it (`overlay::open`, `overlay::fresh_click`).
//! "Show the forks" closes the menu and the forks screen takes the table's place in the next frame; the press that
//! chose it does nothing there (`overlay::settle`, `overlay::stale_click`).
//!
//! Migrate, set apart by a rule as Remove is, ends with a chevron: Enter, Right, its letter or a click opens the
//! core's Migrate menu (`Repos::run_menu`), which the window draws as a side menu, a second glass pane of the same
//! overlay beside the menu (`overlay::submenu_rect`, `overlay::want_submenu`), with the core's two lines in its
//! words and keys (`MigrateItem`). The side menu follows the core's state (`Repos::migrate_highlight`): the window
//! keeps none of its own. While it is open, Migrate keeps the selection's fill; Up and Down (`k`, `j`, Home, End)
//! move in it and stop at its ends, Right does nothing, Left, Esc or `q` close it and the menu stays, a click on
//! Migrate closes it too, and the menu's letters do nothing; Enter, a line's letter or a click runs the line
//! (`Repos::run_migrate`), whose dialog the bridge opens in the next frame. The pointer moving over another line of
//! the menu closes the side menu and highlights that line. It is drawn only in a frame whose bake placed its glass,
//! and never in a frame that closes it; the key or click that opened it does nothing in it
//! (`overlay::open_submenu`).

use devassist_core::repos::state::{Menu, MenuItem, MigrateItem, Repos};
use egui::{Align2, Color32, Event, FontFamily, FontId, Id, Key, Painter, Pos2, Rect, Response, Sense, Ui, Vec2, pos2, vec2};

use crate::assets::Icon;
use crate::baker::Drawn;
use crate::overlay;
use crate::theme::{
    HAIRLINE, ICON, KEY_TEXT, MENU_CHEVRON, MENU_KEY_WIDTH, MENU_LINE, MENU_LINE_OPACITY, MENU_PAD, MENU_ROW, MENU_ROW_GAP, MENU_ROW_PAD,
    MENU_ROW_RADIUS, MENU_WIDTH, Palette, SUBMENU_WIDTH, TEXT,
};
use crate::widgets::{self, draw_icon, font};

/// The lines the window shows: every line of the core's, in its order.
const SHOWN: [MenuItem; MenuItem::ALL.len()] = MenuItem::ALL;

/// The drawing's icon of each line of [`SHOWN`], in its order.
const ICONS: [Icon; SHOWN.len()] = [
    Icon::ArrowDownToLine,
    Icon::FolderOpen,
    Icon::ExternalLink,
    Icon::Group,
    Icon::Copy,
    Icon::Eye,
    Icon::GitPullRequest,
    Icon::GitFork,
    Icon::CopyPlus,
    Icon::RefreshCw,
    Icon::Truck,
    Icon::Trash2,
];

/// The drawing's icon of each line of the side menu, in the order of the core's `MigrateItem::ALL`.
const SIDE_ICONS: [Icon; MigrateItem::ALL.len()] = [Icon::FolderOutput, Icon::CloudUpload];

/// The sign the core ends the words of a line that opens a submenu with; the window draws the drawing's chevron
/// instead.
const OPENS: char = '▸';

/// The lines the window's menu shows, in order: every line of the core's.
#[must_use]
pub fn shown() -> &'static [MenuItem] {
    &SHOWN
}

/// The words of `item`'s line, for one repository or for several: the core's, without the sign it ends Migrate's
/// with ([`OPENS`]); the forks' line in the words of the core's `menu` while it is open ("Update the forks" once every
/// chosen repository has them).
fn words(item: MenuItem, many: bool, menu: Option<&Menu>) -> &'static str {
    let label = match menu {
        Some(menu) if item == MenuItem::Forks => menu.label(item),
        _ => item.label(many),
    };
    label.trim_end_matches(OPENS).trim_end()
}

/// The room `item`'s line takes beside its words: the row's padding at both ends, the icon and the gap after it, the
/// gap before the key and the key, and for a line that opens the side menu its chevron and the gap before it.
fn beside_words(item: MenuItem) -> f32 {
    let chevron = if opens_side(item) { MENU_CHEVRON + MENU_ROW_GAP } else { 0.0 };
    2.0 * MENU_ROW_PAD + ICON + 2.0 * MENU_ROW_GAP + MENU_KEY_WIDTH + chevron
}

/// How wide the menu is whose lines' words are as wide as `lines` says: the theme's [`MENU_WIDTH`], or, when a line's
/// words, icon and key need more, as wide as the longest needs (in whole points), its pane's padding included.
fn width(lines: &[(MenuItem, f32)]) -> f32 {
    lines.iter().map(|(item, words)| 2.0 * MENU_PAD + beside_words(*item) + words.ceil()).fold(MENU_WIDTH, f32::max)
}

/// How wide the menu of `repos` is, its lines' words (for its chosen repositories) measured with `painter`'s faces.
pub(crate) fn pane_width(painter: &Painter, repos: &Repos) -> f32 {
    let words_font = font(TEXT, FontFamily::Proportional);
    let many = repos.chosen().len() > 1;
    let lines: Vec<(MenuItem, f32)> =
        SHOWN.iter().map(|item| (*item, widgets::width(painter, words(*item, many, repos.menu.as_ref()), &words_font))).collect();
    width(&lines)
}

/// Runs `item`, a line that opens no side menu, for `repos`'s chosen repositories, as the core's `Repos::run_menu`
/// does: the menu closes, the ticks stay, and the line's work is asked for. "Show the forks" opens them with
/// `Repos::open_forks_plain`, whose notice for forks that are not cloned names no key of the terminal's.
fn run(repos: &mut Repos, item: MenuItem) {
    if item == MenuItem::ShowForks {
        repos.close_menu();
        repos.open_forks_plain();
    } else {
        repos.run_menu(item);
    }
}

/// Whether a line stands between `item` and the one before it: the forks' block, Migrate and Remove are set apart,
/// as the drawing sets them.
fn rule_before(item: MenuItem) -> bool {
    matches!(item, MenuItem::ShowForks | MenuItem::Migrate | MenuItem::Remove)
}

/// Whether `item`'s line opens the side menu, and ends with a chevron.
fn opens_side(item: MenuItem) -> bool {
    item == MenuItem::Migrate
}

/// The place of the line that opens the side menu among [`SHOWN`].
fn side_line() -> usize {
    SHOWN.iter().position(|item| opens_side(*item)).unwrap_or(0)
}

/// How tall the menu is: its lines, the rules between its blocks, and the room above and below them.
fn height() -> f32 {
    let (rows, rules) = SHOWN.iter().fold((0.0, 0.0), |(rows, rules), item| (rows + 1.0, rules + if rule_before(*item) { 1.0 } else { 0.0 }));
    2.0 * MENU_PAD + rows * MENU_ROW + rules * MENU_LINE
}

/// How tall the side menu is: its lines, and the room above and below them.
fn side_height() -> f32 {
    2.0 * MENU_PAD + MigrateItem::ALL.iter().map(|_| MENU_ROW).sum::<f32>()
}

/// Where the menu stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Places {
    /// Its pane.
    pub pane: Rect,
    /// The side menu's pane, when the core's Migrate menu was open as the frame was placed: its glass is baked there.
    pub side: Option<Rect>,
}

impl Places {
    /// The strip between the menu's pane and its side menu's, over the side menu's height (`theme::SUBMENU_GAP` wide):
    /// part of the overlay, so a click or the pointer there changes nothing. None without a side menu, or when the
    /// side menu lies over the menu (a window too narrow for it at either side).
    #[must_use]
    pub fn gap(&self) -> Option<Rect> {
        let side = self.side?;
        let (left, right) = if side.right() <= self.pane.left() {
            (side.right(), self.pane.left())
        } else if side.left() >= self.pane.right() {
            (self.pane.right(), side.left())
        } else {
            return None;
        };
        Some(Rect::from_min_max(pos2(left, side.top()), pos2(right, side.bottom())))
    }

    /// The room both panes take, where the overlay's controls are.
    #[must_use]
    pub fn whole(&self) -> Rect {
        self.side.map_or(self.pane, |side| self.pane.union(side))
    }
}

/// Where the menu of `repos` stands in `window` for the button at `anchor` ([`place`]), as wide as its lines need,
/// measured with `painter`'s faces ([`pane_width`]), with its side menu while the core's Migrate menu is open.
#[must_use]
pub fn plan(painter: &Painter, window: Rect, anchor: Rect, repos: &Repos) -> Places {
    place(window, anchor, repos.migrate_highlight().is_some(), pane_width(painter, repos))
}

/// Where a menu `width` wide stands in `window` for the button at `anchor`: under it, its right edge on the button's
/// (`overlay::menu_rect`); with `side`, the side menu beside it, level with Migrate (`overlay::submenu_rect`).
fn place(window: Rect, anchor: Rect, side: bool, width: f32) -> Places {
    let pane = overlay::menu_rect(anchor, vec2(width, height()), window);
    let side = side.then(|| overlay::submenu_rect(pane, layout(pane).0[side_line()], vec2(SUBMENU_WIDTH, side_height()), window));
    Places { pane, side }
}

/// The lines of the menu in its pane at `pane`, each with its place, and the places of the rules between them.
fn layout(pane: Rect) -> (Vec<Rect>, Vec<Rect>) {
    let inside = pane.shrink(MENU_PAD);
    let mut top = inside.top();
    let mut rules = Vec::new();
    let rows = SHOWN
        .iter()
        .map(|item| {
            if rule_before(*item) {
                let y = top + MENU_LINE / 2.0;
                rules.push(Rect::from_min_max(
                    pos2(inside.left() + MENU_ROW_PAD, y - HAIRLINE / 2.0),
                    pos2(inside.right() - MENU_ROW_PAD, y + HAIRLINE / 2.0),
                ));
                top += MENU_LINE;
            }
            let row = Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), MENU_ROW));
            top = row.bottom();
            row
        })
        .collect();
    (rows, rules)
}

/// The lines of the side menu in its pane at `pane`, in the order of the core's `MigrateItem::ALL`.
fn side_layout(pane: Rect) -> Vec<Rect> {
    let inside = pane.shrink(MENU_PAD);
    let mut top = inside.top();
    MigrateItem::ALL
        .iter()
        .map(|_| {
            let row = Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), MENU_ROW));
            top = row.bottom();
            row
        })
        .collect()
}

/// The size of the demo's menu (on api and web, as `--show menu` opens it), its words measured with the window's faces:
/// for the tests that place a menu.
#[cfg(test)]
pub(crate) fn demo_size() -> Vec2 {
    let mut repos = crate::demo::repos();
    crate::demo::show(&mut repos, "menu").unwrap_or_default();
    let ctx = crate::widgets::tests::context();
    let mut width = 0.0;
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
    ctx.run_ui(input, |ui| width = pane_width(ui.painter(), &repos)).drop_without_applying_deltas();
    vec2(width, height())
}

/// The place of Migrate's line, which the side menu stands level with, in the menu's pane at `pane`: for the tests
/// that place a side menu.
#[cfg(test)]
pub(crate) fn migrate_line_rect(pane: Rect) -> Rect {
    layout(pane).0[side_line()]
}

/// The places of the menu's lines in its pane at `pane`, in the order of [`shown`]: for the tests that press them.
#[cfg(test)]
pub(crate) fn line_rects(pane: Rect) -> Vec<Rect> {
    layout(pane).0
}

/// The places of the side menu's lines in its pane at `pane`, in the order of the core's `MigrateItem::ALL`: for the
/// tests that press them.
#[cfg(test)]
pub(crate) fn side_line_rects(pane: Rect) -> Vec<Rect> {
    side_layout(pane)
}

/// What a key asks of the open menu or side menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask<T> {
    /// Close it.
    Close,
    /// Move the highlight one line up (`true`) or down.
    Step(bool),
    /// Highlight the first line (`true`) or the last.
    End(bool),
    /// Run the highlighted line.
    Highlighted,
    /// Go back out (Left).
    Back,
    /// Go into the highlighted line's side menu (Right).
    Into,
    /// Run this line.
    Run(T),
}

/// What the keys among `events` ask of the menu, in order, as the terminal app's menu reads them, `letter` giving the
/// line a typed letter runs. Letters are read from the typed text, so they follow the keyboard's layout; a letter of
/// no line is none.
fn asks<T>(events: &[Event], letter: impl Fn(char) -> Option<T>) -> Vec<Ask<T>> {
    let mut asks = Vec::new();
    for event in events {
        match event {
            Event::Key { key, pressed: true, modifiers, .. } if !modifiers.any() => asks.extend(match key {
                Key::Escape => Some(Ask::Close),
                Key::ArrowUp => Some(Ask::Step(true)),
                Key::ArrowDown => Some(Ask::Step(false)),
                Key::Home => Some(Ask::End(true)),
                Key::End => Some(Ask::End(false)),
                Key::Enter => Some(Ask::Highlighted),
                Key::ArrowLeft => Some(Ask::Back),
                Key::ArrowRight => Some(Ask::Into),
                _ => None,
            }),
            Event::Text(text) => {
                for typed in text.chars().map(|typed| typed.to_ascii_lowercase()) {
                    asks.extend(match typed {
                        'q' => Some(Ask::Close),
                        'k' => Some(Ask::Step(true)),
                        'j' => Some(Ask::Step(false)),
                        _ => letter(typed).map(Ask::Run),
                    });
                }
            }
            _ => {}
        }
    }
    asks
}

/// The line of [`SHOWN`] whose letter is `typed`.
fn menu_letter(typed: char) -> Option<MenuItem> {
    SHOWN.iter().copied().find(|item| item.key() == typed)
}

/// The line of the side menu whose letter is `typed`.
fn side_letter(typed: char) -> Option<MigrateItem> {
    MigrateItem::ALL.into_iter().find(|item| item.key() == typed)
}

/// The highlighted line of `lines` after `ask`, from the line at `at`: one up or down, stopping at the ends; the
/// first or the last. From no line, a step goes to the first.
fn moved<T>(at: Option<usize>, ask: Ask<T>, lines: usize) -> Option<usize> {
    let last = lines.saturating_sub(1);
    Some(match (ask, at) {
        (Ask::Step(true), Some(at)) => at.saturating_sub(1),
        (Ask::Step(false), Some(at)) => (at + 1).min(last),
        (Ask::Step(_) | Ask::End(true), _) => 0,
        (Ask::End(false), _) => last,
        (Ask::Close | Ask::Highlighted | Ask::Back | Ask::Into | Ask::Run(_), _) => return at,
    })
}

/// The line of [`SHOWN`] the core highlights; none while the menu is closed.
fn highlighted(repos: &Repos) -> Option<usize> {
    repos.menu_highlight().and_then(|item| SHOWN.iter().position(|shown| *shown == item))
}

/// The id of the menu's line `index`.
fn row_id(index: usize) -> Id {
    Id::unique(("menu-row", index))
}

/// The id of the side menu's line `index`.
fn side_row_id(index: usize) -> Id {
    Id::unique(("submenu-row", index))
}

/// What a person did in the menu in a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Done {
    /// Closed it.
    Close,
    /// Ran this line of it (Migrate opens the side menu).
    Run(MenuItem),
    /// Closed the side menu, back to the menu.
    Back,
    /// Ran this line of the side menu.
    Side(MigrateItem),
}

/// The menu at `places`, for `repos`'s chosen repositories, on the overlay in `ui`: its lines, the highlighted one
/// on the selection's fill, Remove in the danger colour, each line's key at its right, Migrate's before its chevron;
/// and the side menu, while the core's Migrate menu is open and the frame baked its glass (among `drawn`). `outside`
/// is whether the shield was clicked this frame (outside both panes). The highlight follows the pointer only in a
/// frame the pointer moves, so a highlight moved by a key stays while the pointer rests, and a pointer resting where
/// the menu opens does not choose its line. What a person does goes to `repos` through its methods.
pub fn show(ui: &mut Ui, places: Places, repos: &mut Repos, palette: Palette, outside: bool, drawn: &[Drawn]) {
    // No Tab here: the lines take no focus, so Enter and Space press none of them; the arrows move the highlight.
    overlay::walk(ui, &[]);
    let migrate = side_line();
    let open = repos.migrate_highlight().is_some();
    // The side menu is touched and drawn only where this frame's bake placed its glass.
    let side = places.side.filter(|_| open);
    if let Some(side) = side {
        overlay::pane(ui, side, Id::unique("submenu-pane"));
    }
    // The gap between the two panes is theirs: a click there closes nothing.
    if let Some(gap) = side.and(places.gap()) {
        overlay::pane(ui, gap, Id::unique("submenu-gap"));
    }
    let events = if open { overlay::submenu_events(ui.ctx()) } else { overlay::menu_events(ui.ctx()) };
    // While the side menu is open, a click counts only once every press of it came after the side menu opened.
    let fresh = |ui: &Ui, response: &Response| if open { overlay::fresh_submenu_click(ui, response) } else { overlay::fresh_click(ui, response) };
    let rows = layout(places.pane).0;
    let side_rows = side.map_or_else(Vec::new, side_layout);
    let mut at = highlighted(repos);
    let mut side_at = repos.migrate_highlight().and_then(|item| MigrateItem::ALL.iter().position(|known| *known == item));
    // The pointer moved over another line of the menu than Migrate: the side menu closes.
    let mut leave = false;
    let mut done = None;
    let moving = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
    for (index, row) in rows.iter().enumerate() {
        let response = ui.interact(row.intersect(places.pane), row_id(index), Sense::CLICK);
        if moving && response.hovered() {
            at = Some(index);
            leave = open && index != migrate;
        }
        if fresh(ui, &response) {
            done = Some(if open && index == migrate { Done::Back } else { Done::Run(SHOWN[index]) });
        }
    }
    if let Some(pane) = side {
        for (index, row) in side_rows.iter().enumerate() {
            let response = ui.interact(row.intersect(pane), side_row_id(index), Sense::CLICK);
            if moving && response.hovered() {
                (at, side_at, leave) = (Some(migrate), Some(index), false);
            }
            if fresh(ui, &response) {
                done = Some(Done::Side(MigrateItem::ALL[index]));
            }
        }
    }
    let on_side = open && !leave;
    let done = keyed(&events, on_side, &mut at, &mut side_at, done.or(outside.then_some(Done::Close)));
    if leave {
        repos.close_migrate();
        // As every other way it closes: a key down now (one held since before it opened) is not the menu's.
        overlay::close_submenu(ui.ctx());
    }
    if let Some(at) = at {
        repos.highlight_menu(SHOWN[at]);
    }
    if let Some(side_at) = side_at.filter(|_| on_side) {
        repos.highlight_migrate(MigrateItem::ALL[side_at]);
    }

    draw_menu(ui, places.pane, at, repos.chosen().len() > 1, repos.menu.as_ref(), palette);
    // Not in a frame that closes it: no pane stands for a side menu that is gone.
    if let Some(side) = side.filter(|_| on_side && done.is_none()) {
        overlay::submenu_glass(ui, drawn);
        draw_side(ui, side, side_at, palette);
    }
    match done {
        Some(Done::Run(item)) if opens_side(item) => {
            repos.highlight_menu(item);
            repos.run_menu(item);
            overlay::open_submenu(ui.ctx());
        }
        Some(Done::Run(item)) => run(repos, item),
        Some(Done::Close) => repos.close_menu(),
        Some(Done::Back) => {
            repos.close_migrate();
            overlay::close_submenu(ui.ctx());
        }
        Some(Done::Side(item)) => repos.run_migrate(item),
        None => {}
    }
    if repos.migrate_highlight().is_some() && side.is_none() {
        // Opened after this frame's bake: drawn, with its glass, in the next frame, asked for at once.
        ui.ctx().request_repaint();
    }
}

/// What the keys among `events` do, after the clicks of the frame did `done` (nothing more, then): in the side menu
/// when `on_side` (its letters, Left, Esc and `q` back to the menu, Right nothing), else in the menu (Right into
/// Migrate's side menu). Moves the highlight `at` of the menu or `side_at` of the side menu; while the side menu has
/// the keys, the menu's highlight stays on Migrate.
fn keyed(events: &[Event], on_side: bool, at: &mut Option<usize>, side_at: &mut Option<usize>, done: Option<Done>) -> Option<Done> {
    let migrate = side_line();
    let mut done = done;
    if on_side {
        // The menu's letters do nothing here.
        for ask in asks(events, side_letter) {
            if done.is_some() {
                break;
            }
            match ask {
                Ask::Close | Ask::Back => done = Some(Done::Back),
                Ask::Run(item) => done = Some(Done::Side(item)),
                Ask::Highlighted => done = side_at.map(|index| Done::Side(MigrateItem::ALL[index])),
                Ask::Into => {}
                Ask::Step(_) | Ask::End(_) => *side_at = moved(*side_at, ask, MigrateItem::ALL.len()),
            }
        }
        *at = Some(migrate);
        return done;
    }
    for ask in asks(events, menu_letter) {
        if done.is_some() {
            break;
        }
        match ask {
            Ask::Close => done = Some(Done::Close),
            Ask::Run(item) => done = Some(Done::Run(item)),
            Ask::Highlighted => done = at.map(|index| Done::Run(SHOWN[index])),
            Ask::Into => done = (*at == Some(migrate)).then_some(Done::Run(SHOWN[migrate])),
            Ask::Back => {}
            Ask::Step(_) | Ask::End(_) => *at = moved(*at, ask, SHOWN.len()),
        }
    }
    done
}

/// Draws the menu's lines in its pane at `pane` on the overlay in `ui`, the line `at` on the selection's fill, in the
/// words for several repositories when `many`, the forks' line in the words of the core's `menu`.
fn draw_menu(ui: &Ui, pane: Rect, at: Option<usize>, many: bool, menu: Option<&Menu>, palette: Palette) {
    let (rows, rules) = layout(pane);
    let painter = ui.painter().with_clip_rect(pane.intersect(ui.clip_rect()));
    for rule in rules {
        painter.rect_filled(rule, 0.0, palette.on_glass.gamma_multiply(MENU_LINE_OPACITY));
    }
    let words_font = font(TEXT, FontFamily::Proportional);
    for (index, (row, (item, icon))) in rows.iter().zip(SHOWN.into_iter().zip(ICONS)).enumerate() {
        let (ink, icon_ink) = if item == MenuItem::Remove { (palette.danger, palette.danger) } else { (palette.on_glass, palette.quiet) };
        let line =
            Line { words: words(item, many, menu), key: item.key(), icon, ink, icon_ink, lit: Some(index) == at, key_right: key_right(*row, item) };
        line.draw(ui, &painter, *row, palette, &words_font);
        if opens_side(item) {
            draw_icon(&painter, chevron_centre(*row), Icon::ChevronRight, MENU_CHEVRON, palette.quiet);
        }
    }
}

/// Draws the side menu's lines in its pane at `side` on the overlay in `ui`, the line `at` on the selection's fill.
fn draw_side(ui: &Ui, side: Rect, at: Option<usize>, palette: Palette) {
    let painter = ui.painter().with_clip_rect(side.intersect(ui.clip_rect()));
    let words_font = font(TEXT, FontFamily::Proportional);
    for (index, (row, (item, icon))) in side_layout(side).into_iter().zip(MigrateItem::ALL.into_iter().zip(SIDE_ICONS)).enumerate() {
        let line = Line {
            words: item.label(),
            key: item.key(),
            icon,
            ink: palette.on_glass,
            icon_ink: palette.quiet,
            lit: Some(index) == at,
            key_right: row.right() - MENU_ROW_PAD,
        };
        line.draw(ui, &painter, row, palette, &words_font);
    }
}

/// One line of the menu or the side menu, as it is drawn.
struct Line {
    /// Its words.
    words: &'static str,
    /// The core's key that runs it.
    key: char,
    /// Its icon.
    icon: Icon,
    /// The colour of its words.
    ink: Color32,
    /// The colour of its icon.
    icon_ink: Color32,
    /// It is highlighted: on the selection's fill.
    lit: bool,
    /// Where its key's name ends at the right.
    key_right: f32,
}

impl Line {
    /// Draws it at `row` with `painter`: the selection's fill when lit, its icon, its words cut to the room left of
    /// its key, and its key in the quiet colour.
    fn draw(&self, ui: &Ui, painter: &Painter, row: Rect, palette: Palette, words_font: &FontId) {
        if self.lit {
            painter.rect_filled(row, MENU_ROW_RADIUS, palette.selection);
        }
        let y = row.center().y;
        draw_icon(painter, pos2(row.left() + MENU_ROW_PAD + ICON / 2.0, y), self.icon, ICON, self.icon_ink);
        let left = row.left() + MENU_ROW_PAD + ICON + MENU_ROW_GAP;
        let room = self.key_right - MENU_KEY_WIDTH - MENU_ROW_GAP - left;
        painter.text(pos2(left, y), Align2::LEFT_CENTER, widgets::ellipsis(self.words, words_font, room, ui), words_font.clone(), self.ink);
        painter.text(pos2(self.key_right, y), Align2::RIGHT_CENTER, key_name(self.key), font(KEY_TEXT, FontFamily::Proportional), palette.quiet);
    }
}

/// Where the right edge of the key's name of `item`'s line at `row` stands: at the row's padding, or for a line that
/// opens the side menu, before its chevron ([`MENU_CHEVRON`], [`MENU_ROW_GAP`] apart).
fn key_right(row: Rect, item: MenuItem) -> f32 {
    let end = row.right() - MENU_ROW_PAD;
    if opens_side(item) { end - MENU_CHEVRON - MENU_ROW_GAP } else { end }
}

/// The centre of the chevron of the line at `row` that opens the side menu: at the row's right padding.
fn chevron_centre(row: Rect) -> Pos2 {
    pos2(row.right() - MENU_ROW_PAD - MENU_CHEVRON / 2.0, row.center().y)
}

/// The name of a line's key `key` at the end of its line: the core's key, as a capital, as the drawing writes it.
fn key_name(key: char) -> String {
    key.to_ascii_uppercase().to_string()
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::{HELD, Mode as CoreMode};
    use egui::{Memory, Modifiers, Pos2};

    use super::*;
    use crate::dialogs::Which;
    use crate::dialogs::testing::{Window, behind, live, until};
    use crate::session::Session;
    use crate::table;
    use crate::theme::SUBMENU_GAP;

    /// Every line of the core's menu, and whether the window shows it. No wildcard: a new line of the core's must
    /// be placed here on purpose. Every line is shown.
    fn placed(item: MenuItem) -> bool {
        match item {
            MenuItem::Update
            | MenuItem::Open
            | MenuItem::Github
            | MenuItem::Group
            | MenuItem::CopyPath
            | MenuItem::Watch
            | MenuItem::PullRequests
            | MenuItem::ShowForks
            | MenuItem::Forks
            | MenuItem::AllForks
            | MenuItem::Migrate
            | MenuItem::Remove => true,
        }
    }

    /// Every line of the core's Migrate menu, and whether the window's side menu shows it. No wildcard.
    fn placed_in_side(item: MigrateItem) -> bool {
        match item {
            MigrateItem::Folder | MigrateItem::Service => true,
        }
    }

    #[test]
    fn the_menu_lists_every_line_of_the_core() {
        let all: Vec<MenuItem> = MenuItem::ALL.into_iter().filter(|item| placed(*item)).collect();
        assert_eq!(all, MenuItem::ALL, "no line of the core's is hidden");
        assert_eq!(shown().to_vec(), all, "the shown lines, in the core's order");
        // The core's words and keys, one of them in the plural; Migrate's without the core's own arrow, which the
        // window draws as the drawing's chevron.
        let said: Vec<(&str, String)> = shown().iter().copied().map(|item| (words(item, false, None), key_name(item.key()))).collect();
        assert_eq!(
            said,
            [
                ("Update", "U"),
                ("Open the folder", "O"),
                ("Open on GitHub", "G"),
                ("Move to a group…", "M"),
                ("Copy the path", "C"),
                ("Auto-pull on or off", "W"),
                ("Fetch the pull requests", "P"),
                ("Show the forks", "F"),
                ("Clone the forks", "N"),
                ("Update the forks of all repositories", "A"),
                ("Migrate", "T"),
                ("Remove…", "D"),
            ]
            .map(|(label, key)| (label, key.to_owned()))
        );
        assert_eq!(MenuItem::Migrate.label(false), "Migrate ▸", "the core's words, of which the window draws the arrow as an icon");
        assert_eq!((words(MenuItem::Open, true, None), words(MenuItem::CopyPath, true, None)), ("Open the folders", "Copy the paths"));
        // The forks' line in the core's words for the menu it is in: an update once every chosen repository has them.
        let menu = |forks_cloned| devassist_core::repos::state::Menu { repos: vec!["api".to_owned()], selected: 0, migrate: None, forks_cloned };
        assert_eq!(words(MenuItem::Forks, false, Some(&menu(true))), "Update the forks");
        assert_eq!(words(MenuItem::Forks, false, Some(&menu(false))), "Clone the forks");
        assert_eq!(words(MenuItem::ShowForks, false, Some(&menu(true))), "Show the forks");
        // The drawing's icons, and its rules: before the forks' block, before Migrate, before Remove.
        let icon_of = |item: MenuItem| ICONS[line_of(item)];
        assert_eq!([MenuItem::ShowForks, MenuItem::Forks, MenuItem::AllForks].map(icon_of), [Icon::GitFork, Icon::CopyPlus, Icon::RefreshCw]);
        let ruled: Vec<MenuItem> = SHOWN.into_iter().filter(|item| rule_before(*item)).collect();
        assert_eq!(ruled, [MenuItem::ShowForks, MenuItem::Migrate, MenuItem::Remove]);
        // The side menu: every line of the core's, in its order, words and keys.
        let side: Vec<MigrateItem> = MigrateItem::ALL.into_iter().filter(|item| placed_in_side(*item)).collect();
        assert_eq!(side, MigrateItem::ALL);
        let side: Vec<(&str, String)> = MigrateItem::ALL.into_iter().map(|item| (item.label(), key_name(item.key()))).collect();
        assert_eq!(side, [("Move to another folder…", "F"), ("Push to another service…", "S")].map(|(label, key)| (label, key.to_owned())));
    }

    fn press(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    /// A key's ask of the menu.
    type MenuAsk = Ask<MenuItem>;

    #[test]
    fn the_keys_are_read_as_the_terminal_apps_menu_reads_them() {
        let read = |events: Vec<Event>| asks(&events, menu_letter);
        assert_eq!(
            read(vec![press(Key::ArrowUp), press(Key::ArrowDown), press(Key::Home), press(Key::End)]),
            [Ask::Step(true), Ask::Step(false), Ask::End(true), Ask::End(false)]
        );
        assert_eq!(read(vec![press(Key::Enter), press(Key::Escape)]), [Ask::Highlighted, Ask::Close]);
        assert_eq!(read(vec![press(Key::ArrowLeft), press(Key::ArrowRight)]), [Ask::Back, Ask::Into]);
        assert_eq!(read(vec![Event::Text("kjq".to_owned())]), [Ask::Step(true), Ask::Step(false), Ask::Close]);
        assert_eq!(read(vec![Event::Text("u".to_owned())]), [Ask::Run(MenuItem::Update)]);
        assert_eq!(read(vec![Event::Text("D".to_owned())]), [Ask::Run(MenuItem::Remove)], "as the terminal app, a capital too");
        assert_eq!(read(vec![Event::Text("t".to_owned())]), [Ask::Run(MenuItem::Migrate)]);
        // The forks' letters run their lines.
        assert_eq!(
            read(vec![Event::Text("fna".to_owned())]),
            [Ask::Run(MenuItem::ShowForks), Ask::Run(MenuItem::Forks), Ask::Run(MenuItem::AllForks)]
        );
        // A key with Ctrl is no key of the menu's.
        assert_eq!(read(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]), []);
        // The side menu's letters are its own; the menu's do nothing there.
        let side = |events: Vec<Event>| asks(&events, side_letter);
        assert_eq!(side(vec![Event::Text("fS".to_owned())]), [Ask::Run(MigrateItem::Folder), Ask::Run(MigrateItem::Service)]);
        assert_eq!(side(vec![Event::Text("utdm".to_owned())]), []);
    }

    #[test]
    fn the_highlight_stops_at_the_ends_as_in_the_terminal_app() {
        let lines = SHOWN.len();
        assert_eq!(moved(Some(0), MenuAsk::Step(true), lines), Some(0));
        assert_eq!(moved(Some(0), MenuAsk::Step(false), lines), Some(1));
        assert_eq!(moved(Some(11), MenuAsk::Step(false), lines), Some(11));
        assert_eq!(moved(Some(3), MenuAsk::End(true), lines), Some(0));
        assert_eq!(moved(Some(3), MenuAsk::End(false), lines), Some(11));
        // From no line shown, a step goes to the first; Enter and the rest change nothing.
        assert_eq!(moved(None, MenuAsk::Step(true), lines), Some(0));
        assert_eq!(moved(None, MenuAsk::Step(false), lines), Some(0));
        assert_eq!(moved(None, MenuAsk::End(false), lines), Some(11));
        assert_eq!(moved(None, MenuAsk::Highlighted, lines), None);
        // The side menu's two, the same way.
        assert_eq!(moved(Some(1), MenuAsk::Step(false), 2), Some(1));
        assert_eq!(moved(Some(0), MenuAsk::Step(true), 2), Some(0));
        assert_eq!(moved(Some(0), MenuAsk::End(false), 2), Some(1));
    }

    /// The place of `item`'s line among the lines shown.
    fn line_of(item: MenuItem) -> usize {
        SHOWN.iter().position(|shown| *shown == item).unwrap()
    }

    /// How wide the demo's menu is (on api and web, as `--show menu` opens it), measured with the window's faces.
    fn demo_width() -> f32 {
        demo_size().x
    }

    #[test]
    fn the_menu_is_the_drawings() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0));
        // Seven lines, a rule, the forks' three, a rule, Migrate, a rule, Remove, and the pane's room:
        // 8 + 7 × 32 + 9 + 3 × 32 + 9 + 32 + 9 + 32.
        assert!((height() - 419.0).abs() < f32::EPSILON);
        let anchor = Rect::from_min_max(pos2(1388.0, 228.0), pos2(1420.0, 272.0));
        let places = place(window, anchor, false, MENU_WIDTH);
        assert_eq!(places.pane, Rect::from_min_size(pos2(1164.0, 276.0), vec2(256.0, 419.0)));
        assert_eq!(places.side, None);
        let (rows, rules) = layout(places.pane);
        assert_eq!(rows.len(), 12);
        assert_eq!(rows[0], Rect::from_min_size(pos2(1168.0, 280.0), vec2(248.0, 32.0)));
        for block in [&rows[..7], &rows[7..10]] {
            for pair in block.windows(2) {
                assert_eq!(pair[0].bottom(), pair[1].top());
            }
        }
        // The rules in the middle of their nine points: before Show the forks, before Migrate, and before Remove.
        assert_eq!(rules.len(), 3);
        for (rule, line) in rules.iter().zip([7, 10, 11]) {
            assert_eq!(rows[line].top() - rows[line - 1].bottom(), MENU_LINE);
            assert!((rule.center().y - (rows[line - 1].bottom() + MENU_LINE / 2.0)).abs() < f32::EPSILON);
            assert_eq!((rule.left(), rule.right(), rule.height()), (1176.0, 1408.0, 1.0));
        }
        assert_eq!(rows[11].bottom() + MENU_PAD, places.pane.bottom());
        // With Migrate open: its side menu of two lines, 264 wide, at the menu's left, level with Migrate's bottom.
        let places = place(window, anchor, true, MENU_WIDTH);
        let side = places.side.expect("the side menu is placed");
        let migrate = rows[line_of(MenuItem::Migrate)];
        assert_eq!(side, Rect::from_min_max(pos2(1164.0 - SUBMENU_GAP - SUBMENU_WIDTH, migrate.bottom() - 72.0), pos2(1156.0, migrate.bottom())));
        let lines = side_layout(side);
        assert_eq!(lines, [0.0, 32.0].map(|top| Rect::from_min_size(pos2(side.left() + MENU_PAD, side.top() + MENU_PAD + top), vec2(256.0, 32.0))));
        // A wider menu keeps its right edge on the button's.
        let places = place(window, anchor, false, 300.0);
        assert_eq!(places.pane, Rect::from_min_size(pos2(1120.0, 276.0), vec2(300.0, 419.0)));
    }

    #[test]
    fn the_menu_is_as_wide_as_its_longest_line_needs() {
        // What a line takes beside its words: the row's padding at both ends, the icon, a gap after it, a gap before
        // the key, the key; and the pane's own padding.
        let beside = 2.0 * MENU_PAD + 2.0 * MENU_ROW_PAD + ICON + 2.0 * MENU_ROW_GAP + MENU_KEY_WIDTH;
        // Lines that fit the theme's width: the theme's width.
        assert_eq!(width(&[(MenuItem::Update, 40.0), (MenuItem::Remove, 60.0)]), MENU_WIDTH);
        assert_eq!(width(&[(MenuItem::Update, MENU_WIDTH - beside)]), MENU_WIDTH);
        // A line whose words need more: as wide as it needs, in whole points.
        assert_eq!(width(&[(MenuItem::AllForks, 220.0), (MenuItem::Update, 40.0)]), beside + 220.0);
        assert_eq!(width(&[(MenuItem::AllForks, 219.4)]), beside + 220.0);
        // Migrate's chevron takes its room after the key.
        assert_eq!(width(&[(MenuItem::Migrate, 220.0)]), beside + 220.0 + MENU_CHEVRON + MENU_ROW_GAP);
        // The demo's menu: the core's longest line needs more than the theme's width, and every line is drawn whole.
        let measured = demo_width();
        assert!(measured > MENU_WIDTH, "{measured}");
        let mut window = demo();
        open_on(&mut window, "api");
        assert!((window.menu_pane().unwrap().width() - measured).abs() < f32::EPSILON);
        for item in SHOWN {
            let said = words(item, true, window.repos().menu.as_ref());
            let drawn = window.texts.iter().find(|(words, ..)| words == said);
            assert!(drawn.is_some_and(|(_, _, cut)| !cut), "{said} is cut or missing");
        }
    }

    #[test]
    #[expect(clippy::cast_precision_loss, reason = "places in a window of a few hundred points")]
    fn the_wider_menu_never_leaves_the_smallest_window() {
        let size = vec2(960.0, 600.0);
        let window = Rect::from_min_size(Pos2::ZERO, size);
        let room = window.shrink(crate::theme::DIALOG_MARGIN);
        let width = demo_width();
        let panel = crate::shell::layout(size, true, true).panel;
        let lines = crate::repos_view::lines(&crate::demo::repos());
        // Every row's "…" in the smallest window, and buttons all along its edges and across its middle.
        let mut anchors: Vec<Rect> = lines
            .iter()
            .filter_map(|line| match line {
                crate::repos_view::Line::Repo(repo) => table::dots_rect(panel, &lines, 0.0, &repo.key),
                crate::repos_view::Line::Group(_) => None,
            })
            .collect();
        assert!(anchors.len() >= 8, "the rows in view: {}", anchors.len());
        for x in (0..=960).step_by(16) {
            for y in [0.0, 300.0, 568.0] {
                anchors.push(Rect::from_min_size(pos2(x as f32, y), vec2(32.0, 32.0)).intersect(window));
            }
        }
        for y in (0..=600).step_by(16) {
            for x in [0.0, 480.0, 928.0] {
                anchors.push(Rect::from_min_size(pos2(x, y as f32), vec2(32.0, 32.0)).intersect(window));
            }
        }
        // As the window opens it, with its side menu, on the first row in view and on the last whole one.
        let keys: Vec<String> = lines
            .iter()
            .filter_map(|line| match line {
                crate::repos_view::Line::Repo(repo) => Some(repo.key.clone()),
                crate::repos_view::Line::Group(_) => None,
            })
            .filter(|key| table::dots_rect(panel, &lines, 0.0, key).is_some_and(|dots| dots.height() >= 16.0))
            .collect();
        for key in [keys.first().unwrap(), keys.last().unwrap()] {
            let mut drawn = Window::new(Session::demo(crate::demo::repos()));
            drawn.size = [960.0, 600.0];
            drawn.frame(Vec::new());
            open_on(&mut drawn, key);
            tap(&mut drawn, Key::T, Some("t"));
            let (menu_pane, side_pane) = (drawn.menu_pane().unwrap(), drawn.submenu_pane().expect("the side menu is drawn"));
            assert!(window.contains_rect(menu_pane) && room.contains_rect(side_pane), "{key}: {menu_pane:?} {side_pane:?}");
            assert_eq!((menu_pane.size(), side_pane.size()), (vec2(width, height()), vec2(SUBMENU_WIDTH, side_height())), "{key}: cut");
        }
        for anchor in anchors {
            for side in [false, true] {
                let places = place(window, anchor, side, width);
                let what = format!("{places:?} for {anchor:?}");
                assert!(window.contains_rect(places.pane), "{what}: the menu leaves the window");
                assert_eq!(places.pane.size(), vec2(width, height()), "{what}: the menu is cut");
                if let Some(side) = places.side {
                    assert!(room.contains_rect(side), "{what}: the side menu leaves the window");
                    assert_eq!(side.size(), vec2(SUBMENU_WIDTH, side_height()), "{what}: the side menu is cut");
                }
            }
        }
    }

    #[test]
    fn the_migrate_line_has_its_key_before_its_chevron() {
        let row = Rect::from_min_size(pos2(1168.0, 520.0), vec2(248.0, 32.0));
        // The drawing's `fixed(key, 16)`, then the chevron, 8 apart, the chevron at the row's padding.
        assert_eq!(chevron_centre(row), pos2(1416.0 - MENU_ROW_PAD - MENU_CHEVRON / 2.0, row.center().y));
        assert_eq!(key_right(row, MenuItem::Migrate), 1416.0 - MENU_ROW_PAD - MENU_CHEVRON - MENU_ROW_GAP);
        assert_eq!(key_right(row, MenuItem::Remove), 1416.0 - MENU_ROW_PAD);
    }

    /// The demo's window, a first frame drawn.
    fn demo() -> Window {
        let mut window = Window::new(Session::demo(crate::demo::repos()));
        window.frame(Vec::new());
        window
    }

    /// The place of the "…" of the repository `key` in the window's last frame.
    fn dots(window: &Window, key: &str) -> Rect {
        let layout = window.layout();
        table::dots_rect(layout.panel, &crate::repos_view::lines(window.repos()), 0.0, key).unwrap()
    }

    /// Clicks the "…" of the repository `key`, and the frames after it until the menu is drawn.
    fn open_on(window: &mut Window, key: &str) {
        let at = dots(window, key).center();
        window.click(at);
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu), "the menu is open: {}", window.repos().status);
        assert!(window.painted, "and drawn");
    }

    /// The menu's lines in the last frame, with their places.
    fn rows(window: &Window) -> Vec<Rect> {
        layout(window.menu_pane().expect("the menu was drawn")).0
    }

    #[test]
    fn the_dots_open_the_menu_for_that_row_and_clear_other_ticks() {
        let mut window = demo();
        assert_eq!(window.repos().marked, ["api", "web"]);
        open_on(&mut window, "homelab");
        assert_eq!(window.repos().marked, Vec::<String>::new(), "homelab is not ticked: the ticks are cleared");
        assert_eq!(window.repos().selected.as_deref(), Some("homelab"));
        assert_eq!(window.repos().chosen(), ["homelab"]);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update));
        // It stands under the button, its right edge on the button's.
        let pane = rows(&window)[0].expand(MENU_PAD);
        let button = dots(&window, "homelab");
        assert_eq!((pane.right(), pane.top()), (button.right(), button.bottom() + crate::theme::MENU_GAP));
        assert_eq!(window.screen_menu(), Some("homelab"), "the screen knows the row");
    }

    #[test]
    fn the_dots_on_a_ticked_row_keep_the_ticks() {
        let mut window = demo();
        open_on(&mut window, "web");
        assert_eq!(window.repos().marked, ["api", "web"]);
        assert_eq!(window.repos().chosen(), ["api", "web"], "the menu is for both");
        assert_eq!(window.repos().selected.as_deref(), Some("web"));
    }

    #[test]
    fn a_letter_runs_its_item_and_closes_the_menu() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        window.typed(Key::W, "w");
        assert_eq!(window.repos().status, HELD);
        assert!(window.repos().mode == CoreMode::List);
        window.frame(Vec::new());
        assert!(!window.open(), "the overlay went with it");
        // A third of a second on egui's clock, so the next click is no double click with the one that opened it.
        for _ in 0..20 {
            window.frame(Vec::new());
        }
        // Esc closes.
        open_on(&mut window, "homelab");
        window.key(Key::Escape);
        assert!(window.repos().mode == CoreMode::List);
        window.frame(Vec::new());
        assert!(!window.open());
    }

    #[test]
    fn keys_do_not_reach_the_table_while_the_menu_is_open() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        let before = (window.repos().marked.clone(), window.repos().selected.clone(), window.repos().list_filter.clone());
        window.typed(Key::Space, " ");
        window.typed(Key::S, "s");
        window.typed(Key::Slash, "/");
        window.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        window.key(Key::Tab);
        assert!(window.shows(Which::Menu));
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone(), window.repos().list_filter.clone()), before);
        assert_eq!(window.ctx.memory(Memory::focused), None, "Tab gave the focus to nothing");
        // The arrows move the menu's highlight, not the table's.
        window.key(Key::ArrowDown);
        window.key(Key::ArrowDown);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Github));
        window.key(Key::End);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Remove));
        window.key(Key::ArrowDown);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Remove), "it stops at the end");
        window.key(Key::Home);
        window.key(Key::ArrowUp);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update));
        assert_eq!(window.repos().selected.as_deref(), Some("homelab"));
    }

    #[test]
    fn a_click_outside_closes_the_menu_and_goes_no_further() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        // On another row's "…": the menu closes and no other opens, nothing is picked.
        let other = dots(&window, "notes").center();
        window.click(other);
        assert!(window.repos().mode == CoreMode::List);
        assert_eq!(window.repos().selected.as_deref(), Some("homelab"));
        window.frame(Vec::new());
        assert!(!window.open());
    }

    #[test]
    fn the_pointer_moves_the_highlight_and_a_click_runs_the_line() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        let lines = rows(&window);
        window.frame(vec![Event::PointerMoved(lines[3].center())]);
        window.frame(Vec::new());
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Group));
        // A key moves it on while the pointer rests; one highlight only.
        window.key(Key::ArrowDown);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::CopyPath));
        // The click runs the line under it, Move to a group (a dialog), not the highlighted Copy the path (held).
        window.click(lines[3].center());
        assert!(window.shows(Which::Group), "{}", window.repos().status);
        assert_ne!(window.repos().status, HELD);
    }

    /// A press and release at `at`, at the times `pressed` and `pressed + 0.05` seconds.
    fn click_at_time(window: &mut Window, at: Pos2, pressed: f64) {
        let button = |down| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: down, modifiers: Modifiers::NONE };
        window.frame_at(vec![Event::PointerMoved(at)], Some(pressed - 0.01));
        window.frame_at(vec![button(true)], Some(pressed));
        window.frame_at(vec![button(false)], Some(pressed + 0.05));
    }

    /// Opens the menu on `key` at egui's clock 0 to 1 seconds, so the clicks after it start afresh.
    fn open_at_time(window: &mut Window, key: &str) {
        let at = dots(window, key).center();
        click_at_time(window, at, 0.5);
        window.frame_at(Vec::new(), Some(0.6));
        window.frame_at(Vec::new(), Some(0.7));
        assert!(window.shows(Which::Menu));
    }

    /// The buttons of the dialog `probe` shows, placed as offsets from its window's centre: a dialog stands centred,
    /// so in a window of another size its buttons are the same offsets from that window's centre.
    fn button_offsets(probe: &Window) -> Vec<(Rect, String)> {
        let centre = Rect::from_min_size(Pos2::ZERO, vec2(probe.size[0], probe.size[1])).center().to_vec2();
        probe.buttons().into_iter().map(|(place, words)| (place.translate(-centre), words)).collect()
    }

    /// A window size, a row among `keys` of `repos` and a point where the menu's line `line`, opened on that row,
    /// lies over one of `buttons` (offsets from the window's centre): where a double click on that line has its
    /// second click on that button. Looked for in the window sizes `widths` by `heights`.
    fn overlap(
        repos: &Repos,
        keys: &[&str],
        line: usize,
        buttons: &[(Rect, String)],
        widths: &[u16],
        heights: &[u16],
    ) -> ([f32; 2], String, Pos2, String) {
        let lines = crate::repos_view::lines(repos);
        let menu_width = demo_width();
        for &width in widths {
            for &height in heights {
                let size = vec2(f32::from(width), f32::from(height));
                let screen = Rect::from_min_size(Pos2::ZERO, size);
                let panel = crate::shell::layout(size, true, true).panel;
                for key in keys {
                    let Some(anchor) = table::dots_rect(panel, &lines, 0.0, key) else { continue };
                    let row = layout(place(screen, anchor, false, menu_width).pane).0[line];
                    for (button, words) in buttons {
                        let both = row.intersect(button.translate(screen.center().to_vec2()));
                        if both.width() > 4.0 && both.height() > 4.0 {
                            return ([size.x, size.y], (*key).to_owned(), both.center(), words.clone());
                        }
                    }
                }
            }
        }
        panic!("no window size puts the line over a button");
    }

    /// A double click at `at`: its first click at egui's clock 5.0, its second at 5.2, `between` run after the
    /// first click's release (egui's clock standing still).
    fn double_click(window: &mut Window, at: Pos2, between: impl FnOnce(&mut Window)) {
        click_at_time(window, at, 5.0);
        between(window);
        click_at_time(window, at, 5.2);
        window.frame_at(Vec::new(), Some(5.3));
    }

    #[test]
    fn the_second_click_of_a_double_click_on_remove_presses_no_button_of_its_dialog() {
        let mut probe = demo();
        probe.session.repos_mut().unwrap().begin_remove();
        probe.frame(Vec::new());
        let buttons = button_offsets(&probe);
        let (size, key, at, words) = overlap(
            probe.repos(),
            &["api", "web"],
            line_of(MenuItem::Remove),
            &buttons,
            &(520..=760).step_by(20).collect::<Vec<_>>(),
            &(800..=1600).step_by(25).collect::<Vec<_>>(),
        );
        let mut window = Window::new(Session::demo(crate::demo::repos()));
        window.size = size;
        window.frame(Vec::new());
        open_at_time(&mut window, &key);
        assert!(rows(&window)[line_of(MenuItem::Remove)].contains(at), "the point is on Remove…");
        double_click(&mut window, at, |window| {
            window.frame_at(Vec::new(), Some(5.1));
            assert!(window.shows(Which::Remove));
            assert!(window.buttons().iter().any(|(place, said)| *said == words && place.contains(at)), "the point is on {words}");
        });
        assert!(window.shows(Which::Remove), "the second click pressed {words}");
        assert_ne!(window.repos().status, HELD, "the second click pressed {words}");
        assert_eq!(window.repos().installed.len(), 12);
        // A click of its own, later, is one.
        click_at_time(&mut window, at, 9.0);
        window.frame_at(Vec::new(), Some(9.1));
        assert!(!window.shows(Which::Remove), "a click of its own on {words} did nothing");
    }

    #[test]
    fn the_second_click_of_a_double_click_on_update_neither_hides_nor_cancels_the_run() {
        let _live = crate::session::tests::live_git();
        // Enough rows that a row's menu reaches down to the progress dialog's buttons.
        let owned: Vec<String> = (0..18).map(|index| format!("r{index:02}")).collect();
        let names: Vec<&str> = owned.iter().map(String::as_str).collect();
        // The progress dialog of a run of all of them, as it stands while the run goes: Cancel and Hide.
        let mut probe = Window::new(crate::dialogs::testing::session_with(crate::dialogs::testing::run(&names, 1)));
        probe.frame(Vec::new());
        assert_eq!(probe.said(), ["Cancel", "Hide"]);
        let buttons = button_offsets(&probe);
        let (_root, settings) = behind("menu-double", &names);
        let mut window = live(settings, names.len());
        window.session.repos_mut().unwrap().marked = names.iter().map(|name| (*name).to_owned()).collect();
        let (size, key, at, words) = overlap(
            window.repos(),
            &names,
            0,
            &buttons,
            &(900..=1120).step_by(10).collect::<Vec<_>>(),
            &(900..=1400).step_by(10).collect::<Vec<_>>(),
        );
        window.size = size;
        window.frame(Vec::new());
        open_at_time(&mut window, &key);
        assert!(rows(&window)[0].contains(at), "the point is on Update");
        double_click(&mut window, at, |window| {
            // egui's clock stands still while the run starts and its dialog opens.
            let mut tick = 5.06;
            while !window.shows(Which::Progress) || !window.painted {
                window.session.tick();
                window.frame_at(Vec::new(), Some(tick));
                tick += 0.0001;
                assert!(tick < 5.1, "the dialog did not open");
            }
            assert!(window.buttons().iter().any(|(place, said)| *said == words && place.contains(at)), "the point is on {words}");
        });
        assert!(window.shows(Which::Progress), "the second click pressed {words}");
        assert!(!window.job().cancelling, "the second click pressed {words}");
        // A click of its own, later, is one.
        click_at_time(&mut window, at, 9.0);
        window.frame_at(Vec::new(), Some(9.1));
        assert!(!window.shows(Which::Progress) || window.job().cancelling, "a click of its own on {words} did nothing");
        window.session.close();
    }

    #[test]
    fn a_double_click_made_on_an_open_dialog_is_a_click() {
        // The group dialog, opened by the core (no click): a quick double click on Cancel presses it once, on its
        // first click; the first click after opening is never held back.
        let mut window = demo();
        window.session.repos_mut().unwrap().open_group_prompt();
        window.frame_at(Vec::new(), Some(1.0));
        window.frame_at(Vec::new(), Some(1.1));
        assert!(window.shows(Which::Group));
        let (cancel, _) = window.buttons().into_iter().find(|(_, words)| *words == "Cancel").unwrap();
        click_at_time(&mut window, cancel.center(), 5.0);
        assert!(!window.shows(Which::Group), "the first click after opening did nothing");
        click_at_time(&mut window, cancel.center(), 5.2);
        window.frame_at(Vec::new(), Some(5.3));
        assert!(window.repos().mode == CoreMode::List);
    }

    #[test]
    fn a_resting_pointer_does_not_choose_the_line_enter_runs() {
        let mut window = demo();
        window.session.repos_mut().unwrap().clear_marks();
        window.session.repos_mut().unwrap().pick("recipes");
        // The pointer rests where the menu's Remove line will stand.
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0));
        let remove = layout(place(screen, dots(&window, "recipes"), false, demo_width()).pane).0[line_of(MenuItem::Remove)].center();
        window.frame(vec![Event::PointerMoved(remove)]);
        window.frame(Vec::new());
        window.key(Key::Enter);
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu));
        assert_eq!(rows(&window)[line_of(MenuItem::Remove)].center(), remove, "the pointer is over Remove");
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update), "the resting pointer moved the highlight");
        // The Enter that opened it comes up, and a second one runs the highlighted line.
        window.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        window.key(Key::Enter);
        assert!(window.repos().mode == CoreMode::List, "Enter ran Remove, not Update");
        assert_eq!(window.repos().status, HELD, "Enter did not run Update");
    }

    #[test]
    fn enter_runs_the_line_the_core_highlights() {
        // Every line of the core's menu is shown: the core's highlight on Show the forks is drawn there, and Enter
        // runs it (homelab's forks are not cloned: the core says so, without the terminal's keys).
        let mut window = demo();
        open_on(&mut window, "homelab");
        window.session.repos_mut().unwrap().highlight_menu(MenuItem::ShowForks);
        window.frame(Vec::new());
        assert_eq!(lit(&window), [rows(&window)[line_of(MenuItem::ShowForks)]]);
        window.key(Key::Enter);
        assert!(window.repos().mode == CoreMode::List && window.repos().forks.is_none());
        assert_eq!((window.repos().status.as_str(), window.repos().status_error), (NOT_CLONED_HOMELAB, true));
    }

    /// What the core says, in a window's words, of "Show the forks" on homelab, whose forks are not cloned.
    const NOT_CLONED_HOMELAB: &str = "The forks of homelab are not cloned yet. Clone the forks does it.";

    /// The core's refusal of "Show the forks" for more than one repository.
    const ONE_REPOSITORY: &str = "Highlight one repository to see its forks.";

    #[test]
    fn show_the_forks_leaves_the_core_as_run_menu_does() {
        // On api alone (its forks held), on homelab alone (not cloned), and on the ticked api and web: the window's
        // line leaves the core's menu, mode, ticks, highlight and forks as `run_menu` does; only the notice for forks
        // that are not cloned names no key of the terminal's.
        for (row, ticked) in [("api", false), ("homelab", false), ("api", true)] {
            let open = |repos: &mut Repos| {
                if !ticked {
                    repos.clear_marks();
                }
                crate::repos_screen::open_menu(repos, row);
                repos.highlight_menu(MenuItem::ShowForks);
            };
            let (mut window_way, mut core_way) = (crate::demo::repos(), crate::demo::repos());
            open(&mut window_way);
            open(&mut core_way);
            run(&mut window_way, MenuItem::ShowForks);
            core_way.run_menu(MenuItem::ShowForks);
            let what = format!("{row}, ticked {ticked}");
            let state = |repos: &Repos| {
                (
                    repos.mode == CoreMode::List,
                    repos.menu.is_none(),
                    repos.marked.clone(),
                    repos.selected.clone(),
                    repos.forks.as_ref().map(|view| view.repo.clone()),
                    repos.status_error,
                )
            };
            assert_eq!(state(&window_way), state(&core_way), "{what}");
            assert_eq!(window_way.status_stamp(), core_way.status_stamp(), "{what}");
            match (row, ticked) {
                ("homelab", _) => {
                    assert_eq!(window_way.status, NOT_CLONED_HOMELAB, "{what}");
                    assert_eq!(core_way.status, "The forks of homelab are not cloned yet. Enter, then n clones them.", "{what}");
                }
                (_, true) => assert_eq!((window_way.status.as_str(), core_way.status.as_str()), (ONE_REPOSITORY, ONE_REPOSITORY), "{what}"),
                _ => {
                    assert_eq!(window_way.forks.as_ref().map(|view| view.repo.as_str()), Some("api"), "{what}");
                    assert_eq!(window_way.status, core_way.status, "{what}");
                }
            }
        }
    }

    /// How "Show the forks" is chosen in the hand-over to the forks screen.
    #[derive(Clone, Copy, Debug)]
    enum Chose {
        Enter,
        Letter,
        Click,
        DoubleClick,
    }

    #[test]
    fn show_the_forks_opens_them_and_that_press_does_nothing_there() {
        use crate::forks_screen::tests::Win;
        for by in [Chose::Enter, Chose::Letter, Chose::Click, Chose::DoubleClick] {
            let mut win = Win::new(Session::demo(crate::demo::repos()));
            win.step = 0.02;
            win.repos_mut().clear_marks();
            let at = win.dots("api").center();
            win.click(at);
            win.frame(Vec::new());
            assert!(win.repos().mode == CoreMode::Menu, "{by:?}: the menu did not open");
            assert_eq!(win.repos().chosen(), ["api"]);
            let line = layout(win.menu.expect("the menu was placed")).0[line_of(MenuItem::ShowForks)];
            // What opening them alone says, on the same demo.
            let stamp = win.repos().status_stamp();
            let mut probe = crate::demo::repos();
            probe.clear_marks();
            probe.pick("api");
            let before = probe.status_stamp();
            probe.open_forks_plain();
            let opening = probe.status_stamp() - before;
            let held = |win: &mut Win, key: Key, text: Option<&str>| {
                let down = |repeat| {
                    let mut events = vec![Event::Key { key, physical_key: None, pressed: true, repeat, modifiers: Modifiers::NONE }];
                    events.extend(text.map(|text| Event::Text(text.to_owned())));
                    events
                };
                win.frame(down(false));
                for _ in 0..5 {
                    win.frame(down(true));
                }
                win.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
            };
            match by {
                Chose::Enter => {
                    for _ in 0..line_of(MenuItem::ShowForks) {
                        win.key(Key::ArrowDown, None);
                    }
                    assert_eq!(win.repos().menu_highlight(), Some(MenuItem::ShowForks));
                    held(&mut win, Key::Enter, None);
                }
                Chose::Letter => held(&mut win, Key::F, Some("f")),
                Chose::Click => win.click(line.center()),
                Chose::DoubleClick => {
                    win.click(line.center());
                    win.click(line.center());
                }
            }
            for _ in 0..3 {
                win.frame(Vec::new());
            }
            let what = format!("{by:?}");
            assert!(win.repos().forks.is_some() && win.plan.is_some(), "{what}: the forks are not shown: {}", win.repos().status);
            assert!(!overlay::is_open(&win.ctx) && win.menu.is_none(), "{what}: the menu is still there");
            // The point that chose the line lies on a fork's row: a press reaching the screen would have picked it.
            let plan = win.plan.clone().unwrap();
            assert!(plan.rows.iter().any(|row| win.row(&row.owner).contains(line.center())), "{what}: the line lies on no row of the forks");
            // Nothing was done there: no fork picked or opened, no filter typed into, no way back taken.
            let view = win.view();
            assert_eq!((view.selected.clone(), view.filter.as_str(), view.filtering, view.changed_only), (None, "", false, false), "{what}");
            assert!(!win.typing(), "{what}: the forks' filter has the keyboard");
            assert_eq!(win.repos().status_stamp(), stamp + opening, "{what}: the press said or ran something on the forks: {}", win.repos().status);
            assert_ne!(win.repos().status, HELD, "{what}");
        }
    }

    #[test]
    fn a_menu_for_several_says_to_highlight_one_and_opens_nothing() {
        let mut window = demo();
        open_on(&mut window, "api");
        assert_eq!(window.repos().chosen(), ["api", "web"]);
        window.typed(Key::F, "f");
        window.frame(Vec::new());
        assert!(window.repos().mode == CoreMode::List && !window.open() && window.repos().forks.is_none());
        assert_eq!((window.repos().status.as_str(), window.repos().status_error), (ONE_REPOSITORY, true));
        let status = crate::repos_screen::status(&window.session).unwrap();
        assert_eq!((status.text.as_str(), status.error), (ONE_REPOSITORY, true), "the status line shows the core's words");
        assert_eq!(window.repos().marked, ["api", "web"], "the ticks stay");
        // Opened on a row that is not ticked, the other ticks go and the menu is for that row alone: the core looks
        // for its forks.
        let mut window = demo();
        window.session.repos_mut().unwrap().toggle_mark_of("web");
        open_on(&mut window, "homelab");
        assert_eq!((window.repos().marked.clone(), window.repos().chosen()), (Vec::<String>::new(), vec!["homelab".to_owned()]));
        window.typed(Key::F, "f");
        assert_eq!(window.repos().status, NOT_CLONED_HOMELAB);
    }

    #[test]
    fn the_menu_says_update_when_the_forks_are_there() {
        // On api alone, whose forks the demo holds: "Update the forks"; on homelab, which has none, and on api and
        // web together (web has none): "Clone the forks". Each in the core's words for that menu.
        for (row, ticked, said) in [("api", false, "Update the forks"), ("homelab", false, "Clone the forks"), ("api", true, "Clone the forks")] {
            let mut window = demo();
            if !ticked {
                window.session.repos_mut().unwrap().clear_marks();
            }
            open_on(&mut window, row);
            let core = window.repos().menu.as_ref().unwrap().label(MenuItem::Forks);
            assert_eq!(core, said, "{row}");
            assert!(painted(&window, said), "{row}: {said} is not drawn");
            let other = if said == "Clone the forks" { "Update the forks" } else { "Clone the forks" };
            assert!(!painted(&window, other), "{row}: {other} is drawn");
            // Its line runs what the core runs for it: held, the core says so.
            window.typed(Key::N, "n");
            assert_eq!(window.repos().status, HELD, "{row}");
            assert!(window.repos().mode == CoreMode::List, "{row}");
        }
    }

    #[test]
    fn the_fork_letters_run_their_lines_once() {
        // n and a, each held: the held core refuses the run once, and the menu closes; held on, they do nothing on the
        // table under it (a is Add there, which the press that closed the menu must not open).
        for (key, letter) in [(Key::N, "n"), (Key::A, "a")] {
            let mut window = demo();
            window.session.repos_mut().unwrap().clear_marks();
            open_on(&mut window, "api");
            let stamp = window.repos().status_stamp();
            window.frame(vec![key_down(key, false), Event::Text(letter.to_owned())]);
            for _ in 0..5 {
                window.frame(vec![key_down(key, true), Event::Text(letter.to_owned())]);
            }
            window.frame(vec![key_up(key)]);
            window.frame(Vec::new());
            assert_eq!(window.repos().status, HELD, "{letter}");
            assert_eq!(window.repos().status_stamp(), stamp + 1, "{letter} ran more than once");
            assert!(window.repos().mode == CoreMode::List && !window.open(), "{letter}: {:?} opened", which_open(&window));
        }
        // While the side menu is open, the forks' letters do nothing (f is the side menu's own Move).
        for (key, letter) in [(Key::N, "n"), (Key::A, "a")] {
            let mut window = side_open_on_homelab();
            tap(&mut window, key, Some(letter));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_some(), "{letter} acted in the side menu");
            assert_ne!(window.repos().status, HELD, "{letter}");
        }
    }

    /// Which dialog the core's mode asks for in `window`, for a message.
    fn which_open(window: &Window) -> Option<Which> {
        crate::dialogs::which(window.repos())
    }

    #[test]
    fn clone_the_forks_starts_the_run_for_the_chosen_rows() {
        // Held: the core refuses the run of the chosen rows, and of every repository's forks, saying so.
        for item in [MenuItem::Forks, MenuItem::AllForks] {
            let mut window = demo();
            open_on(&mut window, "api");
            let letter = item.key().to_string();
            window.typed(Key::from_name(&letter.to_uppercase()).unwrap(), &letter);
            assert_eq!(window.repos().status, HELD, "{item:?}");
            assert!(window.repos().queue.is_empty() && window.repos().job.is_none() && !window.repos().busy, "{item:?}");
            assert_eq!(window.repos().marked, ["api", "web"], "{item:?}: the held core keeps the ticks");
        }
        // Live, with GitHub held and the forks from local repositories: N on api, whose forks are not cloned, runs
        // the fetch in the progress dialog, which counts the forks; the forks folder is there after it, and the menu
        // then offers to update them.
        let _live = crate::session::tests::live_git();
        let forks = LocalForks::new("menu-clone");
        let mut repos = Repos::new(forks.settings());
        devassist_core::repos::state::testing::hold_github(&mut repos);
        let remotes = forks.remotes();
        let remotes: Vec<(&str, &std::path::Path)> = remotes.iter().map(|(owner, folder)| (*owner, folder.as_path())).collect();
        devassist_core::repos::state::testing::local_forks(&mut repos, &remotes);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        until(&mut window, "read", |window| !window.repos().waiting() && window.repos().installed.len() == 1);
        window.frame(Vec::new());
        assert!(!devassist_core::repos::gitops::forks_cloned(&forks.api()), "the forks are cloned already");
        open_on(&mut window, "api");
        assert_eq!(window.repos().menu.as_ref().unwrap().label(MenuItem::Forks), "Clone the forks");
        window.typed(Key::N, "n");
        until(&mut window, "open", |window| window.shows(Which::Progress) && window.painted);
        until(&mut window, "ended", |window| window.repos().job.as_ref().is_some_and(|job| !job.running()) && !window.repos().busy);
        let job = window.job();
        assert!(matches!(job.result, Some(Ok(_))), "{:?} {:?}", job.result, job.items[0].output);
        assert_eq!(job.items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api"]);
        window.frame(Vec::new());
        let count = window.texts.iter().find(|(words, ..)| words.ends_with(" at a time")).map(|(words, ..)| words.clone());
        assert!(count.as_deref().is_some_and(|count| count.starts_with("2 of 2 forks, ")), "the dialog does not count the forks: {count:?}");
        assert!(devassist_core::repos::gitops::forks_cloned(&forks.api()), "the forks folder is not there");
        // Closed, the menu on api says the run is now an update.
        window.press("Close");
        window.frame(Vec::new());
        assert!(!window.open());
        for _ in 0..20 {
            window.frame(Vec::new());
        }
        open_on(&mut window, "api");
        assert_eq!(window.repos().menu.as_ref().unwrap().label(MenuItem::Forks), "Update the forks");
        assert!(painted(&window, "Update the forks"));
        window.session.close();
    }

    /// An original "on GitHub" with two forks that have commits of their own, all on this disk, and a checkout of the
    /// original, api, whose origin names GitHub while git is sent to the folder: a fetch of api's forks, given the
    /// list of them ([`LocalForks::remotes`]), asks GitHub nothing. Removed when dropped.
    struct LocalForks(crate::dialogs::testing::Temp);

    impl LocalForks {
        fn new(name: &str) -> LocalForks {
            use crate::dialogs::testing::{Temp, git};
            let root = Temp(std::env::temp_dir().join(format!("devassist-gui-{name}-{}", std::process::id())));
            let _ = std::fs::remove_dir_all(&root.0);
            std::fs::create_dir_all(&root.0).unwrap();
            let commit = |dir: &std::path::Path, name: &str| {
                std::fs::write(dir.join(name), name).unwrap();
                git(dir, &["add", name]);
                git(dir, &["commit", "-q", "-m", name]);
            };
            let ada = root.0.join("ada");
            std::fs::create_dir_all(&ada).unwrap();
            git(&ada, &["init", "-q", "-b", "main"]);
            commit(&ada, "one");
            for fork in ["bob", "eve"] {
                git(&root.0, &["clone", "-q", ada.to_str().unwrap(), fork]);
                commit(&root.0.join(fork), &format!("{fork}s"));
            }
            git(&root.0, &["clone", "-q", ada.to_str().unwrap(), "api"]);
            let api = root.0.join("api");
            let github = "https://github.com/ada/api.git";
            git(&api, &["remote", "set-url", "origin", github]);
            git(&api, &["config", &format!("url.{}.insteadOf", ada.display().to_string().replace('\\', "/")), github]);
            // A window another test closed asked its git to stop: that ask is not this test's.
            devassist_core::sys::process::clear_cancel();
            LocalForks(root)
        }

        fn api(&self) -> std::path::PathBuf {
            self.0.0.join("api")
        }

        /// The settings that manage api alone, in the temporary folder.
        fn settings(&self) -> devassist_core::config::Settings {
            devassist_core::config::Settings {
                owners_explicit: true,
                update_check: false,
                update_check_now: false,
                ..devassist_core::config::Settings::for_folder(vec![self.api()], self.0.0.join("devassist.toml"))
            }
        }

        /// The accounts and their repositories on this disk, the original's among them.
        fn remotes(&self) -> Vec<(&'static str, std::path::PathBuf)> {
            ["ada", "bob", "eve"].map(|owner| (owner, self.0.0.join(owner))).into()
        }
    }

    #[test]
    fn the_menu_closes_when_its_row_goes() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        // The list is read again without it.
        window.session.repos_mut().unwrap().installed.retain(|repo| repo.folder != "homelab");
        window.frame(Vec::new());
        assert!(window.repos().mode == CoreMode::List, "the menu stayed for a row that is gone");
        window.frame(Vec::new());
        assert!(!window.open());
        assert_eq!(window.screen_menu(), None);
    }

    #[test]
    fn enter_on_a_highlighted_row_opens_its_menu_at_its_dots() {
        let mut window = demo();
        window.session.repos_mut().unwrap().clear_marks();
        window.session.repos_mut().unwrap().pick("recipes");
        window.key(Key::Enter);
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu));
        assert_eq!(window.repos().chosen(), ["recipes"]);
        let pane = rows(&window)[0].expand(MENU_PAD);
        assert_eq!(pane.top(), dots(&window, "recipes").bottom() + crate::theme::MENU_GAP);
        // The Enter that opened it ran nothing.
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update));
        assert_ne!(window.repos().status, HELD);
        // On a header with nothing ticked, Enter folds. (The first Enter is let go first: a second press of a key
        // egui holds down is a repeat, which folds nothing.)
        window.key(Key::Escape);
        window.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        window.session.repos_mut().unwrap().pick("group::home");
        window.key(Key::Enter);
        assert!(!window.open());
        assert_eq!(window.repos().collapsed, ["home"]);
    }

    /// Runs `choose` on a menu open on the ticked api and web with Remove highlighted, and checks that the remove
    /// dialog came instead, nothing focused, and that the choosing did nothing in it.
    fn remove_comes_instead(choose: impl FnOnce(&mut Window)) {
        let mut window = demo();
        open_on(&mut window, "api");
        window.key(Key::End);
        choose(&mut window);
        assert!(window.shows(Which::Remove), "{}", window.repos().status);
        assert!(window.painted);
        assert_eq!(window.ctx.memory(Memory::focused), None, "the remove dialog opens with nothing focused");
        for _ in 0..3 {
            window.frame(Vec::new());
        }
        assert!(window.shows(Which::Remove));
        assert_ne!(window.repos().status, HELD, "Delete from disk was pressed");
        assert_eq!(window.repos().installed.len(), 12);
        assert_eq!(window.repos().marked, ["api", "web"]);
    }

    #[test]
    fn enter_on_remove_opens_the_remove_dialog_and_deletes_nothing() {
        remove_comes_instead(|window| window.key(Key::Enter));
    }

    #[test]
    fn d_in_the_menu_opens_the_remove_dialog_and_deletes_nothing() {
        remove_comes_instead(|window| window.typed(Key::D, "d"));
    }

    #[test]
    fn a_click_on_remove_opens_the_remove_dialog_and_deletes_nothing() {
        remove_comes_instead(|window| {
            let at = rows(window)[line_of(MenuItem::Remove)].center();
            window.click(at);
        });
    }

    #[test]
    fn move_to_a_group_hands_over_to_the_group_dialog_with_its_field_focused() {
        let mut window = demo();
        open_on(&mut window, "api");
        window.typed(Key::M, "m");
        assert!(window.shows(Which::Group));
        assert_eq!(window.repos().group_prompt.as_ref().map(|prompt| prompt.input.as_str()), Some("Work"), "the m was not typed into it");
    }

    #[test]
    fn the_dots_then_u_update_that_one_checkout_for_real() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("menu-update", &["api", "web"]);
        let mut window = live(settings, 2);
        // The first frame lays the table out; then "…" on api's row, then u.
        window.frame(Vec::new());
        open_on(&mut window, "api");
        window.typed(Key::U, "u");
        until(&mut window, "open", |window| window.shows(Which::Progress));
        assert_eq!(window.job().items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api"]);
        until(&mut window, "ended", |window| !window.job().running());
        assert!(matches!(window.job().result, Some(Ok(_))), "{:?}", window.job().result);
        assert!(root.0.join("api").join("two").exists(), "api was updated");
        assert!(!root.0.join("web").join("two").exists(), "web was not");
        window.session.close();
    }

    #[test]
    fn enter_on_a_row_with_another_ticked_then_u_updates_only_that_row_for_real() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("enter-update", &["api", "web"]);
        let mut window = live(settings, 2);
        // web is ticked, the highlight is on api, which is not: Enter acts as api's "…".
        window.session.repos_mut().unwrap().toggle_mark_of("web");
        window.session.repos_mut().unwrap().pick("api");
        window.frame(Vec::new());
        window.key(Key::Enter);
        window.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(window.shows(Which::Menu));
        assert_eq!(window.repos().marked, Vec::<String>::new(), "web's tick went, as a click on api's \"…\" clears it");
        window.typed(Key::U, "u");
        until(&mut window, "open", |window| window.shows(Which::Progress));
        assert_eq!(window.job().items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api"], "the core was asked for api only");
        until(&mut window, "ended", |window| !window.job().running());
        assert!(root.0.join("api").join("two").exists(), "api was updated");
        assert!(!root.0.join("web").join("two").exists(), "web, ticked before, was not");
        window.session.close();
    }

    #[test]
    fn the_menu_on_a_ticked_pair_stays_for_the_one_left_when_the_other_goes() {
        let mut window = demo();
        open_on(&mut window, "web");
        assert_eq!(window.repos().chosen(), ["api", "web"]);
        // The list is read again without api, the other of the pair.
        let repos = window.session.repos_mut().unwrap();
        repos.installed.retain(|repo| repo.folder != "api");
        repos.marked.retain(|marked| marked != "api");
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu), "the menu went though its row is there");
        assert_eq!(window.screen_menu(), Some("web"));
        assert_eq!(window.repos().chosen(), ["web"], "it acts on what is still there");
        window.typed(Key::U, "u");
        assert_eq!(window.repos().status, HELD);
        assert!(window.repos().mode == CoreMode::List);
    }

    fn key_down(key: Key, repeat: bool) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat, modifiers: Modifiers::NONE }
    }

    fn key_up(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }
    }

    /// `key` pressed, with the text it types, and let go, and a frame after.
    fn tap(window: &mut Window, key: Key, text: Option<&str>) {
        let mut down = vec![key_down(key, false)];
        down.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(down);
        window.frame(vec![key_up(key)]);
        window.frame(Vec::new());
    }

    /// The place of Migrate among the lines shown.
    fn migrate_line() -> usize {
        SHOWN.iter().position(|item| *item == MenuItem::Migrate).unwrap()
    }

    /// Whether the last frame painted `words`.
    fn painted(window: &Window, words: &str) -> bool {
        window.texts.iter().any(|(said, _, _)| said == words)
    }

    /// The side menu's lines in the last frame, with their places.
    fn side_rows(window: &Window) -> Vec<Rect> {
        side_layout(window.submenu_pane().expect("the side menu was drawn"))
    }

    /// The lines the last frame painted on the selection's fill, in the menu and the side menu. (In the light mode the
    /// rules and the row's "…" have the same colour: a line's fill is a line's height, inside a pane.)
    fn lit(window: &Window) -> Vec<Rect> {
        let selection = crate::theme::palette(crate::theme::Mode::Light).selection;
        let panes: Vec<Rect> = window.menu_pane().into_iter().chain(window.submenu_pane()).collect();
        window
            .fills
            .iter()
            .filter(|(rect, fill)| *fill == selection && rect.height() == MENU_ROW && panes.iter().any(|pane| pane.contains_rect(*rect)))
            .map(|(rect, _)| *rect)
            .collect()
    }

    #[test]
    fn migrate_opens_beside_the_menu_and_left_goes_back() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        let lines = rows(&window);
        let migrate = migrate_line();
        // Right on another line opens nothing.
        tap(&mut window, Key::ArrowRight, None);
        assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Update), None));
        tap(&mut window, Key::End, None);
        tap(&mut window, Key::ArrowUp, None);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Migrate));
        // On Migrate, Right opens its side menu: not drawn in the frame of the key, whose bake did not know it, but
        // in the next one, with its glass.
        window.frame(vec![key_down(Key::ArrowRight, false)]);
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder));
        assert!(window.baked_side.is_none() && !painted(&window, MigrateItem::Folder.label()), "the side menu was drawn without its glass");
        assert!(window.repaint, "the next frame was not asked for at once");
        window.frame(vec![key_up(Key::ArrowRight)]);
        assert!(window.baked_side.is_some(), "its glass was not baked");
        assert_eq!(window.baked_side, window.submenu_pane(), "drawn where its glass was baked");
        assert!(MigrateItem::ALL.iter().all(|item| painted(&window, item.label())), "its lines in the core's words");
        // Beside the menu, at its left, level with Migrate's bottom.
        let (side, pane) = (window.submenu_pane().unwrap(), window.menu_pane().unwrap());
        assert_eq!((side.right() + SUBMENU_GAP, side.bottom(), side.width()), (pane.left(), lines[migrate].bottom(), SUBMENU_WIDTH));
        // Migrate keeps the selection's fill, its first line has it, nothing else.
        assert_eq!(lit(&window), [lines[migrate], side_rows(&window)[0]]);
        // Up and Down move in it and stop at its ends, Home and End go there; Right does nothing.
        let steps = [
            (Key::ArrowDown, MigrateItem::Service),
            (Key::ArrowDown, MigrateItem::Service),
            (Key::ArrowUp, MigrateItem::Folder),
            (Key::ArrowUp, MigrateItem::Folder),
            (Key::End, MigrateItem::Service),
            (Key::Home, MigrateItem::Folder),
            (Key::ArrowRight, MigrateItem::Folder),
        ];
        for (key, highlighted) in steps {
            tap(&mut window, key, None);
            assert_eq!(window.repos().migrate_highlight(), Some(highlighted), "after {key:?}");
            assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Migrate), "after {key:?}");
        }
        assert_eq!(lit(&window), [lines[migrate], side_rows(&window)[0]]);
        // The menu's letters do nothing while it is open.
        for (key, letter) in [(Key::U, "u"), (Key::D, "d"), (Key::M, "m"), (Key::T, "t")] {
            tap(&mut window, key, Some(letter));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight() == Some(MigrateItem::Folder), "{letter} acted");
        }
        assert_ne!(window.repos().status, HELD);
        // Left closes it and the menu stays, on Migrate; no frame after shows its pane.
        tap(&mut window, Key::ArrowLeft, None);
        assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Migrate), None));
        assert!(window.shows(Which::Menu));
        assert!(window.baked_side.is_none() && window.submenu_pane().is_none() && !painted(&window, MigrateItem::Folder.label()));
        assert_eq!(lit(&window), [lines[migrate]]);
        // Enter on Migrate opens it again; Esc closes it, and a second Esc the menu.
        tap(&mut window, Key::Enter, None);
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder));
        assert!(window.baked_side.is_some() && painted(&window, MigrateItem::Folder.label()));
        tap(&mut window, Key::Escape, None);
        assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none());
        assert!(!painted(&window, MigrateItem::Folder.label()));
        tap(&mut window, Key::Escape, None);
        assert!(window.repos().mode == CoreMode::List && !window.open());
        assert_ne!(window.repos().status, HELD);
    }

    #[test]
    fn the_pointer_moves_in_the_side_menu_and_back_to_the_menu() {
        let mut window = demo();
        open_at_time(&mut window, "homelab");
        let lines = rows(&window);
        let migrate = migrate_line();
        // A click on Migrate opens its side menu; the pointer resting on Migrate chooses nothing in it.
        click_at_time(&mut window, lines[migrate].center(), 2.0);
        window.frame_at(Vec::new(), Some(2.1));
        window.frame_at(Vec::new(), Some(2.2));
        assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Migrate), Some(MigrateItem::Folder)));
        let side = side_rows(&window);
        // Over its second line, the pointer highlights it, and Migrate keeps its fill.
        window.frame_at(vec![Event::PointerMoved(side[1].center())], Some(2.3));
        window.frame_at(Vec::new(), Some(2.4));
        assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Migrate), Some(MigrateItem::Service)));
        assert_eq!(lit(&window), [lines[migrate], side[1]]);
        // Back over Migrate, it stays open as it is.
        window.frame_at(vec![Event::PointerMoved(lines[migrate].center())], Some(2.5));
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Service));
        // A click on Migrate closes it and keeps the menu; another opens it again.
        click_at_time(&mut window, lines[migrate].center(), 4.0);
        window.frame_at(Vec::new(), Some(4.1));
        assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none());
        click_at_time(&mut window, lines[migrate].center(), 6.0);
        window.frame_at(Vec::new(), Some(6.1));
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder));
        // Over another line of the menu, it closes and that line is highlighted.
        window.frame_at(vec![Event::PointerMoved(lines[2].center())], Some(6.2));
        assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Github), None));
        window.frame_at(Vec::new(), Some(6.3));
        assert!(window.shows(Which::Menu) && window.submenu_pane().is_none() && !painted(&window, MigrateItem::Folder.label()));
        assert_eq!(lit(&window), [lines[2]]);
        // A double click on Migrate opens it, and its second click does not close it again.
        click_at_time(&mut window, lines[migrate].center(), 7.0);
        click_at_time(&mut window, lines[migrate].center(), 7.2);
        window.frame_at(Vec::new(), Some(7.3));
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "the double click's second click closed it");
        // A click outside both closes both, and goes no further.
        let outside = pos2(400.0, 160.0);
        assert!(!window.menu_pane().unwrap().contains(outside) && !window.submenu_pane().unwrap().contains(outside));
        click_at_time(&mut window, outside, 9.0);
        assert!(window.repos().mode == CoreMode::List);
        window.frame_at(Vec::new(), Some(9.2));
        assert!(!window.open());
        assert_eq!(window.repos().selected.as_deref(), Some("homelab"));
        assert_ne!(window.repos().status, HELD);
    }

    #[test]
    fn a_click_on_the_side_menus_empty_room_keeps_both_open() {
        let mut window = demo();
        open_at_time(&mut window, "homelab");
        let migrate = rows(&window)[migrate_line()].center();
        click_at_time(&mut window, migrate, 2.0);
        window.frame_at(Vec::new(), Some(2.1));
        let side = window.submenu_pane().unwrap();
        click_at_time(&mut window, pos2(side.center().x, side.top() + MENU_PAD / 2.0), 4.0);
        window.frame_at(Vec::new(), Some(4.1));
        assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_some(), "the click went through its pane");
    }

    /// How a line of the side menu is chosen.
    #[derive(Clone, Copy, Debug)]
    enum By {
        Enter,
        Letter,
        Click,
        DoubleClick,
    }

    #[test]
    fn a_line_of_the_side_menu_opens_its_dialog_and_that_press_does_nothing_in_it() {
        // What the core opens the move's folder field with, for homelab alone.
        let folder_text = {
            let mut repos = crate::demo::repos();
            repos.clear_marks();
            repos.pick("homelab");
            repos.run_migrate(MigrateItem::Folder);
            repos.prompt.as_ref().unwrap().input.clone()
        };
        for (index, item) in MigrateItem::ALL.into_iter().enumerate() {
            for by in [By::Enter, By::Letter, By::Click, By::DoubleClick] {
                let what = format!("{item:?} by {by:?}");
                let mut window = demo();
                open_at_time(&mut window, "homelab");
                window.frame_at(vec![key_down(Key::T, false), Event::Text("t".to_owned())], Some(1.0));
                window.frame_at(vec![key_up(Key::T)], Some(1.1));
                window.frame_at(Vec::new(), Some(1.2));
                assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "{what}: the side menu did not open");
                let at = side_rows(&window)[index].center();
                let letter = item.key().to_string();
                let code = Key::from_name(&letter.to_uppercase()).unwrap();
                match by {
                    By::Enter => {
                        if index == 1 {
                            window.frame_at(vec![key_down(Key::ArrowDown, false)], Some(1.3));
                            window.frame_at(vec![key_up(Key::ArrowDown)], Some(1.4));
                        }
                        // Pressed, held (its repeats) and let go.
                        window.frame_at(vec![key_down(Key::Enter, false)], Some(2.0));
                        window.frame_at(Vec::new(), Some(2.05));
                        window.frame_at(vec![key_down(Key::Enter, true)], Some(2.1));
                        window.frame_at(vec![key_down(Key::Enter, true)], Some(2.15));
                        window.frame_at(vec![key_up(Key::Enter)], Some(2.2));
                    }
                    By::Letter => {
                        // Typed, held (its repeats type it again) and let go.
                        window.frame_at(vec![key_down(code, false), Event::Text(letter.clone())], Some(2.0));
                        window.frame_at(Vec::new(), Some(2.05));
                        window.frame_at(vec![key_down(code, true), Event::Text(letter.clone())], Some(2.1));
                        window.frame_at(vec![key_down(code, true), Event::Text(letter.clone())], Some(2.15));
                        window.frame_at(vec![key_up(code)], Some(2.2));
                    }
                    By::Click => click_at_time(&mut window, at, 2.0),
                    By::DoubleClick => {
                        // Its second click lands where the line was: on the dialog or its scrim.
                        click_at_time(&mut window, at, 2.0);
                        click_at_time(&mut window, at, 2.2);
                    }
                }
                for tick in [3.0, 3.1, 3.2] {
                    window.frame_at(Vec::new(), Some(tick));
                }
                let dialog = match item {
                    MigrateItem::Folder => Which::Folder,
                    MigrateItem::Service => Which::Push,
                };
                assert!(window.shows(dialog), "{what}: no dialog: {}", window.repos().status);
                assert!(window.painted && window.menu_pane().is_none() && window.submenu_pane().is_none(), "{what}: a menu with the dialog");
                assert!(!painted(&window, "Migrate") && !painted(&window, item.label()), "{what}: the menus' words with the dialog");
                // Nothing was done in it: its field holds what the core opened it with, nothing was refused or run.
                assert_ne!(window.repos().status, HELD, "{what}");
                match item {
                    MigrateItem::Folder => {
                        let prompt = window.repos().prompt.as_ref().unwrap();
                        assert_eq!((prompt.input.as_str(), prompt.error.as_ref()), (folder_text.as_str(), None), "{what}");
                    }
                    MigrateItem::Service => {
                        let prompt = window.repos().service_prompt.as_ref().unwrap();
                        assert_eq!((prompt.repo.as_str(), prompt.input.as_str(), prompt.error.as_ref()), ("homelab", "", None), "{what}");
                    }
                }
            }
        }
    }

    #[test]
    fn no_frame_shows_a_menu_and_the_dialog_a_side_line_opens() {
        for item in MigrateItem::ALL {
            let mut window = demo();
            open_on(&mut window, "homelab");
            tap(&mut window, Key::T, Some("t"));
            let letter = item.key().to_string();
            let code = Key::from_name(&letter.to_uppercase()).unwrap();
            window.frame(vec![key_down(code, false), Event::Text(letter)]);
            // The frame of the letter still shows the menus as they were; every frame after, the dialog alone.
            let mut frames = 0;
            while frames < 4 {
                window.frame(vec![]);
                frames += 1;
                let menus = painted(&window, "Migrate") || painted(&window, item.label()) || window.menu_pane().is_some();
                assert!(!menus, "{item:?}: frame {frames} after the letter shows a menu");
            }
            assert!(window.open() && window.painted);
        }
    }

    #[test]
    fn a_push_with_several_rows_chosen_is_refused_in_the_cores_words() {
        let mut window = demo();
        open_on(&mut window, "api");
        assert_eq!(window.repos().chosen(), ["api", "web"]);
        tap(&mut window, Key::T, Some("t"));
        tap(&mut window, Key::S, Some("s"));
        // The core's refusal of the same push, on the demo as it starts (api and web ticked).
        let mut probe = crate::demo::repos();
        probe.run_migrate(MigrateItem::Service);
        assert!(probe.service_prompt.is_none() && probe.status_error);
        for _ in 0..3 {
            window.frame(Vec::new());
        }
        assert!(window.repos().mode == CoreMode::List && !window.open(), "a dialog opened");
        assert_eq!(window.repos().status, probe.status);
        assert_ne!(window.repos().status, HELD);
        let status = crate::repos_screen::status(&window.session).unwrap();
        assert_eq!((status.text.as_str(), status.error), (probe.status.as_str(), true), "the status line shows the core's words");
        assert_eq!(window.repos().marked, ["api", "web"], "the ticks stay");
    }

    #[test]
    fn a_held_s_whose_push_is_refused_does_not_sort_the_list_after_the_menu_closes() {
        let mut window = demo();
        open_on(&mut window, "api");
        assert_eq!(window.repos().chosen(), ["api", "web"]);
        tap(&mut window, Key::T, Some("t"));
        let by_name = |window: &Window| window.repos().sort == devassist_core::repos::state::SortMode::Name;
        let sort = by_name(&window);
        // `s` on the side menu with two rows ticked: the core refuses the push and the menu closes. Held, `s` repeats
        // with its text: on the screen that would be Sort.
        window.frame(vec![key_down(Key::S, false), Event::Text("s".to_owned())]);
        for _ in 0..6 {
            window.frame(vec![key_down(Key::S, true), Event::Text("s".to_owned())]);
            assert!(window.repos().mode == CoreMode::List && !window.open());
            assert_eq!(by_name(&window), sort, "a repeat of the s that closed the menu sorted the list");
        }
        window.frame(vec![key_up(Key::S)]);
        window.frame(Vec::new());
        assert_eq!(by_name(&window), sort);
        // A fresh `s` sorts.
        tap(&mut window, Key::S, Some("s"));
        assert_ne!(by_name(&window), sort, "a fresh s did not sort");
    }

    /// The menu on homelab with its side menu open by its letter, drawn.
    fn side_open_on_homelab() -> Window {
        let mut window = demo();
        open_on(&mut window, "homelab");
        tap(&mut window, Key::T, Some("t"));
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "the side menu did not open");
        assert!(window.submenu_pane().is_some());
        window
    }

    /// The strip between the menu's pane and the side menu's, over the side menu's height, in the last frame.
    fn gap_strip(window: &Window) -> Rect {
        let (pane, side) = (window.menu_pane().unwrap(), window.submenu_pane().unwrap());
        let (left, right) = if side.right() <= pane.left() { (side.right(), pane.left()) } else { (pane.right(), side.left()) };
        Rect::from_min_max(pos2(left, side.top()), pos2(right, side.bottom()))
    }

    #[test]
    fn a_held_esc_closes_the_side_menu_and_the_menu_stays() {
        // Esc, q and Left, each held over several frames (its repeats, and for q its text again), then let go.
        for (key, text) in [(Key::Escape, None), (Key::Q, Some("q")), (Key::ArrowLeft, None)] {
            let mut window = side_open_on_homelab();
            let pressed = |repeat| {
                let mut events = vec![key_down(key, repeat)];
                events.extend(text.map(|text: &str| Event::Text(text.to_owned())));
                events
            };
            window.frame(pressed(false));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "{key:?} did not close the side menu");
            for _ in 0..4 {
                window.frame(pressed(true));
                window.frame(Vec::new());
                assert!(window.shows(Which::Menu), "{key:?} held closed the menu");
                assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Migrate), None), "{key:?} held");
            }
            window.frame(vec![key_up(key)]);
            window.frame(Vec::new());
            assert!(window.shows(Which::Menu), "{key:?}: its release closed the menu");
            assert_ne!(window.repos().status, HELD);
            // A fresh Esc closes it.
            tap(&mut window, Key::Escape, None);
            assert!(window.repos().mode == CoreMode::List, "{key:?}: a fresh Esc did nothing");
        }
    }

    #[test]
    fn a_key_held_as_the_side_menu_closed_is_forgotten_by_a_menu_opened_later() {
        // The side menu closed by Esc or q held down, and the menu closed by a click outside while it is still down;
        // let go with no menu open. A menu opened later takes a fresh press of that key in the first frame it is
        // drawn.
        for (key, text) in [(Key::Escape, None), (Key::Q, Some("q"))] {
            let press = |repeat| {
                let mut events = vec![key_down(key, repeat)];
                events.extend(text.map(|text: &str| Event::Text(text.to_owned())));
                events
            };
            let mut window = side_open_on_homelab();
            window.frame(press(false));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "{key:?} did not close the side menu");
            window.click(pos2(400.0, 160.0));
            assert!(window.repos().mode == CoreMode::List, "{key:?}: the click outside left the menu open");
            window.frame(vec![key_up(key)]);
            window.frame(Vec::new());
            assert!(!window.open());
            // A new menu, by Enter on homelab: its first frame keeps it open with its shield and paints nothing; the
            // next draws it, and a fresh press of the key there closes it.
            window.session.repos_mut().unwrap().enter_row();
            window.frame(Vec::new());
            assert!(window.shows(Which::Menu) && !window.painted, "{key:?}: the menu was drawn in the frame it opened");
            window.frame(press(false));
            assert!(window.repos().mode == CoreMode::List, "{key:?}: a fresh press in a new menu did nothing");
        }
        // In one menu: the side menu closed by a held Esc, opened again by a click on Migrate while Esc is still down;
        // Esc let go while it is open; closed by the pointer moving over another line of the menu. A fresh Esc in the
        // first frame after that closes the menu.
        let mut window = demo();
        open_at_time(&mut window, "homelab");
        let lines = rows(&window);
        let migrate = lines[migrate_line()].center();
        click_at_time(&mut window, migrate, 2.0);
        window.frame_at(Vec::new(), Some(2.1));
        assert!(window.repos().migrate_highlight().is_some(), "the side menu did not open");
        window.frame_at(vec![key_down(Key::Escape, false)], Some(3.0));
        window.frame_at(vec![key_down(Key::Escape, true)], Some(3.1));
        assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "Esc did not close the side menu");
        click_at_time(&mut window, migrate, 4.0);
        window.frame_at(Vec::new(), Some(4.1));
        assert!(window.repos().migrate_highlight().is_some(), "the side menu did not open again");
        window.frame_at(vec![key_up(Key::Escape)], Some(4.2));
        window.frame_at(vec![Event::PointerMoved(lines[1].center())], Some(4.3));
        assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "the pointer did not close the side menu");
        window.frame_at(vec![key_down(Key::Escape, false)], Some(4.4));
        assert!(window.repos().mode == CoreMode::List, "a fresh Esc left the menu open");
        assert_ne!(window.repos().status, HELD);
    }

    #[test]
    fn a_key_held_as_the_pointer_closes_the_side_menu_does_nothing_in_the_menu() {
        // Esc and `t` held while the pointer moves from the side menu onto another line of the menu, which closes the
        // side menu: their repeats, and `t`'s text, are not the menu's while they stay down. Esc is held from before the
        // side menu opened (by a click on Migrate), so the side menu itself took none of its presses.
        for (key, text) in [(Key::Escape, None), (Key::T, Some("t"))] {
            let pressed = |repeat| {
                let mut events = vec![key_down(key, repeat)];
                events.extend(text.map(|text: &str| Event::Text(text.to_owned())));
                events
            };
            let mut window = demo();
            open_at_time(&mut window, "homelab");
            let lines = rows(&window);
            let migrate = lines[migrate_line()].center();
            if key == Key::Escape {
                // Opened, then closed by Esc held down, then opened again by a click while Esc stays down.
                click_at_time(&mut window, migrate, 2.0);
                window.frame_at(Vec::new(), Some(2.1));
                window.frame_at(pressed(false), Some(2.5));
                assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "Esc did not close the side menu");
                window.frame_at(pressed(true), Some(2.6));
                click_at_time(&mut window, migrate, 4.0);
            } else {
                // Opened by `t`, which stays down.
                window.frame_at(pressed(false), Some(4.0));
            }
            window.frame_at(pressed(true), Some(4.1));
            window.frame_at(pressed(true), Some(4.15));
            assert!(window.repos().migrate_highlight().is_some(), "{key:?}: the side menu is not open");
            // The pointer onto another line closes it; the key is still down.
            window.frame_at(vec![Event::PointerMoved(lines[1].center())], Some(4.2));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "{key:?}: the pointer did not close the side menu");
            for (index, at) in [4.3, 4.35, 4.4, 4.45].into_iter().enumerate() {
                window.frame_at(pressed(true), Some(at));
                assert!(window.shows(Which::Menu), "{key:?} held closed the menu ({index})");
                assert!(window.repos().migrate_highlight().is_none(), "{key:?} held opened the side menu again ({index})");
            }
            window.frame_at(vec![key_up(key)], Some(4.5));
            window.frame_at(Vec::new(), Some(4.6));
            assert!(window.shows(Which::Menu), "{key:?}: its release closed the menu");
            assert_ne!(window.repos().status, HELD);
        }
    }

    #[test]
    fn a_click_in_the_gap_between_the_panes_does_nothing() {
        // The menu under homelab's "…" at the right of the table (the side menu at its left), and under a button at
        // the window's left (the side menu flipped to its right).
        for anchor in [None, Some(Rect::from_min_size(pos2(30.0, 300.0), vec2(32.0, 32.0)))] {
            let mut window = demo();
            window.anchor = anchor;
            let repos = window.session.repos_mut().unwrap();
            repos.clear_marks();
            repos.pick("homelab");
            repos.enter_row();
            window.frame_at(Vec::new(), Some(0.5));
            window.frame_at(Vec::new(), Some(0.6));
            assert!(window.shows(Which::Menu));
            let migrate = rows(&window)[migrate_line()].center();
            click_at_time(&mut window, migrate, 2.0);
            window.frame_at(Vec::new(), Some(2.1));
            let (pane, side) = (window.menu_pane().unwrap(), window.submenu_pane().unwrap());
            assert_eq!(side.left() > pane.right(), anchor.is_some(), "the side menu is not where this placement puts it");
            let gap = gap_strip(&window);
            assert!((gap.width() - SUBMENU_GAP).abs() < 1e-3 && gap.height() > 0.0, "{anchor:?}: {gap:?}");
            click_at_time(&mut window, gap.center(), 4.0);
            window.frame_at(Vec::new(), Some(4.1));
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_some(), "{anchor:?}: a click in the gap closed them");
            assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Migrate), "{anchor:?}");
            // Just above the strip, outside both: both close.
            click_at_time(&mut window, pos2(gap.center().x, gap.top() - 3.0), 6.0);
            assert!(window.repos().mode == CoreMode::List, "{anchor:?}: a click outside both left them open");
        }
    }

    #[test]
    fn the_pointer_reaches_the_side_menu_through_the_gap_in_both_placements() {
        // The menu under homelab's "…" at the right of the table (the side menu at its left), and under a button at
        // the window's left (the side menu flipped to its right).
        for anchor in [None, Some(Rect::from_min_size(pos2(30.0, 300.0), vec2(32.0, 32.0)))] {
            let mut window = demo();
            window.anchor = anchor;
            let repos = window.session.repos_mut().unwrap();
            repos.clear_marks();
            repos.pick("homelab");
            repos.enter_row();
            window.frame(Vec::new());
            window.frame(Vec::new());
            tap(&mut window, Key::T, Some("t"));
            let (pane, side) = (window.menu_pane().unwrap(), window.submenu_pane().unwrap());
            assert_eq!(side.left() > pane.right(), anchor.is_some(), "the side menu is not where this placement puts it");
            let lines = rows(&window);
            let (migrate, rule) = (lines[migrate_line()], lines[migrate_line() - 1].bottom() + MENU_LINE / 2.0);
            let push = side_rows(&window)[1];
            let gap = gap_strip(&window);
            // Over Migrate, over the rule above it and the pane's padding, through the gap, onto Push.
            let path = [
                migrate.center(),
                pos2(migrate.center().x, rule),
                pos2(pane.left() + MENU_PAD / 2.0, migrate.center().y),
                pos2(pane.right() - MENU_PAD / 2.0, migrate.center().y),
                gap.center(),
                push.center(),
            ];
            for at in path {
                window.frame(vec![Event::PointerMoved(at)]);
                assert!(window.repos().migrate_highlight().is_some(), "{anchor:?}: the side menu closed at {at:?}");
                assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Migrate), "{anchor:?} at {at:?}");
            }
            assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Service), "{anchor:?}: Push is not highlighted");
            // Onto another line of the menu, it closes.
            window.frame(vec![Event::PointerMoved(lines[1].center())]);
            assert_eq!((window.repos().menu_highlight(), window.repos().migrate_highlight()), (Some(MenuItem::Open), None), "{anchor:?}");
        }
    }

    #[test]
    fn the_menu_and_its_side_menu_close_together_when_the_row_goes() {
        let mut window = side_open_on_homelab();
        window.session.repos_mut().unwrap().installed.retain(|repo| repo.folder != "homelab");
        window.frame(Vec::new());
        assert!(window.repos().mode == CoreMode::List, "the menu stayed for a row that is gone");
        assert!(!window.painted, "a pane was drawn in the frame they closed");
        assert!(!painted(&window, "Migrate") && !painted(&window, MigrateItem::Folder.label()));
        window.frame(Vec::new());
        assert!(!window.open() && window.submenu_pane().is_none());
        assert_eq!(window.screen_menu(), None);
    }

    #[test]
    fn the_core_closing_the_side_menu_after_the_bake_draws_no_side_menu() {
        let mut window = side_open_on_homelab();
        window.between = Some(Box::new(|session: &mut Session| session.repos_mut().unwrap().close_migrate()));
        window.frame(Vec::new());
        assert!(window.baked_side.is_some(), "the bake placed it");
        assert!(painted(&window, "Migrate"), "the menu is drawn");
        assert!(MigrateItem::ALL.iter().all(|item| !painted(&window, item.label())), "the side menu was drawn after the core closed it");
        window.frame(Vec::new());
        assert!(window.baked_side.is_none() && window.shows(Which::Menu));
    }

    #[test]
    fn a_held_key_that_opens_the_side_menu_runs_none_of_its_lines() {
        for (key, text) in [(Key::Enter, None), (Key::ArrowRight, None), (Key::T, Some("t"))] {
            let mut window = demo();
            open_on(&mut window, "homelab");
            tap(&mut window, Key::End, None);
            tap(&mut window, Key::ArrowUp, None);
            let pressed = |repeat| {
                let mut events = vec![key_down(key, repeat)];
                events.extend(text.map(|text: &str| Event::Text(text.to_owned())));
                events
            };
            window.frame(pressed(false));
            for _ in 0..5 {
                window.frame(pressed(true));
                window.frame(Vec::new());
            }
            window.frame(vec![key_up(key)]);
            window.frame(Vec::new());
            assert!(window.shows(Which::Menu), "{key:?} held ran a line");
            assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "{key:?} held");
            assert!(window.repos().prompt.is_none() && window.repos().service_prompt.is_none(), "{key:?} held");
            assert_ne!(window.repos().status, HELD);
        }
    }

    /// The button of the dialog the side menu's `item` opens for `key` alone, in a window of `size`, that the second
    /// click of a double click on that line must not press: the Move dialog's main button, and the Push dialog's
    /// Cancel (its "Push" is off while the address is empty, so a press of it would prove nothing; Cancel always
    /// works, and a stray press of it closes the dialog). Its place and its words.
    fn guarded_button(item: MigrateItem, key: &str, size: [f32; 2]) -> (Rect, String) {
        let mut probe = Window::new(Session::demo(crate::demo::repos()));
        probe.size = size;
        let repos = probe.session.repos_mut().unwrap();
        repos.clear_marks();
        repos.pick(key);
        repos.run_migrate(item);
        probe.frame(Vec::new());
        probe.frame(Vec::new());
        let buttons = probe.buttons();
        let guarded = match item {
            MigrateItem::Folder => buttons.last(),
            MigrateItem::Service => buttons.first(),
        };
        guarded.cloned().unwrap()
    }

    /// A window size the app allows (from 960 by 600) and a row alone where the side menu's line `item`, its menu
    /// opened on that row, lies over the guarded button of the dialog that line opens ([`guarded_button`]), with the
    /// point of the overlap and the button's words.
    fn side_overlap(item: MigrateItem) -> Option<([f32; 2], &'static str, Pos2, String)> {
        let index = MigrateItem::ALL.iter().position(|known| *known == item).unwrap();
        let keys =
            ["dotfiles", "homelab", "recipes", "DevAssist", "release-scripts", "customer-portal-i18n", "portal", "notes", "old-blog", "sandbox"];
        let lines = crate::repos_view::lines(&crate::demo::repos());
        let menu_width = demo_width();
        for width in (960_u16..=1600).step_by(40) {
            for height in (600_u16..=1100).step_by(50) {
                let size = [f32::from(width), f32::from(height)];
                let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
                let panel = crate::shell::layout(screen.size(), true, true).panel;
                // A row's button wholly in view (as tall as the first row's): a row cut at the panel's foot is
                // scrolled into view when it is pressed.
                let whole = table::dots_rect(panel, &lines, 0.0, keys[0]).map_or(0.0, |dots| dots.height());
                for key in keys {
                    let Some(anchor) = table::dots_rect(panel, &lines, 0.0, key).filter(|dots| (dots.height() - whole).abs() < 1e-3) else {
                        continue;
                    };
                    let line = side_layout(place(screen, anchor, true, menu_width).side.unwrap())[index];
                    // The dialog that line opens for that row, as it stands then.
                    let (button, words) = guarded_button(item, key, size);
                    let both = line.intersect(button);
                    if both.width() > 4.0 && both.height() > 4.0 {
                        return Some((size, key, both.center(), words));
                    }
                }
            }
        }
        None
    }

    #[test]
    fn the_second_click_of_a_double_click_on_a_side_line_presses_no_button_of_its_dialog() {
        for item in MigrateItem::ALL {
            let Some((size, key, at, words)) = side_overlap(item) else {
                panic!("{item:?}: no window size puts its line over a button of its dialog");
            };
            eprintln!("{item:?}: {size:?} on {key} at {at:?}, over {words}");
            let mut window = Window::new(Session::demo(crate::demo::repos()));
            window.size = size;
            window.frame(Vec::new());
            open_at_time(&mut window, key);
            window.frame_at(vec![key_down(Key::T, false), Event::Text("t".to_owned())], Some(1.0));
            window.frame_at(vec![key_up(Key::T)], Some(1.1));
            window.frame_at(Vec::new(), Some(1.2));
            let index = MigrateItem::ALL.iter().position(|known| *known == item).unwrap();
            assert!(side_rows(&window)[index].contains(at), "{item:?}: the point is not on its line");
            let dialog = if item == MigrateItem::Folder { Which::Folder } else { Which::Push };
            double_click(&mut window, at, |window| {
                window.frame_at(Vec::new(), Some(5.1));
                assert!(window.shows(dialog), "{item:?}: no dialog");
                assert!(window.buttons().iter().any(|(place, said)| *said == words && place.contains(at)), "{item:?}: the point is not on {words}");
            });
            assert!(window.shows(dialog), "{item:?}: the second click pressed {words}");
            assert_ne!(window.repos().status, HELD, "{item:?}: the second click pressed {words}");
            let refused = match item {
                MigrateItem::Folder => window.repos().prompt.as_ref().map(|prompt| prompt.error.is_some()),
                MigrateItem::Service => window.repos().service_prompt.as_ref().map(|prompt| prompt.error.is_some()),
            };
            assert_eq!(refused, Some(false), "{item:?}: the second click pressed {words}");
            if item == MigrateItem::Service {
                // The point is live: a click of its own there, later, is Cancel's, and closes the dialog.
                click_at_time(&mut window, at, 9.0);
                window.frame_at(Vec::new(), Some(9.1));
                assert!(!window.open() && window.repos().mode == CoreMode::List, "{item:?}: a click of its own on {words} did nothing");
            }
        }
    }
}
