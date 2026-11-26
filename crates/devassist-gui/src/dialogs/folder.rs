//! The folder dialog ([`super::Which::Folder`]), for the core's `Mode::Path`: it asks for a folder, and the core does
//! with it what its question is for (`Purpose`): an Import, or a Move.
//!
//! Import adds the GitHub clones in the folder, and the folder itself, to the overview. The design draws no such
//! dialog; it is built from the Add dialog's parts, 600 wide: the header, "Folder" over the folder field with the
//! folders that match under it, the core's reason under the field when it refused the folder, and the buttons.
//!
//! Move takes the chosen checkouts into another folder, as the design's `13-move` draws it, 600 wide: the header
//! (its title names them by the remove dialog's rule, [`super::remove::named`]), "Move into" over the folder field and
//! the quiet hint under it (the core's reason in its place when it refused the folder), "What moves": a box with a
//! row for each checkout (its folder, an arrow, where it goes; "is there already" or "cannot move there" at the
//! right of one that stays), which scrolls past [`MOVE_ROWS`] rows, the note on another drive when the core can tell
//! that the move copies, and the buttons. The main button counts the rows that move, and is off while none can.
//!
//! What differs between the purposes is said in one place ([`words`]): the title, the quiet line, the words over
//! the field and the main button's. A purpose the dialog does not show has no words, and the bridge shows no
//! dialog for it.
//!
//! The field has the focus when the dialog opens. Its text is the core's (`Repos::set_path_input` as it is typed); the
//! folders that match, and for a move each checkout's row (`Repos::move_lines`) and whether it crosses to another
//! drive (`Repos::move_crosses_volumes`), are asked for once when it opens and again only when the text changes
//! (they read the disk). Enter in the field (with no suggestion lit) and the main button ask the core to go ahead
//! (`Action::Confirm`): when it refuses the folder, the dialog stays with the core's reason under the field, in the
//! danger colour, until the text changes; else the dialog closes: an import runs in the core on the window's thread
//! and its status line says what was done; a move is the core's run, whose progress dialog the bridge opens. Esc,
//! Cancel and the scrim close it through the core (`Repos::close_path`).

use std::sync::Arc;

use devassist_core::repos::gitops;
use devassist_core::repos::state::{Action, MoveLine, Purpose, Repos};
use egui::text::LayoutJob;
use egui::{Align2, Color32, FontFamily, FontId, Galley, Id, Key, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::{
    Dialogs, Header, Parts, Plan, folder_suggestions, inset, list_height, main_id, most_scroll, remove, rows as count_rows, take_press, under_galley,
};
use crate::assets::{self, Icon};
use crate::overlay::{self, Kind};
use crate::theme::{
    ADD_FOLDER_GAP, BUTTON_HEIGHT, DIALOG_FIELD_HEIGHT, DIALOG_GAP, DIALOG_MARGIN, DIALOG_PAD, FOLDER_DIALOG_WIDTH, HAIRLINE, HEADER_LINE_HEIGHT,
    ICON, LINE_OPACITY, LIST_GAP, LIST_PAD, LIST_ROW, MOVE_LEAST_ROWS, MOVE_NOTE_LINE_HEIGHT, MOVE_NOTE_WIDTH, MOVE_ROWS, Palette, TEXT, TEXT_SMALL,
};
use crate::widgets::{self, font};

/// The button that closes the dialog.
const CANCEL: &str = "Cancel";

/// Import's words over the field.
const IMPORT_LABEL: &str = "Folder";
/// Import's title.
const IMPORT_TITLE: &str = "Import clones you already have";
/// Import's quiet line.
const IMPORT_LINE: &str = "Adds the GitHub clones in a folder to the overview. The folder itself counts too. Nothing on disk is changed.";
/// Import's main button.
const IMPORT_BUTTON: &str = "Import";

/// Move's words over the field.
const MOVE_LABEL: &str = "Move into";
/// Move's quiet line.
const MOVE_LINE: &str = "Each keeps its folder name, its group, the automatic pull and the saved pull requests.";
/// Move's hint under the field, in the quiet colour.
const MOVE_HINT: &str = "A folder that does not exist yet is created.";
/// Move's main button for one row that moves (and while none can).
const MOVE_BUTTON: &str = "Move";
/// The note of a row that is in the folder already, in the warning colour.
const THERE: &str = "is there already";
/// The note of a row whose move the core will refuse, in the warning colour.
const TAKEN: &str = "cannot move there";

/// What the dialog says for one purpose.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Words {
    /// The title.
    title: String,
    /// The quiet line under it.
    line: &'static str,
    /// The words over the field.
    label: &'static str,
    /// The main button's words.
    button: String,
    /// Whether the main button can be pressed.
    on: bool,
}

/// Move's title for the checkouts `repos` (row name and path): the remove dialog's rule
/// ("Move api and web to another folder", "Move 5 repositories to another folder").
fn move_title(repos: &[(String, std::path::PathBuf)]) -> String {
    let names: Vec<String> = repos.iter().map(|(name, _)| name.clone()).collect();
    format!("Move {} to another folder", remove::named(&names))
}

/// Move's main button when `moving` rows move: "Move" for one (and for none, when it is off), else
/// "Move {n} repositories".
fn move_button(moving: usize) -> String {
    if moving > 1 { format!("{MOVE_BUTTON} {moving} repositories") } else { MOVE_BUTTON.to_owned() }
}

/// The dialog's words for `purpose`, with `moving` rows of a move that will move; `None` for a purpose it does not
/// show (the core's own folder question of a clone, which Add stands for).
fn words(purpose: &Purpose, moving: usize) -> Option<Words> {
    match purpose {
        Purpose::Import => {
            Some(Words { title: IMPORT_TITLE.to_owned(), line: IMPORT_LINE, label: IMPORT_LABEL, button: IMPORT_BUTTON.to_owned(), on: true })
        }
        Purpose::Move(repos) => {
            Some(Words { title: move_title(repos), line: MOVE_LINE, label: MOVE_LABEL, button: move_button(moving), on: moving > 0 })
        }
        Purpose::Clone(_) => None,
    }
}

/// Whether the dialog shows the core's folder question for `purpose`.
pub(super) fn shown(purpose: &Purpose) -> bool {
    words(purpose, 0).is_some()
}

/// Whether `line`'s checkout moves: the folder is usable and it neither stays nor is refused.
fn moves(line: &MoveLine) -> bool {
    line.to.is_some() && !line.there && !line.taken
}

/// How many of `lines` move.
fn moving(lines: &[MoveLine]) -> usize {
    lines.iter().filter(|line| moves(line)).count()
}

/// The note at the right of `line`'s row, in the warning colour: one that stays where it is says so, one whose move
/// the core will refuse says that; none for the others.
fn note(line: &MoveLine) -> Option<&'static str> {
    if line.there {
        Some(THERE)
    } else if line.taken {
        Some(TAKEN)
    } else {
        None
    }
}

/// The drive the move takes the checkouts to, to name in the note: that of the first row that moves, when the core can
/// tell it from the path (`gitops::volume_root`: on Windows only).
fn target_root(lines: &[MoveLine]) -> Option<String> {
    lines.iter().filter(|line| moves(line)).find_map(|line| line.to.as_deref().and_then(gitops::volume_root))
}

