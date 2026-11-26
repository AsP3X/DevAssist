//! The Add dialog ([`super::Which::Add`]), for the core's `Mode::Add`: it finds repositories on GitHub, lets a
//! person tick the ones to clone and choose the folder they go into, and clones them in one step
//! (`Repos::clone_marked_into`). It is the design's `5 · Add a repository`, 600 wide: the header, the search field,
//! the core's status line under it, the found repositories in a box that scrolls, "Clone into" with the folder
//! field and its note, and the buttons.
//!
//! The search field has the focus when the dialog opens. Letters only type; Space ticks the highlighted row and is
//! never typed (a search has no spaces, as in the terminal app); Up and Down move the core's highlight
//! (`Repos::pick_add`), and the list follows it; Enter does what the main button does. In the folder field Space
//! types, and Up and Down are the field's own (its suggestions). A click on a row's checkbox ticks it
//! (`Repos::toggle_add_mark_of`); a click elsewhere on the row highlights it. An installed row is drawn at half
//! strength and can be highlighted, but not ticked.
//!
//! The main button clones what the core will clone: the ticked repositories, also those the search hides (their
//! names stand on a quiet line above the buttons), else the highlighted one. Esc, Cancel and the scrim close the
//! dialog through the core (`Action::Cancel`). The core's words are shown as the core has them: its status line
//! (`Repos::add_status`, with a spinner while it looks something up, and "Retry" when GitHub failed), what it
//! says when nothing is found (`Repos::add_empty_message`), and the reason it refused the folder, under the field.

use devassist_core::repos::github::RemoteRepo;
use devassist_core::repos::state::{Action, AddStatus, Repos};
use egui::text::LayoutJob;
use egui::{Align2, FontFamily, Id, Key, Painter, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, folder_suggestions, inset, main_id, rows, take_press};
use crate::assets::{self, Icon};
use crate::overlay::{self, Kind};
use crate::theme::{
    ADD_FOLDER_GAP, ADD_INSTALLED_BOX_OPACITY, ADD_LEAST_ROWS, ADD_PICK, ADD_ROW, ADD_ROW_PAD, ADD_STATUS_GAP, ADD_STATUS_ROW, ADD_TEXT_GAP,
    ADD_WIDTH, BUTTON_HEIGHT, BUTTON_PAD, CHECKBOX, CHECKBOX_STROKE_OPACITY, DIALOG_FIELD_HEIGHT, DIALOG_FIELD_RADIUS, DIALOG_GAP, DIALOG_MARGIN,
    DIALOG_PAD, FIELD_EDGE_OPACITY, HAIRLINE, HEADER_LINE_HEIGHT, HOVER_OPACITY, LINE_OPACITY, Palette, RADIUS_SMALL, SPINNER, SPINNER_FRAME,
    SPINNER_GAP, SPINNER_STEPS, SPINNER_STROKE, SPINNER_SWEEP, SPINNER_TURN, TEXT, TEXT_SMALL,
};
use crate::widgets::{self, FolderDid, Tick, font};

/// The title.
const TITLE: &str = "Add a repository";
/// The quiet line under the title.
const LINE: &str = "Search your accounts, type owner/ for another account, or paste a GitHub link.";
/// What the search field says while it is empty.
const HINT: &str = "Search";
/// The words over the folder field.
const CLONE_INTO: &str = "Clone into";
/// The note under the folder field, while the core has not refused the folder.
const NOTE: &str = "A folder that does not exist yet is created.";
/// The button that closes the dialog.
const CANCEL: &str = "Cancel";
/// The main button while it cannot clone anything.
const CLONE: &str = "Clone";
/// The button beside a status line that says GitHub failed.
const RETRY: &str = "Retry";
/// The note at the right of an installed row.
const INSTALLED: &str = "Installed";
/// The word at the right of an archived repository.
const ARCHIVED: &str = "archived";
/// The word at the right of a private repository.
const PRIVATE: &str = "private";
/// How the line that names the ticked repositories the search hides starts.
const ALSO_TICKED: &str = "Also ticked: ";

/// The id of the search field, which has the focus when the dialog opens.
pub(super) fn search_id() -> Id {
    Id::unique("dialog-add-search")
}

/// The id of the folder field.
pub(super) fn folder_id() -> Id {
    Id::unique("dialog-add-folder")
}

/// The id of the Cancel button.
fn cancel_id() -> Id {
    Id::unique("dialog-add-cancel")
}

/// The id of the Retry button.
fn retry_id() -> Id {
    Id::unique("dialog-add-retry")
}

/// What the Add dialog keeps of its own from one frame to the next, made when it opens ([`State::new`]).
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// The folder field's text: the core's `clone_dir` when the dialog opens.
    folder: String,
    /// The folders that match it ([`folder_suggestions`]), asked for again only when it changes.
    suggestions: Vec<String>,
    /// Why the core refused to clone into it, said under the field until the text changes.
    refused: Option<String>,
    /// The row the list showed highlighted last and where it stood, so the list follows the highlight when it
    /// moves to another row or its row moves.
    shown: Option<(String, usize)>,
    /// The search as the field held it last: when the core's differs (it listed an account, making "word" into
    /// "word/"), the field's cursor goes to its end.
    search: String,
}

impl State {
    /// The folder field's text as the dialog keeps it.
    #[cfg(test)]
    pub(super) fn folder_text(&self) -> &str {
        &self.folder
    }

    /// The state of an Add dialog that opens on `repos`: the folder field at the core's `clone_dir`, and the
    /// folders that match it, asked for once (a text the dialog sets itself is never typed, so the field never
    /// says it changed).
    pub(super) fn new(repos: &Repos) -> State {
        let folder = repos.clone_dir.display().to_string();
        let suggestions = folder_suggestions(repos, &folder);
        State { folder, suggestions, refused: None, shown: None, search: repos.add_filter.clone() }
    }
}

/// The core's status line, as the dialog shows it: without the terminal app's hint at Tab, which in the window
/// moves the focus (`Repos::add_status_plain`).
fn status_of(repos: &Repos) -> AddStatus {
    repos.add_status_plain()
}

/// The name a found repository shows: its name alone while every row of `list` is of one owner, else
/// "owner/name".
fn shown_name(list: &[RemoteRepo], remote: &RemoteRepo) -> String {
    if list.iter().all(|other| other.owner.eq_ignore_ascii_case(&remote.owner)) { remote.name.clone() } else { remote.id() }
}

/// The note at the right of a found repository's row, and whether it is the installed one: "Installed", or what
/// GitHub says of it (archived, private), if anything.
fn note(repos: &Repos, remote: &RemoteRepo) -> Option<(String, bool)> {
    if repos.is_installed(remote) {
        return Some((INSTALLED.to_owned(), true));
    }
    match (remote.archived, remote.private) {
        (true, true) => Some((format!("{ARCHIVED} · {PRIVATE}"), false)),
        (true, false) => Some((ARCHIVED.to_owned(), false)),
        (false, true) => Some((PRIVATE.to_owned(), false)),
        (false, false) => None,
    }
}

