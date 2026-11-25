//! The window's colours, sizes and materials, from the Quill design.

use std::time::Duration;

use devassist_core::config::Theme;
use devassist_glass::material::{self, Lookup, Material, Params};
use egui::Color32;
use serde_json::{Value, json};

/// The window's size when it opens.
pub const WINDOW: [f32; 2] = [1440.0, 900.0];
/// The smallest the window can be made.
pub const MIN_WINDOW: [f32; 2] = [960.0, 600.0];
/// The gap between the window's edge and its panes, and between panes.
pub const INSET: f32 = 12.0;
/// The sidebar's width.
pub const SIDEBAR_WIDTH: f32 = 256.0;
/// The height of the bar at the top.
pub const BAR_HEIGHT: f32 = 36.0;
/// The height of the status bar.
pub const STATUS_HEIGHT: f32 = 32.0;
/// The height of a row in the sidebar's navigation.
pub const NAV_ROW: f32 = 32.0;
/// The corner radius of small things.
pub const RADIUS_SMALL: f32 = 4.0;
/// The corner radius of a row.
pub const RADIUS_ROW: f32 = 12.0;
/// The corner radius of a panel.
pub const RADIUS_PANEL: f32 = 16.0;
/// The corner radius of a pane.
pub const RADIUS_PANE: f32 = 20.0;
/// The size of body text.
pub const TEXT: f32 = 14.0;
/// The size of small text.
pub const TEXT_SMALL: f32 = 12.0;
/// The size of a title.
pub const TEXT_TITLE: f32 = 16.0;
/// The size of an icon.
pub const ICON: f32 = 16.0;
/// How far the backdrop is blurred behind the panes.
pub const BACKDROP_BLUR: f32 = 24.0;
/// How strong the quiet text colour is, of the full text colour.
pub const QUIET_OPACITY: f32 = 0.68;
/// How long the window must be left alone after a resize before its glass is redrawn at the new size.
pub const RESIZE_SETTLE: Duration = Duration::from_millis(150);
/// The width of one of the window's own caption buttons.
pub const CAPTION_BUTTON: f32 = 40.0;
/// The room kept at the left of the top bar for the traffic lights on macOS.
pub const TRAFFIC_LIGHT_SPACE: f32 = 90.0;

/// Whether the window is drawn light or dark.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    /// Dark text on light glass.
    Light,
    /// Light text on dark glass.
    Dark,
}

impl Mode {
    /// The mode the theme setting comes to, `system_dark` being what the system asks for.
    #[must_use]
    pub fn of(theme: Theme, system_dark: bool) -> Mode {
        match theme {
            Theme::Dark => Mode::Dark,
            Theme::System if system_dark => Mode::Dark,
            Theme::Light | Theme::System => Mode::Light,
        }
    }
}

/// The window's colours in one mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    /// Text and lines on glass.
    pub on_glass: Color32,
    /// Text on glass that is dark in both modes.
    pub on_dark_glass: Color32,
    /// Secondary text on glass: `on_glass` at [`QUIET_OPACITY`].
    pub quiet: Color32,
    /// The fill of a selected row.
    pub selection: Color32,
    /// The accent colour.
    pub accent: Color32,
    /// The fill of an input field.
    pub field: Color32,
    /// What is laid over the window behind a dialog.
    pub scrim: Color32,
    /// Text that warns.
    pub warning: Color32,
    /// Text that reports a failure.
    pub danger: Color32,
    /// The fill behind a failure.
    pub danger_tint: Color32,
    /// Text that reports success.
    pub ok: Color32,
    /// The colour the backdrop's wash starts from.
    pub wash_base: Color32,
    /// The colour of the veil laid over the backdrop.
    pub veil: Color32,
}

