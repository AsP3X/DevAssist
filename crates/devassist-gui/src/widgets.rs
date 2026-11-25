//! The small pieces the table is drawn with, painted as the shell paints (text and icons through a [`Painter`]),
//! not egui's stock widgets: a checkbox, and text cut to the room it has.

use std::hash::Hash;

use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Response, Role, Sense, Stroke, StrokeKind, Ui, Vec2, WidgetInfo, pos2};

use crate::assets::{self, Icon};
use crate::repos_view::shorten_path;
use crate::theme::{CHECK_MARK, CHECKBOX, CHECKBOX_STROKE_OPACITY, HAIRLINE, Palette, RADIUS_SMALL};

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
    let response = ui.interact(rect, ui.scope_id().with(("checkbox", id_source)), Sense::click());
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