/// The main button's words for `repos` with the found `list`, and whether it can be pressed: "Clone {n}
/// repositories" for what is ticked (also the ticks the search hides), else "Clone owner/name" for the highlighted
/// row when it can be cloned, else "Clone", off.
fn main_words(repos: &Repos, list: &[RemoteRepo]) -> (String, bool) {
    let ticked = repos.add_marked.iter().filter(|remote| !repos.is_installed(remote)).count();
    if !repos.add_marked.is_empty() {
        return match ticked {
            0 => (CLONE.to_owned(), false),
            1 => ("Clone 1 repository".to_owned(), true),
            many => (format!("Clone {many} repositories"), true),
        };
    }
    let highlighted = repos.add_selected.as_deref().and_then(|id| list.iter().find(|remote| remote.id().eq_ignore_ascii_case(id)));
    match highlighted {
        Some(remote) if !repos.is_installed(remote) => (format!("{CLONE} {}", shown_name(list, remote)), true),
        _ => (CLONE.to_owned(), false),
    }
}

/// The words of the Add dialog's buttons in `repos`, from the left.
#[cfg(test)]
pub(super) fn said(repos: &Repos) -> Vec<String> {
    vec![CANCEL.to_owned(), main_words(repos, &repos.add_list()).0]
}

/// The ticked repositories `list` does not show ("owner/name"): the search hides them, and they are cloned all the
/// same.
fn hidden_ticks(repos: &Repos, list: &[RemoteRepo]) -> Vec<String> {
    repos.add_marked.iter().filter(|ticked| !list.iter().any(|remote| remote.id().eq_ignore_ascii_case(&ticked.id()))).map(RemoteRepo::id).collect()
}

/// The line that names `hidden` ("Also ticked: owner/a, owner/b"), as many of them as `fits` one line, the rest
/// counted ("and 3 more"); `None` when there are none.
fn also_line(hidden: &[String], fits: impl Fn(&str) -> bool) -> Option<String> {
    if hidden.is_empty() {
        return None;
    }
    let line = |shown: usize| {
        let names = hidden[..shown].join(", ");
        match hidden.len() - shown {
            0 => format!("{ALSO_TICKED}{names}"),
            rest => format!("{ALSO_TICKED}{names} and {rest} more"),
        }
    };
    (1..=hidden.len()).rev().map(line).find(|candidate| fits(candidate)).or_else(|| Some(line(1)))
}

/// The plain word the search holds while nothing is found, which the core can list as an account ("List
/// github.com/{word}").
fn list_word(repos: &Repos, list: &[RemoteRepo]) -> Option<String> {
    if list.is_empty() { repos.add_word() } else { None }
}

/// The parts of the Add dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The search field.
    pub(super) search: Rect,
    /// The core's status line, with the spinner or the Retry button.
    pub(super) status: Rect,
    /// The Retry button, when GitHub failed.
    pub(super) retry: Option<Rect>,
    /// The box of the found repositories.
    pub(super) list: Rect,
    /// "Clone into".
    pub(super) label: Rect,
    /// The folder field with the room its rows take.
    pub(super) folder: Rect,
    /// The note under it, or the reason the folder was refused.
    pub(super) note: Rect,
    /// The line that names the ticked repositories the search hides, when there are some.
    pub(super) also: Option<Rect>,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// The height of a line of `size`-point text.
fn line_height(size: f32) -> f32 {
    size * HEADER_LINE_HEIGHT
}

/// How tall the found repositories' box and the folder field's rows are, given `spare` points beside the fixed
/// parts: the box wants `content`, the rows `rows` (the folder field's room beyond the bare field). The box keeps
/// [`ADD_LEAST_ROWS`] rows (or all, when fewer), then the rows take what they want, then the box what is left;
/// when even that is too much, the rows keep one row and the box gives up the rest. The two never take more than
/// `spare`.
fn share(spare: f32, content: f32, rows: f32, one_row: f32) -> (f32, f32) {
    let spare = spare.max(0.0);
    let least = content.min(rows_f(ADD_LEAST_ROWS) * ADD_ROW);
    let mut list = (spare - rows).max(least).min(content);
    let mut folder = (spare - list).max(one_row.min(rows)).min(rows);
    if list + folder > spare {
        folder = folder.min(spare);
        list = spare - folder;
    }
    (list, folder)
}

/// `count` as points' worth of rows.
fn rows_f(count: usize) -> f32 {
    rows(count)
}

/// Where the Add dialog's parts are in `window`, with a header `header` points tall, `found` rows in its box (one
/// at least: the core's message stands there when nothing is found), the folder field showing `folder_rows` rows,
/// a Retry button `retry` wide when GitHub failed, and the line of the hidden ticks or not. The box takes what the
/// window leaves (see [`share`]); the buttons stay inside the pane.
fn add_places(window: Rect, header: f32, found: usize, folder_rows: usize, retry: Option<f32>, also: bool) -> Places {
    let also_want = if also { line_height(TEXT) + ADD_FOLDER_GAP } else { 0.0 };
    let fixed = 2.0 * DIALOG_PAD
        + header
        + DIALOG_GAP
        + DIALOG_FIELD_HEIGHT
        + 2.0 * ADD_STATUS_GAP
        + ADD_STATUS_ROW
        + DIALOG_GAP
        + line_height(TEXT)
        + ADD_FOLDER_GAP
        + DIALOG_FIELD_HEIGHT
        + ADD_FOLDER_GAP
        + line_height(TEXT_SMALL)
        + DIALOG_GAP
        + also_want
        + BUTTON_HEIGHT;
    let room = window.height() - 2.0 * DIALOG_MARGIN;
    let content = rows_f(found.max(1)) * ADD_ROW;
    let rows_want = widgets::folder_field_height(folder_rows) - DIALOG_FIELD_HEIGHT;
    let one_row = widgets::folder_field_height(1) - DIALOG_FIELD_HEIGHT;
    let (list_height, rows_height) = share(room - fixed, content, rows_want, one_row);
    let pane = overlay::dialog_rect(window, vec2(ADD_WIDTH, fixed + list_height + rows_height));
    let inside = pane.shrink(DIALOG_PAD);
    let width = inside.width();
    let at = |top: f32, height: f32| Rect::from_min_size(pos2(inside.left(), top), vec2(width, height));
    let header = at(inside.top(), header);
    let search = at(header.bottom() + DIALOG_GAP, DIALOG_FIELD_HEIGHT);
    let status = at(search.bottom() + ADD_STATUS_GAP, ADD_STATUS_ROW);
    let retry = retry.map(|wide| Rect::from_min_max(pos2(status.right() - wide, status.top()), status.max));
    let list = at(status.bottom() + ADD_STATUS_GAP, list_height);
    let label = at(list.bottom() + DIALOG_GAP, line_height(TEXT));
    let folder = at(label.bottom() + ADD_FOLDER_GAP, DIALOG_FIELD_HEIGHT + rows_height);
    let note = at(folder.bottom() + ADD_FOLDER_GAP, line_height(TEXT_SMALL));
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    let also = also.then(|| at(buttons.top() - ADD_FOLDER_GAP - line_height(TEXT), line_height(TEXT)));
    Places { pane, header, search, status, retry, list, label, folder, note, also, buttons }
}

/// How wide a small button with `words` is: its words in the medium face and [`BUTTON_PAD`] at either side.
fn small_button_width(painter: &Painter, words: &str) -> f32 {
    2.0 * BUTTON_PAD + widgets::width(painter, words, &font(TEXT, assets::medium_family()))
}

/// Where the Add dialog of `repos` stands in `window`, its header laid out with `painter`'s faces, its folder field
/// as `state` has it.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos, state: &State) -> Plan {
    let header = Header::new(painter, TITLE, LINE, ADD_WIDTH - 2.0 * DIALOG_PAD);
    let list = repos.add_list();
    let folder_rows = widgets::folder_field_rows(painter.ctx(), folder_id(), state.suggestions.len());
    let retry = status_of(repos).error.then(|| small_button_width(painter, RETRY));
    let also = !hidden_ticks(repos, &list).is_empty();
    let places = add_places(window, header.height(), list.len(), folder_rows, retry, also);
    Plan::dialog(header, Parts::Add(places))
}

