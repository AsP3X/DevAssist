//! The small pieces the screens are drawn with, painted as the shell paints (text and icons through a
//! [`Painter`]), not egui's stock widgets: a checkbox, text cut to the room it has, a glass capsule of actions, the
//! glass text field (whose text editing is egui's own), a dialog's folder field with the folders that match under
//! it, and a dialog's button.

use std::hash::Hash;

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditState;
use egui::{
    Align2, Color32, Context, Event, EventFilter, FontFamily, FontId, Frame, Id, Key, Margin, Modifiers, Painter, Pos2, Rect, Response, RichText,
    Role, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2, WidgetInfo, pos2, vec2,
};

use crate::assets::{self, Icon};
use crate::baker::Want;
use crate::overlay::Kind;
use crate::repos_view::shorten_path;
use crate::shell::Wants;
use crate::theme::{
    ACTION_GAP, ACTION_HEIGHT, ACTION_PAD, BUTTON_OFF_OPACITY, BUTTON_PAD, BUTTON_RADIUS, CAPSULE_GAP, CAPSULE_PAD, CHECK_MARK, CHECKBOX,
    CHECKBOX_STROKE_OPACITY, DIALOG_FIELD_HEIGHT, DIALOG_FIELD_RADIUS, DIVIDER_HEIGHT, DIVIDER_OPACITY, DIVIDER_WIDTH, FIELD_EDGE_OPACITY, FIELD_PAD,
    FOCUS_GAP, FOCUS_RING, FOLDER_GAP, FOLDER_INSET_PAD, FOLDER_PICK, FOLDER_ROW, FOLDER_ROWS, HAIRLINE, HOVER_OPACITY, ICON, ICON_GAP, LABEL_PAD,
    MAIN_ACTION_PAD, Palette, RADIUS_SMALL, Surface, TEXT,
};

/// The mark that ends a text cut short.
const ELLIPSIS: &str = "…";

/// What a checkbox shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tick {
    /// Empty.
    Off,
    /// Ticked.
    On,
    /// Some of what it stands for is ticked.
    Some,
}

/// Draws a checkbox in `state` for the hit area `rect` and returns what the pointer did to it. The box, a
/// [`CHECKBOX`] square, sits at the left of `rect`, centred on its height, as the design's check column holds it;
/// all of `rect` that is inside the `ui`'s clip takes the press. `id_source` tells this checkbox from the others
/// (the key of what it ticks); `label` is what a screen reader says.
pub fn checkbox(ui: &mut Ui, rect: Rect, id_source: impl Hash + std::fmt::Debug, state: Tick, palette: Palette, label: &str) -> Response {
    let response = ui.interact(rect, ui.scope_id().with(("checkbox", id_source)), Sense::CLICK);
    response.widget_info(|| WidgetInfo::selected(Role::CheckBox, true, state == Tick::On, label));
    let shape = box_rect(rect);
    let painter = ui.painter();
    match state {
        Tick::Off => {
            painter.rect_stroke(
                shape,
                RADIUS_SMALL,
                Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(CHECKBOX_STROKE_OPACITY)),
                StrokeKind::Inside,
            );
        }
        Tick::On | Tick::Some => {
            painter.rect_filled(shape, RADIUS_SMALL, palette.accent);
            let mark = if state == Tick::On { Icon::Check } else { Icon::Minus };
            draw_icon(painter, shape.center(), mark, CHECK_MARK, palette.on_dark_glass);
        }
    }
    response
}

/// The box of a checkbox whose hit area is `rect`: a [`CHECKBOX`] square at its left, centred on its height.
#[must_use]
pub fn box_rect(rect: Rect) -> Rect {
    Rect::from_min_size(pos2(rect.left(), rect.center().y - CHECKBOX / 2.0), Vec2::splat(CHECKBOX))
}

/// `text`, or its start and "…", whichever is the longest that fits `room` points in `font`; nothing when not
/// even the "…" fits.
#[must_use]
pub fn ellipsis(text: &str, font: &FontId, room: f32, ui: &Ui) -> String {
    let fits = |candidate: &str| width(ui.painter(), candidate, font) <= room;
    if fits(text) {
        return text.to_owned();
    }
    if !fits(ELLIPSIS) {
        return String::new();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |count: usize| chars[..count].iter().collect::<String>().trim_end().to_owned() + ELLIPSIS;
    // The longest start that fits, found by halving: a start's width grows with its length.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&cut(mid)) { low = mid } else { high = mid - 1 }
    }
    cut(low)
}

/// `path` with as much of its middle left out ([`shorten_path`], the terminal app's rule) as it takes to fit
/// `room` points in `font`; nothing when no shortening fits.
#[must_use]
pub fn path_to_fit(path: &str, font: &FontId, room: f32, ui: &Ui) -> String {
    let fits = |candidate: &str| width(ui.painter(), candidate, font) <= room;
    if fits(path) {
        return path.to_owned();
    }
    // The most characters whose shortening fits, found by halving: a shorter budget gives a narrower path.
    let (mut low, mut high) = (0, path.chars().count());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&shorten_path(path, mid)) { low = mid } else { high = mid - 1 }
    }
    if low == 0 { String::new() } else { shorten_path(path, low) }
}

/// How wide `text` is in `font`, in points.
#[must_use]
pub fn width(painter: &Painter, text: &str, font: &FontId) -> f32 {
    painter.layout_no_wrap(text.to_owned(), font.clone(), Color32::PLACEHOLDER).size().x
}

/// What a capsule holds, from its left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item<'a> {
    /// Words that say something and take no press.
    Label(&'a str),
    /// A label that stands for longer words: `shown` is drawn as a label is, `name` is its name to a screen reader
    /// and its hover text. It takes no press.
    Count {
        /// What is drawn.
        shown: &'a str,
        /// What it stands for.
        name: &'a str,
    },
    /// The thin upright line between a label and the actions after it; it takes no press.
    Divider,
    /// A button: its icon, its words (none for an icon alone, in the quiet colour), whether it is on, whether it is
    /// drawn in the danger colour, and whether it shows its icon only.
    Action {
        /// Its icon.
        icon: Icon,
        /// Its words; empty for an icon alone.
        words: &'a str,
        /// It is on, and drawn on the selection's fill.
        on: bool,
        /// Its icon and words are in the danger colour.
        danger: bool,
        /// Only its icon is drawn, in a round button; its words are its name to a screen reader and its hover text.
        compact: bool,
    },
}

