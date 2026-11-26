//! The group dialog ([`super::Which::Group`]), for the core's `Mode::Group`: it moves the chosen repositories into
//! a group ("Move to a group"), or gives a group a new name ("Rename group") when it was asked for on a group's
//! header. The design draws no such dialog; it is built to the remove dialog's pattern: 480 wide, the same header,
//! then a text field as the design's `field` draws one, a note under it, and the groups there are as rows to click.
//!
//! The name field has the focus when the dialog opens: typing goes to it, Enter in it accepts (`Repos::accept_group`),
//! Up and Down in it walk through the groups there are (`Repos::step_group`, as in the terminal app), and Tab leaves
//! it for the buttons. A click on a group puts its name in the field, and gives the field the focus back; the group
//! whose name is in the field is drawn on the selection's fill. Esc and a click on the scrim close the dialog
//! (`Repos::close_group`). The core checks the name; what it says is wrong is shown where the note stands.

use devassist_core::repos::state::{GroupPrompt, Repos};
use egui::{Align2, FontFamily, Id, Key, Modifiers, Painter, Rect, Sense, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, inset, list_height, main_id, most_scroll, rows};
use crate::assets::Icon;
use crate::overlay::{self, Kind};
use crate::theme::{
    BUTTON_HEIGHT, DIALOG_FIELD_HEIGHT, DIALOG_GAP, DIALOG_PAD, DIALOG_WIDTH, FIELD_EDGE_OPACITY, GROUP_DIALOG_GAP, GROUP_PICK, GROUP_ROWS, HAIRLINE,
    HEADER_LINE_HEIGHT, HOVER_OPACITY, LIST_PAD, LIST_ROW, Palette, TEXT,
};
use crate::widgets::{self, font};

/// The title when repositories move.
const MOVE_TITLE: &str = "Move to a group";
/// The title when a group gets a new name.
const RENAME_TITLE: &str = "Rename group";
/// The note under the field when repositories move.
const NOTE: &str = "A new name makes a new group. Empty takes it out of its group.";
/// The quiet line when no repository is chosen and no group is renamed.
const NOTHING: &str = "No repository is chosen.";
/// What the field says while it is empty.
const HINT: &str = "Group name";
/// The button that closes the dialog.
const CANCEL: &str = "Cancel";
/// The main button when repositories move.
const MOVE: &str = "Move";
/// The main button when a group gets a new name.
const RENAME: &str = "Rename";

/// The id of the group dialog's name field, which has the focus when it opens.
pub(super) fn field_id() -> Id {
    Id::unique("dialog-group-field")
}

/// The id of the group dialog's Cancel button.
fn cancel_id() -> Id {
    Id::unique("dialog-group-cancel")
}

/// The quiet line under the title: what moves, or that a renamed group keeps its repositories.
fn line(prompt: &GroupPrompt) -> String {
    if let Some(old) = &prompt.rename {
        return format!("The repositories in {old} stay in it.");
    }
    match prompt.repos.as_slice() {
        // The core opens no such dialog with nothing chosen; were it to, the line says so, not "0 repositories".
        [] => NOTHING.to_owned(),
        [one] => format!("Moves {one}."),
        many => {
            let shown = many[..many.len().min(3)].join(", ");
            match many.len().saturating_sub(3) {
                0 => format!("Moves {} repositories: {shown}.", many.len()),
                rest => format!("Moves {} repositories: {shown} and {rest} more.", many.len()),
            }
        }
    }
}

/// What stands under the field: what the core said is wrong with the name (`true`), else the note, which a rename
/// does not show.
fn note(prompt: &GroupPrompt) -> Option<(&str, bool)> {
    match (&prompt.error, &prompt.rename) {
        (Some(error), _) => Some((error.as_str(), true)),
        (None, Some(_)) => None,
        (None, None) => Some((NOTE, false)),
    }
}

/// The words of the group dialog's buttons for `prompt`, from the left: Cancel, then the main one.
fn button_words(prompt: &GroupPrompt) -> [&'static str; 2] {
    [CANCEL, if prompt.rename.is_some() { RENAME } else { MOVE }]
}