/// What a person did in the Add dialog in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The search typed, when it changed.
    search: Option<String>,
    /// What was clicked in the found repositories' box.
    clicked: Option<Clicked>,
    /// Retry.
    retry: bool,
    /// The main button, or Enter in a field.
    accept: bool,
    /// Cancel, Esc or the scrim.
    close: bool,
}

/// The Add dialog at `places`, with `header`, for `repos`, on the overlay in `ui`; `scrim` is whether the scrim was
/// clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(mut state) = dialogs.add.take() else { return };
    // As the terminal app does at every draw: a highlight on a row that is there, the preferred one when none is.
    repos.ensure_selection();
    let mut done = Done { close: scrim, ..Done::default() };
    let status = status_of(repos);
    let (_, main_on) = main_words(repos, &repos.add_list());
    let mut ids = vec![search_id()];
    if places.retry.is_some() {
        ids.push(retry_id());
    }
    ids.extend([folder_id(), cancel_id()]);
    if main_on {
        ids.push(main_id());
    }
    overlay::walk(ui, &ids);

    // The search field's keys, before anything sees them: Space ticks and is never typed, Up and Down move the
    // highlight, Enter is the main button's.
    if ui.memory(|memory| memory.has_focus(search_id())) {
        if take_press(ui, Key::Space, Some(" "))
            && let Some(id) = repos.add_selected.clone()
        {
            repos.toggle_add_mark_of(&id);
        }
        let down = take_press(ui, Key::ArrowDown, None);
        let up = take_press(ui, Key::ArrowUp, None);
        if down || up {
            step(repos, down);
        }
        done.accept |= take_press(ui, Key::Enter, None);
    }
    let list = repos.add_list();
    let (main, main_on) = main_words(repos, &list);
    let word = list_word(repos, &list);
    header.draw(ui, places.header, palette);

    // The folder field first: it takes Esc, Enter, Up and Down while its rows show; the dialog's own come after.
    let folder_focused = ui.memory(|memory| memory.has_focus(folder_id()));
    let did = folder(ui, places, &mut state, palette);
    if did.changed {
        state.suggestions = folder_suggestions(repos, &state.folder);
        state.refused = None;
    }
    if folder_focused {
        done.accept |= take_press(ui, Key::Enter, None);
    }
    done.close |= take_press(ui, Key::Escape, None);

    let mut search = repos.add_filter.clone();
    if search != state.search {
        // The core changed the search: typing goes on at its end, and the list starts at its top.
        widgets::put_cursor_at_end(ui.ctx(), search_id(), &search);
        dialogs.list_scroll = 0.0;
    }
    field_frame(ui, places.search, palette);
    widgets::field(ui, places.search, &mut search, HINT, Icon::Search, palette, search_id());
    if search != repos.add_filter {
        // A new search starts at the top of what it finds.
        dialogs.list_scroll = 0.0;
        done.search = Some(search.clone());
    }
    state.search = search;
    done.retry = status_line(ui, places, &status, palette);
    done.clicked = found(ui, places.list, repos, &list, word.as_deref(), palette, dialogs, &mut state);
    if let Some(rect) = places.also
        && let Some(line) =
            also_line(&hidden_ticks(repos, &list), |line| widgets::width(ui.painter(), line, &font(TEXT, FontFamily::Proportional)) <= rect.width())
    {
        let words = widgets::ellipsis(&line, &font(TEXT, FontFamily::Proportional), rect.width(), ui);
        ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, words, font(TEXT, FontFamily::Proportional), palette.quiet);
    }

    let widths = [widgets::button_width(ui, CANCEL), widgets::button_width(ui, &main)];
    if let [cancel, main_place] = overlay::buttons(places.buttons, &widths).as_slice() {
        done.close |= widgets::button(ui, *cancel, CANCEL, Kind::Plain, palette, cancel_id());
        if main_on {
            done.accept |= widgets::button(ui, *main_place, &main, Kind::Main, palette, main_id());
        } else {
            widgets::button_off(ui, *main_place, &main, palette);
        }
    }
    // Enter with nothing to clone lists the plain word typed, as the core's message says.
    let accept = done.accept && (main_on || word.is_some());
    act(ui, repos, &mut state, &Done { accept, ..done });
    dialogs.add = Some(state);
}

/// Moves the core's highlight one row down (`down`) or up among the found repositories; with none, to the first.
fn step(repos: &mut Repos, down: bool) {
    let ids: Vec<String> = repos.add_list().iter().map(RemoteRepo::id).collect();
    let Some(last) = ids.len().checked_sub(1) else { return };
    let at = repos.add_selected.as_deref().and_then(|selected| ids.iter().position(|id| id.eq_ignore_ascii_case(selected)));
    let next = match (at, down) {
        (None, _) => 0,
        (Some(at), true) => (at + 1).min(last),
        (Some(at), false) => at.saturating_sub(1),
    };
    repos.pick_add(&ids[next]);
}

/// Paints a dialog's text field's fill and outline at `rect`.
fn field_frame(ui: &Ui, rect: Rect, palette: Palette) {
    ui.painter().rect_filled(rect, DIALOG_FIELD_RADIUS, palette.field);
    let edge = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(FIELD_EDGE_OPACITY));
    ui.painter().rect_stroke(rect, DIALOG_FIELD_RADIUS, edge, StrokeKind::Inside);
}

/// "Clone into", the folder field with `state`'s text and suggestions, and under it the note or the reason the
/// folder was refused, in the danger colour.
fn folder(ui: &mut Ui, places: Places, state: &mut State, palette: Palette) -> FolderDid {
    ui.painter().text(places.label.left_center(), Align2::LEFT_CENTER, CLONE_INTO, font(TEXT, assets::medium_family()), palette.on_glass);
    let did = widgets::folder_field(ui, places.folder, &mut state.folder, &state.suggestions, palette, folder_id());
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let (words, colour) = match &state.refused {
        Some(reason) if !did.changed => (reason.as_str(), palette.danger),
        _ => (NOTE, palette.quiet),
    };
    let words = widgets::ellipsis(words, &small, places.note.width(), ui);
    ui.painter().text(places.note.left_center(), Align2::LEFT_CENTER, words, small, colour);
    did
}

/// The core's status line at `places.status`: a spinner while it looks something up (drawn from the frame's time;
/// the next frame is asked for only then), its words in the quiet colour, or in the danger colour beside a Retry
/// button when GitHub failed. Says whether Retry was pressed.
fn status_line(ui: &mut Ui, places: Places, status: &AddStatus, palette: Palette) -> bool {
    let rect = places.status;
    let mut left = rect.left();
    if status.busy {
        spinner(ui, pos2(left + SPINNER / 2.0, rect.center().y), palette);
        left += SPINNER + SPINNER_GAP;
        ui.ctx().request_repaint_after(SPINNER_FRAME);
    }
    let right = places.retry.map_or(rect.right(), |retry| retry.left() - ADD_ROW_PAD);
    let regular = font(TEXT, FontFamily::Proportional);
    let words = widgets::ellipsis(&status.text, &regular, (right - left).max(0.0), ui);
    let colour = if status.error { palette.danger } else { palette.quiet };
    ui.painter().text(pos2(left, rect.center().y), Align2::LEFT_CENTER, words, regular, colour);
    places.retry.is_some_and(|retry| widgets::button(ui, retry, RETRY, Kind::Plain, palette, retry_id()))
}