impl<'a> Item<'a> {
    /// A button with `icon` and `words`, neither on, nor in the danger colour, nor compact.
    #[must_use]
    pub const fn action(icon: Icon, words: &'a str) -> Item<'a> {
        Item::Action { icon, words, on: false, danger: false, compact: false }
    }
}

/// Adds the glass pane of a capsule or a field at `rect` to the panes baked this frame, its ends round.
pub fn add_glass(wants: &mut Wants, id: &'static str, rect: Rect) {
    wants.add(Want { id, rect, radius: rect.height() / 2.0, surface: Surface::Capsule });
}

/// How wide `item` is in a capsule.
fn item_width(painter: &Painter, item: &Item<'_>) -> f32 {
    let medium = font(TEXT, assets::medium_family());
    match item {
        Item::Label(words) | Item::Count { shown: words, .. } => LABEL_PAD[0] + width(painter, words, &medium) + LABEL_PAD[1],
        Item::Divider => DIVIDER_WIDTH,
        Item::Action { words: "", .. } | Item::Action { compact: true, .. } => ACTION_HEIGHT,
        Item::Action { words, .. } => ACTION_PAD[0] + ICON + ACTION_GAP + width(painter, words, &medium) + ACTION_PAD[1],
    }
}

/// How wide a capsule holding `items` is.
#[must_use]
pub fn capsule_width(painter: &Painter, items: &[Item<'_>]) -> f32 {
    let gaps = CAPSULE_GAP * f32::from(u16::try_from(items.len().saturating_sub(1)).unwrap_or(u16::MAX));
    2.0 * CAPSULE_PAD + items.iter().map(|item| item_width(painter, item)).sum::<f32>() + gaps
}

/// The places of `items` in a capsule at `rect`, each [`ACTION_HEIGHT`] high and centred on its height.
pub(crate) fn item_rects(painter: &Painter, rect: Rect, items: &[Item<'_>]) -> Vec<Rect> {
    let mut left = rect.left() + CAPSULE_PAD;
    items
        .iter()
        .map(|item| {
            let width = item_width(painter, item);
            let place = Rect::from_min_size(pos2(left, rect.center().y - ACTION_HEIGHT / 2.0), vec2(width, ACTION_HEIGHT));
            left = place.right() + CAPSULE_GAP;
            place
        })
        .collect()
}

/// Draws `items` in the capsule at `rect`, whose glass is baked ([`add_glass`]), and says which action was pressed.
pub fn capsule(ui: &mut Ui, rect: Rect, items: &[Item<'_>], palette: Palette) -> Option<usize> {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let medium = font(TEXT, assets::medium_family());
    let mut pressed = None;
    for (index, (item, place)) in items.iter().zip(item_rects(&painter, rect, items)).enumerate() {
        match *item {
            Item::Label(words) => {
                painter.text(pos2(place.left() + LABEL_PAD[0], place.center().y), Align2::LEFT_CENTER, words, medium.clone(), palette.on_glass);
            }
            Item::Count { shown, name } => {
                let response = ui.interact(place, ui.scope_id().with(("capsule", rect.min.x.to_bits(), index)), Sense::hover());
                response.widget_info(|| WidgetInfo::labeled(Role::Label, true, name));
                response.on_hover_text(name);
                painter.text(pos2(place.left() + LABEL_PAD[0], place.center().y), Align2::LEFT_CENTER, shown, medium.clone(), palette.on_glass);
            }
            Item::Divider => {
                let line = Rect::from_center_size(place.center(), vec2(DIVIDER_WIDTH, DIVIDER_HEIGHT));
                painter.rect_filled(line, 0.0, palette.on_glass.gamma_multiply(DIVIDER_OPACITY));
            }
            Item::Action { icon, words, on, danger, compact } => {
                let mut response = ui.interact(place, ui.scope_id().with(("capsule", rect.min.x.to_bits(), index)), Sense::CLICK);
                let name = if words.is_empty() { icon.name() } else { words };
                response.widget_info(|| WidgetInfo::selected(Role::Button, true, on, name));
                if compact && !words.is_empty() {
                    response = response.on_hover_text(words);
                }
                let ink = if danger { palette.danger } else { palette.on_glass };
                let fill = if on {
                    palette.selection
                } else if response.hovered() {
                    palette.on_glass.gamma_multiply(HOVER_OPACITY)
                } else {
                    Color32::TRANSPARENT
                };
                painter.rect_filled(place, ACTION_HEIGHT / 2.0, fill);
                if words.is_empty() {
                    draw_icon(&painter, place.center(), icon, ICON, palette.quiet);
                } else if compact {
                    draw_icon(&painter, place.center(), icon, ICON, ink);
                } else {
                    draw_icon(&painter, pos2(place.left() + ACTION_PAD[0] + ICON / 2.0, place.center().y), icon, ICON, ink);
                    let x = place.left() + ACTION_PAD[0] + ICON + ACTION_GAP;
                    painter.text(pos2(x, place.center().y), Align2::LEFT_CENTER, words, medium.clone(), ink);
                }
                if response.clicked() {
                    pressed = Some(index);
                }
            }
        }
    }
    pressed
}

/// How wide the top row's main button with `icon` and `words` is ([`main_action`]): with its words, or as wide as
/// it is `high` with its icon alone (`compact`).
#[must_use]
pub fn main_action_width(painter: &Painter, words: &str, compact: bool, high: f32) -> f32 {
    if compact {
        high
    } else {
        MAIN_ACTION_PAD[0] + ICON + ACTION_GAP + width(painter, words, &font(TEXT, assets::medium_family())) + MAIN_ACTION_PAD[1]
    }
}

/// Draws the top row's main button at `rect` as the design's `primary` draws it: a fill, never glass, in the accent
/// colour, round at its ends, with `icon` and `words` in white; with its icon alone when `compact`, its words then
/// its name and hover text. `id` names it. Like every control of the screen it senses clicks but never takes the
/// keyboard's focus (`Sense::CLICK`, not `Sense::click()`): the filter is the one control there that holds it, so
/// it needs no focus ring. Says whether it was clicked.
pub fn main_action(ui: &mut Ui, rect: Rect, icon: Icon, words: &str, compact: bool, palette: Palette, id: Id) -> bool {
    let mut response = ui.interact(rect, id, Sense::CLICK);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, words));
    if compact {
        response = response.on_hover_text(words);
    }
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let radius = rect.height() / 2.0;
    painter.rect_filled(rect, radius, palette.accent);
    if response.hovered() {
        painter.rect_filled(rect, radius, palette.on_glass.gamma_multiply(HOVER_OPACITY));
    }
    let ink = palette.on_dark_glass;
    if compact {
        draw_icon(&painter, rect.center(), icon, ICON, ink);
    } else {
        draw_icon(&painter, pos2(rect.left() + MAIN_ACTION_PAD[0] + ICON / 2.0, rect.center().y), icon, ICON, ink);
        let x = rect.left() + MAIN_ACTION_PAD[0] + ICON + ACTION_GAP;
        painter.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, words, font(TEXT, assets::medium_family()), ink);
    }
    response.clicked()
}

/// Draws a one-line text field at `rect`, whose glass is baked ([`add_glass`]): `icon` at its left, then `text`,
/// or `hint` in the quiet colour while `text` is empty. `id` names it, so it can be given the focus. While it has
/// the focus, Tab and the arrows do not move the focus away. The hint is drawn in the quiet colour: egui gives a
/// field's hint its style's weak text colour whatever colour the hint asks for, so that colour is set to `quiet` for
/// the field.
pub fn field(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str, icon: Icon, palette: Palette, id: Id) -> Response {
    field_keeping(ui, rect, text, hint, icon, palette, id, false)
}

/// [`field`], which also keeps the focus on Esc when `escape` (egui's own Esc takes it away).
#[expect(clippy::too_many_arguments, reason = "the field's own seven and whether it keeps Esc")]
fn field_keeping(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str, icon: Icon, palette: Palette, id: Id, escape: bool) -> Response {
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    draw_icon(&painter, pos2(rect.left() + FIELD_PAD + ICON / 2.0, rect.center().y), icon, ICON, palette.quiet);
    let room = Rect::from_min_max(pos2(rect.left() + FIELD_PAD + ICON + ICON_GAP, rect.top()), pos2(rect.right() - FIELD_PAD, rect.bottom()));
    let regular = font(TEXT, FontFamily::Proportional);
    let edit = TextEdit::singleline(text)
        .id(id)
        .font(regular.clone())
        .text_color(palette.on_glass)
        .hint_text(RichText::new(hint).font(regular).color(palette.quiet))
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .desired_width(room.width())
        .min_size(room.size())
        .align(Align2::LEFT_CENTER)
        // Tab and the arrows stay with the field: nothing else on a screen takes the keyboard's focus, and the
        // screen reads the up and down arrows itself.
        .event_filter(EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape });
    ui.scope(|ui| {
        ui.visuals_mut().weak_text_color = Some(palette.quiet);
        ui.put(room, edit)
    })
    .inner
}

/// What a person did in a [`folder_field`] in one frame.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FolderDid {
    /// The text changed: typed, or a suggestion taken. The caller asks for new suggestions then, and only then.
    pub changed: bool,
    /// The suggestion taken, as the caller gave it; the text is now that folder with a separator at its end.
    pub took: Option<String>,
}

/// What a folder field keeps from one frame to the next, in egui's memory under its id.
#[derive(Clone, Debug, Default)]
struct FolderRows {
    /// The lit row, which Enter takes; none while the keyboard is in the field itself.
    highlight: Option<usize>,
    /// Esc closed the rows: they stay closed until the text changes.
    closed: bool,
    /// How far the rows are moved up, in points.
    scroll: f32,
    /// The suggestions of the last frame, to tell when they change.
    suggestions: Vec<String>,
    /// The text of the last frame, to tell when it changes.
    text: String,
    /// The rows were drawn in the last frame.
    shown: bool,
    /// The keys the rows took while they are held: their repeats are the rows' too, and reach nothing else.
    held: Vec<Key>,
    /// The pass the field was last drawn in ([`Context::cumulative_pass_nr`]).
    pass: u64,
}

/// The id a folder field `id` keeps its [`FolderRows`] under.
fn folder_rows_id(id: Id) -> Id {
    id.with("folder-rows")
}

/// The [`FolderRows`] of the folder field `id`. A field that was not drawn in the last pass (its dialog closed, and
/// is open again) starts afresh: no row lit, its rows not closed, no key held, its rows at the top.
fn folder_rows(ctx: &Context, id: Id) -> FolderRows {
    let mut rows = ctx.data(|data| data.get_temp::<FolderRows>(folder_rows_id(id))).unwrap_or_default();
    if rows.pass + 1 < ctx.cumulative_pass_nr() {
        rows = FolderRows { suggestions: rows.suggestions, text: rows.text, ..FolderRows::default() };
    }
    rows
}

