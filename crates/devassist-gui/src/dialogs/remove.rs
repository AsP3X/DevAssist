//! The remove dialog ([`super::Which::Remove`]), for the core's `Mode::ConfirmRemove`: its title names what goes
//! ([`remove_title`]), a list says where each folder is and whether it holds uncommitted changes, and three
//! buttons decide: Cancel, Forget (off the list, the folder stays) and "Delete from disk". None of them is the main
//! one: the dialog opens with no control focused, and Enter does nothing until Tab has chosen a button. Esc and a
//! click on the scrim cancel. Deleting goes to the core's queue; how it went is said on the status line.

use devassist_core::repos::state::{RemoveLine, Repos};
use egui::{Align2, FontFamily, Id, Key, Modifiers, Painter, Rect, Stroke, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, inset, list_height, most_scroll, rows};
use crate::assets::Icon;
use crate::overlay::{self, Kind};
use crate::theme::{
    BUTTON_HEIGHT, DIALOG_GAP, DIALOG_PAD, DIALOG_WIDTH, HAIRLINE, ICON, LINE_OPACITY, LIST_GAP, LIST_PAD, LIST_ROW, Palette, REMOVE_ROWS, TEXT,
    TEXT_SMALL,
};
use crate::widgets::{self, font};

/// The quiet line under the title.
const LINE: &str =
    "Delete removes the folders from disk. Forget only takes them off the list and leaves the folders. The repositories on GitHub are never touched.";
/// The note of a folder without uncommitted changes.
const CLEAN: &str = "clean";
/// The note of a folder with uncommitted changes, in the warning colour.
const DIRTY: &str = "uncommitted changes";

/// What a button of the remove dialog does when pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Closes the dialog; nothing is removed.
    Cancel,
    /// Takes the repositories off the list; their folders stay.
    Forget,
    /// Deletes the folders from disk.
    Delete,
}

/// The remove dialog's buttons, from the left: their words, their kind and what they do.
const BUTTONS: [(&str, Kind, Press); 3] =
    [("Cancel", Kind::Plain, Press::Cancel), ("Forget", Kind::Plain, Press::Forget), ("Delete from disk", Kind::Danger, Press::Delete)];

/// The words of the remove dialog's buttons, from the left.
#[cfg(test)]
pub(super) fn button_words() -> Vec<&'static str> {
    BUTTONS.iter().map(|(words, _, _)| *words).collect()
}

/// The id of the remove dialog's button `index`, for the keyboard's focus.
fn button_id(index: usize) -> Id {
    Id::unique(("dialog-remove-button", index))
}

/// The remove dialog's title for the repositories `names`: it names up to three ("Remove api and web?"), then
/// counts them ("Remove 5 repositories?").
#[must_use]
pub fn remove_title(names: &[String]) -> String {
    match names {
        [one] => format!("Remove {one}?"),
        [first, second] => format!("Remove {first} and {second}?"),
        [first, second, third] => format!("Remove {first}, {second} and {third}?"),
        many => format!("Remove {} repositories?", many.len()),
    }
}

/// The note at the right of `line`'s path, and whether it is a warning.
fn note(line: &RemoveLine) -> (&'static str, bool) {
    if line.dirty { (DIRTY, true) } else { (CLEAN, false) }
}

/// What `line` shows: where its folder is, or its name once it is no longer on the list.
fn shown_path(line: &RemoveLine) -> String {
    if line.path.as_os_str().is_empty() { line.name.clone() } else { line.path.to_string_lossy().into_owned() }
}

/// The parts of the remove dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The box of the paths.
    pub(super) list: Rect,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// Where the remove dialog's parts are in `window`, with a header `header` points tall and `lines` paths: the