/// Draws the spinner centred on `center`: an arc that turns once every [`SPINNER_TURN`] seconds of the frame's time.
#[expect(clippy::cast_possible_truncation, reason = "a share of one turn, between 0 and 1, fits an f32")]
fn spinner(ui: &Ui, center: Pos2, palette: Palette) {
    let turn = (ui.input(|input| input.time) / SPINNER_TURN).fract() as f32;
    let radius = (SPINNER - SPINNER_STROKE) / 2.0;
    let start = turn * std::f32::consts::TAU;
    let steps = f32::from(SPINNER_STEPS);
    let points: Vec<Pos2> = (0..=SPINNER_STEPS)
        .map(|step| {
            let angle = start + SPINNER_SWEEP * std::f32::consts::TAU * f32::from(step) / steps;
            center + radius * vec2(angle.cos(), angle.sin())
        })
        .collect();
    ui.painter().add(Shape::line(points, Stroke::new(SPINNER_STROKE, palette.quiet)));
}

/// What a click in the found repositories' box did.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Clicked {
    /// The checkbox of the row of this repository ("owner/name").
    Tick(String),
    /// The row of this repository elsewhere.
    Pick(String),
    /// The quiet row that lists a typed account.
    ListAccount,
}

/// The scroll of rows [`ADD_ROW`] tall in a box `room` points tall that shows the whole row `index`, moving `scroll`
/// as little as it takes.
fn follow(index: usize, scroll: f32, room: f32) -> f32 {
    let top = rows_f(index) * ADD_ROW;
    let bottom = top + ADD_ROW;
    if top < scroll {
        top
    } else if bottom > scroll + room {
        (bottom - room).max(0.0)
    } else {
        scroll
    }
}

/// How far `count` rows can be moved up in a box `room` points tall.
fn most_scroll(count: usize, room: f32) -> f32 {
    (rows_f(count) * ADD_ROW - room).max(0.0)
}

/// Draws the found repositories `list` in the box at `rect`, scrolled by the wheel and following the core's
/// highlight; with none, the quiet row that lists the typed `word` as an account, or the core's message. Says
/// what was clicked.
#[expect(clippy::too_many_arguments, reason = "the box, the core and its list, the word, and the dialog's state")]
fn found(
    ui: &mut Ui,
    rect: Rect,
    repos: &Repos,
    list: &[RemoteRepo],
    word: Option<&str>,
    palette: Palette,
    dialogs: &mut Dialogs,
    state: &mut State,
) -> Option<Clicked> {
    let inset = inset(ui, rect, palette);
    let mut clicked = None;
    if list.is_empty() {
        let regular = font(TEXT, FontFamily::Proportional);
        let row = Rect::from_min_size(rect.min, vec2(rect.width(), ADD_ROW.min(rect.height())));
        let at = pos2(row.left() + ADD_ROW_PAD, row.center().y);
        let room = row.width() - 2.0 * ADD_ROW_PAD;
        if let Some(word) = word {
            let response = ui.interact(row.intersect(ui.clip_rect()), Id::unique("dialog-add-list-account"), Sense::CLICK);
            if response.hovered() {
                inset.painter.rect_filled(row.shrink2(vec2(ADD_PICK[0], 0.0)), ADD_PICK[1], palette.on_glass.gamma_multiply(HOVER_OPACITY));
            }
            let words = widgets::ellipsis(&format!("List github.com/{word}"), &regular, room, ui);
            inset.painter.text(at, Align2::LEFT_CENTER, words, regular, palette.quiet);
            clicked = overlay::fresh_click(ui, &response).then_some(Clicked::ListAccount);
        } else {
            let message = repos.add_empty_message();
            let words = widgets::ellipsis(&message, &regular, room, ui);
            inset.painter.text(at, Align2::LEFT_CENTER, words, regular, palette.quiet);
        }
        return clicked;
    }
    let highlighted = repos.add_selected.as_deref().and_then(|id| list.iter().position(|remote| remote.id().eq_ignore_ascii_case(id)));
    let now = repos.add_selected.clone().zip(highlighted);
    if now != state.shown {
        state.shown = now;
        if let Some(index) = highlighted {
            dialogs.list_scroll = follow(index, dialogs.list_scroll, rect.height());
        }
    }
    dialogs.list_scroll = (dialogs.list_scroll - inset.wheel).clamp(0.0, most_scroll(list.len(), rect.height()));
    let saved = ui.clip_rect();
    ui.set_clip_rect(rect.intersect(saved));
    for (index, remote) in list.iter().enumerate() {
        let top = rect.top() + rows_f(index) * ADD_ROW - dialogs.list_scroll;
        let row = Rect::from_min_size(pos2(rect.left(), top), vec2(rect.width(), ADD_ROW));
        if !row.intersect(rect).is_positive() {
            continue;
        }
        let id = remote.id();
        let (on_row, on_box) = found_row(ui, row, index, repos, list, remote, highlighted == Some(index), palette);
        if index + 1 != list.len() {
            let line = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(LINE_OPACITY));
            inset.painter.hline(row.x_range(), row.bottom() - HAIRLINE / 2.0, line);
        }
        if on_box {
            clicked = Some(Clicked::Tick(id));
        } else if on_row {
            clicked = Some(Clicked::Pick(id));
        }
    }
    ui.set_clip_rect(saved);
    clicked
}