/// The words of the group dialog's buttons in `repos`, from the left.
#[cfg(test)]
pub(super) fn said(repos: &Repos) -> Vec<&'static str> {
    repos.group_prompt.as_ref().map_or_default(|prompt| button_words(prompt).to_vec())
}

/// The group among `groups` whose name is `typed`, as the core matches it.
fn picked(groups: &[String], typed: &str) -> Option<usize> {
    groups.iter().position(|group| group.eq_ignore_ascii_case(typed.trim()))
}

/// The parts of the group dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The name field.
    pub(super) field: Rect,
    /// The note under it, when there is one.
    pub(super) note: Option<Rect>,
    /// The box of the groups, when there are some.
    pub(super) list: Option<Rect>,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// The height of the note's line.
fn note_height() -> f32 {
    TEXT * HEADER_LINE_HEIGHT
}

/// Where the group dialog's parts are in `window`, with a header `header` points tall, a note or not, and `groups`
/// groups: the header, the field, the note, the groups (up to [`GROUP_ROWS`]; more scroll), the buttons at the
/// bottom. In a window too short for all of it, the list gives up rows; the buttons stay inside the pane.
fn group_places(window: Rect, header: f32, note: bool, groups: usize) -> Places {
    let note_want = if note { GROUP_DIALOG_GAP + note_height() } else { 0.0 };
    let list_want = if groups > 0 { list_height(groups.min(GROUP_ROWS)) } else { 0.0 };
    let list_gap = if groups > 0 { GROUP_DIALOG_GAP } else { 0.0 };
    let height = 2.0 * DIALOG_PAD + header + 2.0 * DIALOG_GAP + DIALOG_FIELD_HEIGHT + note_want + list_gap + list_want + BUTTON_HEIGHT;
    let pane = overlay::dialog_rect(window, vec2(DIALOG_WIDTH, height));
    let inside = pane.shrink(DIALOG_PAD);
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    let header = Rect::from_min_size(inside.min, vec2(inside.width(), header.min(inside.height())));
    let field = Rect::from_min_size(pos2(inside.left(), header.bottom() + DIALOG_GAP), vec2(inside.width(), DIALOG_FIELD_HEIGHT));
    let note = note.then(|| Rect::from_min_size(pos2(inside.left(), field.bottom() + GROUP_DIALOG_GAP), vec2(inside.width(), note_height())));
    let top = note.map_or(field.bottom(), |note| note.bottom()) + GROUP_DIALOG_GAP;
    let list = (groups > 0).then(|| {
        let room = (buttons.top() - DIALOG_GAP - top).max(0.0);
        Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), list_want.min(room)))
    });
    Places { pane, header, field, note, list, buttons }
}

/// Where the group dialog of `repos` stands in `window`, its header laid out with `painter`'s faces; `None`
/// without the core's prompt.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos) -> Option<Plan> {
    let prompt = repos.group_prompt.as_ref()?;
    let title = if prompt.rename.is_some() { RENAME_TITLE } else { MOVE_TITLE };
    let header = Header::new(painter, title, &line(prompt), DIALOG_WIDTH - 2.0 * DIALOG_PAD);
    let places = group_places(window, header.height(), note(prompt).is_some(), repos.groups().len());
    Some(Plan::dialog(header, Parts::Group(places)))
}

/// The scroll of a list of rows [`LIST_ROW`] tall in a box `room` points tall that shows the whole row `index`,
/// moving `scroll` as little as it takes.
fn follow(index: usize, scroll: f32, room: f32) -> f32 {
    let top = LIST_PAD[0] + rows(index) * LIST_ROW;
    let bottom = top + LIST_ROW;
    if top - LIST_PAD[0] < scroll {
        top - LIST_PAD[0]
    } else if bottom + LIST_PAD[0] > scroll + room {
        (bottom + LIST_PAD[0] - room).max(0.0)
    } else {
        scroll
    }
}

/// What a person did in the group dialog in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The name typed into the field, when it changed.
    typed: Option<String>,
    /// The group clicked.
    clicked: Option<String>,
    /// Enter in the field, or the main button.
    accept: bool,
    /// Cancel, Esc or the scrim.
    close: bool,
}

