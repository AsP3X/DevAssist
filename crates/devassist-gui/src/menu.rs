//! The row's menu: what can be done to the chosen repositories (the ticked ones, else the highlighted one), opened
//! by a row's "…" or by Enter on a row (`repos_screen`), drawn on the overlay ([`crate::overlay`]) while the core's
//! mode is `Mode::Menu`, through the same bridge as the dialogs (`dialogs::Which::Menu`).
//!
//! It shows the core's lines in the core's words and with the core's keys (`MenuItem::label`, `MenuItem::key`), as
//! far as the window can do them ([`shown`]): the forks' three lines and Migrate wait for their screens. A menu has
//! a shield but no scrim; a click outside it or Esc closes it, and the click goes no further. It is not walked with
//! Tab: as in the terminal app, Up and Down (and `k` and `j`) move the highlight and stop at the ends, Home and End
//! go to them, Enter runs the highlighted line, a line's letter runs that line, and `q` closes. A letter of a line
//! the window does not show does nothing. The pointer, when it moves over a line, highlights it, so the mouse and
//! the keys never show two highlights; a pointer that only rests where the menu opens chooses nothing. A line that
//! opens a dialog (Update, Move to a group, Remove) hands the overlay over to it in the next frame, and the key or
//! click that chose the line does nothing in it (`overlay::open`, `overlay::fresh_click`).

use devassist_core::repos::state::{MenuItem, Repos};
use egui::{Align2, Event, FontFamily, Id, Key, Rect, Sense, Ui, Vec2, pos2, vec2};

use crate::assets::Icon;
use crate::overlay;
use crate::theme::{
    HAIRLINE, ICON, KEY_TEXT, MENU_KEY_WIDTH, MENU_LINE, MENU_LINE_OPACITY, MENU_PAD, MENU_ROW, MENU_ROW_GAP, MENU_ROW_PAD, MENU_ROW_RADIUS,
    MENU_WIDTH, Palette, TEXT,
};
use crate::widgets::{self, draw_icon, font};

/// The lines the window shows, in the core's order.
const SHOWN: [MenuItem; 8] = [
    MenuItem::Update,
    MenuItem::Open,
    MenuItem::Github,
    MenuItem::Group,
    MenuItem::CopyPath,
    MenuItem::Watch,
    MenuItem::PullRequests,
    MenuItem::Remove,
];

/// The drawing's icon of each line of [`SHOWN`], in its order.
const ICONS: [Icon; SHOWN.len()] =
    [Icon::ArrowDownToLine, Icon::FolderOpen, Icon::ExternalLink, Icon::Group, Icon::Copy, Icon::Eye, Icon::GitPullRequest, Icon::Trash2];

/// The lines the window's menu shows, in order: what the window can do so far. The core's other lines (the forks'
/// three and Migrate) wait for the screens they open.
#[must_use]
pub fn shown() -> &'static [MenuItem] {
    &SHOWN
}

/// Whether a line stands between `item` and the one before it: Remove is set apart, as the drawing sets it.
fn rule_before(item: MenuItem) -> bool {
    item == MenuItem::Remove
}

/// How tall the menu is: its lines, the rule before Remove, and the room above and below them.
fn height() -> f32 {
    let (rows, rules) = SHOWN.iter().fold((0.0, 0.0), |(rows, rules), item| (rows + 1.0, rules + if rule_before(*item) { 1.0 } else { 0.0 }));
    2.0 * MENU_PAD + rows * MENU_ROW + rules * MENU_LINE
}

/// Where the menu stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Places {
    /// Its pane.
    pub pane: Rect,
}

/// Where the menu stands in `window` for the button at `anchor`: under it, its right edge on the button's
/// (`overlay::menu_rect`).
#[must_use]
pub fn plan(window: Rect, anchor: Rect) -> Places {
    Places { pane: overlay::menu_rect(anchor, vec2(MENU_WIDTH, height()), window) }
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

/// The places of the menu's lines in its pane at `pane`, in the order of [`shown`]: for the tests that press them.
#[cfg(test)]
pub(crate) fn line_rects(pane: Rect) -> Vec<Rect> {
    layout(pane).0
}

/// What a key asks of the open menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Close it.
    Close,
    /// Move the highlight one line up (`true`) or down.
    Step(bool),
    /// Highlight the first line (`true`) or the last.
    End(bool),
    /// Run the highlighted line.
    Highlighted,
    /// Run this line.
    Run(MenuItem),
}