/// The note on another drive, naming its `root` when the core can tell it ("E: is another drive."), else "That folder
/// is on another drive.": off Windows a mount point cannot be told from a path.
fn drive_note(root: Option<&str>) -> String {
    let first = root.map_or_else(|| "That folder is on another drive.".to_owned(), |root| format!("{root} is another drive."));
    format!(
        "{first} Everything is copied first, and the original is deleted only after the copy is complete. Close programs that have files of \
         these repositories open."
    )
}

/// How the line under the field is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tone {
    /// The quiet colour: a move's hint, or why the typed folder cannot be used yet.
    Quiet,
    /// The danger colour: the core refused the folder.
    Danger,
}

/// What stands under the field and how: the core's refusal `error` (until the text changes); for a move, why the
/// folder typed cannot be used (`reason`, the core's `path_reason`), else the hint; nothing under Import's field
/// otherwise.
fn under_words(error: Option<&str>, reason: Option<&str>, moving: bool) -> Option<(String, Tone)> {
    match (error, moving) {
        (Some(error), _) => Some((error.to_owned(), Tone::Danger)),
        (None, true) => Some((reason.unwrap_or(MOVE_HINT).to_owned(), Tone::Quiet)),
        (None, false) => None,
    }
}

/// The id of the folder field, which has the focus when the dialog opens.
pub(super) fn field_id() -> Id {
    Id::unique("dialog-folder-field")
}

/// The id of the Cancel button.
fn cancel_id() -> Id {
    Id::unique("dialog-folder-cancel")
}

/// What the folder dialog keeps of its own from one frame to the next, made when it opens ([`State::new`]).
#[derive(Clone, Debug, Default)]
pub(super) struct State {
    /// The field's text as it stood last: the core's, which the dialog follows when the core changes it.
    text: String,
    /// The folders that match it ([`folder_suggestions`]), asked for again only when it changes.
    suggestions: Vec<String>,
    /// For a move, each checkout and where it goes (`Repos::move_lines`), asked for again only when the text changes.
    lines: Vec<MoveLine>,
    /// For a move, whether it takes a checkout to another drive (`Repos::move_crosses_volumes`), asked for with them.
    crosses: Option<bool>,
    /// For a move, why the folder typed cannot be used (`Repos::path_reason`), asked for with them.
    reason: Option<String>,
}

impl State {
    /// The state of a folder dialog that opens on `repos`: the core's text, and what goes with it, asked for once (a
    /// text the dialog did not type never says it changed).
    pub(super) fn new(repos: &Repos) -> State {
        let mut state = State::default();
        state.follow(repos, repos.prompt.as_ref().map_or_default(|prompt| prompt.input.clone()));
        state
    }

    /// The text is `text` now, the core's: asks the disk again for the folders that match it and, for a move, where
    /// each checkout goes and whether it crosses to another drive.
    fn follow(&mut self, repos: &Repos, text: String) {
        self.suggestions = folder_suggestions(repos, &text);
        self.lines = repos.move_lines();
        self.crosses = if self.lines.is_empty() { None } else { repos.move_crosses_volumes() };
        // Only a move says why while typing; Import's line is the refusal alone.
        self.reason = if self.lines.is_empty() { None } else { repos.path_reason() };
        self.text = text;
    }

    /// The note on another drive, when the core can tell that the move crosses to one.
    fn other_drive(&self) -> Option<String> {
        if self.crosses == Some(true) { Some(drive_note(target_root(&self.lines).as_deref())) } else { None }
    }

    /// What stands under the field for `repos`'s question, and how ([`under_words`]).
    fn under(&self, repos: &Repos) -> Option<(String, Tone)> {
        let prompt = repos.prompt.as_ref()?;
        under_words(prompt.error.as_deref(), self.reason.as_deref(), matches!(prompt.purpose, Purpose::Move(_)))
    }
}

/// The words of the folder dialog's buttons in `repos`, from the left.
#[cfg(test)]
pub(super) fn said(repos: &Repos) -> Vec<String> {
    let moving = moving(&repos.move_lines());
    repos.prompt.as_ref().and_then(|prompt| words(&prompt.purpose, moving)).map_or_default(|words| vec![CANCEL.to_owned(), words.button])
}

/// The line under the field at `places` while it says why the folder cannot be used for `repos` (the core's refusal,
/// or a move's reason while typing, as `state` has it); not while it shows a move's hint.
#[cfg(test)]
pub(super) fn reason_box(places: Places, repos: &Repos, state: &State) -> Option<Rect> {
    let refused = repos.prompt.as_ref().is_some_and(|prompt| prompt.error.is_some());
    if refused || state.reason.is_some() { places.note.or(places.hint) } else { None }
}

/// The parts of the folder dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The words over the field.
    pub(super) label: Rect,
    /// The folder field with the room its rows take.
    pub(super) field: Rect,
    /// Import: the core's reason under the field, when it refused the folder.
    pub(super) note: Option<Rect>,
    /// Move: the hint under the field, or the core's reason in its place.
    pub(super) hint: Option<Rect>,
    /// Move: the box of the rows, one a checkout.
    pub(super) list: Option<Rect>,
    /// Move: the note on another drive, when the core can tell that the move crosses to one.
    pub(super) drive: Option<Rect>,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// What stands under the folder field besides its rows.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Under {
    /// Import: the core's reason, `refused` points tall, when it refused the folder.
    Import {
        /// How tall the reason is, when it refused the folder.
        refused: Option<f32>,
    },
    /// Move: the line under the field, `below` points tall, a box of `lines` rows, and the note on another drive,
    /// `drive` points tall, when it shows.
    Move {
        /// How tall the line under the field is.
        below: f32,
        /// How many checkouts move.
        lines: usize,
        /// How tall the note on another drive is, when it shows.
        drive: Option<f32>,
    },
}

/// The height of a line of `size`-point text.
fn line_height(size: f32) -> f32 {
    size * HEADER_LINE_HEIGHT
}