/// header at the top, the buttons at the bottom, the paths between them (up to [`REMOVE_ROWS`]; more scroll). In
/// a window too short for all of it, the list gives up rows; the buttons stay inside the pane.
fn remove_places(window: Rect, header: f32, lines: usize) -> Places {
    let list_want = list_height(lines.min(REMOVE_ROWS));
    let height = 2.0 * DIALOG_PAD + header + 2.0 * DIALOG_GAP + list_want + BUTTON_HEIGHT;
    let pane = overlay::dialog_rect(window, vec2(DIALOG_WIDTH, height));
    let inside = pane.shrink(DIALOG_PAD);
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    let header = Rect::from_min_size(inside.min, vec2(inside.width(), header.min(inside.height())));
    let top = header.bottom() + DIALOG_GAP;
    let room = (buttons.top() - DIALOG_GAP - top).max(0.0);
    let list = Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), list_want.min(room)));
    Places { pane, header, list, buttons }
}

/// Where the remove dialog for `repos`'s chosen folders stands in `window`, its header laid out with `painter`'s
/// faces.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos) -> Plan {
    let lines = repos.remove_lines();
    let names: Vec<String> = lines.iter().map(|line| line.name.clone()).collect();
    let header = Header::new(painter, &remove_title(&names), LINE, DIALOG_WIDTH - 2.0 * DIALOG_PAD);
    let places = remove_places(window, header.height(), lines.len());
    Plan::dialog(header, Parts::Remove(places))
}

/// The remove dialog at `places`, with `header`, for `repos`'s chosen folders, on the overlay in `ui`; `scrim` is
/// whether the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let ids: Vec<Id> = (0..BUTTONS.len()).map(button_id).collect();
    overlay::walk(ui, &ids);
    header.draw(ui, places.header, palette);
    list(ui, places.list, &repos.remove_lines(), palette, dialogs);
    let widths: Vec<f32> = BUTTONS.iter().map(|(words, _, _)| widgets::button_width(ui, words)).collect();
    let mut pressed = None;
    for ((place, (words, kind, press)), id) in overlay::buttons(places.buttons, &widths).into_iter().zip(BUTTONS).zip(ids) {
        if widgets::button(ui, place, words, kind, palette, id) {
            pressed = Some(press);
        }
    }
    let escape = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    if pressed.is_none() && (escape || scrim) {
        pressed = Some(Press::Cancel);
    }
    match pressed {
        Some(Press::Cancel) => repos.cancel_remove(),
        Some(Press::Forget) => repos.forget_chosen(),
        Some(Press::Delete) => repos.confirm_remove(),
        None => {}
    }
}

