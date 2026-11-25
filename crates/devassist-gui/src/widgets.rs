//! The small pieces the screens are drawn with, painted as the shell paints (text and icons through a
//! [`Painter`]), not egui's stock widgets: a checkbox, text cut to the room it has, a glass capsule of actions, the
//! glass text field (whose text editing is egui's own) and a dialog's button.

use std::hash::Hash;

use egui::{
    Align2, Color32, EventFilter, FontFamily, FontId, Frame, Id, Margin, Painter, Pos2, Rect, Response, RichText, Role, Sense, Stroke, StrokeKind,
    TextEdit, Ui, Vec2, WidgetInfo, pos2, vec2,
};

use crate::assets::{self, Icon};
use crate::baker::Want;
use crate::overlay::Kind;
use crate::repos_view::shorten_path;
use crate::shell::Wants;
use crate::theme::{
    ACTION_GAP, ACTION_HEIGHT, ACTION_PAD, BUTTON_OFF_OPACITY, BUTTON_PAD, BUTTON_RADIUS, CAPSULE_GAP, CAPSULE_PAD, CHECK_MARK, CHECKBOX,
    CHECKBOX_STROKE_OPACITY, FIELD_PAD, FOCUS_GAP, FOCUS_RING, HAIRLINE, HOVER_OPACITY, ICON, ICON_GAP, LABEL_PAD, Palette, RADIUS_SMALL, Surface,
    TEXT,
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
    /// A button: its icon, its words (none for an icon alone), and whether it is on.
    Action {
        /// Its icon.
        icon: Icon,
        /// Its words; empty for an icon alone.
        words: &'a str,
        /// It is on, and drawn on the selection's fill.
        on: bool,
    },
}

/// Adds the glass pane of a capsule or a field at `rect` to the panes baked this frame, its ends round.
pub fn add_glass(wants: &mut Wants, id: &'static str, rect: Rect) {
    wants.add(Want { id, rect, radius: rect.height() / 2.0, surface: Surface::Capsule });
}

/// How wide `item` is in a capsule.
fn item_width(painter: &Painter, item: &Item<'_>) -> f32 {
    let medium = font(TEXT, assets::medium_family());
    match item {
        Item::Label(words) => LABEL_PAD[0] + width(painter, words, &medium) + LABEL_PAD[1],
        Item::Action { words: "", .. } => ACTION_HEIGHT,
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
fn item_rects(painter: &Painter, rect: Rect, items: &[Item<'_>]) -> Vec<Rect> {
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
            Item::Action { icon, words, on } => {
                let response = ui.interact(place, ui.scope_id().with(("capsule", rect.min.x.to_bits(), index)), Sense::CLICK);
                let name = if words.is_empty() { icon.name() } else { words };
                response.widget_info(|| WidgetInfo::selected(Role::Button, true, on, name));
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
                } else {
                    draw_icon(&painter, pos2(place.left() + ACTION_PAD[0] + ICON / 2.0, place.center().y), icon, ICON, palette.on_glass);
                    let x = place.left() + ACTION_PAD[0] + ICON + ACTION_GAP;
                    painter.text(pos2(x, place.center().y), Align2::LEFT_CENTER, words, medium.clone(), palette.on_glass);
                }
                if response.clicked() {
                    pressed = Some(index);
                }
            }
        }
    }
    pressed
}

/// Draws a one-line text field at `rect`, whose glass is baked ([`add_glass`]): `icon` at its left, then `text`,
/// or `hint` in the quiet colour while `text` is empty. `id` names it, so it can be given the focus. While it has
/// the focus, Tab and the arrows do not move the focus away.
pub fn field(ui: &mut Ui, rect: Rect, text: &mut String, hint: &str, icon: Icon, palette: Palette, id: Id) -> Response {
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
        .event_filter(EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, ..EventFilter::default() });
    ui.put(room, edit)
}

/// How wide a dialog's button with `words` is: its words in the medium face and [`BUTTON_PAD`] at either side.
#[must_use]
pub fn button_width(ui: &Ui, words: &str) -> f32 {
    2.0 * BUTTON_PAD + width(ui.painter(), words, &font(TEXT, assets::medium_family()))
}

/// Draws a dialog's button of `kind` with `words` centred in it at `rect`, and returns what the pointer and the
/// keyboard did to it. Its fill is a fill, never glass: the selection's for a plain one, the accent with white
/// words for the main one, the danger tint with the danger colour's words for a danger one.
///
/// `id` names it, so [`crate::overlay::walk`] can give it egui's keyboard focus: Enter and Space press it while it has
/// the focus, and a ring in the accent colour, [`FOCUS_GAP`] out, marks it then. It is the one control that takes
/// the focus besides the filter field, and it is drawn only on an overlay, which takes every key from the screen
/// under it.
pub fn button(ui: &mut Ui, rect: Rect, words: &str, kind: Kind, palette: Palette, id: Id) -> Response {
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
        painter.rect_stroke(rect.expand(FOCUS_GAP), BUTTON_RADIUS + FOCUS_GAP, Stroke::new(FOCUS_RING, palette.accent), StrokeKind::Outside);
    }
    painter.with_clip_rect(rect.intersect(ui.clip_rect())).text(
        rect.center(),
        Align2::CENTER_CENTER,
        words,
        font(TEXT, assets::medium_family()),
        ink,
    );
    response
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
        let items = [Item::Label("2 selected"), Item::Action { icon: Icon::X, words: "", on: false }];
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
    fn a_glass_control_is_baked_as_a_capsule_with_round_ends() {
        let mut wants = Wants::default();
        add_glass(&mut wants, "filter", Rect::from_min_size(pos2(280.0, 60.0), vec2(280.0, 36.0)));
        let [want] = wants.as_slice() else { panic!("{wants:?}") };
        assert_eq!((want.id, want.surface, want.radius), ("filter", Surface::Capsule, 18.0));
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