/// Where the folder dialog's parts are in `window`, with a header `header` points tall, the field showing `rows`
/// suggestions, and `under` under it. The dialog grows with the rows, as far as the window lets it. A move's box
/// takes what the window leaves, at least [`MOVE_LEAST_ROWS`] rows (all, when fewer) and at most [`MOVE_ROWS`]; the
/// field's rows give up their room before that, and the buttons stay inside the pane.
fn folder_places(window: Rect, header: f32, rows: usize, under: Under) -> Places {
    let small = match under {
        Under::Import { refused } => refused.unwrap_or_default(),
        Under::Move { below, .. } => below,
    };
    let (under_field, list_want, list_least, drive_want) = match under {
        Under::Import { refused } => (refused.map_or(0.0, |refused| ADD_FOLDER_GAP + refused), 0.0, 0.0, 0.0),
        Under::Move { below, lines, drive } => (
            ADD_FOLDER_GAP + below,
            DIALOG_GAP + list_height(lines.min(MOVE_ROWS)),
            DIALOG_GAP + list_height(lines.min(MOVE_LEAST_ROWS)),
            drive.map_or(0.0, |drive| DIALOG_GAP + drive),
        ),
    };
    let fixed = 2.0 * DIALOG_PAD
        + header
        + DIALOG_GAP
        + line_height(TEXT)
        + ADD_FOLDER_GAP
        + DIALOG_FIELD_HEIGHT
        + under_field
        + drive_want
        + DIALOG_GAP
        + BUTTON_HEIGHT;
    let spare = (window.height() - 2.0 * DIALOG_MARGIN - fixed).max(0.0);
    let rows_want = widgets::folder_field_height(rows) - DIALOG_FIELD_HEIGHT;
    let rows_height = rows_want.min((spare - list_least).max(0.0));
    let list_room = list_want.min(spare - rows_height);
    let pane = overlay::dialog_rect(window, vec2(FOLDER_DIALOG_WIDTH, fixed + rows_height + list_room));
    let inside = pane.shrink(DIALOG_PAD);
    let at = |top: f32, height: f32| Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), height));
    let header = at(inside.top(), header);
    let label = at(header.bottom() + DIALOG_GAP, line_height(TEXT));
    let field = at(label.bottom() + ADD_FOLDER_GAP, DIALOG_FIELD_HEIGHT + rows_height);
    let below = at(field.bottom() + ADD_FOLDER_GAP, small);
    let (note, hint) = match under {
        Under::Import { refused } => (refused.map(|_| below), None),
        Under::Move { .. } => (None, Some(below)),
    };
    let (list, drive) = match under {
        Under::Import { .. } => (None, None),
        Under::Move { drive, .. } => {
            let list = at(below.bottom() + DIALOG_GAP, (list_room - DIALOG_GAP).max(0.0));
            (Some(list), drive.map(|drive| at(list.bottom() + DIALOG_GAP, drive)))
        }
    };
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    Places { pane, header, label, field, note, hint, list, drive, buttons }
}

/// The note on another drive naming `root`, laid out with `painter`'s faces: quiet 12-point text that wraps at
/// [`MOVE_NOTE_WIDTH`], at the design's line height.
fn drive_galley(painter: &Painter, note: &str) -> Arc<Galley> {
    let mut job = LayoutJob::simple(note.to_owned(), font(TEXT_SMALL, FontFamily::Proportional), Color32::PLACEHOLDER, MOVE_NOTE_WIDTH);
    for section in &mut job.sections {
        section.format.line_height = Some(TEXT_SMALL * MOVE_NOTE_LINE_HEIGHT);
    }
    painter.layout_job(job)
}

/// Where the folder dialog of `repos` stands in `window`, its header laid out with `painter`'s faces, its field and
/// a move's rows as `state` has them; `None` without a folder question the dialog shows.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos, state: &State) -> Option<Plan> {
    let prompt = repos.prompt.as_ref()?;
    let words = words(&prompt.purpose, moving(&state.lines))?;
    let header = Header::new(painter, &words.title, words.line, FOLDER_DIALOG_WIDTH - 2.0 * DIALOG_PAD);
    let rows = widgets::folder_field_rows(painter.ctx(), field_id(), state.suggestions.len());
    let width = FOLDER_DIALOG_WIDTH - 2.0 * DIALOG_PAD;
    let below = state.under(repos).map(|(words, _)| under_galley(painter, &words, width).size().y);
    let under = match prompt.purpose {
        Purpose::Move(_) => Under::Move {
            below: below.unwrap_or_else(|| line_height(TEXT_SMALL)),
            lines: state.lines.len(),
            drive: state.other_drive().map(|note| drive_galley(painter, &note).size().y),
        },
        _ => Under::Import { refused: below },
    };
    let places = folder_places(window, header.height(), rows, under);
    Some(Plan::dialog(header, Parts::Folder(places)))
}

/// What a person did in the folder dialog in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The text typed, or a suggestion taken, when it changed.
    typed: Option<String>,
    /// The main button, or Enter in the field.
    accept: bool,
    /// Cancel, Esc or the scrim.
    close: bool,
}

/// The folder dialog at `places`, with `header`, for `repos`'s folder question, on the overlay in `ui`; `scrim` is
/// whether the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(mut state) = dialogs.folder.take() else { return };
    let Some((input, error)) = repos.prompt.as_ref().map(|prompt| (prompt.input.clone(), prompt.error.clone())) else {
        dialogs.folder = Some(state);
        return;
    };
    if input != state.text {
        // The core changed the text: the field follows it, typing goes on at its end, and what goes with it is asked for.
        widgets::put_cursor_at_end(ui.ctx(), field_id(), &input);
        state.follow(repos, input);
    }
    let Some(words) = repos.prompt.as_ref().and_then(|prompt| words(&prompt.purpose, moving(&state.lines))) else {
        dialogs.folder = Some(state);
        return;
    };
    let mut ids = vec![field_id(), cancel_id()];
    if words.on {
        ids.push(main_id());
    }
    overlay::walk(ui, &ids);
    header.draw(ui, places.header, palette);
    ui.painter().text(places.label.left_center(), Align2::LEFT_CENTER, words.label, font(TEXT, assets::medium_family()), palette.on_glass);

    // The field first: it takes Esc, Enter, Up and Down while its rows show; the dialog's own come after.
    let focused = ui.memory(|memory| memory.has_focus(field_id()));
    let mut text = state.text.clone();
    let did = widgets::folder_field(ui, places.field, &mut text, &state.suggestions, palette, field_id());
    let mut done = Done { close: scrim, ..Done::default() };
    if did.changed {
        done.typed = Some(text);
    }
    if focused {
        done.accept |= take_press(ui, Key::Enter, None);
    }
    done.close |= take_press(ui, Key::Escape, None);

    // Under the field: the core's refusal while it stands (until the text changes), else for a move why the folder
    // cannot be used, or the hint; wrapped, never cut.
    let error = error.filter(|_| !did.changed);
    let moving_purpose = places.hint.is_some();
    if let (Some(rect), Some((words, tone))) = (places.note.or(places.hint), under_words(error.as_deref(), state.reason.as_deref(), moving_purpose)) {
        let colour = if tone == Tone::Danger { palette.danger } else { palette.quiet };
        ui.painter().galley(rect.min, under_galley(ui.painter(), &words, rect.width()), colour);
    }
    if let Some(rect) = places.list {
        list(ui, rect, &state.lines, palette, dialogs);
    }
    if let (Some(rect), Some(note)) = (places.drive, state.other_drive()) {
        let note = drive_galley(ui.painter(), &note);
        let painter = ui.painter();
        let first_line = TEXT_SMALL * MOVE_NOTE_LINE_HEIGHT;
        widgets::draw_icon(painter, pos2(rect.left() + ICON / 2.0, rect.top() + first_line / 2.0), Icon::HardDrive, ICON, palette.quiet);
        painter.galley(pos2(rect.left() + ICON + LIST_GAP, rect.top()), note, palette.quiet);
    }

    let widths = [widgets::button_width(ui, CANCEL), widgets::button_width(ui, &words.button)];
    if let [cancel, main] = overlay::buttons(places.buttons, &widths).as_slice() {
        done.close |= widgets::button(ui, *cancel, CANCEL, Kind::Plain, palette, cancel_id());
        if words.on {
            done.accept |= widgets::button(ui, *main, &words.button, Kind::Main, palette, main_id());
        } else {
            widgets::button_off(ui, *main, &words.button, palette);
        }
    }
    done.accept &= words.on;
    act(ui, repos, &mut state, &done);
    dialogs.folder = Some(state);
}