/// The window's colours in `mode`.
#[must_use]
pub fn palette(mode: Mode) -> Palette {
    let rgb = Color32::from_rgb;
    let rgba = Color32::from_rgba_unmultiplied;
    match mode {
        Mode::Light => Palette {
            on_glass: rgb(0x00, 0x00, 0x00),
            on_dark_glass: Color32::WHITE,
            quiet: rgb(0x00, 0x00, 0x00).gamma_multiply(QUIET_OPACITY),
            selection: rgba(0, 0, 0, 0x1f),
            accent: rgb(0x0a, 0x47, 0xa3),
            field: rgba(255, 255, 255, 0x99),
            scrim: rgba(0, 0, 0, 0x33),
            warning: rgb(0x6e, 0x3a, 0x00),
            danger: rgb(0xb3, 0x26, 0x1e),
            danger_tint: rgba(0xb3, 0x26, 0x1e, 0x1a),
            ok: rgb(0x1c, 0x6b, 0x3a),
            wash_base: rgb(0xdf, 0xe8, 0xf5),
            veil: rgb(0xff, 0xff, 0xff),
        },
        Mode::Dark => Palette {
            on_glass: rgb(0xff, 0xff, 0xff),
            on_dark_glass: Color32::WHITE,
            quiet: rgb(0xff, 0xff, 0xff).gamma_multiply(QUIET_OPACITY),
            selection: rgba(255, 255, 255, 0x24),
            accent: rgb(0x2a, 0x66, 0xd6),
            field: rgba(255, 255, 255, 0x1a),
            scrim: rgba(0, 0, 0, 0x73),
            warning: rgb(0xf5, 0xbd, 0x55),
            danger: rgb(0xff, 0xb9, 0xb1),
            danger_tint: rgba(0x6b, 0x1d, 0x18, 0xcc),
            ok: rgb(0x77, 0xdb, 0xa0),
            wash_base: rgb(0x0b, 0x14, 0x22),
            veil: rgb(0x06, 0x0d, 0x18),
        },
    }
}

/// The veil's opacity over the backdrop `backdrop`: at the top 45 % of the window, and at the bottom. A key
/// that names no backdrop gets the values of `"lake"`.
#[must_use]
pub fn veil(backdrop: &str, mode: Mode) -> [f32; 2] {
    match (backdrop, mode) {
        ("midday", Mode::Dark) => [0.45, 0.72],
        ("dusk", Mode::Light) => [0.2, 0.62],
        ("dusk", Mode::Dark) => [0.2, 0.5],
        ("night", Mode::Light) => [0.5, 0.82],
        ("night", Mode::Dark) => [0.1, 0.35],
        (_, Mode::Light) => [0.0, 0.0],
        (_, Mode::Dark) => [0.5, 0.82],
    }
}

/// A kind of pane, each with its own material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    /// The sidebar: glass.
    Sidebar,
    /// A capsule floating over the content: glass.
    Capsule,
    /// The status bar: thin glass.
    StatusBar,
    /// A panel holding content: frosted, not glass.
    Panel,
}

/// The material of `surface` in `mode`, as the design document writes it.
fn material_value(surface: Surface, mode: Mode) -> Value {
    let appearance = match mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    match surface {
        Surface::Sidebar => json!({ "type": "liquid_glass", "depth": "panel", "appearance": appearance }),
        Surface::Capsule => json!({ "type": "liquid_glass", "appearance": appearance }),
        Surface::StatusBar => json!({ "type": "liquid_glass", "depth": "bar", "appearance": appearance }),
        Surface::Panel => json!({ "type": "frosted", "thickness": "thick", "appearance": appearance }),
    }
}