/// `count` as points' worth of rows.
fn row_count(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// How tall a [`folder_field`] showing `rows` rows is: the field ([`DIALOG_FIELD_HEIGHT`]) and, when it shows
/// any, the [`FOLDER_GAP`] and the box of up to [`FOLDER_ROWS`] rows under it (more scroll).
///
/// A dialog sizes itself with this, passing [`folder_field_rows`]: it grows when the rows open and shrinks when
/// they close (on Esc, and while the field has no focus).
#[must_use]
pub fn folder_field_height(rows: usize) -> f32 {
    if rows == 0 {
        DIALOG_FIELD_HEIGHT
    } else {
        DIALOG_FIELD_HEIGHT + FOLDER_GAP + 2.0 * FOLDER_INSET_PAD + row_count(rows.min(FOLDER_ROWS)) * FOLDER_ROW
    }
}

/// How many rows the folder field `id` with `suggestions` suggestions shows in this frame, to pass to
/// [`folder_field_height`]: none while the field has no focus or Esc has closed its rows, else `suggestions` up to
/// [`FOLDER_ROWS`]. Asked before the field is drawn, it goes by the field's state after the last frame: a key that
/// opens or closes the rows in this frame (Esc, or typing after Esc) shows in the room the next frame, and the field
/// never draws rows outside the room it is given.
#[must_use]
pub fn folder_field_rows(ctx: &Context, id: Id, suggestions: usize) -> usize {
    if suggestions == 0 || folder_rows(ctx, id).closed || !ctx.memory(|memory| memory.has_focus(id)) { 0 } else { suggestions.min(FOLDER_ROWS) }
}

/// The box of the rows of a folder field at `rect` with `count` suggestions: under the field, as tall as
/// [`folder_field_height`] gives it, cut to `rect`.
fn folder_inset(rect: Rect, count: usize) -> Rect {
    let top = rect.top() + DIALOG_FIELD_HEIGHT + FOLDER_GAP;
    let bottom = (rect.top() + folder_field_height(count)).min(rect.bottom());
    Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), bottom.max(top)))
}

/// The row of the suggestion `index` in the box `inset`, its rows moved up by `scroll`.
fn folder_row(inset: Rect, index: usize, scroll: f32) -> Rect {
    let top = inset.top() + FOLDER_INSET_PAD + row_count(index) * FOLDER_ROW - scroll;
    Rect::from_min_size(pos2(inset.left(), top), vec2(inset.width(), FOLDER_ROW))
}

/// How far `count` rows can be moved up in a box `height` points tall.
fn folder_most_scroll(count: usize, height: f32) -> f32 {
    (2.0 * FOLDER_INSET_PAD + row_count(count) * FOLDER_ROW - height).max(0.0)
}

/// The scroll that shows the whole row `index` in a box `height` points tall, moving `scroll` as little as it takes.
fn folder_follow(index: usize, scroll: f32, height: f32) -> f32 {
    let top = row_count(index) * FOLDER_ROW;
    let bottom = top + FOLDER_ROW + 2.0 * FOLDER_INSET_PAD;
    if top < scroll {
        top
    } else if bottom > scroll + height {
        (bottom - height).max(0.0)
    } else {
        scroll
    }
}

/// The text a taken suggestion `folder` leaves in the field: the folder with the platform's separator at its end,
/// unless it ends in one already.
#[must_use]
fn taken_text(folder: &str) -> String {
    if folder.ends_with(std::path::is_separator) { folder.to_owned() } else { format!("{folder}{}", std::path::MAIN_SEPARATOR) }
}

/// `path`, or as much of its end as fits `room` points in `font` with "…" in front, so its last folder stays in
/// view: cut on a path separator (`\` or `/`) where one leaves an end that fits, else by characters; nothing when not
/// even the "…" fits.
#[must_use]
pub(crate) fn path_tail(path: &str, font: &FontId, room: f32, ui: &Ui) -> String {
    let fits = |candidate: &str| width(ui.painter(), candidate, font) <= room;
    if fits(path) {
        return path.to_owned();
    }
    if !fits(ELLIPSIS) {
        return String::new();
    }
    // The first separator from the left whose end fits keeps the most whole folders.
    let separators = path.char_indices().filter(|(at, c)| *at > 0 && matches!(c, '\\' | '/'));
    if let Some(cut) = separators.map(|(at, _)| format!("{ELLIPSIS}{}", &path[at..])).find(|cut| fits(cut)) {
        return cut;
    }
    let chars: Vec<char> = path.chars().collect();
    let tail = |count: usize| ELLIPSIS.to_owned() + &chars[chars.len() - count..].iter().collect::<String>();
    // The longest end that fits, found by halving: an end's width grows with its length.
    let (mut low, mut high) = (0, chars.len());
    while low < high {
        let mid = (low + high).div_ceil(2);
        if fits(&tail(mid)) { low = mid } else { high = mid - 1 }
    }
    tail(low)
}

/// What a folder field's hint says while it is empty.
const FOLDER_HINT: &str = "Folder";

/// Draws a folder field at `rect`, [`folder_field_height`] of [`folder_field_rows`] tall: a dialog's text field (the
/// field's fill, its outline at [`FIELD_EDGE_OPACITY`], the folder icon, [`FOLDER_HINT`] in the quiet colour while
/// it is empty), and under it, while it has the focus and `suggestions` are some, a box with the field's fill of up
/// to [`FOLDER_ROWS`] rows, one a suggestion, which scrolls with the wheel. No row is drawn outside `rect`.
/// `suggestions` are the caller's: full folder paths that match the text (the core's `Repos::path_suggestions`),
/// asked for again only when [`FolderDid::changed`] says so. `id` names the field, so it can be given the focus.
///
/// The keyboard, while the rows show: Down moves the light into the rows and down them (it stays on the last), Up
/// back up and from the first back into the field; the lit row is scrolled into view, and new suggestions light
/// none. Enter takes the lit row. With none lit, Enter is left to the dialog, and the field keeps the focus (egui's
/// own would give it up), so a person can type on when the dialog refuses the folder. A click takes a row
/// ([`crate::overlay::fresh_click`]). One row at most is filled: while one is lit by the keys, the row under the
/// pointer is not, and the pointer moving over the rows puts the keys' light out and lights the row under it on
/// the hover's fill; a resting pointer takes nothing. Taking a row makes the text that folder with the platform's
/// separator at its end, the cursor after it, and the field keeps the focus. Esc closes the rows, which stay
/// closed until the text changes; a second Esc is the dialog's. A long folder is cut from the left on a separator,
/// so its last folder's name shows; its hover text is all of it. There is no Tab completion: Tab
/// is [`crate::overlay::walk`]'s.
///
/// Every key the rows take (Down, Up, Enter with a row lit, Esc) is taken from egui's input, also while it is held
/// and repeats, so nothing after the field sees it. A dialog with a folder field reads its own Esc and Enter after
/// drawing the field. A field not drawn in the last frame (its dialog closed and opened again) starts afresh: no
/// row lit, its rows open.
pub fn folder_field(ui: &mut Ui, rect: Rect, text: &mut String, suggestions: &[String], palette: Palette, id: Id) -> FolderDid {
    let ctx = ui.ctx().clone();
    let mut rows = folder_rows(&ctx, id);
    if rows.suggestions != suggestions {
        rows.suggestions = suggestions.to_vec();
        rows.highlight = None;
        rows.scroll = 0.0;
    }
    if rows.text != *text {
        rows.text.clone_from(text);
        rows.closed = false;
    }
    // The repeats of a key the rows took are theirs as long as it is held.
    ui.input_mut(|input| {
        rows.held.retain(|key| input.key_down(*key));
        input.events.retain(|event| !matches!(event, Event::Key { key, pressed: true, repeat: true, .. } if rows.held.contains(key)));
    });
    let inset = folder_inset(rect, suggestions.len());
    // The pointer moving over the rows puts out the keys' light: one row at most is ever filled.
    if rows.shown && ui.input(|input| input.pointer.delta() != Vec2::ZERO && input.pointer.hover_pos().is_some_and(|at| inset.contains(at))) {
        rows.highlight = None;
    }
    let focused = ui.memory(|memory| memory.has_focus(id));
    let mut did = FolderDid::default();
    if focused && !rows.closed && !suggestions.is_empty() {
        let mut held = Vec::new();
        let mut take = |ui: &mut Ui, key: Key| {
            let taken = ui.input_mut(|input| input.consume_key(Modifiers::NONE, key));
            if taken {
                held.push(key);
            }
            taken
        };
        let last = suggestions.len() - 1;
        let before = rows.highlight;
        if take(ui, Key::ArrowDown) {
            rows.highlight = Some(rows.highlight.map_or(0, |index| (index + 1).min(last)));
        }
        if take(ui, Key::ArrowUp) {
            rows.highlight = rows.highlight.and_then(|index| index.checked_sub(1));
        }
        if let Some(index) = rows.highlight {
            if rows.highlight != before {
                rows.scroll = folder_follow(index, rows.scroll, inset.height());
            }
            if take(ui, Key::Enter) {
                did.took = Some(suggestions[index].clone());
            }
        }
        if take(ui, Key::Escape) {
            rows.closed = true;
            rows.highlight = None;
        }
        rows.held.extend(held);
    }
    if let Some(folder) = &did.took {
        *text = taken_text(folder);
        put_cursor_at_end(&ctx, id, text);
    }

    let field_rect = Rect::from_min_size(rect.min, vec2(rect.width(), DIALOG_FIELD_HEIGHT));
    ui.painter().rect_filled(field_rect, DIALOG_FIELD_RADIUS, palette.field);
    let edge = Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(FIELD_EDGE_OPACITY));
    ui.painter().rect_stroke(field_rect, DIALOG_FIELD_RADIUS, edge, StrokeKind::Inside);
    // Esc stays with the field while the rows take it: while they show, and while the Esc that closed them is held.
    let keeps_escape = (!rows.closed && !suggestions.is_empty()) || rows.held.contains(&Key::Escape);
    // An Enter the rows did not take is the dialog's; egui's field gives up the focus on it, and it stays.
    let enter = focused && ui.input(|input| input.key_pressed(Key::Enter));
    let before = text.clone();
    field_keeping(ui, field_rect, text, FOLDER_HINT, Icon::Folder, palette, id, keeps_escape);
    if enter {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    if *text != before {
        rows.closed = false;
        rows.highlight = None;
    }

    // A press or a click on the rows would take the focus from the field (egui gives it up on a click elsewhere):
    // it stays there.
    let pressed_on_rows = focused
        && rows.shown
        && ui.input(|input| {
            (input.pointer.any_pressed() || input.pointer.any_released()) && input.pointer.interact_pos().is_some_and(|at| inset.contains(at))
        });
    if pressed_on_rows {
        ui.memory_mut(|memory| memory.request_focus(id));
    }
    // Rows only in the room given: none when `rect` is the bare field.
    let show = ui.memory(|memory| memory.has_focus(id)) && !rows.closed && !suggestions.is_empty() && inset.height() > 0.0;
    if show {
        let clicked = draw_folder_rows(ui, inset, suggestions, &mut rows, palette, id);
        if let (Some(folder), None) = (clicked, &did.took) {
            *text = taken_text(&folder);
            put_cursor_at_end(&ctx, id, text);
            ui.memory_mut(|memory| memory.request_focus(id));
            did.took = Some(folder);
        }
    }
    rows.shown = show;
    rows.pass = ctx.cumulative_pass_nr();
    did.changed = *text != rows.text;
    rows.text.clone_from(text);
    ctx.data_mut(|data| data.insert_temp(folder_rows_id(id), rows));
    did
}