/// Draws a move's `lines` in the box at `rect`, scrolled by the wheel: for each, the folder's icon, where it is, an
/// arrow and where it goes (each cut from the left to show its last folder, the whole path its hover text), the note
/// of one that stays at the right; a line between two.
fn list(ui: &Ui, rect: Rect, lines: &[MoveLine], palette: Palette, dialogs: &mut Dialogs) {
    let inset = inset(ui, rect, palette);
    dialogs.list_scroll = (dialogs.list_scroll - inset.wheel).clamp(0.0, most_scroll(lines.len(), rect.height()));
    let clip = rect.intersect(ui.clip_rect());
    let regular = font(TEXT, FontFamily::Proportional);
    let medium = font(TEXT, assets::medium_family());
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let painter = &inset.painter;
    for (index, line) in lines.iter().enumerate() {
        let top = rect.top() + LIST_PAD[0] + count_rows(index) * LIST_ROW - dialogs.list_scroll;
        let row = Rect::from_min_max(pos2(rect.left() + LIST_PAD[1], top), pos2(rect.right() - LIST_PAD[1], top + LIST_ROW));
        if !row.intersects(rect) {
            continue;
        }
        if index + 1 != lines.len() {
            painter.hline(row.x_range(), row.bottom() - HAIRLINE / 2.0, Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(LINE_OPACITY)));
        }
        let y = row.center().y;
        widgets::draw_icon(painter, pos2(row.left() + ICON / 2.0, y), Icon::FolderGit2, ICON, palette.quiet);
        let mut right = row.right();
        if let Some(words) = note(line) {
            painter.text(pos2(right, y), Align2::RIGHT_CENTER, words, small.clone(), palette.warning);
            right -= widgets::width(painter, words, &small) + LIST_GAP;
        }
        let left = row.left() + ICON + LIST_GAP;
        let half = ((right - left - ICON - 2.0 * LIST_GAP) / 2.0).max(0.0);
        let from = Rect::from_min_max(pos2(left, row.top()), pos2(left + half, row.bottom()));
        let to = Rect::from_min_max(pos2(from.right() + 2.0 * LIST_GAP + ICON, row.top()), pos2(right, row.bottom()));
        path_cell(ui, painter, from, clip, &line.from.display().to_string(), &regular, palette.on_glass, Id::unique(("dialog-move-from", index)));
        widgets::draw_icon(painter, pos2(from.right() + LIST_GAP + ICON / 2.0, y), Icon::ArrowRight, ICON, palette.quiet);
        // While the folder is not usable yet: the name it keeps, quiet.
        let (target, colour) = line.to.as_ref().map_or_else(
            || (line.from.file_name().map_or_default(|name| name.to_string_lossy().into_owned()), palette.quiet),
            |target| (target.display().to_string(), palette.on_glass),
        );
        path_cell(ui, painter, to, clip, &target, &medium, colour, Id::unique(("dialog-move-to", index)));
    }
}

/// Draws `path` at the left of `cell` with `painter`, in `face` and `colour`, cut from the left on a separator so its
/// last folder shows; when cut, the whole path is the hover text of the part of `cell` inside `clip`, under `id`.
#[expect(clippy::too_many_arguments, reason = "the cell, its clip, the path, its face and colour, and the hover's id")]
fn path_cell(ui: &Ui, painter: &Painter, cell: Rect, clip: Rect, path: &str, face: &FontId, colour: Color32, id: Id) {
    let words = widgets::path_tail(path, face, cell.width(), ui);
    painter.text(cell.left_center(), Align2::LEFT_CENTER, &words, face.clone(), colour);
    let seen = cell.intersect(clip);
    if words != path && seen.is_positive() {
        ui.interact(seen, id, Sense::hover()).on_hover_text(path);
    }
}