/// What the keys among `events` ask of the menu, in order, as the terminal app's menu reads them. Letters are read
/// from the typed text, so they follow the keyboard's layout; a letter of a line the window does not show is none.
fn asks(events: &[Event]) -> Vec<Ask> {
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
                _ => None,
            }),
            Event::Text(text) => {
                for typed in text.chars().map(|typed| typed.to_ascii_lowercase()) {
                    asks.extend(match typed {
                        'q' => Some(Ask::Close),
                        'k' => Some(Ask::Step(true)),
                        'j' => Some(Ask::Step(false)),
                        _ => SHOWN.iter().find(|item| item.key() == typed).map(|item| Ask::Run(*item)),
                    });
                }
            }
            _ => {}
        }
    }
    asks
}

/// The highlighted line after `ask`, from the line at `at`: one up or down, stopping at the ends; the first or the
/// last. From no line, a step goes to the first.
fn moved(at: Option<usize>, ask: Ask) -> Option<usize> {
    let last = SHOWN.len() - 1;
    Some(match (ask, at) {
        (Ask::Step(true), Some(at)) => at.saturating_sub(1),
        (Ask::Step(false), Some(at)) => (at + 1).min(last),
        (Ask::Step(_) | Ask::End(true), _) => 0,
        (Ask::End(false), _) => last,
        (Ask::Close | Ask::Highlighted | Ask::Run(_), _) => return at,
    })
}

/// The line of [`SHOWN`] the core highlights; none when it highlights one the window does not show, and then
/// Enter runs nothing.
fn highlighted(repos: &Repos) -> Option<usize> {
    repos.menu_highlight().and_then(|item| SHOWN.iter().position(|shown| *shown == item))
}

/// The id of the menu's line `index`.
fn row_id(index: usize) -> Id {
    Id::unique(("menu-row", index))
}

/// The menu at `places`, for `repos`'s chosen repositories, on the overlay in `ui`: its lines, the highlighted one
/// on the selection's fill, Remove in the danger colour, each line's key at its right. `outside` is whether the
/// shield was clicked this frame (outside the pane). The highlight follows the pointer only in a frame the pointer
/// moves, so a highlight moved by a key stays while the pointer rests, and a pointer resting where the menu opens
/// does not choose its line. What a person does goes to `repos` through its methods.
pub fn show(ui: &mut Ui, places: Places, repos: &mut Repos, palette: Palette, outside: bool) {
    // No Tab here: the lines take no focus, so Enter and Space press none of them; the arrows move the highlight.
    overlay::walk(ui, &[]);
    let (rows, rules) = layout(places.pane);
    let mut at = highlighted(repos);
    let mut chosen = None;
    let moving = ui.input(|input| input.pointer.delta() != Vec2::ZERO);
    for (index, row) in rows.iter().enumerate() {
        let response = ui.interact(row.intersect(places.pane), row_id(index), Sense::CLICK);
        if moving && response.hovered() {
            at = Some(index);
        }
        if overlay::fresh_click(ui, &response) {
            chosen = Some(Ask::Run(SHOWN[index]));
        }
    }
    let mut done = chosen.or(outside.then_some(Ask::Close));
    for ask in asks(&ui.input(|input| input.events.clone())) {
        if done.is_some() {
            break;
        }
        match ask {
            Ask::Close | Ask::Run(_) => done = Some(ask),
            Ask::Highlighted => done = at.map(|at| Ask::Run(SHOWN[at])),
            Ask::Step(_) | Ask::End(_) => at = moved(at, ask),
        }
    }
    if let Some(at) = at {
        repos.highlight_menu(SHOWN[at]);
    }

    let many = repos.chosen().len() > 1;
    let painter = ui.painter().with_clip_rect(places.pane.intersect(ui.clip_rect()));
    for rule in rules {
        painter.rect_filled(rule, 0.0, palette.on_glass.gamma_multiply(MENU_LINE_OPACITY));
    }
    let words = font(TEXT, FontFamily::Proportional);
    let small = font(KEY_TEXT, FontFamily::Proportional);
    for (index, (row, (item, icon))) in rows.iter().zip(SHOWN.into_iter().zip(ICONS)).enumerate() {
        if Some(index) == at {
            painter.rect_filled(*row, MENU_ROW_RADIUS, palette.selection);
        }
        let (ink, icon_ink) = if item == MenuItem::Remove { (palette.danger, palette.danger) } else { (palette.on_glass, palette.quiet) };
        let y = row.center().y;
        draw_icon(&painter, pos2(row.left() + MENU_ROW_PAD + ICON / 2.0, y), icon, ICON, icon_ink);
        let left = row.left() + MENU_ROW_PAD + ICON + MENU_ROW_GAP;
        let room = row.right() - MENU_ROW_PAD - MENU_KEY_WIDTH - MENU_ROW_GAP - left;
        painter.text(pos2(left, y), Align2::LEFT_CENTER, widgets::ellipsis(item.label(many), &words, room, ui), words.clone(), ink);
        painter.text(pos2(row.right() - MENU_ROW_PAD, y), Align2::RIGHT_CENTER, key_name(item), small.clone(), palette.quiet);
    }

    match done {
        Some(Ask::Run(item)) => repos.run_menu(item),
        Some(Ask::Close) => repos.close_menu(),
        _ => {}
    }
}