/// Draws `lines` in the box at `rect`: for each, the folder's icon, its path (shortened in the middle to fit, as
/// the table's Folder column does) and its note, a line between two; scrolled by the wheel.
fn list(ui: &Ui, rect: Rect, lines: &[RemoveLine], palette: Palette, dialogs: &mut Dialogs) {
    let inset = inset(ui, rect, palette);
    dialogs.list_scroll = (dialogs.list_scroll - inset.wheel).clamp(0.0, most_scroll(lines.len(), rect.height()));
    let regular = font(TEXT, FontFamily::Proportional);
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let painter = &inset.painter;
    for (index, line) in lines.iter().enumerate() {
        let top = rect.top() + LIST_PAD[0] + rows(index) * LIST_ROW - dialogs.list_scroll;
        let row = Rect::from_min_max(pos2(rect.left() + LIST_PAD[1], top), pos2(rect.right() - LIST_PAD[1], top + LIST_ROW));
        if !row.intersects(rect) {
            continue;
        }
        if index + 1 != lines.len() {
            painter.hline(row.x_range(), row.bottom() - HAIRLINE / 2.0, Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(LINE_OPACITY)));
        }
        let y = row.center().y;
        widgets::draw_icon(painter, pos2(row.left() + ICON / 2.0, y), Icon::FolderGit2, ICON, palette.quiet);
        let (words, warning) = note(line);
        painter.text(pos2(row.right(), y), Align2::RIGHT_CENTER, words, small.clone(), if warning { palette.warning } else { palette.quiet });
        let left = row.left() + ICON + LIST_GAP;
        let room = row.right() - widgets::width(painter, words, &small) - LIST_GAP - left;
        let path = widgets::path_to_fit(&shown_path(line), &regular, room, ui);
        painter.text(pos2(left, y), Align2::LEFT_CENTER, path, regular.clone(), palette.on_glass);
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use devassist_core::repos::state::{HELD, Mode as CoreMode};
    use egui::{Event, Memory, Pos2};

    use super::super::Which;
    use super::super::testing::{Window, behind, live, until, wheel};
    use super::*;
    use crate::session::Session;
    use crate::theme::WINDOW;

    /// The remove dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Remove(places) = window.parts() else { panic!("not the remove dialog") };
        places
    }

    /// The demo's repositories with the remove dialog open for its ticked api and web.
    fn removing() -> Window {
        let mut session = Session::demo(crate::demo::repos());
        session.repos_mut().unwrap().begin_remove();
        Window::new(session)
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn the_title_names_up_to_three_and_then_counts() {
        assert_eq!(remove_title(&names(&["api"])), "Remove api?");
        assert_eq!(remove_title(&names(&["api", "web"])), "Remove api and web?");
        assert_eq!(remove_title(&names(&["api", "web", "portal"])), "Remove api, web and portal?");
        assert_eq!(remove_title(&names(&["api", "web", "portal", "notes", "sandbox"])), "Remove 5 repositories?");
        // The drawing's dialog, as the demo opens it.
        let mut window = removing();
        window.frame(Vec::new());
        assert!(window.shows(Which::Remove));
        let lines = window.repos().remove_lines();
        assert_eq!(remove_title(&lines.iter().map(|line| line.name.clone()).collect::<Vec<_>>()), "Remove api and web?");
        assert_eq!(window.said(), ["Cancel", "Forget", "Delete from disk"]);
    }

    #[test]
    fn each_path_says_whether_it_is_clean() {
        let clean = RemoveLine { name: "api".to_owned(), path: PathBuf::from(r"D:\code\work\api"), dirty: false };
        let dirty = RemoveLine { dirty: true, ..clean.clone() };
        assert_eq!(note(&clean), ("clean", false));
        assert_eq!(note(&dirty), ("uncommitted changes", true));
        assert_eq!(shown_path(&clean), r"D:\code\work\api");
        let gone = RemoveLine { name: "gone".to_owned(), path: PathBuf::new(), dirty: false };
        assert_eq!(shown_path(&gone), "gone", "a folder no longer on the list shows its name");
    }

    #[test]
    fn enter_does_nothing_in_the_remove_dialog() {
        let mut window = removing();
        window.frame(Vec::new());
        assert!(window.shows(Which::Remove));
        assert_eq!(window.ctx.memory(Memory::focused), None, "no button has the focus");
        window.key(Key::Enter);
        window.key(Key::Space);
        assert!(window.shows(Which::Remove), "Enter pressed a button nobody chose");
        assert_eq!(window.repos().marked, ["api", "web"]);
        // Tab chooses Cancel, the first; Enter presses it, and nothing is removed.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(button_id(0)));
        window.key(Key::Enter);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_eq!(window.repos().installed.len(), 12);
        assert_eq!(window.repos().marked, ["api", "web"], "the ticks stay");

        // Esc and a click on the scrim cancel too.
        let cancels: [fn(&mut Window); 2] = [|window| window.key(Key::Escape), |window| window.click(pos2(40.0, 40.0))];
        for cancel in cancels {
            window.session.repos_mut().unwrap().begin_remove();
            window.frame(Vec::new());
            assert!(window.shows(Which::Remove));
            cancel(&mut window);
            assert!(!window.open() && window.repos().mode == CoreMode::List);
            assert_eq!(window.repos().installed.len(), 12);
        }
    }

    fn shift_tab() -> Event {
        Event::Key { key: Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::SHIFT }
    }

    #[test]
    fn shift_tab_from_nothing_chooses_cancel_and_never_delete() {
        let mut window = removing();
        window.frame(Vec::new());
        assert_eq!(window.ctx.memory(Memory::focused), None);
        window.frame(vec![shift_tab()]);
        window.frame(Vec::new());
        assert_eq!(window.ctx.memory(Memory::focused), Some(button_id(0)), "Shift-Tab chose a button other than Cancel");
        window.key(Key::Enter);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_ne!(window.repos().status, HELD, "Delete from disk was pressed");
        assert_eq!(window.repos().installed.len(), 12);
        assert_eq!(window.repos().marked, ["api", "web"], "Cancel keeps the ticks");
    }

    #[test]
    fn the_tables_keys_do_nothing_while_the_remove_dialog_is_open() {
        let mut window = removing();
        window.frame(Vec::new());
        let before = (window.repos().marked.clone(), window.repos().selected.clone());
        let keys = |window: &mut Window| {
            window.typed(Key::Space, " ");
            window.typed(Key::D, "d");
            window.typed(Key::M, "m");
            window.key(Key::ArrowDown);
            window.key(Key::ArrowUp);
            window.key(Key::Delete);
        };
        keys(&mut window);
        assert!(window.shows(Which::Remove), "{:?}", window.repos().status);
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone()), before);
        // With Cancel chosen, the arrows and letters still reach nothing under the dialog.
        window.key(Key::Tab);
        window.typed(Key::D, "d");
        window.key(Key::ArrowDown);
        window.key(Key::ArrowUp);
        assert!(window.shows(Which::Remove));
        assert_eq!(window.ctx.memory(Memory::focused), Some(button_id(0)));
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone()), before);
        assert_eq!(window.repos().installed.len(), 12);
    }

    /// A press at `from` released at `to`, over five frames.
    fn drag(window: &mut Window, from: Pos2, to: Pos2) {
        let button = |at, pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for events in [vec![Event::PointerMoved(from)], vec![button(from, true)], vec![Event::PointerMoved(to)], vec![button(to, false)], Vec::new()]
        {
            window.frame(events);
        }
    }

    #[test]
    fn only_a_press_and_release_on_delete_deletes() {
        let mut window = removing();
        window.frame(Vec::new());
        let delete = window.buttons().into_iter().find(|(_, words)| *words == "Delete from disk").unwrap().0;
        // Empty glass beside the buttons, left of Cancel: neither a button nor the scrim.
        let pane = parts(&window).pane;
        let beside = pos2(pane.left() + DIALOG_PAD + 4.0, delete.center().y);
        drag(&mut window, beside, delete.center());
        assert!(window.shows(Which::Remove), "a press begun beside it and released on it deleted");
        assert_ne!(window.repos().status, HELD);
        drag(&mut window, delete.center(), beside);
        assert!(window.shows(Which::Remove), "a press begun on it and released beside it deleted");
        assert_ne!(window.repos().status, HELD);
        // A real click on it does.
        window.click(delete.center());
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
    }

    #[test]
    fn the_demo_forgets_in_memory_and_deletes_nothing() {
        let mut window = removing();
        window.frame(Vec::new());
        window.press("Delete from disk");
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
        assert_eq!(window.repos().installed.len(), 12, "a held core removes nothing");

        window.session.repos_mut().unwrap().begin_remove();
        window.frame(Vec::new());
        window.press("Forget");
        assert!(!window.open());
        assert_eq!(window.repos().installed.len(), 10);
        assert!(window.repos().installed.iter().all(|repo| repo.folder != "api" && repo.folder != "web"));
        assert_eq!(window.repos().status, "2 repositories are off the list. Their folders stay on disk.");
    }

    #[test]
    fn a_dialog_with_fifty_paths_keeps_its_buttons_in_view() {
        for size in [[960.0, 600.0], [720.0, 400.0]] {
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for count in [1, 2, 8, 9, 50] {
                let places = remove_places(screen, 83.0, count);
                assert!(screen.contains_rect(places.pane), "{size:?} {count}: {places:?}");
                for part in [places.header, places.list, places.buttons] {
                    assert!(places.pane.contains_rect(part), "{size:?} {count}: {part:?} leaves {:?}", places.pane);
                }
                assert!((places.buttons.height() - BUTTON_HEIGHT).abs() < 1e-3);
                assert!(places.list.bottom() + DIALOG_GAP <= places.buttons.top() + 1e-3);
                assert!(places.list.height() <= list_height(count.min(REMOVE_ROWS)) + 1e-3);
                assert!(places.list.height() >= LIST_ROW, "{size:?} {count}: no room for a row: {places:?}");
            }
        }
        // In the smallest window, fifty paths still scroll to the last and the buttons stay in view.
        for size in [[960.0, 600.0], [720.0, 400.0]] {
            scrolls_to_the_end(size);
        }
        // The drawing's dialog: 480 wide, two paths in a box 80 tall, under a header of 84.
        let places = remove_places(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1])), 84.0, 2);
        assert!((places.pane.width() - 480.0).abs() < 1e-3 && (places.list.height() - 80.0).abs() < 1e-3, "{places:?}");
        assert!((places.pane.height() - 296.0).abs() < 1e-3, "{places:?}");
    }

    /// Fifty paths in a window of `size`: as many rows as fit show (eight when there is room), the wheel reaches
    /// the last and no further, the buttons stay in view and Cancel still works.
    fn scrolls_to_the_end(size: [f32; 2]) {
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
        let mut session = Session::demo(crate::demo::repos());
        session.repos_mut().unwrap().mode = CoreMode::ConfirmRemove { folders: (0..50).map(|index| format!("repository-{index}")).collect() };
        let mut window = Window::new(session);
        window.size = size;
        window.frame(Vec::new());
        let list = parts(&window).list;
        if size[1] >= 600.0 {
            assert!((list.height() - list_height(REMOVE_ROWS)).abs() < 1e-3);
        }
        assert!(list.height() >= LIST_ROW && list.height() < list_height(50), "{size:?}: {list:?}");
        window.frame(vec![Event::PointerMoved(list.center())]);
        for _ in 0..60 {
            window.frame(vec![wheel(-200.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        let most = most_scroll(50, list.height());
        assert!((window.dialogs.list_scroll - most).abs() < 1e-3, "{} of {most}", window.dialogs.list_scroll);
        assert!(window.buttons().iter().all(|(place, _)| parts(&window).pane.contains_rect(*place) && screen.contains_rect(*place)));
        window.press("Cancel");
        assert!(!window.open());
    }

    #[test]
    fn forget_keeps_the_folder_and_delete_removes_it() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("remove", &["api", "web"]);
        let mut window = live(settings, 2);
        let (api, web) = (root.0.join("api"), root.0.join("web"));

        // Forget: off the list, the folder stays, the settings no longer name it.
        window.session.repos_mut().unwrap().marked = vec!["api".to_owned()];
        window.session.repos_mut().unwrap().begin_remove();
        window.frame(Vec::new());
        assert!(window.shows(Which::Remove));
        window.press("Forget");
        assert!(!window.open());
        assert!(window.repos().installed.iter().all(|repo| repo.folder != "api"));
        assert!(api.join("one").exists(), "Forget left the folder");
        let saved = std::fs::read_to_string(root.0.join("devassist.toml")).unwrap();
        let named = |path: &std::path::Path| {
            let raw = path.display().to_string();
            saved.contains(&raw) || saved.contains(&raw.replace('\\', "\\\\"))
        };
        assert!(!named(&api) && named(&web), "{saved}");

        // Delete from disk: the folder goes; the dialog closes at once, and the status line says how it went.
        window.session.repos_mut().unwrap().marked = vec!["web".to_owned()];
        window.session.repos_mut().unwrap().begin_remove();
        window.frame(Vec::new());
        window.press("Delete from disk");
        assert!(!window.shows(Which::Remove));
        until(&mut window, "removed", |window| !web.exists() && !window.repos().busy);
        assert!(window.repos().installed.is_empty(), "{:?}", window.repos().installed.iter().map(|repo| &repo.folder).collect::<Vec<_>>());
        assert!(api.exists(), "only web was deleted");
        for kept in [root.0.clone(), root.0.join("web-origin"), root.0.join("api-origin"), root.0.join("devassist.toml")] {
            assert!(kept.exists(), "{} went with web", kept.display());
        }
        assert!(!window.open(), "no dialog came up for it");
        window.session.close();
    }
}