/// The group dialog at `places`, with `header`, for `repos`'s prompt, on the overlay in `ui`; `scrim` is whether
/// the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let escape = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    overlay::walk(ui, &[field_id(), cancel_id(), main_id()]);
    // Up and Down in the field walk through the groups there are; the field never sees them.
    if ui.memory(|memory| memory.has_focus(field_id())) {
        if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown)) {
            repos.step_group(true);
        } else if ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowUp)) {
            repos.step_group(false);
        }
    }
    let Some(prompt) = repos.group_prompt.as_ref() else { return };
    header.draw(ui, places.header, palette);
    let mut done = Done { close: escape || scrim, ..Done::default() };
    let mut name = prompt.input.clone();
    let radius = DIALOG_FIELD_HEIGHT / 2.0;
    ui.painter().rect_filled(places.field, radius, palette.field);
    let edge = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(FIELD_EDGE_OPACITY));
    ui.painter().rect_stroke(places.field, radius, edge, StrokeKind::Inside);
    let field = widgets::field(ui, places.field, &mut name, HINT, Icon::Group, palette, field_id());
    done.accept = field.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
    if name != prompt.input {
        done.typed = Some(name.clone());
    }
    if let (Some(rect), Some((words, error))) = (places.note, note(prompt)) {
        let small = font(TEXT, FontFamily::Proportional);
        let words = widgets::ellipsis(words, &small, rect.width(), ui);
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, words, small, if error { palette.danger } else { palette.quiet });
    }
    if let Some(rect) = places.list {
        done.clicked = list(ui, rect, &repos.groups(), &name, palette, dialogs);
    }
    let words = button_words(prompt);
    let widths: Vec<f32> = words.iter().map(|words| widgets::button_width(ui, words)).collect();
    let places = overlay::buttons(places.buttons, &widths);
    if let [cancel, main] = places.as_slice() {
        done.close |= widgets::button(ui, *cancel, words[0], Kind::Plain, palette, cancel_id());
        done.accept |= widgets::button(ui, *main, words[1], Kind::Main, palette, main_id());
    }
    act(ui, repos, &done);
}

/// Does in `repos` what a person did in the group dialog; gives the field the focus back after a click on a group
/// and after a name the core refused.
fn act(ui: &Ui, repos: &mut Repos, done: &Done) {
    if done.close {
        repos.close_group();
        return;
    }
    if let Some(name) = done.typed.as_deref().or(done.clicked.as_deref()) {
        repos.set_group_name(name);
    }
    let mut refocus = done.clicked.is_some();
    if done.accept {
        repos.accept_group();
        // Refused: what is wrong stands under the field in the next frame, and the field has the focus again.
        refocus |= repos.group_prompt.is_some();
        ui.ctx().request_repaint();
    }
    if refocus {
        ui.memory_mut(|memory| memory.request_focus(field_id()));
    }
}