/// Does in `repos` what a person did in the folder dialog in one frame; keeps the dialog's own `state`.
fn act(ui: &Ui, repos: &mut Repos, state: &mut State, done: &Done) {
    if done.close {
        repos.close_path();
        return;
    }
    if let Some(text) = &done.typed {
        repos.set_path_input(text);
        state.follow(repos, text.clone());
    }
    if done.accept {
        repos.run(Action::Confirm);
        if repos.prompt.is_some() {
            // Refused: the reason stands under the field in the next frame, and the field has the keys again.
            ui.memory_mut(|memory| memory.request_focus(field_id()));
        }
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use devassist_core::config::Settings;
    use devassist_core::repos::state::testing::{hold_github, installed, test_settings};
    use devassist_core::repos::state::{HELD, MigrateItem, Mode as CoreMode};
    use egui::{Event, Memory, Modifiers, Pos2};

    use super::super::Which;
    use super::super::testing::{Temp, Window, behind, git, until, wheel};
    use super::*;
    use crate::session::Session;
    use crate::theme::{MIN_WINDOW, WINDOW};

    /// The folder dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Folder(places) = window.parts() else { panic!("not the folder dialog") };
        places
    }

    fn state(window: &Window) -> &State {
        window.dialogs.folder.as_ref().unwrap()
    }

    fn key(key: Key, pressed: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    /// A press of `code` typing `text`, and its release.
    fn tap(window: &mut Window, code: Key, text: Option<&str>) {
        let mut press = vec![key(code, true, Modifiers::NONE)];
        press.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(press);
        window.frame(vec![key(code, false, Modifiers::NONE)]);
    }

    /// Ctrl-A in the field, then `text` typed over it.
    fn type_over(window: &mut Window, text: &str) {
        window.frame(vec![key(Key::A, true, Modifiers::COMMAND)]);
        window.frame(vec![key(Key::A, false, Modifiers::COMMAND), Event::Text(text.to_owned())]);
        window.frame(Vec::new());
    }

    /// A temporary folder `name` with two clones in it whose origins name GitHub (nothing is fetched from them) and a
    /// folder that is no clone, and the settings, kept inside the temporary folder, of a session without repositories.
    fn two_clones(name: &str) -> (Temp, std::path::PathBuf, devassist_core::config::Settings) {
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-{name}-{}", std::process::id())));
        let _ = std::fs::remove_dir_all(&root.0);
        let code = root.0.join("code");
        for repo in ["alpha", "beta"] {
            let dir = code.join(repo);
            std::fs::create_dir_all(&dir).unwrap();
            git(&dir, &["init", "-q", "-b", "main"]);
            std::fs::write(dir.join("readme"), repo).unwrap();
            git(&dir, &["add", "readme"]);
            git(&dir, &["commit", "-q", "-m", "one"]);
            git(&dir, &["remote", "add", "origin", &format!("https://github.com/acme/{repo}.git")]);
        }
        std::fs::create_dir_all(code.join("notes")).unwrap();
        let settings = devassist_core::config::Settings {
            config_path: root.0.join("devassist.toml"),
            clone_dir: root.0.clone(),
            owners: Vec::new(),
            ..test_settings()
        };
        (root, code, settings)
    }

    #[test]
    fn import_adds_the_clones_in_a_folder_and_says_how_many() {
        let _live = crate::session::tests::live_git();
        let (root, code, settings) = two_clones("import");
        let mut repos = Repos::offline(settings);
        // The reload after the import asks GitHub nothing: no test reaches the network.
        hold_github(&mut repos);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        // i opens Import, its field at the clone folder and with the keys; the i is not typed into it.
        tap(&mut window, Key::I, Some("i"));
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder), "{}", window.repos().status);
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        assert_eq!(state(&window).text, root.0.display().to_string(), "the i was typed");
        assert_eq!(window.said(), ["Cancel", "Import"]);
        type_over(&mut window, &code.display().to_string());
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, code.display().to_string(), "the text is the core's");
        tap(&mut window, Key::Enter, None);
        assert!(!window.open(), "Import stayed open: {:?}", window.repos().prompt.as_ref().and_then(|prompt| prompt.error.clone()));
        assert_eq!(window.repos().status, format!("Added 2 repositories from {}.", code.display()));
        until(&mut window, "read", |window| !window.repos().waiting());
        let mut folders: Vec<&str> = window.repos().installed.iter().map(|repo| repo.folder.as_str()).collect();
        folders.sort_unstable();
        assert_eq!(folders, ["alpha", "beta"]);
        assert!(root.0.join("devassist.toml").is_file(), "the settings were not saved in the temporary folder");
        // Import again, by its button: nothing new, in the core's words.
        tap(&mut window, Key::I, Some("i"));
        window.frame(Vec::new());
        type_over(&mut window, &code.display().to_string());
        window.press("Import");
        assert_eq!(window.repos().status, format!("Everything in {} is on the list already.", code.display()));
        window.session.close();
    }

    #[test]
    fn a_folder_that_is_a_file_is_refused_in_the_cores_words() {
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-import-file-{}", std::process::id())));
        std::fs::create_dir_all(&root.0).unwrap();
        let file = root.0.join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        // A held demo checks the text alone: it closes on Import, refusing the work, and looks at nothing.
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        let reads = super::super::disk_reads();
        type_over(&mut window, &file.display().to_string());
        tap(&mut window, Key::Enter, None);
        assert!(!window.open() && window.repos().status == HELD, "the held Import did not refuse the work");
        assert_eq!(super::super::disk_reads(), reads, "the held Import looked at the folder");
        // A live session looks, and refuses the file in the core's words.
        let mut window = live_import();
        assert!(window.shows(Which::Folder));
        assert_eq!(parts(&window).note, None, "nothing refused yet");
        type_over(&mut window, &file.display().to_string());
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "Import closed on a file");
        let reason = window.repos().prompt.as_ref().unwrap().error.clone().unwrap();
        assert_eq!(reason, format!("{} is a file, not a folder.", file.display()));
        let note = parts(&window).note.expect("the reason under the field");
        assert!(note.top() >= parts(&window).field.bottom() && note.bottom() <= parts(&window).buttons.top());
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "the field keeps the keys");
        assert_ne!(window.repos().status, HELD, "the refusal is no held work");
        // The button says it too; typing takes the reason away.
        window.press("Import");
        assert!(window.shows(Which::Folder) && window.repos().prompt.as_ref().unwrap().error.is_some());
        let field = parts(&window).field;
        window.click(Pos2::new(field.center().x, field.top() + DIALOG_FIELD_HEIGHT / 2.0));
        tap(&mut window, Key::X, Some("x"));
        assert_eq!(window.repos().prompt.as_ref().unwrap().error, None);
        window.frame(Vec::new());
        assert_eq!(parts(&window).note, None);
        // Esc closes it through the core.
        window.key(Key::Escape);
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    #[test]
    fn a_held_import_of_a_real_folder_changes_nothing() {
        // The held core checks the folder by its text alone, then refuses the work and says so.
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let before = repos.checkouts.clone();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        type_over(&mut window, &std::env::temp_dir().display().to_string());
        window.press("Import");
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
        assert_eq!(window.repos().checkouts, before);
        assert!(!window.repos().config_path.exists());
    }

    #[test]
    fn the_dialog_fits_every_window_with_its_rows() {
        for size in [MIN_WINDOW, WINDOW] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for (rows, note) in [(0, false), (0, true), (6, false), (6, true)] {
                let places = folder_places(window, 60.0, rows, Under::Import { refused: note.then_some(17.4) });
                assert!(window.contains_rect(places.pane), "{size:?} {rows} {note}");
                assert!((places.pane.width() - FOLDER_DIALOG_WIDTH).abs() < 1e-3);
                for part in [places.header, places.label, places.field, places.buttons] {
                    assert!(places.pane.contains_rect(part), "{size:?} {rows} {note}: {part:?}");
                }
                assert!(places.note.is_some() == note && places.note.is_none_or(|rect| rect.bottom() <= places.buttons.top()));
                assert!(places.field.height() >= DIALOG_FIELD_HEIGHT);
            }
        }
        // The dialog grows with the rows.
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
        let import = Under::Import { refused: None };
        assert!(folder_places(window, 60.0, 3, import).pane.height() > folder_places(window, 60.0, 0, import).pane.height());
    }

    #[test]
    fn the_demos_import_suggests_its_own_folders_and_never_asks_the_disk() {
        let before = super::super::disk_reads();
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "import").unwrap();
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        assert_eq!(state(&window).text, crate::demo::CODE_DIR);
        type_over(&mut window, &format!("{}{}", crate::demo::CODE_DIR, std::path::MAIN_SEPARATOR));
        assert_eq!(state(&window).suggestions.len(), 4, "{:?}", state(&window).suggestions);
        assert_eq!(super::super::disk_reads(), before, "the held demo read the disk for folders");
        // A suggestion taken by Enter is the core's text too, and imports nothing.
        tap(&mut window, Key::ArrowDown, None);
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "the Enter that took a row went on to import");
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, state(&window).text);
        assert!(state(&window).text.ends_with(std::path::MAIN_SEPARATOR));
    }

    #[test]
    fn an_enter_held_into_the_first_frame_does_not_press_the_main_button() {
        for (name, main) in [("import", "Import"), ("move", "Move 2 repositories")] {
            let mut repos = crate::demo::repos();
            crate::demo::show(&mut repos, name).unwrap();
            let mut window = Window::new(Session::demo(repos));
            let enter = |pressed, repeat| Event::Key { key: Key::Enter, physical_key: None, pressed, repeat, modifiers: Modifiers::NONE };
            window.frame(vec![enter(true, false)]);
            for _ in 0..3 {
                window.frame(vec![enter(true, true)]);
            }
            assert!(window.shows(Which::Folder), "{name}: the Enter that opened it pressed {main}");
            assert_ne!(window.repos().status, HELD, "{name}");
            assert_eq!(window.said(), ["Cancel", main]);
            window.frame(vec![enter(false, false)]);
            // Tab from the field goes to Cancel, then the main button.
            window.key(Key::Tab);
            assert_eq!(window.ctx.memory(Memory::focused), Some(cancel_id()), "{name}");
            window.key(Key::Tab);
            assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()), "{name}");
        }
    }

    #[test]
    fn importing_a_folder_without_clones_closes_and_says_so_in_the_cores_words() {
        let _live = crate::session::tests::live_git();
        let (root, _code, settings) = two_clones("import-none");
        let empty = root.0.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        let mut repos = Repos::offline(settings);
        hold_github(&mut repos);
        repos.run(Action::Import);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder));
        type_over(&mut window, &empty.display().to_string());
        tap(&mut window, Key::Enter, None);
        assert!(!window.open(), "Import stayed open: {:?}", window.repos().prompt.as_ref().and_then(|prompt| prompt.error.clone()));
        assert_eq!(window.repos().status, format!("No GitHub clones in {}.", empty.display()));
        assert!(window.repos().installed.is_empty() && window.repos().checkouts.is_empty());
        window.session.close();
    }

    /// The demo with the Move dialog open for its repositories `marked`, as the Migrate menu's move line opens it,
    /// its folder `text`; one frame drawn.
    fn move_dialog(marked: &[&str], text: &str) -> Window {
        let mut repos = crate::demo::repos();
        repos.marked = marked.iter().map(|name| (*name).to_owned()).collect();
        repos.run_migrate(MigrateItem::Folder);
        repos.set_path_input(text);
        let mut window = Window::new(Session::demo(repos));
        window.frame(Vec::new());
        window
    }

    /// Where each row of the Move dialog goes, as the dialog keeps them.
    fn targets(window: &Window) -> Vec<Option<PathBuf>> {
        state(window).lines.iter().map(|line| line.to.clone()).collect()
    }

    /// The demo's folder of its Work group, where api and web are.
    const WORK: &str = r"D:\code\work";
    /// A folder on another drive than the demo's.
    const OTHER: &str = r"E:\work";

    #[test]
    #[cfg_attr(not(windows), ignore = "the demo's checkouts and these folders are Windows paths (drives D: and E:)")]
    fn the_rows_say_where_each_goes_as_the_folder_is_typed() {
        // The title follows the remove dialog's rule.
        let one = |name: &str| (name.to_owned(), PathBuf::from(WORK).join(name));
        assert_eq!(move_title(&[one("api")]), "Move api to another folder");
        assert_eq!(move_title(&[one("api"), one("web")]), "Move api and web to another folder");
        assert_eq!(move_title(&["a", "b", "c", "d", "e"].map(one)), "Move 5 repositories to another folder");
        assert_eq!(
            (move_button(1), move_button(2), move_button(12)),
            ("Move".to_owned(), "Move 2 repositories".to_owned(), "Move 12 repositories".to_owned())
        );

        // It opens where api and web are: both are there already, and nothing can be moved.
        let mut window = move_dialog(&["api", "web"], WORK);
        assert!(window.shows(Which::Folder));
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()));
        assert!(state(&window).lines.iter().all(|line| line.there));
        assert_eq!(window.said(), ["Cancel", "Move"]);
        let prompt = window.repos().prompt.as_ref().unwrap();
        assert_eq!(words(&prompt.purpose, 0).unwrap().title, "Move api and web to another folder");
        assert!(!words(&prompt.purpose, 0).unwrap().on, "the main button is off while nothing moves");
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "Enter moved what cannot move");
        assert_ne!(window.repos().status, HELD);

        // Another folder: each row says where it goes, and the button counts them.
        type_over(&mut window, OTHER);
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, OTHER, "the text is the core's");
        assert_eq!(targets(&window), [Some(PathBuf::from(OTHER).join("api")), Some(PathBuf::from(OTHER).join("web"))]);
        assert_eq!(window.said(), ["Cancel", "Move 2 repositories"]);
        // A folder that is no full path: nowhere to go yet, and the button is off.
        type_over(&mut window, "dev");
        assert_eq!(targets(&window), [None, None]);
        assert_eq!(window.said(), ["Cancel", "Move"]);
        let main = window.buttons()[1].0;
        window.click(main.center());
        assert!(window.shows(Which::Folder), "the button that is off moved");
        // Back to the other drive, and the button: the held demo refuses the work and says so; the ticks stay.
        let field = parts(&window).field;
        window.click(Pos2::new(field.center().x, field.top() + DIALOG_FIELD_HEIGHT / 2.0));
        type_over(&mut window, OTHER);
        window.press("Move 2 repositories");
        assert!(!window.open());
        assert_eq!(window.repos().status, HELD);
        assert_eq!(window.repos().marked, ["api", "web"]);
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "the demo's checkouts and these folders are Windows paths (drives D: and E:)")]
    fn a_row_that_is_there_already_says_so_and_does_not_count() {
        // api is in the Work folder already; notes moves there.
        let mut window = move_dialog(&["api", "notes"], WORK);
        let lines = state(&window).lines.clone();
        assert_eq!((lines[0].name.as_str(), lines[0].there), ("api", true));
        assert_eq!((note(&lines[0]), note(&lines[1])), (Some("is there already"), None));
        assert_eq!(targets(&window)[1], Some(PathBuf::from(WORK).join("notes")));
        assert_eq!(window.said(), ["Cancel", "Move"], "the one that moves is counted, not the one that stays");
        // Into api itself: the core refuses that one, and the other still moves.
        type_over(&mut window, &format!(r"{WORK}\api"));
        let lines = state(&window).lines.clone();
        assert!(lines[0].taken && !lines[0].there);
        assert_eq!((note(&lines[0]), note(&lines[1])), (Some("cannot move there"), None));
        assert_eq!(window.said(), ["Cancel", "Move"]);
        // When every row stays, the button is off and Enter does nothing.
        let mut window = move_dialog(&["api", "web", "portal"], WORK);
        assert_eq!(window.said(), ["Cancel", "Move"]);
        assert_eq!(moving(&state(&window).lines), 0);
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder));
        assert_ne!(window.repos().status, HELD);
    }

    /// A live session that reads no folder of its own and calls no network (`Repos::offline` on the test settings),
    /// with the Import dialog open; one frame drawn.
    fn live_import() -> Window {
        let mut repos = Repos::offline(test_settings());
        repos.open_import();
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        window
    }

    /// A live session as [`live_import`] with two checkouts, api and web, in folders of `root`, both ticked and the
    /// Move dialog open for them; one frame drawn.
    fn live_move(root: &Path) -> Window {
        let mut repos = Repos::offline(test_settings());
        for name in ["api", "web"] {
            let mut repo = installed("acme", name);
            repo.path = root.join("here").join(name);
            std::fs::create_dir_all(&repo.path).unwrap();
            repos.checkouts.push(repo.path.clone());
            repos.installed.push(repo);
        }
        repos.marked = vec!["api".to_owned(), "web".to_owned()];
        repos.run_migrate(MigrateItem::Folder);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        window
    }

    #[test]
    fn a_move_into_a_file_is_refused_in_the_cores_words() {
        // The held demo tells where each goes by the text alone, and its Move refuses the work without a look.
        let file = std::env::current_exe().unwrap();
        let mut window = move_dialog(&["api", "web"], WORK);
        let reads = super::super::disk_reads();
        type_over(&mut window, &file.display().to_string());
        assert_eq!(window.said(), ["Cancel", "Move 2 repositories"]);
        window.press("Move 2 repositories");
        assert!(!window.open() && window.repos().status == HELD, "the held move did not refuse the work");
        assert_eq!(super::super::disk_reads(), reads, "the held move looked at the folder");
        // A live session looks as the text changes: the core's reason stands under the field, and nothing moves.
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-move-file-{}", std::process::id())));
        let mut window = live_move(&root.0);
        type_over(&mut window, &file.display().to_string());
        let said = format!("{} is a file, not a folder.", file.display());
        assert_eq!(window.repos().path_reason(), Some(said.clone()));
        let places = parts(&window);
        let hint = places.hint.expect("the line under the field");
        assert!(hint.top() >= places.field.bottom() && hint.bottom() <= places.list.unwrap().top());
        assert!(
            window.texts.iter().any(|(words, rect, cut)| *words == said && !cut && hint.contains_rect(*rect)),
            "the reason is not under the field"
        );
        assert_eq!(window.said(), ["Cancel", "Move"], "nothing can move into a file");
        tap(&mut window, Key::Enter, None);
        assert!(window.shows(Which::Folder), "the move went on into a file");
        assert_ne!(window.repos().status, HELD, "the refusal is no held work");
        assert_eq!(window.ctx.memory(Memory::focused), Some(field_id()), "the field keeps the keys");
        window.session.close();
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "the demo's checkouts and these folders are Windows paths (drives D: and E:)")]
    fn the_other_drive_note_shows_only_when_the_core_can_tell() {
        assert_eq!(
            drive_note(Some("E:")),
            "E: is another drive. Everything is copied first, and the original is deleted only after the copy is complete. Close programs that \
             have files of these repositories open."
        );
        let mut window = move_dialog(&["api", "web"], OTHER);
        assert_eq!(state(&window).crosses, Some(true));
        assert!(state(&window).other_drive().is_some_and(|note| note.starts_with("E: is another drive.")));
        let places = parts(&window);
        let (list, drive) = (places.list.unwrap(), places.drive.expect("the note on another drive"));
        assert!(drive.top() >= list.bottom() && drive.bottom() <= places.buttons.top() && places.pane.contains_rect(drive));
        let tall = places.pane.height();
        // The same drive: no note, and the dialog is shorter by it.
        type_over(&mut window, r"D:\elsewhere");
        assert_eq!(state(&window).crosses, Some(false));
        assert_eq!(parts(&window).drive, None);
        assert!(parts(&window).pane.height() < tall);
        // A folder the core cannot judge: no note either.
        type_over(&mut window, "dev");
        assert_eq!(state(&window).crosses, None);
        assert_eq!(parts(&window).drive, None);
        // Rows that stay do not count: all of them there, no note.
        let window = move_dialog(&["api", "web"], WORK);
        assert_eq!((state(&window).crosses, parts(&window).drive), (Some(false), None));
    }

    #[test]
    fn the_note_names_no_drive_it_cannot_tell() {
        assert_eq!(
            drive_note(None),
            "That folder is on another drive. Everything is copied first, and the original is deleted only after the copy is complete. Close \
             programs that have files of these repositories open."
        );
        // A target whose drive the core cannot tell from its text (a path without a drive, as every path off Windows).
        let line = MoveLine {
            name: "api".to_owned(),
            from: PathBuf::from(r"D:\code\api"),
            to: Some(PathBuf::from("/mnt/work/api")),
            there: false,
            taken: false,
        };
        let state = State { lines: vec![line], crosses: Some(true), ..State::default() };
        assert!(state.other_drive().is_some_and(|note| note.starts_with("That folder is on another drive.")), "{:?}", state.other_drive());
    }

    #[test]
    fn a_folder_that_cannot_be_used_says_why_while_typing() {
        // A live session (not held): the core looks at the disk.
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-move-why-{}", std::process::id())));
        let code = root.0.join("code");
        let mut repos = Repos::offline(Settings { config_path: root.0.join("devassist.toml"), ..test_settings() });
        for name in ["api", "web"] {
            let mut repo = installed("acme", name);
            repo.folder = name.to_owned();
            repo.path = code.join(name);
            std::fs::create_dir_all(&repo.path).unwrap();
            repos.checkouts.push(repo.path.clone());
            repos.marked.push(name.to_owned());
            repos.installed.push(repo);
        }
        let file = root.0.join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        repos.run_migrate(MigrateItem::Folder);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder));
        let under = |window: &Window| state(window).under(window.repos());
        // Where they are: the hint.
        assert_eq!(under(&window), Some((MOVE_HINT.to_owned(), Tone::Quiet)));
        // A file: nothing can move, and the line under the field says why, quietly; nothing was submitted.
        type_over(&mut window, &file.display().to_string());
        assert_eq!(targets(&window), [None, None]);
        assert_eq!(window.said(), ["Cancel", "Move"]);
        assert_eq!(under(&window), Some((format!("{} is a file, not a folder.", file.display()), Tone::Quiet)));
        assert_eq!(window.repos().prompt.as_ref().unwrap().error, None);
        // No full path: the core's words for that.
        type_over(&mut window, "dev");
        assert!(under(&window).is_some_and(|(words, tone)| words.starts_with("Use a full path") && tone == Tone::Quiet), "{:?}", under(&window));
        // A usable folder: the hint again.
        type_over(&mut window, &root.0.join("moved").display().to_string());
        assert_eq!(under(&window), Some((MOVE_HINT.to_owned(), Tone::Quiet)));
        assert_eq!(window.said(), ["Cancel", "Move 2 repositories"]);
        // Import says nothing while typing: its line is the refusal only.
        let state = State { reason: Some("why".to_owned()), ..State::default() };
        assert_eq!(under_words(None, state.reason.as_deref(), false), None);
        assert_eq!(under_words(Some("no"), Some("why"), true), Some(("no".to_owned(), Tone::Danger)), "a refusal stands over the reason");
    }

    #[test]
    fn a_long_reason_wraps_and_keeps_its_end() {
        let file = std::env::current_exe().unwrap();
        let said = format!("{} is a file, not a folder.", file.display());
        let root = Temp(std::env::temp_dir().join(format!("devassist-gui-long-reason-{}", std::process::id())));
        for name in ["import", "move"] {
            // Live: a held session checks the text alone, and gives no reason a file would.
            let mut window = if name == "import" { live_import() } else { live_move(&root.0) };
            window.size = MIN_WINDOW;
            window.frame(Vec::new());
            type_over(&mut window, &file.display().to_string());
            tap(&mut window, Key::Enter, None);
            assert!(window.shows(Which::Folder), "{name}: no refusal");
            let reason = if name == "import" { window.repos().prompt.as_ref().unwrap().error.clone() } else { window.repos().path_reason() };
            assert_eq!(reason.as_deref(), Some(said.as_str()), "{name}");
            let places = parts(&window);
            let line = places.note.or(places.hint).unwrap();
            assert!(line.height() > 1.5 * line_height(TEXT_SMALL), "{name}: the reason is not wrapped: {line:?}");
            let screen = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
            assert!(screen.contains_rect(places.pane) && places.pane.contains_rect(places.buttons), "{name}");
            assert!(line.bottom() <= places.list.map_or(places.buttons.top(), |list| list.top()), "{name}");
        }
        // Laid out whole: every word of it, nothing cut.
        let mut galley = None;
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        crate::widgets::tests::context()
            .run_ui(input, |ui| galley = Some(under_galley(ui.painter(), &said, FOLDER_DIALOG_WIDTH - 2.0 * DIALOG_PAD)))
            .drop_without_applying_deltas();
        let galley = galley.unwrap();
        assert!(!galley.elided && galley.text() == said && galley.rows.len() >= 2, "{} rows, cut: {}", galley.rows.len(), galley.elided);
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "the demo's checkouts and these folders are Windows paths (drives D: and E:)")]
    fn fifty_rows_scroll_and_the_buttons_stay_in_view() {
        // Placed: in the smallest window, with the folder rows open and the note shown, the box keeps three rows and
        // the buttons stay inside the pane; in the design's window it shows six.
        let small = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
        let big = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
        // The line under the field at three lines: a long reason.
        let fifty = Under::Move { below: 3.0 * line_height(TEXT_SMALL), lines: 50, drive: Some(3.0 * TEXT_SMALL * MOVE_NOTE_LINE_HEIGHT) };
        for (window, rows) in [(small, 0), (small, 6), (big, 0), (big, 6)] {
            let places = folder_places(window, 45.0, rows, fifty);
            let (list, drive, hint) = (places.list.unwrap(), places.drive.unwrap(), places.hint.unwrap());
            assert!(window.contains_rect(places.pane), "{window:?} {rows}");
            for part in [places.header, places.label, places.field, hint, list, drive, places.buttons] {
                assert!(places.pane.contains_rect(part), "{window:?} {rows}: {part:?} outside {:?}", places.pane);
            }
            assert!(places.field.bottom() <= hint.top() && hint.bottom() <= list.top() && list.bottom() <= drive.top());
            assert!(drive.bottom() <= places.buttons.top(), "{window:?} {rows}");
            let least = if window == small { MOVE_LEAST_ROWS } else { MOVE_ROWS };
            assert!(list.height() >= list_height(least) - 1e-3, "{window:?} {rows}: {list:?}");
            assert!(list.height() <= list_height(MOVE_ROWS) + 1e-3);
        }
        // Drawn: fifty checkouts on another drive, the folder rows open in the smallest window.
        let mut repos = Repos::offline(test_settings());
        repos.hold_work();
        for index in 0..50 {
            let name = format!("repo-{index:02}");
            let mut repo = installed("acme", &name);
            repo.folder.clone_from(&name);
            repo.path = PathBuf::from(r"E:\many").join(&name);
            repos.checkouts.push(repo.path.clone());
            repos.marked.push(name);
            repos.installed.push(repo);
        }
        repos.run_migrate(MigrateItem::Folder);
        let mut window = Window::new(Session::demo(repos));
        window.size = MIN_WINDOW;
        window.frame(Vec::new());
        type_over(&mut window, &format!("{}{}", crate::demo::CODE_DIR, std::path::MAIN_SEPARATOR));
        assert_eq!(state(&window).suggestions.len(), 4);
        assert!(widgets::folder_field_rows(&window.ctx, field_id(), 4) > 0, "the folder rows are not open");
        assert_eq!(window.said(), ["Cancel", "Move 50 repositories"]);
        let places = parts(&window);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
        assert!(places.drive.is_some(), "no note on another drive");
        assert!(screen.contains_rect(places.pane) && places.pane.contains_rect(places.buttons));
        let list = places.list.unwrap();
        assert!(list.height() >= list_height(MOVE_LEAST_ROWS) - 1e-3, "{list:?}");
        // The wheel reaches the last row and no further.
        window.frame(vec![Event::PointerMoved(list.center())]);
        for _ in 0..100 {
            window.frame(vec![wheel(-120.0)]);
        }
        let end = most_scroll(50, list.height());
        assert!(end > 0.0 && (window.dialogs.list_scroll - end).abs() < 1e-3, "{} of {end}", window.dialogs.list_scroll);
        // The buttons are there to press.
        window.press("Cancel");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
    }

    #[test]
    fn moving_two_checkouts_moves_them_and_keeps_their_groups() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("move", &["api", "web"]);
        assert!(settings.config_path.starts_with(&root.0), "the settings are kept in the temporary folder");
        let mut repos = Repos::new(settings);
        // Nothing after the move asks GitHub: no test reaches the network.
        hold_github(&mut repos);
        let mut window = Window::new(Session::Live(Box::new(repos)));
        until(&mut window, "read", |window| !window.repos().waiting() && window.repos().installed.len() == 2);
        // Both in a group, through the core's group question.
        let repos = window.session.repos_mut().unwrap();
        repos.marked = vec!["api".to_owned(), "web".to_owned()];
        repos.open_group_prompt();
        repos.set_group_name("Work");
        repos.accept_group();
        let group_of = |window: &Window| -> Vec<(String, String)> {
            window.repos().installed.iter().map(|repo| (repo.folder.clone(), repo.group.clone())).collect()
        };
        assert_eq!(group_of(&window), [("api".to_owned(), "Work".to_owned()), ("web".to_owned(), "Work".to_owned())]);
        // The Migrate menu's move line, as the terminal app opens it.
        window.session.repos_mut().unwrap().run_migrate(MigrateItem::Folder);
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.shows(Which::Folder), "{}", window.repos().status);
        let target = root.0.join("moved");
        type_over(&mut window, &target.display().to_string());
        assert_eq!(targets(&window), [Some(target.join("api")), Some(target.join("web"))]);
        assert_eq!(state(&window).crosses, Some(false), "one temporary folder is one drive");
        assert_eq!(parts(&window).drive, None);
        window.press("Move 2 repositories");
        // The dialog went; the run's progress dialog shows its two tasks until it has ended.
        until(&mut window, "moved", |window| window.repos().job.as_ref().is_some_and(|job| !job.running()) && !window.repos().busy);
        assert!(window.shows(Which::Progress), "the run's dialog is not shown: {}", window.repos().status);
        assert_eq!(window.job().items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api", "web"]);
        assert!(matches!(&window.job().result, Some(Ok(_))), "{:?}", window.job().result);
        for name in ["api", "web"] {
            assert!(target.join(name).join(".git").exists(), "{name} is not in {}", target.display());
            assert!(!root.0.join(name).exists(), "{name} is still where it was");
        }
        let paths: Vec<&Path> = window.repos().installed.iter().map(|repo| repo.path.as_path()).collect();
        assert_eq!(paths, [target.join("api").as_path(), target.join("web").as_path()]);
        assert_eq!(group_of(&window), [("api".to_owned(), "Work".to_owned()), ("web".to_owned(), "Work".to_owned())]);
        assert!(root.0.join("devassist.toml").is_file(), "the settings were not saved in the temporary folder");
        window.session.close();
    }
}