/// Puts the cursor of the text field `id` at the end of `text`: for a field whose text was set from outside it.
pub fn put_cursor_at_end(ctx: &Context, id: Id, text: &str) {
    let mut state = TextEditState::load(ctx, id).unwrap_or_default();
    state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(text.chars().count()))));
    state.store(ctx, id);
}

/// Draws the rows of `suggestions` in the box at `inset`, scrolled by the wheel, the lit one on the selection's
/// fill and the one under the pointer on the hover's; says which one was clicked.
fn draw_folder_rows(ui: &Ui, inset: Rect, suggestions: &[String], rows: &mut FolderRows, palette: Palette, id: Id) -> Option<String> {
    let inset_drawn = crate::dialogs::inset(ui, inset, palette);
    rows.scroll = (rows.scroll - inset_drawn.wheel).clamp(0.0, folder_most_scroll(suggestions.len(), inset.height()));
    let regular = font(TEXT, FontFamily::Proportional);
    // The rows' words stand where the field's text does.
    let indent = FIELD_PAD + ICON + ICON_GAP;
    let mut clicked = None;
    for (index, folder) in suggestions.iter().enumerate() {
        let row = folder_row(inset, index, rows.scroll);
        let visible = row.intersect(inset).intersect(ui.clip_rect());
        if !visible.is_positive() {
            continue;
        }
        let response = ui.interact(visible, id.with(("folder-row", index)), Sense::CLICK);
        let fill = row.shrink2(vec2(FOLDER_PICK[0], 0.0));
        if rows.highlight == Some(index) {
            inset_drawn.painter.rect_filled(fill, FOLDER_PICK[1], palette.selection);
        } else if rows.highlight.is_none() && response.hovered() {
            inset_drawn.painter.rect_filled(fill, FOLDER_PICK[1], palette.on_glass.gamma_multiply(HOVER_OPACITY));
        }
        let words = path_tail(folder, &regular, row.width() - indent - FIELD_PAD, ui);
        inset_drawn.painter.text(pos2(row.left() + indent, row.center().y), Align2::LEFT_CENTER, &words, regular.clone(), palette.on_glass);
        response.widget_info(|| WidgetInfo::labeled(Role::ListItem, true, folder));
        if crate::overlay::fresh_click(ui, &response) {
            clicked = Some(folder.clone());
        }
        if words != *folder {
            response.on_hover_text(folder);
        }
    }
    clicked
}

/// How wide a dialog's button with `words` is: its words in the medium face and [`BUTTON_PAD`] at either side.
#[must_use]
pub fn button_width(ui: &Ui, words: &str) -> f32 {
    2.0 * BUTTON_PAD + width(ui.painter(), words, &font(TEXT, assets::medium_family()))
}

/// Draws a dialog's button of `kind` with `words` centred in it at `rect`, and says whether a person pressed it
/// in this frame ([`crate::overlay::fresh_click`]: never the rest of the gesture that opened the overlay). Its fill
/// is a fill, never glass: the selection's for a plain one, the accent with white words for the main one, the
/// danger tint with the danger colour's words for a danger one.
///
/// `id` names it, so [`crate::overlay::walk`] can give it egui's keyboard focus: Enter and Space press it while it has
/// the focus, and a ring in the accent colour, [`FOCUS_GAP`] out, marks it then. It is the one control that takes
/// the focus besides the filter field, and it is drawn only on an overlay, which takes every key from the screen
/// under it.
pub fn button(ui: &mut Ui, rect: Rect, words: &str, kind: Kind, palette: Palette, id: Id) -> bool {
    let response = ui.interact(rect, id, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, words));
    let (fill, ink) = match kind {
        Kind::Plain => (palette.selection, palette.on_glass),
        Kind::Main => (palette.accent, palette.on_dark_glass),
        Kind::Danger => (palette.danger_tint, palette.danger),
    };
    let painter = ui.painter();
    painter.rect_filled(rect, BUTTON_RADIUS, fill);
    if response.hovered() {
        painter.rect_filled(rect, BUTTON_RADIUS, palette.on_glass.gamma_multiply(HOVER_OPACITY));
    }
    if response.has_focus() {
        painter.rect_stroke(rect.expand(FOCUS_GAP), BUTTON_RADIUS + FOCUS_GAP, Stroke::new(FOCUS_RING, palette.focus), StrokeKind::Outside);
    }
    painter.with_clip_rect(rect.intersect(ui.clip_rect())).text(
        rect.center(),
        Align2::CENTER_CENTER,
        words,
        font(TEXT, assets::medium_family()),
        ink,
    );
    crate::overlay::fresh_click(ui, &response)
}

/// Draws a plain dialog's button with `words` at `rect` that cannot be pressed: the plain one's fill and words at
/// [`BUTTON_OFF_OPACITY`], taking no press and never the keyboard's focus.
pub fn button_off(ui: &Ui, rect: Rect, words: &str, palette: Palette) {
    let painter = ui.painter();
    painter.rect_filled(rect, BUTTON_RADIUS, palette.selection.gamma_multiply(BUTTON_OFF_OPACITY));
    painter.with_clip_rect(rect.intersect(ui.clip_rect())).text(
        rect.center(),
        Align2::CENTER_CENTER,
        words,
        font(TEXT, assets::medium_family()),
        palette.on_glass.gamma_multiply(BUTTON_OFF_OPACITY),
    );
}

/// A face at `size` points.
#[must_use]
pub fn font(size: f32, family: FontFamily) -> FontId {
    FontId::new(size, family)
}

/// Draws the icon `name` at `size` points centred on `center`.
pub fn draw_icon(painter: &Painter, center: Pos2, name: Icon, size: f32, colour: Color32) {
    painter.text(center, Align2::CENTER_CENTER, assets::icon(name).to_string(), font(size, assets::icons_family()), colour);
}

#[cfg(test)]
pub(crate) mod tests {
    use egui::{Context, FullOutput, RawInput, Rect, vec2};

    use super::*;
    use crate::theme::{self, Mode, TEXT};

    /// A context with the window's faces installed.
    pub(crate) fn context() -> Context {
        let ctx = Context::default();
        assets::install_fonts(&ctx);
        ctx
    }

    /// Runs one frame of `body` in a window of 1440 by 900 points and gives back what it returned.
    fn with_ui<R>(body: impl FnOnce(&mut Ui) -> R) -> R {
        let ctx = context();
        let mut body = Some(body);
        let mut out = None;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
        let output: FullOutput = ctx.run_ui(input, |ui| {
            if let Some(body) = body.take() {
                out = Some(body(ui));
            }
        });
        output.drop_without_applying_deltas();
        out.unwrap()
    }