/// Draws the found repository `remote`, the row `index` of `list`, at `row`: on the selection's fill when
/// `highlighted`, else on the hover's fill under the pointer unless it is installed; its checkbox (one that takes no
/// click when it is installed), its name over its description, its note at the right; an installed row set back
/// (its words in the palette's `installed` colours, its checkbox at [`ADD_INSTALLED_BOX_OPACITY`]). Says whether
/// the row and its checkbox were clicked.
#[expect(clippy::too_many_arguments, reason = "the row, the core and its list, the repository and how it is drawn")]
fn found_row(
    ui: &mut Ui,
    row: Rect,
    index: usize,
    repos: &Repos,
    list: &[RemoteRepo],
    remote: &RemoteRepo,
    highlighted: bool,
    palette: Palette,
) -> (bool, bool) {
    let response = ui.interact(row.intersect(ui.clip_rect()), Id::unique(("dialog-add-row", index)), Sense::CLICK);
    let fill = row.shrink2(vec2(ADD_PICK[0], 0.0));
    let installed = repos.is_installed(remote);
    if highlighted {
        ui.painter().rect_filled(fill, ADD_PICK[1], palette.selection);
    } else if response.hovered() && !installed {
        // An installed row gives no pointer feedback: nothing is done with it by pointing, and a hover over its
        // set-back words would cost them their contrast (the controller's ruling of 2026-11-26).
        ui.painter().rect_filled(fill, ADD_PICK[1], palette.on_glass.gamma_multiply(HOVER_OPACITY));
    }
    let (ink, quiet) = if installed { (palette.installed, palette.installed_quiet) } else { (palette.on_glass, palette.quiet) };
    let check = Rect::from_min_size(pos2(row.left() + ADD_ROW_PAD, row.top()), vec2(CHECKBOX + ADD_ROW_PAD, row.height()));
    let mut on_box = false;
    if installed {
        let shape = widgets::box_rect(check);
        let stroke = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(CHECKBOX_STROKE_OPACITY * ADD_INSTALLED_BOX_OPACITY));
        ui.painter().rect_stroke(shape, RADIUS_SMALL, stroke, StrokeKind::Inside);
    } else {
        let tick = if repos.is_marked(remote) { Tick::On } else { Tick::Off };
        let response = widgets::checkbox(ui, check, ("dialog-add", index), tick, palette, &remote.id());
        on_box = overlay::fresh_click(ui, &response);
    }
    let left = row.left() + ADD_ROW_PAD + CHECKBOX + ADD_ROW_PAD;
    let mut right = row.right() - ADD_ROW_PAD;
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    if let Some((words, is_installed)) = note(repos, remote) {
        let (face, colour) = if is_installed { (font(TEXT_SMALL, assets::medium_family()), ink) } else { (small.clone(), quiet) };
        ui.painter().text(pos2(right, row.center().y), Align2::RIGHT_CENTER, &words, face.clone(), colour);
        right -= widgets::width(ui.painter(), &words, &face) + ADD_ROW_PAD;
    }
    let room = (right - left).max(0.0);
    let name = widgets::ellipsis(&shown_name(list, remote), &font(TEXT, assets::medium_family()), room, ui);
    let name = ui.painter().layout_job(LayoutJob::simple_singleline(name, font(TEXT, assets::medium_family()), ink));
    let about = (!remote.description.is_empty()).then(|| {
        ui.painter().layout_job(LayoutJob::simple_singleline(widgets::ellipsis(&remote.description, &small, room, ui), small.clone(), quiet))
    });
    let height = name.size().y + about.as_ref().map_or(0.0, |about| ADD_TEXT_GAP + about.size().y);
    let top = row.center().y - height / 2.0;
    let name_height = name.size().y;
    ui.painter().galley(pos2(left, top), name, ink);
    if let Some(about) = about {
        ui.painter().galley(pos2(left, top + name_height + ADD_TEXT_GAP), about, quiet);
    }
    (overlay::fresh_click(ui, &response), on_box)
}