/// Draws `groups` in the box at `rect`, one row each, the one named `typed` on the selection's fill, scrolled by the
/// wheel and following the named one. Says which one was clicked.
fn list(ui: &Ui, rect: Rect, groups: &[String], typed: &str, palette: Palette, dialogs: &mut Dialogs) -> Option<String> {
    let inset = inset(ui, rect, palette);
    let picked = picked(groups, typed);
    if picked != dialogs.list_shown {
        dialogs.list_shown = picked;
        if let Some(index) = picked {
            dialogs.list_scroll = follow(index, dialogs.list_scroll, rect.height());
        }
    }
    dialogs.list_scroll = (dialogs.list_scroll - inset.wheel).clamp(0.0, most_scroll(groups.len(), rect.height()));
    let regular = font(TEXT, FontFamily::Proportional);
    let mut clicked = None;
    for (index, group) in groups.iter().enumerate() {
        let top = rect.top() + LIST_PAD[0] + rows(index) * LIST_ROW - dialogs.list_scroll;
        let row = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), LIST_ROW));
        let visible = row.intersect(rect);
        if !visible.is_positive() {
            continue;
        }
        let response = ui.interact(visible, Id::unique(("dialog-group", index)), Sense::CLICK);
        let fill = row.shrink2(vec2(GROUP_PICK[0], 0.0));
        if picked == Some(index) {
            inset.painter.rect_filled(fill, GROUP_PICK[1], palette.selection);
        } else if response.hovered() {
            inset.painter.rect_filled(fill, GROUP_PICK[1], palette.on_glass.gamma_multiply(HOVER_OPACITY));
        }
        let room = row.width() - 2.0 * LIST_PAD[1];
        let words = widgets::ellipsis(group, &regular, room, ui);
        inset.painter.text(pos2(row.left() + LIST_PAD[1], row.center().y), Align2::LEFT_CENTER, words, regular.clone(), palette.on_glass);
        if overlay::fresh_click(ui, &response) {
            clicked = Some(group.clone());
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::{Mode as CoreMode, group_key};
    use egui::{Event, Memory, Pos2};

    use super::super::Which;
    use super::super::testing::{Window, wheel};
    use super::*;
    use crate::session::Session;
    use crate::theme::WINDOW;

    /// The group dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Group(places) = window.parts() else { panic!("not the group dialog") };
        places
    }

    /// The demo's repositories with the group dialog open: for its ticked api and web, or on the header of
    /// `rename`.
    fn grouping(rename: Option<&str>) -> Window {
        let mut session = Session::demo(crate::demo::repos());
        let repos = session.repos_mut().unwrap();
        if let Some(group) = rename {
            repos.clear_marks();
            repos.pick(&group_key(group));
        }
        repos.open_group_prompt();
        let mut window = Window::new(session);
        window.frame(Vec::new());
        window
    }

    fn prompt(window: &Window) -> &GroupPrompt {
        window.repos().group_prompt.as_ref().unwrap()
    }

    /// The centre of the row of the group `index` in the last frame.
    fn row(window: &Window, index: usize) -> Pos2 {
        let list = parts(window).list.unwrap();
        pos2(list.center().x, list.top() + LIST_PAD[0] + rows(index) * LIST_ROW + LIST_ROW / 2.0 - window.dialogs.list_scroll)
    }

    #[test]
    fn a_click_on_a_group_fills_the_field_and_enter_moves_the_rows() {
        let mut window = grouping(None);
        assert!(window.shows(Which::Group));
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "the field has the focus");
        assert_eq!(window.said(), ["Cancel", "Move"]);
        assert_eq!(prompt(&window).input, "Work", "the first one's group");
        assert_eq!(window.repos().groups(), ["home", "Tools", "Work"]);
        assert_eq!(picked(&window.repos().groups(), "work "), Some(2), "the row of the named group is lit");

        window.click(row(&window, 0));
        assert_eq!(prompt(&window).input, "home");
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "the field has the focus back");
        window.key(Key::Enter);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        let group = |name: &str| window.repos().installed.iter().find(|repo| repo.folder == name).unwrap().group.clone();
        assert_eq!((group("api"), group("web")), ("home".to_owned(), "home".to_owned()));
        assert_eq!(window.repos().status, "2 repositories are in home now.");
    }

    #[test]
    fn typing_goes_to_the_field_and_tab_leaves_it_for_the_buttons() {
        let mut window = grouping(None);
        // Select all and type over it, as a person would.
        window.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        window.frame(vec![Event::Text("Archive".to_owned())]);
        assert_eq!(prompt(&window).input, "Archive");
        // Up and Down walk through the groups there are, as in the terminal app.
        window.key(Key::ArrowDown);
        assert_eq!(prompt(&window).input, "home");
        window.key(Key::ArrowUp);
        assert_eq!(prompt(&window).input, "Work");
        // Tab leaves the field for Cancel, then Move; typing then goes nowhere.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.frame(vec![Event::Text("x".to_owned())]);
        assert_eq!(prompt(&window).input, "Work");
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "round to the field");
        // Esc closes it, and nothing moved.
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_eq!(window.repos().installed.iter().filter(|repo| repo.group == "Work").count(), 4);
        // A click on the scrim closes it too.
        window.session.repos_mut().unwrap().open_group_prompt();
        window.frame(Vec::new());
        assert!(window.shows(Which::Group));
        window.click(pos2(40.0, 40.0));
        assert!(!window.open());
    }

    #[test]
    fn enter_on_cancel_moves_nothing_and_enter_on_move_moves_the_rows() {
        let mut window = grouping(None);
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.key(Key::Enter);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_eq!(window.repos().groups(), ["home", "Tools", "Work"]);
        assert_eq!(window.repos().installed.iter().filter(|repo| repo.group == "Work").count(), 4, "nothing moved");
        // Enter comes up, as on a real keyboard: a dialog that opens while it is held takes none of its presses.
        window.frame(vec![Event::Key { key: Key::Enter, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);

        window.session.repos_mut().unwrap().open_group_prompt();
        window.frame(Vec::new());
        window.click(row(&window, 1));
        assert_eq!(prompt(&window).input, "Tools");
        window.key(Key::Tab);
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        window.key(Key::Enter);
        assert!(!window.open());
        let group = |name: &str| window.repos().installed.iter().find(|repo| repo.folder == name).unwrap().group.clone();
        assert_eq!((group("api"), group("web")), ("Tools".to_owned(), "Tools".to_owned()));
    }

    #[test]
    fn the_line_never_counts_nothing() {
        let empty = GroupPrompt { repos: Vec::new(), rename: None, input: String::new(), error: None };
        assert_eq!(line(&empty), "No repository is chosen.");
        let many = GroupPrompt { repos: ["a", "b", "c", "d", "e"].map(str::to_owned).to_vec(), ..empty };
        assert_eq!(line(&many), "Moves 5 repositories: a, b, c and 2 more.");
        for count in 0..6 {
            let prompt =
                GroupPrompt { repos: (0..count).map(|index| format!("r{index}")).collect(), rename: None, input: String::new(), error: None };
            assert!(!line(&prompt).contains("Moves 0") && !line(&prompt).contains(": ."), "{count}: {}", line(&prompt));
        }
    }

    #[test]
    fn the_tables_keys_do_nothing_while_the_group_dialog_is_open() {
        let mut window = grouping(None);
        let before = (window.repos().marked.clone(), window.repos().selected.clone());
        // With a button chosen, the arrows, Space-less letters and Delete reach nothing under the dialog.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()));
        window.key(Key::ArrowDown);
        window.key(Key::ArrowUp);
        window.typed(Key::D, "d");
        window.typed(Key::M, "m");
        window.key(Key::Delete);
        assert!(window.shows(Which::Group));
        assert_eq!(prompt(&window).input, "Work", "the arrows did not step the groups from a button");
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone()), before);
        // In the field, letters and Space are the name's, and the table's ticks stay.
        window.key(Key::Tab);
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        window.typed(Key::Space, " ");
        window.typed(Key::D, "d");
        assert!(window.shows(Which::Group));
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone()), before);
        assert_eq!(prompt(&window).input, "Work d", "Space and d went to the name");
    }

    #[test]
    fn a_held_m_is_not_typed_into_the_name() {
        let mut window = Window::new(Session::demo(crate::demo::repos()));
        window.frame(Vec::new());
        let m = |pressed, repeat| Event::Key { key: Key::M, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE };
        // m opens the dialog; the key stays down and repeats, typing an m each time.
        window.frame(vec![m(true, false), Event::Text("m".to_owned())]);
        for _ in 0..4 {
            window.frame(vec![m(true, true), Event::Text("m".to_owned())]);
        }
        assert!(window.shows(Which::Group));
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        let name = || window.repos().group_prompt.as_ref().map(|prompt| prompt.input.clone());
        assert_eq!(name(), Some("Work".to_owned()), "the held m was typed into the name");
        // Once it is let go, what is typed is the field's.
        window.frame(vec![m(false, false)]);
        window.frame(vec![
            Event::Key { key: Key::S, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text("s".to_owned()),
        ]);
        let name = window.repos().group_prompt.as_ref().map(|prompt| prompt.input.clone());
        assert_eq!(name, Some("Works".to_owned()));
    }

    #[test]
    fn renaming_shows_no_note() {
        let mut window = grouping(Some("Tools"));
        let prompt_now = prompt(&window);
        assert_eq!(prompt_now.rename.as_deref(), Some("Tools"));
        assert_eq!(note(prompt_now), None);
        assert_eq!(line(prompt_now), "The repositories in Tools stay in it.");
        assert_eq!(parts(&window).note, None);
        assert_eq!(window.said(), ["Cancel", "Rename"]);
        // An empty name is refused: what is wrong stands under the field, which keeps the focus.
        window.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
        window.key(Key::Backspace);
        assert_eq!(prompt(&window).input, "");
        window.key(Key::Enter);
        assert!(window.shows(Which::Group));
        assert_eq!(note(prompt(&window)), Some(("A group needs a name.", true)));
        assert!(parts(&window).note.is_some());
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        window.typed(Key::B, "Build");
        window.press("Rename");
        assert!(!window.open());
        assert_eq!(window.repos().groups(), ["Build", "home", "Work"]);

        // Moving shows the note.
        let window = grouping(None);
        assert_eq!(note(prompt(&window)), Some((NOTE, false)));
        assert!(parts(&window).note.is_some());
        assert_eq!(line(prompt(&window)), "Moves 2 repositories: api, web.");
    }

    #[test]
    fn the_list_scrolls_past_six_groups_and_follows_the_named_one() {
        let mut session = Session::demo(crate::demo::repos());
        let repos = session.repos_mut().unwrap();
        for (index, repo) in repos.installed.iter_mut().enumerate() {
            repo.group = format!("group-{index:02}");
        }
        repos.open_group_prompt();
        let mut window = Window::new(session);
        window.frame(Vec::new());
        let list = parts(&window).list.unwrap();
        assert!((list.height() - list_height(GROUP_ROWS)).abs() < 1e-3, "{list:?}");
        assert!(window.buttons().iter().all(|(place, _)| parts(&window).pane.contains_rect(*place)));
        // Down from api's group walks to the last: the list follows it.
        assert_eq!(prompt(&window).input, "group-03");
        for _ in 0..8 {
            window.key(Key::ArrowDown);
        }
        assert_eq!(prompt(&window).input, "group-11");
        let most = most_scroll(12, list.height());
        assert!((window.dialogs.list_scroll - most).abs() < 1e-3, "{} of {most}", window.dialogs.list_scroll);
        // The wheel moves it back to the top and no further.
        window.frame(vec![Event::PointerMoved(list.center())]);
        for _ in 0..30 {
            window.frame(vec![wheel(200.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        assert!(window.dialogs.list_scroll.abs() < 1e-3);
        window.click(row(&window, 1));
        assert_eq!(prompt(&window).input, "group-01");
    }

    #[test]
    fn the_dialog_stays_inside_small_windows() {
        for size in [WINDOW, [960.0, 600.0], [720.0, 400.0]] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for (note, groups) in [(true, 0), (false, 3), (true, 6), (true, 50)] {
                let places = group_places(window, 60.0, note, groups);
                assert!(window.contains_rect(places.pane), "{size:?}: {places:?}");
                let parts = [Some(places.header), Some(places.field), places.note, places.list, Some(places.buttons)];
                for part in parts.into_iter().flatten() {
                    assert!(places.pane.contains_rect(part), "{size:?} {note} {groups}: {part:?} leaves {:?}", places.pane);
                }
                assert_eq!(places.note.is_some(), note);
                assert_eq!(places.list.is_some(), groups > 0);
                assert!(places.list.is_none_or(|list| list.bottom() + DIALOG_GAP <= places.buttons.top() + 1e-3));
            }
        }
        // A list of six follows the named row down, and back up.
        let room = list_height(GROUP_ROWS);
        assert!((follow(0, 0.0, room)).abs() < 1e-3);
        assert!((follow(6, 0.0, room) - LIST_ROW).abs() < 1e-3);
        assert!((follow(1, 100.0, room) - LIST_ROW).abs() < 1e-3);
    }
}
