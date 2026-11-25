//! Frosted panes, drawn without a picture of their own: each is a rounded rectangle that shows the part of the
//! baker's [`Sheet`] lying under it, so a frosted pane costs nothing to move, resize or scroll.

use egui::epaint::RectShape;
use egui::{Color32, Painter, Rect, StrokeKind, emath};

use crate::baker::Sheet;

/// Paints a frosted pane: `rect` with `radius`, showing the part of the sheet that lies under it in a window of
/// `window`, its outline stroked with the sheet's edge light.
pub fn paint(painter: &Painter, window: Rect, rect: Rect, radius: f32, sheet: &Sheet) {
    for shape in shapes(window, rect, radius, sheet) {
        painter.add(shape);
    }
}

/// The part of the sheet's `uv` that lies under `rect` in a window of `window`.
#[must_use]
pub fn uv_under(window: Rect, rect: Rect, sheet_uv: Rect) -> Rect {
    // A window without area shows the sheet's top left, rather than numbers that are no place.
    let along = |at: f32, from: emath::Rangef, to: emath::Rangef| {
        let share = if from.span() > 0.0 { (at - from.min) / from.span() } else { 0.0 };
        to.min + share * to.span()
    };
    let (x, y) = ((window.x_range(), sheet_uv.x_range()), (window.y_range(), sheet_uv.y_range()));
    Rect::from_min_max(
        emath::pos2(along(rect.min.x, x.0, x.1), along(rect.min.y, y.0, y.1)),
        emath::pos2(along(rect.max.x, x.0, x.1), along(rect.max.y, y.0, y.1)),
    )
}

/// The two shapes of a frosted pane: the rounded rectangle filled with the sheet, then its edge over it. The edge
/// is a shape of its own because a textured rectangle's stroke would be painted from the same texture.
pub(crate) fn shapes(window: Rect, rect: Rect, radius: f32, sheet: &Sheet) -> [RectShape; 2] {
    // Not moved to whole pixels: the pane sits where a baked picture of it sat, and its texture under it.
    let fill =
        RectShape::filled(rect, radius, Color32::WHITE).with_texture(sheet.texture, uv_under(window, rect, sheet.uv)).with_round_to_pixels(false);
    let edge = RectShape::stroke(rect, radius, sheet.edge, StrokeKind::Inside).with_round_to_pixels(false);
    [fill, edge]
}

#[cfg(test)]
mod tests {
    use egui::{Color32, Pos2, Rect, Stroke, TextureId, pos2, vec2};

    use super::*;

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(1000.0, 500.0))
    }

    #[test]
    fn a_pane_shows_the_part_of_the_sheet_under_it() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let pane = Rect::from_min_max(pos2(250.0, 100.0), pos2(750.0, 400.0));
        let uv = uv_under(window(), pane, full);
        assert_eq!(uv, Rect::from_min_max(pos2(0.25, 0.2), pos2(0.75, 0.8)));
        // A sheet whose window covers only part of its texture: every number scales with that part.
        let half = Rect::from_min_max(pos2(0.0, 0.0), pos2(0.5, 0.5));
        assert_eq!(uv_under(window(), pane, half), Rect::from_min_max(pos2(0.125, 0.1), pos2(0.375, 0.4)));
        // The whole window is the sheet's whole part.
        assert_eq!(uv_under(window(), window(), half), half);
    }

    #[test]
    fn a_window_without_area_shows_the_sheets_corner() {
        let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let flat = Rect::from_min_size(Pos2::ZERO, vec2(0.0, 500.0));
        let uv = uv_under(flat, Rect::from_min_size(pos2(10.0, 10.0), vec2(20.0, 20.0)), full);
        assert!(uv.min.x.is_finite() && uv.max.x.is_finite() && uv.min.y.is_finite() && uv.max.y.is_finite(), "{uv:?}");
    }

    #[test]
    fn a_pane_is_the_sheet_in_a_rounded_rectangle_with_its_edge_drawn_over_it() {
        let sheet = Sheet {
            texture: TextureId::User(7),
            uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(0.5, 0.5)),
            edge: Stroke::new(1.1, Color32::from_white_alpha(20)),
        };
        let pane = Rect::from_min_max(pos2(250.0, 100.0), pos2(750.0, 400.0));
        let [fill, edge] = shapes(window(), pane, 16.0, &sheet);
        assert_eq!(fill.rect, pane);
        assert_eq!(fill.corner_radius, egui::CornerRadius::same(16));
        assert_eq!(fill.fill, Color32::WHITE, "the texture is shown as it is");
        assert_eq!(fill.stroke, Stroke::NONE, "the edge is a shape of its own, untextured");
        let brush = fill.brush.as_deref().map(|brush| (brush.fill_texture_id, brush.uv));
        assert_eq!(brush, Some((sheet.texture, uv_under(window(), pane, sheet.uv))));
        assert_eq!((edge.rect, edge.corner_radius, edge.stroke), (pane, fill.corner_radius, sheet.edge));
        assert_eq!(edge.stroke_kind, egui::StrokeKind::Inside, "the edge lies inside the pane, as the hairline does");
        assert_eq!((edge.fill, edge.brush.is_none()), (Color32::TRANSPARENT, true));
        // Neither is moved to whole pixels: the pane sits where a baked picture of it sat.
        assert_eq!((fill.round_to_pixels, edge.round_to_pixels), (Some(false), Some(false)));
    }
}