/// Does in `repos` what a person did in the Add dialog in one frame; keeps the dialog's own `state`.
fn act(ui: &Ui, repos: &mut Repos, state: &mut State, done: &Done) {
    if done.close {
        repos.run(Action::Cancel);
        return;
    }
    if let Some(search) = &done.search {
        repos.set_add_filter(search);
    }
    match &done.clicked {
        Some(Clicked::Tick(id)) => repos.toggle_add_mark_of(id),
        Some(Clicked::Pick(id)) => repos.pick_add(id),
        Some(Clicked::ListAccount) => repos.list_typed_account(),
        None => {}
    }
    if done.retry {
        repos.run(Action::Retry);
    }
    if done.accept {
        if let Err(reason) = repos.clone_marked_into(&state.folder) {
            state.refused = Some(reason);
        }
        ui.ctx().request_repaint();
    }
    if done.clicked.is_some() {
        // The keys stay the search's: Space ticks, the arrows move on.
        ui.memory_mut(|memory| memory.request_focus(search_id()));
    }
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::testing::{remote, test_settings};
    use devassist_core::repos::state::{HELD, Mode as CoreMode, RemoteState};
    use egui::{Event, Memory, Modifiers, Pos2};

    use super::super::Which;
    use super::super::testing::{Temp, Window, until, wheel};
    use super::*;
    use crate::session::Session;
    use crate::theme::{MIN_WINDOW, WINDOW};

    /// The Add dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Add(places) = window.parts() else { panic!("not the Add dialog") };
        places
    }

    fn state(window: &Window) -> &State {
        window.dialogs.add.as_ref().unwrap()
    }

    /// The demo with Add open as `--show add` opens it: eight found, two installed, two ticked.
    fn adding() -> Window {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "add").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        window
    }

    /// The centre of the found row `index` in the last frame, and of its checkbox.
    fn row(window: &Window, index: usize) -> (Pos2, Pos2) {
        let list = parts(window).list;
        let y = list.top() + rows_f(index) * ADD_ROW + ADD_ROW / 2.0 - window.dialogs.list_scroll;
        (pos2(list.center().x, y), pos2(list.left() + ADD_ROW_PAD + CHECKBOX / 2.0, y))
    }

    fn index_of(window: &Window, id: &str) -> usize {
        window.repos().add_list().iter().position(|remote| remote.id() == id).unwrap()
    }

    /// A press of `key` typing `text`, and its release: a key the dialog acts on once while it is held acts again
    /// only after it came up (egui calls a press of a key that is down a repeat).
    fn tap(window: &mut Window, key: Key, text: Option<&str>) {
        let event = |pressed| Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Modifiers::NONE };
        let mut press = vec![event(true)];
        press.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(press);
        window.frame(vec![event(false)]);
    }

    fn select_all(window: &mut Window) {
        window.frame(vec![Event::Key { key: Key::A, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::COMMAND }]);
    }

    #[test]
    fn typing_searches_and_only_types() {
        let mut window = adding();
        assert!(window.shows(Which::Add));
        assert_eq!(window.ctx.memory(Memory::focused), Some(search_id()), "the search has the focus");
        let marked = window.repos().marked.clone();
        let ticked: Vec<String> = window.repos().add_marked.iter().map(RemoteRepo::id).collect();
        // The letters the table and the menu use are typed into the search, and nothing else happens.
        for (key, letter) in [(Key::A, "a"), (Key::I, "i"), (Key::D, "d"), (Key::U, "u"), (Key::W, "w"), (Key::M, "m"), (Key::L, "l"), (Key::S, "s")]
        {
            window.typed(key, letter);
        }
        assert_eq!(window.repos().add_filter, "aiduwmls");
        assert!(window.shows(Which::Add));
        assert_eq!(window.repos().marked, marked, "the table's ticks stay");
        // Backspace back to "art": three found.
        for _ in 0..8 {
            window.key(Key::Backspace);
        }
        window.frame(vec![Event::Text("art".to_owned())]);
        assert_eq!(window.repos().add_filter, "art");
        let found: Vec<String> = window.repos().add_list().iter().map(RemoteRepo::id).collect();
        assert_eq!(found, ["acme/artcraft", "acme/artcraft-docs", "acme/artcraft-plugins"]);
        // Up and Down move the core's highlight; Space ticks it and is never typed.
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/artcraft"));
        tap(&mut window, Key::ArrowDown, None);
        tap(&mut window, Key::ArrowDown, None);
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/artcraft-plugins"));
        tap(&mut window, Key::ArrowDown, None);
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/artcraft-plugins"), "the last stays");
        tap(&mut window, Key::Space, Some(" "));
        assert_eq!(window.repos().add_filter, "art", "Space was typed");
        assert!(window.repos().add_marked.iter().any(|remote| remote.id() == "acme/artcraft-plugins"));
        tap(&mut window, Key::ArrowUp, None);
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/artcraft-docs"));
        tap(&mut window, Key::Space, Some(" "));
        assert!(!window.repos().add_marked.iter().any(|remote| remote.id() == "acme/artcraft-docs"), "Space unticks");
        assert_eq!(window.repos().add_filter, "art");
        // A held Space ticks once.
        let space = |repeat| Event::Key { key: Key::Space, physical_key: None, pressed: true, repeat, modifiers: Modifiers::NONE };
        window.frame(vec![space(false), Event::Text(" ".to_owned())]);
        for _ in 0..3 {
            window.frame(vec![space(true), Event::Text(" ".to_owned())]);
        }
        window.frame(vec![Event::Key { key: Key::Space, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(window.repos().add_marked.iter().any(|remote| remote.id() == "acme/artcraft-docs"), "the held Space toggled more than once");
        assert_eq!(window.repos().add_filter, "art");
        assert_eq!(ticked, ["acme/artcraft", "acme/artcraft-docs"], "the demo's ticks");
        // In the folder field, Space types.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()));
        window.typed(Key::Space, " ");
        assert!(state(&window).folder.ends_with(' '), "{:?}", state(&window).folder);
        assert_eq!(window.repos().add_filter, "art");
        // Cancel closes Add through the core, and nothing is cloned.
        window.press("Cancel");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_ne!(window.repos().status, HELD);
        // Esc does too.
        window.session.repos_mut().unwrap().run(Action::Add);
        window.frame(Vec::new());
        assert!(window.shows(Which::Add));
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    #[test]
    fn the_enter_that_opens_add_clones_nothing() {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "add").unwrap();
        let mut window = Window::new(Session::demo(repos));
        let enter = |pressed, repeat| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE };
        window.frame(vec![enter(true, false)]);
        for _ in 0..3 {
            window.frame(vec![enter(true, true)]);
        }
        assert!(window.shows(Which::Add), "the Enter that opened Add pressed its main button");
        assert_ne!(window.repos().status, HELD);
        window.frame(vec![enter(false, false)]);
        window.key(Key::Enter);
        assert!(!window.shows(Which::Add), "a fresh Enter in the search is the main button");
        assert_eq!(window.repos().status, HELD, "the demo's core refused the clone");
    }

    #[test]
    fn a_tick_on_an_installed_row_does_nothing() {
        let mut window = adding();
        let api = index_of(&window, "acme/api");
        assert!(window.repos().is_installed(&window.repos().add_list()[api]));
        let before: Vec<String> = window.repos().add_marked.iter().map(RemoteRepo::id).collect();
        let (on_row, on_box) = row(&window, api);
        window.click(on_box);
        let after: Vec<String> = window.repos().add_marked.iter().map(RemoteRepo::id).collect();
        assert_eq!(after, before, "an installed row was ticked");
        // A click on the row highlights it, even an installed one; nothing is ticked, the button counts the ticks.
        window.click(on_row);
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/api"));
        assert_eq!(window.said(), ["Cancel", "Clone 2 repositories"]);
        assert_eq!(window.ctx.memory(Memory::focused), Some(search_id()), "the keys stay the search's");
        // A click on a box that can be ticked ticks it, and leaves the highlight.
        let plugins = index_of(&window, "acme/artcraft-plugins");
        window.click(row(&window, plugins).1);
        assert!(window.repos().add_marked.iter().any(|remote| remote.id() == "acme/artcraft-plugins"));
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/api"));
        assert_eq!(window.said(), ["Cancel", "Clone 3 repositories"]);
    }

    #[test]
    fn the_button_counts_what_will_be_cloned() {
        let mut window = adding();
        assert_eq!(window.said(), ["Cancel", "Clone 2 repositories"]);
        let repos = window.session.repos_mut().unwrap();
        repos.toggle_add_mark_of("acme/artcraft");
        window.frame(Vec::new());
        assert_eq!(window.said(), ["Cancel", "Clone 1 repository"]);
        window.session.repos_mut().unwrap().toggle_add_mark_of("acme/artcraft-docs");
        window.frame(Vec::new());
        // Nothing ticked: the highlighted row, as the row shows it (owners differ, so with its owner).
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/artcraft"));
        assert_eq!(window.said(), ["Cancel", "Clone acme/artcraft"]);
        // An installed row highlighted: nothing to clone, the button is off and Enter does nothing.
        window.session.repos_mut().unwrap().pick_add("acme/portal");
        window.frame(Vec::new());
        assert_eq!(window.said(), ["Cancel", "Clone"]);
        window.key(Key::Enter);
        assert!(window.shows(Which::Add));
        assert_eq!(state(&window).refused, None);
        // One owner among the rows: the name alone.
        window.frame(vec![Event::Text("glass".to_owned())]);
        assert_eq!(window.said(), ["Cancel", "Clone glass-kit"]);
    }

    #[test]
    fn a_tick_hidden_by_the_search_is_still_counted_and_named() {
        let mut window = adding();
        assert_eq!(parts(&window).also, None, "nothing is hidden");
        window.frame(vec![Event::Text("docs".to_owned())]);
        let list = window.repos().add_list();
        assert_eq!(list.iter().map(RemoteRepo::id).collect::<Vec<_>>(), ["acme/artcraft-docs"]);
        assert_eq!(hidden_ticks(window.repos(), &list), ["acme/artcraft"]);
        assert_eq!(window.said(), ["Cancel", "Clone 2 repositories"], "the hidden tick is counted");
        let also = parts(&window).also.unwrap();
        assert!(also.bottom() <= parts(&window).buttons.top());
        assert_eq!(also_line(&hidden_ticks(window.repos(), &list), |_| true).as_deref(), Some("Also ticked: acme/artcraft"));
        // Too many for one line: the rest are counted.
        let many: Vec<String> = (0..6).map(|index| format!("owner/repository-{index}")).collect();
        let line = also_line(&many, |line| line.len() <= 70).unwrap();
        assert_eq!(line, "Also ticked: owner/repository-0, owner/repository-1 and 4 more");
        assert_eq!(also_line(&many, |_| false).as_deref(), Some("Also ticked: owner/repository-0 and 5 more"));
        assert_eq!(also_line(&[], |_| true), None);
        // The clone takes both: the held demo refuses it as the core does.
        window.press("Clone 2 repositories");
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
        assert_eq!(window.repos().add_marked.len(), 2, "held, the ticks stay");
    }

    #[test]
    fn a_bad_folder_is_said_under_the_field_and_add_stays_open() {
        let mut window = adding();
        assert_eq!(state(&window).folder, crate::demo::CLONE_DIR);
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()));
        select_all(&mut window);
        window.frame(vec![Event::Text("dev".to_owned())]);
        assert_eq!(state(&window).folder, "dev");
        window.key(Key::Enter);
        assert!(window.shows(Which::Add), "Add closed on a folder the core refused");
        let refused = state(&window).refused.clone().unwrap();
        assert!(refused.starts_with("Use a full path"), "{refused}");
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()), "the folder field keeps the focus");
        assert_ne!(window.repos().status, HELD);
        // The button says it too.
        window.press("Clone 2 repositories");
        assert!(window.shows(Which::Add));
        assert!(state(&window).refused.is_some());
        // Typing takes the reason away.
        let field = parts(&window).folder;
        window.click(pos2(field.center().x, field.top() + DIALOG_FIELD_HEIGHT / 2.0));
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()));
        window.typed(Key::X, "x");
        assert_eq!(state(&window).refused, None);
    }

    /// A file URL Git clones from, for a folder on this disk.
    fn file_url(path: &std::path::Path) -> String {
        let text = path.display().to_string().replace('\\', "/");
        if text.starts_with('/') { format!("file://{text}") } else { format!("file:///{text}") }
    }

    fn git(dir: &std::path::Path, args: &[&str]) {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "-c", "commit.gpgsign=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    #[test]
    fn clone_closes_add_and_the_progress_dialog_takes_over() {
        let _live = crate::session::tests::live_git();
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-add-clone-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&root.0);
        let origin = root.0.join("tool-work");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        std::fs::write(origin.join("readme"), "tool").unwrap();
        git(&origin, &["add", "readme"]);
        git(&origin, &["commit", "-q", "-m", "one"]);
        let settings = devassist_core::config::Settings { config_path: root.0.join("devassist.toml"), ..test_settings() };
        let mut repos = Repos::empty(settings);
        repos.remotes_state = RemoteState::Ready;
        let mut tool = remote("storytold", "tool");
        tool.clone_url = file_url(&origin);
        repos.remotes = vec![tool, remote("storytold", "other")];
        repos.run(Action::Add);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        assert!(window.shows(Which::Add));
        let code = root.0.join("code");
        window.key(Key::Tab);
        select_all(&mut window);
        window.frame(vec![Event::Text(code.display().to_string())]);
        assert_eq!(state(&window).folder, code.display().to_string());
        // One owner: the rows show the names alone. A click on tool's row highlights it.
        assert_eq!(window.said(), ["Cancel", "Clone other"]);
        let tool = index_of(&window, "storytold/tool");
        window.click(row(&window, tool).0);
        assert_eq!(window.said(), ["Cancel", "Clone tool"]);
        window.press("Clone tool");
        assert!(window.shows(Which::Progress), "the progress dialog did not take over from Add: {}", window.repos().status);
        until(&mut window, "done", |window| !window.repos().waiting());
        assert!(code.join("tool").join(".git").is_dir(), "{}", window.repos().status);
        assert!(window.repos().status.starts_with("Cloned tool into"), "{}", window.repos().status);
        assert_eq!(window.repos().clone_dir, code);
        window.session.close();
    }

    #[test]
    fn an_error_from_github_offers_retry() {
        let _live = crate::session::tests::live_git();
        let reached = devassist_core::sys::reached(devassist_core::sys::Reach::Network);
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "add").unwrap();
        repos.remotes_state = RemoteState::Failed("GitHub answered 502".to_owned());
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        let status = window.repos().add_status();
        assert!(status.error && status.text == "GitHub answered 502", "{}", status.text);
        let retry = parts(&window).retry.expect("a Retry button beside the error");
        assert!(parts(&window).status.contains_rect(retry));
        // Tab reaches it after the search.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(retry_id()));
        window.click(retry.center());
        assert!(window.shows(Which::Add), "Retry closed Add");
        assert_eq!(devassist_core::sys::reached(devassist_core::sys::Reach::Network), reached, "the held demo asked GitHub");
        // Without an error there is no Retry.
        window.session.repos_mut().unwrap().remotes_state = RemoteState::Ready;
        window.frame(Vec::new());
        assert_eq!(parts(&window).retry, None);
    }

    #[test]
    fn two_hundred_found_scroll_and_the_buttons_stay_in_view() {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "add").unwrap();
        repos.remotes = (0..200).map(|index| remote("acme", &format!("repository-{index:03}"))).collect();
        repos.add_marked.clear();
        let mut window = Window::new(Session::demo(repos));
        window.size = MIN_WINDOW;
        window.frame(Vec::new());
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
        let in_view = |window: &Window| {
            let places = parts(window);
            screen.contains_rect(places.pane)
                && window.buttons().iter().all(|(place, _)| places.pane.contains_rect(*place) && screen.contains_rect(*place))
                && places.list.bottom() + DIALOG_GAP <= places.label.top() + 1e-3
                && places.note.bottom() <= places.buttons.top()
        };
        assert!(in_view(&window));
        let list = parts(&window).list;
        assert!(list.height() >= rows_f(ADD_LEAST_ROWS) * ADD_ROW - 1e-3, "{list:?}");
        // The wheel reaches the last row and no further.
        window.frame(vec![Event::PointerMoved(list.center())]);
        for _ in 0..200 {
            window.frame(vec![wheel(-400.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        let most = most_scroll(200, list.height());
        assert!((window.dialogs.list_scroll - most).abs() < 1e-3, "{} of {most}", window.dialogs.list_scroll);
        // A new highlight is followed back into view, and so is Down.
        window.session.repos_mut().unwrap().pick_add("acme/repository-001");
        window.frame(Vec::new());
        assert!((window.dialogs.list_scroll - ADD_ROW).abs() < 1e-3, "the highlight was not followed: {}", window.dialogs.list_scroll);
        for _ in 0..10 {
            tap(&mut window, Key::ArrowDown, None);
        }
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/repository-011"));
        let top = rows_f(11) * ADD_ROW - window.dialogs.list_scroll;
        assert!(top >= -1e-3 && top + ADD_ROW <= list.height() + 1e-3, "row 10 at {top} in {}", list.height());

        // The folder field's rows open too (the demo's four made-up folders): the buttons stay in view, the rows
        // show one row at least.
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()));
        select_all(&mut window);
        window.frame(vec![Event::Text(format!("{}{}", crate::demo::CODE_DIR, std::path::MAIN_SEPARATOR))]);
        window.frame(Vec::new());
        assert_eq!(state(&window).suggestions.len(), 4);
        let places = parts(&window);
        assert!(places.folder.height() >= widgets::folder_field_height(1) - 1e-3, "{places:?}");
        assert!(in_view(&window), "{places:?}");
        // In every window from the smallest up, with and without the rows, all of it stays inside.
        for size in [MIN_WINDOW, WINDOW] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for (found, folder_rows, also) in [(0, 0, false), (8, 0, false), (200, 6, true), (200, 0, true), (2, 6, false)] {
                let places = add_places(window, 44.0, found, folder_rows, Some(80.0), also);
                assert!(window.contains_rect(places.pane), "{size:?}: {places:?}");
                for part in [places.header, places.search, places.status, places.list, places.label, places.folder, places.note, places.buttons] {
                    assert!(places.pane.contains_rect(part), "{size:?} {found} {folder_rows}: {part:?} leaves {:?}", places.pane);
                }
                assert!(places.also.is_none_or(|also| also.top() >= places.note.bottom() && also.bottom() <= places.buttons.top()));
                if folder_rows == 0 {
                    assert!(places.list.height() >= rows_f(found.clamp(1, ADD_LEAST_ROWS)) * ADD_ROW - 1e-3, "{size:?} {found}: {places:?}");
                }
            }
        }
    }

    #[test]
    fn closing_add_leaves_no_lookup_that_moves_the_highlight() {
        let reached = devassist_core::sys::reached(devassist_core::sys::Reach::Network);
        let mut repos = Repos::offline(test_settings());
        repos.remotes = vec![remote("storytold", "alpha"), remote("storytold", "beta")];
        repos.run(Action::Add);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        // An account that is not in the catalog: the core plans a lookup for it.
        window.frame(vec![Event::Text("someone/".to_owned())]);
        assert_eq!(window.repos().add_filter, "someone/");
        assert!(window.repos().waiting(), "no lookup was planned");
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        let selected = (window.repos().selected.clone(), window.repos().add_selected.clone());
        // Until the core waits for nothing (the planned lookup's time has passed and the core let it go, or it was
        // dropped as Add closed), the window takes in what the core's workers send: nothing was started.
        until(&mut window, "waiting for nothing", |window| !window.repos().waiting());
        for _ in 0..5 {
            window.session.tick();
            window.frame(Vec::new());
        }
        assert!(!window.repos().waiting(), "a lookup still runs");
        assert_eq!((window.repos().selected.clone(), window.repos().add_selected.clone()), selected);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        assert_eq!(devassist_core::sys::reached(devassist_core::sys::Reach::Network), reached, "GitHub was asked");
    }

    #[test]
    fn the_cores_folder_question_of_a_clone_brings_add_back() {
        let mut window = adding();
        window.session.repos_mut().unwrap().clone_selected();
        assert!(window.repos().mode == CoreMode::Path && window.repos().prompt.is_some());
        window.frame(Vec::new());
        assert!(window.repos().mode == CoreMode::Add && window.repos().prompt.is_none(), "the bridge left the core's question open");
        assert!(window.shows(Which::Add) && window.painted, "no Add drawn over it");
        assert!(window.baked, "Add was drawn without its glass");
        assert_eq!(window.repos().add_marked.len(), 2, "the ticks stay");
    }

    #[test]
    fn a_plain_word_with_nothing_found_lists_that_account() {
        let mut window = adding();
        window.frame(vec![Event::Text("zzzz".to_owned())]);
        assert!(window.repos().add_list().is_empty());
        assert_eq!(list_word(window.repos(), &window.repos().add_list()).as_deref(), Some("zzzz"));
        let first = row(&window, 0).0;
        window.click(first);
        assert_eq!(window.repos().add_filter, "zzzz/", "the account is listed");
        assert!(window.shows(Which::Add));
        assert_eq!(window.ctx.memory(Memory::focused), Some(search_id()));
    }

    #[test]
    fn the_demo_suggests_its_own_folders_and_never_asks_the_disk() {
        let before = super::super::disk_reads();
        let mut window = adding();
        let sep = std::path::MAIN_SEPARATOR;
        let code = crate::demo::CODE_DIR;
        assert_eq!(state(&window).suggestions, [format!("{code}{sep}work"), format!("{code}{sep}work-archive")]);
        window.key(Key::Tab);
        assert_eq!(window.ctx.memory(Memory::focused), Some(folder_id()));
        select_all(&mut window);
        window.frame(vec![Event::Text(format!("{code}{sep}"))]);
        assert_eq!(state(&window).suggestions.len(), 4, "{:?}", state(&window).suggestions);
        window.frame(vec![Event::Text("P".to_owned())]);
        assert_eq!(state(&window).suggestions, [format!("{code}{sep}play")], "matched as Tab matches, ignoring case");
        assert_eq!(super::super::disk_reads(), before, "the held demo read the disk for folders");
        // A live session asks the disk.
        let mut repos = Repos::offline(test_settings());
        repos.run(Action::Add);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        assert_eq!(super::super::disk_reads(), before + 1);
    }

    #[test]
    fn no_status_in_add_names_tab() {
        let mut window = adding();
        for typed in ["", "artcraft", "zzzz", "acme/", "someone/"] {
            window.session.repos_mut().unwrap().set_add_filter(typed);
            window.frame(Vec::new());
            let text = status_of(window.repos()).text;
            assert!(!text.to_lowercase().contains("tab"), "{typed:?}: {text}");
        }
        window.session.repos_mut().unwrap().set_add_filter("artcraft");
        assert!(window.repos().add_status().text.contains("tab lists"), "the terminal app's words changed");
        assert_eq!(status_of(window.repos()).text, "8 repositories · 2 cloned");
    }

    #[test]
    fn typing_goes_on_after_the_slash_when_an_account_was_listed() {
        let mut window = adding();
        // Nothing ticked: with ticks, Enter clones them.
        window.session.repos_mut().unwrap().toggle_add_mark_of("acme/artcraft");
        window.session.repos_mut().unwrap().toggle_add_mark_of("acme/artcraft-docs");
        window.frame(vec![Event::Text("zzzz".to_owned())]);
        assert!(window.repos().add_list().is_empty());
        window.dialogs.list_scroll = 30.0;
        // Enter lists the plain word as an account: the core makes it "zzzz/".
        tap(&mut window, Key::Enter, None);
        assert_eq!(window.repos().add_filter, "zzzz/");
        assert!(window.shows(Which::Add));
        assert_eq!(window.ctx.memory(Memory::focused), Some(search_id()));
        assert!(window.dialogs.list_scroll.abs() < 1e-3, "the list kept its scroll for a new search");
        tap(&mut window, Key::X, Some("x"));
        assert_eq!(window.repos().add_filter, "zzzz/x", "the letter did not land after the slash");
    }

    #[test]
    fn space_on_an_installed_highlight_ticks_nothing() {
        let mut window = adding();
        window.session.repos_mut().unwrap().pick_add("acme/portal");
        window.frame(Vec::new());
        let before: Vec<String> = window.repos().add_marked.iter().map(RemoteRepo::id).collect();
        tap(&mut window, Key::Space, Some(" "));
        let after: Vec<String> = window.repos().add_marked.iter().map(RemoteRepo::id).collect();
        assert_eq!(after, before);
        assert_eq!(window.repos().add_filter, "", "Space was typed");
        assert!(window.shows(Which::Add));
    }

    #[test]
    fn enter_while_a_lookup_runs_with_nothing_to_clone_does_nothing() {
        let reached = devassist_core::sys::reached(devassist_core::sys::Reach::Network);
        let mut repos = Repos::offline(test_settings());
        repos.remotes = vec![remote("storytold", "alpha")];
        repos.run(Action::Add);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        window.frame(vec![Event::Text("someone/".to_owned())]);
        assert!(window.repos().add_status().busy, "{}", window.repos().add_status().text);
        assert_eq!(window.said(), ["Cancel", "Clone"]);
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Add));
        assert_eq!(window.repos().add_filter, "someone/");
        assert_eq!(state(&window).refused, None);
        assert_eq!(window.ctx.memory(Memory::focused), Some(search_id()), "the search lost the focus");
        window.key(Key::Escape);
        assert!(!window.open());
        assert_eq!(devassist_core::sys::reached(devassist_core::sys::Reach::Network), reached);
    }

    #[test]
    fn an_installed_row_shows_no_hover() {
        let mut window = adding();
        let hover = crate::theme::palette(crate::theme::Mode::Light).on_glass.gamma_multiply(crate::theme::HOVER_OPACITY);
        let hovered = |window: &Window, index: usize| {
            let (centre, _) = row(window, index);
            let whole = Rect::from_center_size(centre, vec2(parts(window).list.width(), ADD_ROW));
            window.fills.iter().any(|(rect, fill)| *fill == hover && whole.contains_rect(*rect))
        };
        // An ordinary row that is not highlighted shows the hover.
        let plugins = index_of(&window, "acme/artcraft-plugins");
        window.frame(vec![Event::PointerMoved(row(&window, plugins).0)]);
        window.frame(Vec::new());
        assert!(hovered(&window, plugins), "an ordinary row shows no hover");
        // An installed row shows none: nothing is done with it by pointing.
        let api = index_of(&window, "acme/api");
        window.frame(vec![Event::PointerMoved(row(&window, api).0)]);
        window.frame(Vec::new());
        assert!(!hovered(&window, api), "an installed row shows the hover");
        // A click still highlights it, and the highlight is drawn.
        window.click(row(&window, api).0);
        assert_eq!(window.repos().add_selected.as_deref(), Some("acme/api"));
        let selection = crate::theme::palette(crate::theme::Mode::Light).selection;
        let (centre, _) = row(&window, api);
        let whole = Rect::from_center_size(centre, vec2(parts(&window).list.width(), ADD_ROW));
        assert!(window.fills.iter().any(|(rect, fill)| *fill == selection && whole.contains_rect(*rect)), "the highlight is not drawn");
    }
}
