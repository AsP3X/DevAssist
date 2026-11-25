//! The row's menu: what can be done to the chosen repositories (the ticked ones, else the highlighted one), opened
//! by a row's "…" or by Enter on a row (`repos_screen`), drawn on the overlay ([`crate::overlay`]) while the core's
//! mode is `Mode::Menu`, through the same bridge as the dialogs (`dialogs::Which::Menu`).
//!
//! It shows the core's lines in the core's words and with the core's keys (`MenuItem::label`, `MenuItem::key`), as
//! far as the window can do them ([`shown`]): the forks' three lines and Migrate wait for their screens. A menu has
//! a shield but no scrim; a click outside it or Esc closes it, and the click goes no further. It is not walked with
//! Tab: as in the terminal app, Up and Down (and `k` and `j`) move the highlight and stop at the ends, Home and End
//! go to them, Enter runs the highlighted line, a line's letter runs that line, and `q` closes. A letter of a line
//! the window does not show does nothing. The pointer over a line highlights it, so the mouse and the keys never
//! show two highlights. A line that opens a dialog (Update, Move to a group, Remove) hands the overlay over to it in
//! the next frame, and the key or click that chose the line does nothing in it (`overlay::open`).

use devassist_core::repos::state::{MenuItem, Repos};
use egui::{Align2, Event, FontFamily, Id, Key, Rect, Sense, Ui, pos2, vec2};

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
/// last.
fn moved(at: usize, ask: Ask) -> usize {
    let last = SHOWN.len() - 1;
    match ask {
        Ask::Step(true) => at.saturating_sub(1),
        Ask::Step(false) => (at + 1).min(last),
        Ask::End(true) => 0,
        Ask::End(false) => last,
        Ask::Close | Ask::Highlighted | Ask::Run(_) => at,
    }
}

/// The line of [`SHOWN`] the core highlights; the first when it highlights one the window does not show.
fn highlighted(repos: &Repos) -> usize {
    repos.menu_highlight().and_then(|item| SHOWN.iter().position(|shown| *shown == item)).unwrap_or(0)
}

/// The id of the menu's line `index`.
fn row_id(index: usize) -> Id {
    Id::unique(("menu-row", index))
}

/// The menu at `places`, for `repos`'s chosen repositories, on the overlay in `ui`: its lines, the highlighted one
/// on the selection's fill, Remove in the danger colour, each line's key at its right. `outside` is whether the
/// shield was clicked this frame (outside the pane); `hovered` is the line the pointer was over in the last frame,
/// kept between frames, so a highlight moved by a key stays until the pointer moves to another line. What a person
/// does goes to `repos` through its methods.
pub fn show(ui: &mut Ui, places: Places, repos: &mut Repos, palette: Palette, hovered: &mut Option<usize>, outside: bool) {
    // No Tab here: the lines take no focus, so Enter and Space press none of them; the arrows move the highlight.
    overlay::walk(ui, &[]);
    let (rows, rules) = layout(places.pane);
    let mut at = highlighted(repos);
    let mut chosen = None;
    let mut over = None;
    for (index, row) in rows.iter().enumerate() {
        let response = ui.interact(row.intersect(places.pane), row_id(index), Sense::CLICK);
        if response.hovered() {
            over = Some(index);
        }
        if response.clicked() {
            chosen = Some(Ask::Run(SHOWN[index]));
        }
    }
    if over != *hovered {
        *hovered = over;
        if let Some(index) = over {
            at = index;
        }
    }
    let mut done = chosen.or(outside.then_some(Ask::Close));
    for ask in asks(&ui.input(|input| input.events.clone())) {
        if done.is_some() {
            break;
        }
        match ask {
            Ask::Close | Ask::Run(_) => done = Some(ask),
            Ask::Highlighted => done = Some(Ask::Run(SHOWN[at])),
            Ask::Step(_) | Ask::End(_) => at = moved(at, ask),
        }
    }
    repos.highlight_menu(SHOWN[at]);

    let many = repos.chosen().len() > 1;
    let painter = ui.painter().with_clip_rect(places.pane.intersect(ui.clip_rect()));
    for rule in rules {
        painter.rect_filled(rule, 0.0, palette.on_glass.gamma_multiply(MENU_LINE_OPACITY));
    }
    let words = font(TEXT, FontFamily::Proportional);
    let small = font(KEY_TEXT, FontFamily::Proportional);
    for (index, (row, (item, icon))) in rows.iter().zip(SHOWN.into_iter().zip(ICONS)).enumerate() {
        if index == at {
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
        assert_eq!(moved(0, Ask::Step(true)), 0);
        assert_eq!(moved(0, Ask::Step(false)), 1);
        assert_eq!(moved(7, Ask::Step(false)), 7);
        assert_eq!(moved(3, Ask::End(true)), 0);
        assert_eq!(moved(3, Ask::End(false)), 7);
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
        window.frame(vec![Event::PointerMoved(lines[5].center())]);
        window.frame(Vec::new());
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Watch));
        // A key moves it on while the pointer rests; one highlight only.
        window.key(Key::ArrowDown);
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::PullRequests));
        window.click(lines[5].center());
        assert!(window.repos().mode == CoreMode::List);
        assert_eq!(window.repos().status, HELD, "the watch was asked for");
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
        // On a header with nothing ticked, Enter folds.
        window.key(Key::Escape);
        window.frame(Vec::new());
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
}