/// The name of `item`'s key at the end of its line: the core's key, as a capital, as the drawing writes it.
fn key_name(item: MenuItem) -> String {
    item.key().to_ascii_uppercase().to_string()
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

    /// Every line of the core's menu, and whether the window shows it. No wildcard: a new line of the core's must
    /// be placed here on purpose.
    fn placed(item: MenuItem) -> bool {
        match item {
            MenuItem::Update
            | MenuItem::Open
            | MenuItem::Github
            | MenuItem::Group
            | MenuItem::CopyPath
            | MenuItem::Watch
            | MenuItem::PullRequests
            | MenuItem::Remove => true,
            MenuItem::ShowForks | MenuItem::Forks | MenuItem::AllForks | MenuItem::Migrate => false,
        }
    }

    #[test]
    fn the_menu_lists_only_what_this_plan_can_do() {
        let all: Vec<MenuItem> = MenuItem::ALL.into_iter().filter(|item| placed(*item)).collect();
        assert_eq!(shown().to_vec(), all, "the shown lines, in the core's order");
        assert_eq!(
            shown(),
            [
                MenuItem::Update,
                MenuItem::Open,
                MenuItem::Github,
                MenuItem::Group,
                MenuItem::CopyPath,
                MenuItem::Watch,
                MenuItem::PullRequests,
                MenuItem::Remove
            ]
        );
        // The core's words and keys, one of them in the plural.
        let words: Vec<(&str, String)> = shown().iter().copied().map(|item| (item.label(false), key_name(item))).collect();
        assert_eq!(
            words,
            [
                ("Update", "U"),
                ("Open the folder", "O"),
                ("Open on GitHub", "G"),
                ("Move to a group…", "M"),
                ("Copy the path", "C"),
                ("Auto-pull on or off", "W"),
                ("Fetch the pull requests", "P"),
                ("Remove…", "D"),
            ]
            .map(|(label, key)| (label, key.to_owned()))
        );
        assert_eq!((MenuItem::Open.label(true), MenuItem::CopyPath.label(true)), ("Open the folders", "Copy the paths"));
    }

    fn press(key: Key) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }
    }

    #[test]
    fn the_keys_are_read_as_the_terminal_apps_menu_reads_them() {
        let read = |events: Vec<Event>| asks(&events);
        assert_eq!(
            read(vec![press(Key::ArrowUp), press(Key::ArrowDown), press(Key::Home), press(Key::End)]),
            [Ask::Step(true), Ask::Step(false), Ask::End(true), Ask::End(false)]
        );
        assert_eq!(read(vec![press(Key::Enter), press(Key::Escape)]), [Ask::Highlighted, Ask::Close]);
        assert_eq!(read(vec![Event::Text("kjq".to_owned())]), [Ask::Step(true), Ask::Step(false), Ask::Close]);
        assert_eq!(read(vec![Event::Text("u".to_owned())]), [Ask::Run(MenuItem::Update)]);
        assert_eq!(read(vec![Event::Text("D".to_owned())]), [Ask::Run(MenuItem::Remove)], "as the terminal app, a capital too");
        // The forks' and Migrate's letters do nothing here.
        assert_eq!(read(vec![Event::Text("fnat".to_owned())]), []);
        // A key with Ctrl is no key of the menu's.
        assert_eq!(read(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]), []);
    }

    #[test]
    fn the_highlight_stops_at_the_ends_as_in_the_terminal_app() {
        assert_eq!(moved(Some(0), Ask::Step(true)), Some(0));
        assert_eq!(moved(Some(0), Ask::Step(false)), Some(1));
        assert_eq!(moved(Some(7), Ask::Step(false)), Some(7));
        assert_eq!(moved(Some(3), Ask::End(true)), Some(0));
        assert_eq!(moved(Some(3), Ask::End(false)), Some(7));
        // From no line shown, a step goes to the first; Enter and the rest change nothing.
        assert_eq!(moved(None, Ask::Step(true)), Some(0));
        assert_eq!(moved(None, Ask::Step(false)), Some(0));
        assert_eq!(moved(None, Ask::End(false)), Some(7));
        assert_eq!(moved(None, Ask::Highlighted), None);
    }

    #[test]
    fn the_menu_is_the_drawings() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0));
        // Seven lines, the rule, Remove, and the pane's room: 8 + 7 × 32 + 9 + 32.
        assert!((height() - 273.0).abs() < f32::EPSILON);
        let anchor = Rect::from_min_max(pos2(1388.0, 228.0), pos2(1420.0, 272.0));
        let places = plan(window, anchor);
        assert_eq!(places.pane, Rect::from_min_size(pos2(1164.0, 276.0), vec2(256.0, 273.0)));
        let (rows, rules) = layout(places.pane);
        assert_eq!(rows.len(), 8);
        assert_eq!(rows[0], Rect::from_min_size(pos2(1168.0, 280.0), vec2(248.0, 32.0)));
        for pair in rows[..7].windows(2) {
            assert_eq!(pair[0].bottom(), pair[1].top());
        }
        // The rule in the middle of its nine points, between Fetch the pull requests and Remove.
        assert_eq!(rules.len(), 1);
        assert_eq!(rows[7].top() - rows[6].bottom(), MENU_LINE);
        assert!((rules[0].center().y - (rows[6].bottom() + MENU_LINE / 2.0)).abs() < f32::EPSILON);
        assert_eq!((rules[0].left(), rules[0].right(), rules[0].height()), (1176.0, 1408.0, 1.0));
        assert_eq!(rows[7].bottom() + MENU_PAD, places.pane.bottom());
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
        // A letter of a line the window does not show does nothing; Esc closes.
        open_on(&mut window, "homelab");
        window.typed(Key::F, "f");
        window.typed(Key::T, "t");
        assert!(window.shows(Which::Menu));
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
    fn button_offsets(probe: &Window) -> Vec<(Rect, &'static str)> {
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
        buttons: &[(Rect, &'static str)],
        widths: &[u16],
        heights: &[u16],
    ) -> ([f32; 2], String, Pos2, &'static str) {
        let lines = crate::repos_view::lines(repos);
        for &width in widths {
            for &height in heights {
                let size = vec2(f32::from(width), f32::from(height));
                let screen = Rect::from_min_size(Pos2::ZERO, size);
                let panel = crate::shell::layout(size, true, true).panel;
                for key in keys {
                    let Some(anchor) = table::dots_rect(panel, &lines, 0.0, key) else { continue };
                    let row = layout(plan(screen, anchor).pane).0[line];
                    for (button, words) in buttons {
                        let both = row.intersect(button.translate(screen.center().to_vec2()));
                        if both.width() > 4.0 && both.height() > 4.0 {
                            return ([size.x, size.y], (*key).to_owned(), both.center(), words);
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
            7,
            &buttons,
            &(520..=760).step_by(20).collect::<Vec<_>>(),
            &(800..=1600).step_by(25).collect::<Vec<_>>(),
        );
        let mut window = Window::new(Session::demo(crate::demo::repos()));
        window.size = size;
        window.frame(Vec::new());
        open_at_time(&mut window, &key);
        assert!(rows(&window)[7].contains(at), "the point is on Remove…");
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
        let remove = layout(plan(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0)), dots(&window, "recipes")).pane).0[7].center();
        window.frame(vec![Event::PointerMoved(remove)]);
        window.frame(Vec::new());
        window.key(Key::Enter);
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu));
        assert_eq!(rows(&window)[7].center(), remove, "the pointer is over Remove");
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update), "the resting pointer moved the highlight");
        // The Enter that opened it comes up, and a second one runs the highlighted line.
        window.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        window.key(Key::Enter);
        assert!(window.repos().mode == CoreMode::List, "Enter ran Remove, not Update");
        assert_eq!(window.repos().status, HELD, "Enter did not run Update");
    }

    #[test]
    fn enter_runs_nothing_while_the_core_highlights_a_line_the_window_does_not_show() {
        let mut window = demo();
        open_on(&mut window, "homelab");
        window.session.repos_mut().unwrap().highlight_menu(MenuItem::Migrate);
        window.frame(Vec::new());
        window.key(Key::Enter);
        assert!(window.shows(Which::Menu), "Enter ran a line nobody highlighted");
        assert_ne!(window.repos().status, HELD);
        // The arrows bring the highlight back onto the lines shown.
        window.key(Key::ArrowDown);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update));
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
            let at = rows(window)[7].center();
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
}