    #[test]
    fn an_empty_fields_hint_is_drawn_in_the_quiet_colour() {
        for mode in [Mode::Light, Mode::Dark] {
            let palette = theme::palette(mode);
            let ctx = context();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
            let mut text = String::new();
            let output = ctx.run_ui(input, |ui| {
                let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(280.0, 36.0));
                field(ui, rect, &mut text, "Filter repositories", Icon::Search, palette, Id::unique("hint-test"));
            });
            // egui would draw it in its own weak text colour, whatever colour the hint asks for.
            let hint = output.shapes.iter().find_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) if shape.galley.text() == "Filter repositories" => {
                    let section = shape.galley.job.sections[0].format.color;
                    Some(if section == Color32::PLACEHOLDER { shape.fallback_color } else { section })
                }
                _ => None,
            });
            output.drop_without_applying_deltas();
            assert_eq!(hint, Some(palette.quiet), "{mode:?}");
        }
    }

    #[test]
    fn the_top_rows_main_button_never_takes_the_keyboards_focus() {
        let palette = theme::palette(Mode::Light);
        let ctx = context();
        let id = Id::unique("main-action-test");
        let rect = Rect::from_min_size(pos2(100.0, 100.0), vec2(180.0, 36.0));
        let tab = egui::Event::Key { key: egui::Key::Tab, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        let input = |events| RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), events, ..Default::default() };
        let focus_ring = |output: &FullOutput| {
            output.shapes.iter().any(|clipped| matches!(&clipped.shape, egui::Shape::Rect(shape) if shape.stroke.color == palette.focus))
        };
        // Asked for the focus, and Tab pressed: it keeps neither, so no ring is ever drawn on it.
        ctx.run_ui(input(Vec::new()), |ui| {
            ui.memory_mut(|memory| memory.request_focus(id));
            main_action(ui, rect, Icon::Plus, "Add repository", false, palette, id);
        })
        .drop_without_applying_deltas();
        for events in [Vec::new(), vec![tab.clone()], vec![tab]] {
            let output = ctx.run_ui(input(events), |ui| {
                main_action(ui, rect, Icon::Plus, "Add repository", false, palette, id);
            });
            assert!(!ctx.memory(|memory| memory.has_focus(id)), "the top row's main button took the keyboard's focus");
            assert!(!focus_ring(&output));
            output.drop_without_applying_deltas();
        }
    }

    #[test]
    fn a_name_that_does_not_fit_ends_in_an_ellipsis() {
        let font = font(TEXT, assets::medium_family());
        let (long, short, none, width_of_long) = with_ui(|ui| {
            let long = ellipsis("customer-portal-translations", &font, 80.0, ui);
            let width_of_long = width(ui.painter(), &long, &font);
            (long, ellipsis("api", &font, 80.0, ui), ellipsis("customer-portal-translations", &font, 0.0, ui), width_of_long)
        });
        assert!(long.ends_with('…'), "{long}");
        assert!(long.starts_with("custom"), "{long}");
        assert!(width_of_long <= 80.0, "{long} is {width_of_long} wide");
        assert_eq!(short, "api");
        assert_eq!(none, "");
    }

    #[test]
    fn a_cut_text_keeps_as_much_as_fits() {
        let font = font(TEXT, FontFamily::Proportional);
        let (cut, longer) = with_ui(|ui| {
            let cut = ellipsis("customer-portal-translations", &font, 120.0, ui);
            // One character more than was kept does not fit.
            let kept = cut.trim_end_matches('…').chars().count();
            let longer: String = "customer-portal-translations".chars().take(kept + 1).collect::<String>() + "…";
            (cut, width(ui.painter(), &longer, &font))
        });
        assert!(cut.ends_with('…'));
        assert!(longer > 120.0, "{cut}: one more character would still fit");
    }

    #[test]
    fn a_folder_loses_its_middle_to_fit_its_column() {
        let font = font(TEXT, FontFamily::Proportional);
        let path = r"D:\code\tools\release-scripts";
        let (whole, column, narrow, narrow_width, none) = with_ui(|ui| {
            let whole = path_to_fit(path, &font, 300.0, ui);
            // The folder column of the design: the drawing's path loses its middle there too.
            let column = path_to_fit(path, &font, theme::COLUMN_FOLDER, ui);
            let narrow = path_to_fit(path, &font, 110.0, ui);
            let narrow_width = width(ui.painter(), &narrow, &font);
            (whole, column, narrow, narrow_width, path_to_fit(path, &font, 0.0, ui))
        });
        assert_eq!(whole, path);
        assert!(column.starts_with(r"D:\…\") && column.ends_with(r"\release-scripts"), "{column}");
        assert!(narrow.starts_with(r"D:\…"), "{narrow}");
        assert!(narrow.ends_with("scripts"), "{narrow}");
        assert!(narrow_width <= 110.0, "{narrow} is {narrow_width} wide");
        assert_eq!(none, "");
    }

    #[test]
    fn a_capsule_is_as_wide_as_its_items_and_only_its_actions_take_a_press() {
        let palette = theme::palette(Mode::Light);
        let items = [Item::Label("2 selected"), Item::action(Icon::X, "")];
        let width = with_ui(|ui| capsule_width(ui.painter(), &items));
        let words = with_ui(|ui| self::width(ui.painter(), "2 selected", &font(TEXT, assets::medium_family())));
        // The design's padding, the label's padding and words, a gap, and the round clear button.
        assert!((width - (CAPSULE_PAD + LABEL_PAD[0] + words + LABEL_PAD[1] + CAPSULE_GAP + ACTION_HEIGHT + CAPSULE_PAD)).abs() < 1e-3, "{width}");
        let rect = Rect::from_min_size(pos2(880.0, 60.0), vec2(width, 36.0));
        let press = |at: Pos2| {
            let ctx = context();
            let button =
                |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            let mut said = Vec::new();
            for events in [vec![], vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), events, ..Default::default() };
                ctx.run_ui(input, |ui| said.push(capsule(ui, rect, &items, palette))).drop_without_applying_deltas();
            }
            said.into_iter().flatten().collect::<Vec<usize>>()
        };
        assert_eq!(press(pos2(rect.right() - CAPSULE_PAD - ACTION_HEIGHT / 2.0, rect.center().y)), [1]);
        assert_eq!(press(pos2(rect.left() + CAPSULE_PAD + LABEL_PAD[0] + 4.0, rect.center().y)), Vec::<usize>::new());
    }

    #[test]
    fn a_compact_action_is_a_round_button_and_a_divider_its_line() {
        let full = [Item::Label("2"), Item::Divider, Item::action(Icon::Trash2, "Remove")];
        let compact = [Item::Label("2"), Item::Divider, Item::Action { icon: Icon::Trash2, words: "Remove", on: false, danger: true, compact: true }];
        let (full, compact, words, label) = with_ui(|ui| {
            let medium = font(TEXT, assets::medium_family());
            (
                capsule_width(ui.painter(), &full),
                capsule_width(ui.painter(), &compact),
                width(ui.painter(), "Remove", &medium),
                width(ui.painter(), "2", &medium),
            )
        });
        let start = 2.0 * CAPSULE_PAD + LABEL_PAD[0] + label + LABEL_PAD[1] + 2.0 * CAPSULE_GAP + DIVIDER_WIDTH;
        assert!((compact - (start + ACTION_HEIGHT)).abs() < 1e-3, "{compact}");
        assert!((full - (start + ACTION_PAD[0] + ICON + ACTION_GAP + words + ACTION_PAD[1])).abs() < 1e-3, "{full}");
    }

    #[test]
    fn a_glass_control_is_baked_as_a_capsule_with_round_ends() {
        let mut wants = Wants::default();
        add_glass(&mut wants, "filter", Rect::from_min_size(pos2(280.0, 60.0), vec2(280.0, 36.0)));
        let [want] = wants.as_slice() else { panic!("{wants:?}") };
        assert_eq!((want.id, want.surface, want.radius), ("filter", Surface::Capsule, 18.0));
    }

    /// `parts` joined with the platform's separator.
    pub(crate) fn path(parts: &[&str]) -> String {
        parts.join(std::path::MAIN_SEPARATOR_STR)
    }

    /// The folders the test dialog's stand-in for the core knows.
    pub(crate) fn folders() -> Vec<String> {
        let long = "customer-portal-translations-and-the-documentation-of-them";
        [
            &["D:"][..],
            &["D:", "code"],
            &["D:", "code", "tools"],
            &["D:", "code", "work"],
            &["D:", "code", "work", "api"],
            &["D:", "code", "work", "web"],
            &["D:", "code", "work", long],
            &["D:", "docs"],
        ]
        .iter()
        .map(|parts| path(parts))
        .collect()
    }

    /// What the core would suggest for `text` among `folders`: the folders in the folder `text` ends in whose names
    /// start as its last part does, ignoring case.
    pub(crate) fn matching(folders: &[String], text: &str) -> Vec<String> {
        let cut = text.rfind(std::path::is_separator).map_or(0, |at| at + 1);
        let (parent, start) = text.split_at(cut);
        folders
            .iter()
            .filter(|folder| {
                folder.len() > parent.len()
                    && folder.is_char_boundary(parent.len())
                    && folder[..parent.len()].eq_ignore_ascii_case(parent)
                    && !folder[parent.len()..].contains(std::path::is_separator)
                    && folder[parent.len()..].to_lowercase().starts_with(&start.to_lowercase())
            })
            .cloned()
            .collect()
    }

    /// The test dialog's title.
    const FOLDER_TITLE: &str = "Clone into";
    /// The test dialog's quiet line.
    const FOLDER_LINE: &str = "Type a folder, or take one of those that match.";

    /// The id of the test dialog's folder field.
    pub(crate) fn folder_id() -> Id {
        Id::unique("test-folder-field")
    }

    fn folder_cancel_id() -> Id {
        Id::unique("test-folder-cancel")
    }

    fn folder_main_id() -> Id {
        Id::unique("test-folder-main")
    }

    /// Where the test dialog and its folder field stand in `window` with `rows` rows shown
    /// ([`folder_field_rows`]): its pane, its header, the field's place ([`folder_field_height`] of `rows` tall) and
    /// its buttons' room.
    pub(crate) fn folder_dialog_places(painter: &Painter, window: Rect, rows: usize) -> (Rect, crate::dialogs::Header, Rect, Rect) {
        use crate::theme::{BUTTON_HEIGHT, DIALOG_GAP, DIALOG_PAD, DIALOG_WIDTH};
        let header = crate::dialogs::Header::new(painter, FOLDER_TITLE, FOLDER_LINE, DIALOG_WIDTH - 2.0 * DIALOG_PAD);
        let height = 2.0 * DIALOG_PAD + header.height() + 2.0 * DIALOG_GAP + folder_field_height(rows) + BUTTON_HEIGHT;
        let pane = crate::overlay::dialog_rect(window, vec2(DIALOG_WIDTH, height));
        let inside = pane.shrink(DIALOG_PAD);
        let field =
            Rect::from_min_size(pos2(inside.left(), inside.top() + header.height() + DIALOG_GAP), vec2(inside.width(), folder_field_height(rows)));
        let buttons = Rect::from_min_max(pos2(inside.left(), inside.bottom() - BUTTON_HEIGHT), inside.max);
        (pane, header, field, buttons)
    }

    /// What the test dialog did in a frame.
    #[derive(Clone, Debug, PartialEq)]
    pub(crate) struct FolderShown {
        /// What the folder field said.
        pub(crate) did: FolderDid,
        /// Esc, Cancel or the scrim closed the dialog.
        pub(crate) closed: bool,
        /// Enter reached the dialog, or its main button was pressed.
        pub(crate) accepted: bool,
        /// Where the folder field stood.
        pub(crate) field: Rect,
    }

    /// Draws the test dialog with a folder field on the overlay over `window`, its glass among `drawn`: the shield,
    /// the scrim, the glass, the pane, the walk, the header, the field and its two buttons; then the dialog's own
    /// keys, read after the field as a dialog with a folder field reads them: Esc closes it, Enter accepts.
    pub(crate) fn show_folder_dialog(
        ctx: &Context,
        window: Rect,
        text: &mut String,
        suggestions: &[String],
        palette: theme::Palette,
        drawn: &[crate::baker::Drawn],
    ) -> FolderShown {
        crate::overlay::show(ctx, |ui| {
            let scrim = crate::overlay::shield(ui, window, Id::unique("test-folder-shield"));
            crate::overlay::scrim(ui, window, palette);
            crate::overlay::glass(ui, drawn);
            let rows = folder_field_rows(ui.ctx(), folder_id(), suggestions.len());
            let (pane, header, field, buttons) = folder_dialog_places(ui.painter(), window, rows);
            crate::overlay::pane(ui, pane, Id::unique("test-folder-pane"));
            ui.set_clip_rect(pane);
            crate::overlay::walk(ui, &[folder_id(), folder_cancel_id(), folder_main_id()]);
            header.draw(ui, Rect::from_min_size(pane.shrink(crate::theme::DIALOG_PAD).min, vec2(field.width(), header.height())), palette);
            let did = folder_field(ui, field, text, suggestions, palette, folder_id());
            let words = ["Cancel", "Clone"];
            let widths = words.map(|words| button_width(ui, words));
            let places = crate::overlay::buttons(buttons, &widths);
            let cancel = button(ui, places[0], words[0], Kind::Plain, palette, folder_cancel_id());
            let main = button(ui, places[1], words[1], Kind::Main, palette, folder_main_id());
            let escape = ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            let enter = ui.input(|input| input.key_pressed(egui::Key::Enter));
            FolderShown { did, closed: escape || cancel || scrim, accepted: enter || main, field }
        })
    }

    /// The test dialog in a window of its own, with a stand-in for the core that suggests from [`folders`] each
    /// time the text changed.
    struct FolderTest {
        ctx: Context,
        text: String,
        suggestions: Vec<String>,
        open: bool,
        /// The overlay was opened.
        opened: bool,
        /// What the dialog did in each frame.
        shown: Vec<FolderShown>,
        /// The colours of the rectangles the last frame filled.
        fills: Vec<Color32>,
    }

    impl FolderTest {
        /// The dialog open with `typed` in its field, which has the focus.
        fn new(typed: &str) -> FolderTest {
            let mut dialog = FolderTest {
                ctx: context(),
                text: typed.to_owned(),
                suggestions: matching(&folders(), typed),
                open: true,
                opened: false,
                shown: Vec::new(),
                fills: Vec::new(),
            };
            dialog.frame(Vec::new());
            dialog.frame(Vec::new());
            dialog
        }

        /// Opens the dialog again, closed for a frame first, its field holding what it held.
        fn reopen(&mut self) {
            self.open = false;
            self.frame(Vec::new());
            (self.open, self.opened) = (true, false);
            self.frame(Vec::new());
            self.frame(Vec::new());
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), events, ..Default::default() };
            let (text, suggestions, open, opened) = (&mut self.text, &self.suggestions, &mut self.open, &mut self.opened);
            let mut shown = None;
            let output = self.ctx.run_ui(input, |ui| {
                let window = ui.ctx().content_rect();
                if *open && !*opened {
                    crate::overlay::open(ui.ctx(), Some(folder_id()));
                    *opened = true;
                }
                if *open {
                    let said = show_folder_dialog(ui.ctx(), window, text, suggestions, theme::palette(Mode::Light), &[]);
                    *open = !said.closed;
                    shown = Some(said);
                }
            });
            self.fills = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Rect(rect) => Some(rect.fill),
                    _ => None,
                })
                .collect();
            output.drop_without_applying_deltas();
            if let Some(shown) = shown {
                if shown.did.changed {
                    self.suggestions = matching(&folders(), &self.text);
                }
                self.shown.push(shown);
            }
        }

        fn key(&mut self, key: egui::Key) {
            self.frame(vec![egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]);
            self.frame(vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE }]);
        }

        fn typed(&mut self, key: egui::Key, text: &str) {
            self.frame(vec![
                egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE },
                egui::Event::Text(text.to_owned()),
            ]);
            self.frame(vec![egui::Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE }]);
        }

        fn click(&mut self, at: Pos2) {
            let button =
                |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
            for events in [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                self.frame(events);
            }
        }

        fn focused(&self) -> Option<Id> {
            self.ctx.memory(egui::Memory::focused)
        }

        fn rows(&self) -> FolderRows {
            folder_rows(&self.ctx, folder_id())
        }

        /// Whether any frame since `from` took a suggestion, and which.
        fn took_since(&self, from: usize) -> Vec<String> {
            self.shown[from..].iter().filter_map(|shown| shown.did.took.clone()).collect()
        }

        /// The centre of the suggestion `index` in the last frame.
        fn row(&self, index: usize) -> Pos2 {
            let field = self.shown.last().unwrap().field;
            folder_row(folder_inset(field, self.suggestions.len()), index, self.rows().scroll).center()
        }
    }

    #[test]
    fn a_folder_field_is_as_tall_as_its_field_and_up_to_six_rows() {
        assert!((folder_field_height(0) - 36.0).abs() < 1e-3);
        let rows = |count: f32| 36.0 + FOLDER_GAP + 2.0 * FOLDER_INSET_PAD + count * 32.0;
        assert!((folder_field_height(1) - rows(1.0)).abs() < 1e-3);
        assert!((folder_field_height(6) - rows(6.0)).abs() < 1e-3);
        assert!((folder_field_height(40) - rows(6.0)).abs() < 1e-3, "more than six scroll");
        let field = Rect::from_min_size(pos2(100.0, 100.0), vec2(432.0, folder_field_height(3)));
        let inset = folder_inset(field, 3);
        assert!((inset.top() - (136.0 + FOLDER_GAP)).abs() < 1e-3 && (inset.bottom() - field.bottom()).abs() < 1e-3, "{inset:?}");
        assert!((folder_row(inset, 1, 0.0).top() - (inset.top() + FOLDER_INSET_PAD + 32.0)).abs() < 1e-3);
    }

    #[test]
    fn a_taken_folder_ends_in_a_separator() {
        let sep = std::path::MAIN_SEPARATOR;
        assert_eq!(taken_text(&path(&["D:", "code"])), format!("D:{sep}code{sep}"));
        assert_eq!(taken_text(&format!("D:{sep}")), format!("D:{sep}"), "one is enough");
    }

    #[test]
    fn down_and_enter_take_a_suggestion_and_keep_the_focus() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        assert_eq!(test.focused(), Some(folder_id()));
        assert_eq!(test.suggestions, [path(&["D:", "code", "tools"]), path(&["D:", "code", "work"])]);
        assert!(test.rows().shown, "the rows show while the field has the focus");
        assert_eq!(test.rows().highlight, None);
        // Down into the rows, down to the last and no further, up back out of them.
        test.key(egui::Key::ArrowDown);
        assert_eq!(test.rows().highlight, Some(0));
        test.key(egui::Key::ArrowDown);
        test.key(egui::Key::ArrowDown);
        assert_eq!(test.rows().highlight, Some(1), "down from the last stays on it");
        test.key(egui::Key::ArrowUp);
        test.key(egui::Key::ArrowUp);
        assert_eq!(test.rows().highlight, None, "up from the first is back in the field");
        assert_eq!(test.text, path(&["D:", "code", ""]), "the arrows did not touch the text");
        // Enter takes the lit one: the text is that folder and a separator, and the field keeps the focus.
        test.key(egui::Key::ArrowDown);
        let from = test.shown.len();
        test.key(egui::Key::Enter);
        assert_eq!(test.took_since(from), [path(&["D:", "code", "tools"])]);
        assert_eq!(test.text, path(&["D:", "code", "tools", ""]));
        assert!(test.open, "the Enter was the field's");
        assert!(test.shown[from..].iter().all(|shown| !shown.accepted), "the dialog never saw the Enter");
        assert_eq!(test.focused(), Some(folder_id()));
        // The cursor is at the end: what is typed next goes after the separator.
        test.typed(egui::Key::X, "x");
        assert_eq!(test.text, path(&["D:", "code", "tools", "x"]));
    }

    #[test]
    fn down_stops_at_the_last_row_and_the_rows_follow_the_lit_one() {
        let mut test = FolderTest::new("");
        test.suggestions = (0..10).map(|index| path(&["D:", &format!("folder-{index}")])).collect();
        test.frame(Vec::new());
        assert_eq!(test.rows().highlight, None, "new suggestions light none");
        for _ in 0..12 {
            test.key(egui::Key::ArrowDown);
        }
        assert_eq!(test.rows().highlight, Some(9));
        let field = test.shown.last().unwrap().field;
        let inset = folder_inset(field, 10);
        let lit = folder_row(inset, 9, test.rows().scroll);
        assert!(test.rows().scroll > 0.0 && inset.contains_rect(lit), "{lit:?} is not in {inset:?}");
        // New suggestions light none again and start at the top.
        test.suggestions.truncate(3);
        test.frame(Vec::new());
        assert_eq!((test.rows().highlight, test.rows().scroll), (None, 0.0));
    }

    #[test]
    fn a_click_takes_a_suggestion() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        // The pointer resting on a row takes nothing; Down lights the first row, and the resting pointer leaves it lit.
        let from = test.shown.len();
        let work = test.row(1);
        test.frame(vec![egui::Event::PointerMoved(work)]);
        test.key(egui::Key::ArrowDown);
        for _ in 0..5 {
            test.frame(vec![egui::Event::PointerMoved(work)]);
        }
        assert_eq!(test.took_since(from), Vec::<String>::new(), "a resting pointer chose something");
        assert_eq!(test.rows().highlight, Some(0), "a resting pointer moved the highlight");
        // A click on another row than the lit one takes the row clicked.
        test.click(work);
        assert_eq!(test.took_since(from), [path(&["D:", "code", "work"])]);
        assert_eq!(test.text, path(&["D:", "code", "work", ""]));
        assert!(test.open);
        assert_eq!(test.focused(), Some(folder_id()), "the field has the focus back");
        assert!(test.rows().shown, "and shows what is in the folder taken");
        assert_eq!(test.suggestions.len(), 3);
        // Typing goes on at the end of the text.
        test.typed(egui::Key::A, "a");
        assert_eq!(test.text, path(&["D:", "code", "work", "a"]));
    }

    #[test]
    fn a_lit_row_is_the_only_one_filled_until_the_pointer_moves_over_the_rows() {
        let hover = theme::palette(Mode::Light).on_glass.gamma_multiply(HOVER_OPACITY);
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        let work = test.row(1);
        test.frame(vec![egui::Event::PointerMoved(work)]);
        test.frame(Vec::new());
        assert!(test.fills.contains(&hover), "a row under the pointer is lit on the hover's fill");
        test.key(egui::Key::ArrowDown);
        test.frame(Vec::new());
        assert_eq!(test.rows().highlight, Some(0));
        assert!(!test.fills.contains(&hover), "the row under the pointer is filled beside the lit one");
        // The pointer moving over the rows puts the keys' light out; the hover is drawn again.
        test.frame(vec![egui::Event::PointerMoved(work + vec2(3.0, 1.0))]);
        test.frame(Vec::new());
        assert_eq!(test.rows().highlight, None);
        assert!(test.fills.contains(&hover));
    }

    #[test]
    fn a_reopened_dialog_starts_with_no_row_lit_and_its_rows_open() {
        // Closed with a row lit: opened again, no row is lit, and Enter takes nothing.
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        test.key(egui::Key::ArrowDown);
        assert_eq!(test.rows().highlight, Some(0));
        test.reopen();
        assert_eq!(test.rows().highlight, None, "the lit row came back");
        assert!(test.rows().shown);
        let from = test.shown.len();
        test.key(egui::Key::Enter);
        assert_eq!(test.took_since(from), Vec::<String>::new());
        // Closed with its rows closed by Esc: opened again, they show.
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        test.key(egui::Key::Escape);
        assert!(test.open && !test.rows().shown);
        test.reopen();
        assert!(test.rows().shown, "the rows Esc closed stayed closed");
    }

    #[test]
    fn the_field_takes_the_room_of_the_rows_it_shows() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        let height = |test: &FolderTest| test.shown.last().unwrap().field.height();
        assert!((height(&test) - folder_field_height(2)).abs() < 1e-3, "{}", height(&test));
        assert_eq!(folder_field_rows(&test.ctx, folder_id(), 2), 2);
        // After Esc: the bare field.
        test.key(egui::Key::Escape);
        test.frame(Vec::new());
        assert_eq!(folder_field_rows(&test.ctx, folder_id(), 2), 0);
        assert!((height(&test) - folder_field_height(0)).abs() < 1e-3, "{}", height(&test));
        // Typing opens the rows again, and the room follows.
        test.typed(egui::Key::T, "t");
        test.frame(Vec::new());
        assert!(test.rows().shown);
        assert!((height(&test) - folder_field_height(1)).abs() < 1e-3, "{}", height(&test));
        // Without the focus: the bare field, and no row is drawn.
        test.key(egui::Key::Tab);
        test.frame(Vec::new());
        assert_eq!(folder_field_rows(&test.ctx, folder_id(), 1), 0);
        assert!(!test.rows().shown);
        assert!((height(&test) - folder_field_height(0)).abs() < 1e-3, "{}", height(&test));
    }

    #[test]
    fn keys_on_a_button_are_the_dialogs() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        test.key(egui::Key::Tab);
        test.key(egui::Key::Tab);
        assert_eq!(test.focused(), Some(folder_main_id()));
        let from = test.shown.len();
        test.key(egui::Key::ArrowDown);
        assert_eq!(test.rows().highlight, None, "Down on a button lit a row");
        test.key(egui::Key::Enter);
        assert!(test.shown[from..].iter().any(|shown| shown.accepted), "Enter did not reach the dialog");
        assert_eq!(test.took_since(from), Vec::<String>::new());
        test.key(egui::Key::Escape);
        assert!(!test.open, "Esc did not reach the dialog");
        assert_eq!(test.text, path(&["D:", "code", ""]));
    }

    #[test]
    fn esc_closes_the_suggestions_before_the_dialog() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        assert!(test.rows().shown);
        // Pressed and held: the press closes the rows, its repeats do nothing.
        let escape =
            |repeat| egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat, modifiers: egui::Modifiers::NONE };
        test.frame(vec![escape(false)]);
        test.frame(vec![escape(true)]);
        test.frame(vec![escape(true)]);
        assert!(test.open, "the Esc that closed the rows closed the dialog");
        assert!(!test.rows().shown);
        assert_eq!(test.focused(), Some(folder_id()), "the field keeps the focus");
        test.frame(vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        // They stay closed until the text changes.
        test.frame(Vec::new());
        test.key(egui::Key::ArrowDown);
        assert!(!test.rows().shown && test.rows().highlight.is_none());
        test.typed(egui::Key::W, "w");
        assert!(test.rows().shown, "typing opens them again");
        assert_eq!(test.suggestions, [path(&["D:", "code", "work"])]);
        // Esc closes them again, and then the next is the dialog's.
        test.key(egui::Key::Escape);
        assert!(test.open && !test.rows().shown);
        test.key(egui::Key::Escape);
        assert!(!test.open, "the second Esc closes the dialog");
    }

    #[test]
    fn enter_with_no_row_lit_keeps_the_focus_and_reaches_the_dialog() {
        let mut test = FolderTest::new(&path(&["D:", "code", ""]));
        assert!(test.rows().shown && test.rows().highlight.is_none());
        let from = test.shown.len();
        test.key(egui::Key::Enter);
        assert_eq!(test.took_since(from), Vec::<String>::new());
        assert!(test.shown[from..].iter().any(|shown| shown.accepted), "the dialog did not see the Enter");
        assert_eq!(test.text, path(&["D:", "code", ""]));
        // The dialog refused it, say: the person types on.
        assert_eq!(test.focused(), Some(folder_id()), "Enter took the focus from the field");
        test.typed(egui::Key::W, "w");
        assert_eq!(test.text, path(&["D:", "code", "w"]));
    }

    #[test]
    fn a_long_path_keeps_its_last_folder_in_view() {
        let font = font(TEXT, FontFamily::Proportional);
        let long = r"D:\code\customers\portal\translations\release-scripts";
        let (whole, cut, cut_width, name_only, name_width) = with_ui(|ui| {
            let cut = path_tail(long, &font, 200.0, ui);
            let name_only = path_tail(r"D:\code\customer-portal-translations-and-the-documentation-of-them", &font, 120.0, ui);
            (
                path_tail(long, &font, 600.0, ui),
                cut.clone(),
                width(ui.painter(), &cut, &font),
                name_only.clone(),
                width(ui.painter(), &name_only, &font),
            )
        });
        assert_eq!(whole, long);
        assert!(cut.starts_with(r"…\") && cut.ends_with(r"\release-scripts"), "{cut}");
        assert!(cut_width <= 200.0, "{cut} is {cut_width} wide");
        // Cut on a separator, as much kept as fits: one more folder would not.
        let more = with_ui(|ui| {
            let at = long[..long.len() - cut.len() + "…".len()].rfind('\\').unwrap();
            width(ui.painter(), &format!("…{}", &long[at..]), &font)
        });
        assert!(more > 200.0, "{cut} could keep one more folder");
        // A last folder that alone does not fit keeps its end.
        assert!(name_only.starts_with('…') && name_only.ends_with("of-them"), "{name_only}");
        assert!(name_width <= 120.0, "{name_only} is {name_width} wide");

        // In the field's rows the long one is drawn cut from the left, keeping its end.
        let dialog = FolderTest::new(&path(&["D:", "code", "work", ""]));
        assert_eq!(dialog.suggestions.len(), 3);
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
        let mut typed = dialog.text.clone();
        let output = dialog.ctx.run_ui(input, |ui| {
            show_folder_dialog(ui.ctx(), ui.ctx().content_rect(), &mut typed, &dialog.suggestions, theme::palette(Mode::Light), &[]);
        });
        let drawn: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(shape) => Some(shape.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        assert!(drawn.iter().any(|words| words.starts_with('…') && words.ends_with("-of-them")), "{drawn:?}");
    }

    #[test]
    fn letters_that_are_keys_only_type() {
        let mut test = FolderTest::new(&path(&["D:", ""]));
        test.key(egui::Key::ArrowDown);
        assert_eq!(test.rows().highlight, Some(0));
        let from = test.shown.len();
        let keys = [
            (egui::Key::A, "a"),
            (egui::Key::I, "i"),
            (egui::Key::D, "d"),
            (egui::Key::U, "u"),
            (egui::Key::W, "w"),
            (egui::Key::M, "m"),
            (egui::Key::L, "l"),
            (egui::Key::S, "s"),
            (egui::Key::Space, " "),
        ];
        for (key, letter) in keys {
            test.typed(key, letter);
        }
        assert_eq!(test.text, path(&["D:", "aiduwmls "]));
        assert!(test.took_since(from).is_empty(), "a key took a suggestion");
        assert!(test.open && test.shown[from..].iter().all(|shown| !shown.closed && !shown.accepted));
        assert_eq!(test.focused(), Some(folder_id()));
    }

    /// Where the pointer rests in a picture of the test dialog in `window` showing `rows` rows: on the second row.
    pub(crate) fn folder_shot_pointer(painter: &Painter, window: Rect, rows: usize) -> Pos2 {
        let (_, _, field, _) = folder_dialog_places(painter, window, rows);
        folder_row(folder_inset(field, rows), 1, 0.0).center()
    }

    /// The middle of the row of the suggestion `index` under a folder field at `field` (with the room its rows take),
    /// its rows unscrolled.
    pub(crate) fn folder_row_centre(field: Rect, index: usize) -> Pos2 {
        folder_row(folder_inset(field, index + 1), index, 0.0).center()
    }

    /// The events of frame `frame` of a picture of the test dialog: the pointer resting at `pointer` all along, and,
    /// when `lit`, Down in the third frame (the pointer has stopped by then), which lights the first row.
    pub(crate) fn folder_shot_events(frame: usize, pointer: Pos2, lit: bool) -> Vec<egui::Event> {
        let mut events = vec![egui::Event::PointerMoved(pointer)];
        if lit && frame == 2 {
            events.push(egui::Event::Key {
                key: egui::Key::ArrowDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        events
    }

    /// A picture of the test dialog over the lake in `mode`, its field holding `D:\code\work\` with its three
    /// folders under it: the first lit by Down, the pointer resting on the second (which is not filled), the third
    /// too long and cut.
    #[expect(clippy::cast_precision_loss, reason = "picture sizes of a few thousand pixels")]
    fn folder_picture(mode: Mode) -> devassist_glass::raster::Bitmap {
        use std::collections::HashMap;

        use crate::baker::{Drawn, Scene, pane_picture};
        use crate::theme::WINDOW;
        let palette = theme::palette(mode);
        let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
        let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: "lake".to_owned(), blur: true };
        let (wall, mut bitmap) = crate::picture::wallpaper_window(&scene);
        let ctx = context();
        ctx.set_theme(if mode == Mode::Dark { egui::Theme::Dark } else { egui::Theme::Light });
        let mut text = path(&["D:", "code", "work", ""]);
        let suggestions = matching(&folders(), &text);
        let (mut pane, mut pointer) = (Rect::NOTHING, Pos2::ZERO);
        let mut output = ctx.run_ui(RawInput { screen_rect: Some(window), ..Default::default() }, |ui| {
            pane = folder_dialog_places(ui.painter(), window, suggestions.len()).0;
            pointer = folder_shot_pointer(ui.painter(), window, suggestions.len());
        });
        let glass = pane_picture(&wall, &scene, &crate::overlay::want_dialog(pane)).unwrap();
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(pane.width() / glass.w as f32, pane.height() / glass.h as f32));
        let drawn = [Drawn { id: crate::overlay::DIALOG, texture: egui::TextureId::User(2), rect: pane, uv }];
        let mut textures = HashMap::from([(egui::TextureId::User(2), glass.image())]);
        crate::picture::take_textures(std::mem::take(&mut output.textures_delta), &mut textures);
        let mut shapes = Vec::new();
        let mut taken = FolderDid::default();
        for frame in 0..4 {
            let events = folder_shot_events(frame, pointer, true);
            let mut output = ctx.run_ui(RawInput { screen_rect: Some(window), events, ..Default::default() }, |ui| {
                if frame == 0 {
                    crate::overlay::open(ui.ctx(), Some(folder_id()));
                }
                taken = show_folder_dialog(ui.ctx(), window, &mut text, &suggestions, palette, &drawn).did;
            });
            crate::picture::take_textures(std::mem::take(&mut output.textures_delta), &mut textures);
            shapes = std::mem::take(&mut output.shapes);
        }
        assert_eq!(taken, FolderDid::default(), "the picture took nothing");
        let rows = folder_rows(&ctx, folder_id());
        assert!(rows.shown && rows.highlight == Some(0), "{rows:?}");
        crate::picture::paint(&mut bitmap, &ctx.tessellate(shapes, 1.0), &textures);
        bitmap
    }

    #[test]
    fn the_folder_field_is_drawn_in_both_modes() {
        for (mode, name) in [(Mode::Light, "folder-field-light.png"), (Mode::Dark, "folder-field-dark.png")] {
            let bitmap = folder_picture(mode);
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots").join(name);
            crate::screenshot::write_png(&path, &bitmap.image()).unwrap();
            eprintln!("wrote {}", path.display());
        }
    }

    #[test]
    fn a_checkbox_says_its_state_and_draws_its_box_at_the_left() {
        let hit = Rect::from_min_max(pos2(100.0, 200.0), pos2(124.0, 244.0));
        assert_eq!(box_rect(hit), Rect::from_min_size(pos2(100.0, 214.0), vec2(16.0, 16.0)));
        let palette = theme::palette(Mode::Light);
        let shapes = |state: Tick| {
            let ctx = context();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 400.0))), ..Default::default() };
            let output = ctx.run_ui(input, |ui| {
                checkbox(ui, hit, "api", state, palette, "Select api");
            });
            let count = output.shapes.len();
            output.drop_without_applying_deltas();
            count
        };
        // An empty box is its outline; a ticked one its fill and its mark.
        assert_eq!(shapes(Tick::Off) + 1, shapes(Tick::On));
        assert_eq!(shapes(Tick::On), shapes(Tick::Some));
    }
}