/// The numbers to draw `surface` with in `mode`, as a pane of `size`.
#[must_use]
pub fn params(surface: Surface, mode: Mode, size: (f32, f32)) -> Params {
    // A constant that fails to read is a programming error (a test covers them); draw frosted rather than panic.
    let material = Material::read(&material_value(surface, mode)).or_else(|| Material::read(&material::fresh(false))).unwrap_or_else(|| Material {
        kind: material::Kind::Frosted(material::Thickness::Regular),
        appearance: material::Appearance::Auto,
        tint: None,
        depth: None,
        advanced: serde_json::Map::new(),
    });
    let no_number = |_: &str| None;
    let no_colour = |_: &str| None;
    material.resolve(size, mode == Mode::Dark, &Lookup { number: &no_number, color: &no_colour })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colours_are_the_quill_documents() {
        let (light, dark) = (palette(Mode::Light), palette(Mode::Dark));
        assert_eq!(light.on_glass, Color32::from_rgb(0x00, 0x00, 0x00));
        assert_eq!(dark.on_glass, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(light.accent, Color32::from_rgb(0x0a, 0x47, 0xa3));
        assert_eq!(dark.accent, Color32::from_rgb(0x2a, 0x66, 0xd6));
        assert_eq!(light.warning, Color32::from_rgb(0x6e, 0x3a, 0x00));
        assert_eq!(dark.warning, Color32::from_rgb(0xf5, 0xbd, 0x55));
        assert_eq!(light.danger, Color32::from_rgb(0xb3, 0x26, 0x1e));
        assert_eq!(dark.danger, Color32::from_rgb(0xff, 0xb9, 0xb1));
        assert_eq!(light.ok, Color32::from_rgb(0x1c, 0x6b, 0x3a));
        assert_eq!(dark.ok, Color32::from_rgb(0x77, 0xdb, 0xa0));
        assert_eq!(light.wash_base, Color32::from_rgb(0xdf, 0xe8, 0xf5));
        assert_eq!(dark.wash_base, Color32::from_rgb(0x0b, 0x14, 0x22));
        assert_eq!(light.veil, Color32::from_rgb(0xff, 0xff, 0xff));
        assert_eq!(dark.veil, Color32::from_rgb(0x06, 0x0d, 0x18));
        assert_eq!(light.selection, Color32::from_rgba_unmultiplied(0, 0, 0, 0x1f));
        assert_eq!(dark.selection, Color32::from_rgba_unmultiplied(255, 255, 255, 0x24));
        assert_eq!(light.field, Color32::from_rgba_unmultiplied(255, 255, 255, 0x99));
        assert_eq!(dark.field, Color32::from_rgba_unmultiplied(255, 255, 255, 0x1a));
        assert_eq!(light.scrim, Color32::from_rgba_unmultiplied(0, 0, 0, 0x33));
        assert_eq!(dark.scrim, Color32::from_rgba_unmultiplied(0, 0, 0, 0x73));
        assert_eq!(light.danger_tint, Color32::from_rgba_unmultiplied(0xb3, 0x26, 0x1e, 0x1a));
        assert_eq!(dark.danger_tint, Color32::from_rgba_unmultiplied(0x6b, 0x1d, 0x18, 0xcc));
        assert_eq!(light.on_dark_glass, Color32::WHITE);
        assert_eq!(light.quiet, Color32::from_rgba_unmultiplied(0, 0, 0, 173));
        assert_eq!(dark.quiet, Color32::from_rgba_unmultiplied(255, 255, 255, 173));
    }

    #[test]
    fn every_backdrop_has_its_veil_in_both_themes() {
        let want = [
            ("lake", [0.0, 0.0], [0.5, 0.82]),
            ("golden", [0.0, 0.0], [0.5, 0.82]),
            ("midday", [0.0, 0.0], [0.45, 0.72]),
            ("forest", [0.0, 0.0], [0.5, 0.82]),
            ("dusk", [0.2, 0.62], [0.2, 0.5]),
            ("night", [0.5, 0.82], [0.1, 0.35]),
        ];
        for (key, light, dark) in want {
            assert_eq!(veil(key, Mode::Light), light, "{key}");
            assert_eq!(veil(key, Mode::Dark), dark, "{key}");
        }
        assert_eq!(veil("moon", Mode::Light), veil("lake", Mode::Light)); // an unknown key is the default backdrop
    }

    #[test]
    fn the_surfaces_are_the_designs_materials() {
        let size = (256.0, 876.0);
        assert!(params(Surface::Sidebar, Mode::Light, size).glass);
        assert!(params(Surface::Capsule, Mode::Dark, (120.0, 36.0)).glass);
        assert!(params(Surface::StatusBar, Mode::Light, (1148.0, 32.0)).glass);
        assert!(!params(Surface::Panel, Mode::Light, (1148.0, 600.0)).glass);
        assert!(params(Surface::Sidebar, Mode::Dark, size).dark);
        assert!(!params(Surface::Sidebar, Mode::Light, size).dark);
        for surface in [Surface::Sidebar, Surface::Capsule, Surface::StatusBar, Surface::Panel] {
            for mode in [Mode::Light, Mode::Dark] {
                assert!(Material::read(&material_value(surface, mode)).is_some(), "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn the_mode_follows_the_theme_setting_or_the_system() {
        assert_eq!(Mode::of(Theme::Light, true), Mode::Light);
        assert_eq!(Mode::of(Theme::Dark, false), Mode::Dark);
        assert_eq!(Mode::of(Theme::System, true), Mode::Dark);
        assert_eq!(Mode::of(Theme::System, false), Mode::Light);
    }
}
