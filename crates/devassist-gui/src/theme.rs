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
pub const QUIET_OPACITY: f32 = 0.74;
/// How far up from the window's bottom the veil keeps its lower strength, as a share of the height; above, it
/// rises evenly to its strength at the top.
pub const VEIL_KNEE: f32 = 0.45;
/// The width of one of the window's own caption buttons.
pub const CAPTION_BUTTON: f32 = 40.0;
/// The room kept at the left of the top bar for the traffic lights on macOS.
pub const TRAFFIC_LIGHT_SPACE: f32 = 90.0;
/// How far in from the window's edge a press resizes it, where the window draws its frame itself. Quill's value.
pub const WINDOW_EDGE: f32 = 6.0;
/// How far along an edge from a corner a press resizes the window both ways. Quill's value.
pub const WINDOW_CORNER: f32 = 16.0;
/// The room between the sidebar's sides and foot and its rows.
pub const SIDEBAR_PADDING: f32 = 8.0;
/// The room between the sidebar's top and its first row.
pub const SIDEBAR_TOP: f32 = 16.0;
/// The room inside a row before its icon or text.
pub const ROW_PADDING: f32 = 8.0;
/// The gap between an icon and its text.
pub const ICON_GAP: f32 = 8.0;
/// The gap between two rows of the sidebar.
pub const ROW_GAP: f32 = 4.0;
/// The gap before each group of the sidebar's rows.
pub const GROUP_GAP: f32 = 20.0;
/// The height of a group's label in the sidebar.
pub const LABEL_ROW: f32 = 24.0;
/// The gap between a group's label and its first row.
pub const LABEL_GAP: f32 = 8.0;
/// The side of the accent square that marks the workspace at the sidebar's top.
pub const WORKSPACE_MARK: f32 = 32.0;
/// The gap between the sidebar button and the title.
pub const BAR_GAP: f32 = 12.0;
/// The room the top row keeps for the title, from its left edge; the title is shorter, and the window's own
/// buttons start further right.
pub const TITLE_ROOM: f32 = 472.0;
/// The closest any text comes to a panel's edge: the smaller of the two side paddings the design's table rows keep
/// (12 at the left, 8 at the right).
pub const PANEL_TEXT_INSET: f32 = 8.0;
/// The room at either end of the status bar before its text.
pub const STATUS_PADDING: f32 = 16.0;

/// The height of a repository row in the table.
pub const ROW_HEIGHT: f32 = 44.0;
/// The height of the table's heading row.
pub const HEAD_HEIGHT: f32 = 40.0;
/// The height of a group's heading row in the table.
pub const GROUP_HEIGHT: f32 = 36.0;
/// The room at the left of a table row before its first column.
pub const ROW_PAD_LEFT: f32 = 12.0;
/// The room at the right of a table row after its last column.
pub const ROW_PAD_RIGHT: f32 = 8.0;
/// The gap between two columns of the table.
pub const COLUMN_GAP: f32 = 12.0;
/// The width of the column with the row's checkbox.
pub const COLUMN_CHECK: f32 = 24.0;
/// The width of the column with the repository's folder.
pub const COLUMN_FOLDER: f32 = 176.0;
/// The width of the column with the checked-out branch.
pub const COLUMN_BRANCH: f32 = 136.0;
/// The width of the column that says whether the checkout has uncommitted changes.
pub const COLUMN_CHANGES: f32 = 104.0;
/// The width of the column with the ahead and behind rule and its counts.
pub const COLUMN_PULLS: f32 = 134.0;
/// The width of the column with the origin's address.
pub const COLUMN_ORIGIN: f32 = 288.0;
/// The width of the column with the row's menu button.
pub const COLUMN_ACTION: f32 = 32.0;
/// The least width the name column keeps; it takes what the other columns leave.
pub const COLUMN_NAME_LEAST: f32 = 120.0;
/// How many points one commit takes in the ahead and behind rule.
pub const RULE_UNIT: f32 = 8.0;
/// How many commits the rule shows each way from its middle; more are drawn as this many.
pub const RULE_HALF: u32 = 10;
/// The width of the rule: its commits each way, at [`RULE_UNIT`] points each, and one more point for the middle
/// line.
pub const RULE_WIDTH: f32 = rule_width();
/// The height of the rule.
pub const RULE_HEIGHT: f32 = 16.0;
/// The height of the bar that shows the commits ahead or behind.
pub const RULE_BAR: f32 = 6.0;
/// The distance from the rule's top to its bar.
pub const RULE_BAR_TOP: f32 = 2.0;
/// The height of the rule's small tick at each commit.
pub const RULE_TICK: f32 = 4.0;
/// The height of the taller tick at every fifth commit.
pub const RULE_TICK_FIFTH: f32 = 7.0;
/// The gap between the rule and the number beside it.
pub const RULE_LABEL_GAP: f32 = 12.0;
/// How strong the rule's small ticks are, of the text colour.
pub const RULE_TICK_OPACITY: f32 = 0.28;
/// How strong the rule's taller ticks are, of the text colour.
pub const RULE_FIFTH_OPACITY: f32 = 0.5;
/// The room between a view capsule's edge and the controls in it.
pub const CAPSULE_PAD: f32 = 4.0;
/// The gap between two controls in a capsule.
pub const CAPSULE_GAP: f32 = 2.0;
/// The height of an action button.
pub const ACTION_HEIGHT: f32 = 28.0;
/// The room inside an action button before its first item and after its last, left then right.
pub const ACTION_PAD: [f32; 2] = [8.0, 10.0];
/// The gap between the items of an action button, and between action buttons.
pub const ACTION_GAP: f32 = 6.0;
/// The width of a search field.
pub const FIELD_WIDTH: f32 = 280.0;
/// The room inside a search field before its icon.
pub const FIELD_PAD: f32 = 12.0;
/// The side of a checkbox.
pub const CHECKBOX: f32 = 16.0;
/// The gap between the fold arrow, the name and the note in a group's heading row.
pub const GROUP_ROW_GAP: f32 = 8.0;
/// The gap between the controls in the bar above the table.
pub const TOOLBAR_GAP: f32 = 8.0;
/// How strong the lines of the table are, of the text colour.
pub const LINE_OPACITY: f32 = 0.10;
/// How strong the fill of the table's heading row is, of the text colour.
pub const HEAD_FILL_OPACITY: f32 = 0.06;
/// How strong the fill of a group's heading row is, of the text colour.
pub const GROUP_FILL_OPACITY: f32 = 0.03;
/// How strong the fill under the pointer is, of the text colour.
pub const HOVER_OPACITY: f32 = 0.05;
/// How strong the outline of an unchecked checkbox is, of the text colour.
pub const CHECKBOX_STROKE_OPACITY: f32 = 0.4;
/// How strong the fill of a selected row is in the light mode, of the accent colour.
pub const SELECTED_OPACITY_LIGHT: f32 = 0.14;
/// How strong the fill of a selected row is in the dark mode, of the accent colour.
pub const SELECTED_OPACITY_DARK: f32 = 0.32;

/// The width of the lines under the table's rows, of the rule's ticks and of its zero line (`LINE`'s
/// `strokeWidth: { bottom: 1 }`, the ticks' `size: 1` and the origin's `width: 1` in `v5-app.mjs`).
pub const HAIRLINE: f32 = 1.0;
/// The size of a small icon: the watched eye, the sort arrow and the pull request states (`icon(…, 14)` in
/// `reposTable` and `pullCell`).
pub const ICON_SMALL: f32 = 14.0;
/// The size of the mark in a ticked checkbox (`icon("check", WHITE, 12)` in `checkbox`).
pub const CHECK_MARK: f32 = 12.0;
/// The gap between a repository's name and its eye (the `Name` row's `gap: 8` in `reposTable`).
pub const NAME_ICON_GAP: f32 = 8.0;
/// The gap between an icon and its text in a cell: the branch, the sort arrow, a failure (`gap: 6` in
/// `reposTable` and `origin`).
pub const CELL_ICON_GAP: f32 = 6.0;
/// The gap between a pull request state's icon and its number (`piece`'s `gap: 4` in `pullCell`).
pub const PULL_ICON_GAP: f32 = 4.0;
/// The gap between the parts of the pull requests cell (`gap: 8` in `pullCell`).
pub const PULL_GAP: f32 = 8.0;
/// The gap between the rule's zero line and the head's words "Behind" (ending before it) and "Ahead" (starting
/// after it): the legend's `fixed("Behind", ZERO - 8, …)` and `gap: 17` (8, the line, 8) in `reposTable`.
pub const RULE_LEGEND_GAP: f32 = 8.0;
/// The gap that breaks a bar past the rule's scale, near its outer end (the fork table's "Ahead, past the
/// scale", from 141 to 147, in `forward`).
pub const RULE_BREAK_GAP: f32 = 6.0;
/// The length of a broken bar's outer piece past the gap (the fork table's 14 in `forward`).
pub const RULE_BREAK_END: f32 = 14.0;
/// The height of a progress bar; its ends are round (`bar`'s `height: 6`, `cornerRadius: 3`).
pub const PROGRESS_HEIGHT: f32 = 6.0;
/// The least length of a progress bar's filled part (`Math.max(6, …)` in `bar`).
pub const PROGRESS_LEAST: f32 = 6.0;
/// How strong a progress bar's track is, of the text colour (`fill: soft(0.12)` in `bar`).
pub const PROGRESS_TRACK_OPACITY: f32 = 0.12;

/// The value of [`RULE_WIDTH`].
#[expect(clippy::cast_precision_loss, reason = "RULE_HALF is a few commits, far below what an f32 holds exactly")]
const fn rule_width() -> f32 {
    (2 * RULE_HALF) as f32 * RULE_UNIT + 1.0
}

// Tuning, not design: how fine, how long and when the window makes and keeps its pictures. These set the work
// and memory the glass takes, not the design's look.

/// How many pixels of a blurred picture one standard deviation of its blur spans: a blurred picture holds no
/// detail finer than its blur, so it is made only this fine.
pub const COARSE_SIGMA_PX: f32 = 4.0;
/// The fewest pixels a coarse picture has along the window's shorter side.
pub const COARSE_LEAST_PIXELS: f32 = 8.0;
/// A pane's picture that no frame has wanted for this many frames is dropped.
pub const PICTURE_KEEP_FRAMES: u64 = 120;
/// How many frames a `--screenshot` run draws before it takes the picture (fonts, layout and the glass have
/// settled by then).
pub const FRAMES_BEFORE_SHOT: u64 = 120;
/// How long the window must be left alone after a resize before its glass is redrawn at the new size.
pub const RESIZE_SETTLE: Duration = Duration::from_millis(150);
/// How long the window may sleep between two looks at the core's workers while the core is busy.
pub const TICK_BUSY: Duration = Duration::from_millis(50);
/// How long the window may sleep between two looks at the core while it is idle (the core's own timers, such as
/// the automatic pull of watched repositories, still run on these looks).
pub const TICK_IDLE: Duration = Duration::from_secs(1);
/// How long the demo's repositories are left alone by the core's automatic pull: the demo's folders are not on
/// any disk, so the pull must not start (30 days).
pub const DEMO_WATCH_HOLD: Duration = Duration::from_hours(30 * 24);

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
            veil: rgb(0x06, 0x0d, 0x18),
        },
    }
}

/// The fill of a selected row in `mode`: the accent colour at [`SELECTED_OPACITY_LIGHT`] or
/// [`SELECTED_OPACITY_DARK`].
#[must_use]
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "an opacity between 0 and 1 times 255 rounds to 0..=255")]
pub fn selected_fill(mode: Mode) -> Color32 {
    let opacity = match mode {
        Mode::Light => SELECTED_OPACITY_LIGHT,
        Mode::Dark => SELECTED_OPACITY_DARK,
    };
    let accent = palette(mode).accent;
    Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), (255.0 * opacity).round() as u8)
}

/// The veil's two strengths over the backdrop `backdrop`: the first in the lower part of the window (its bottom
/// [`VEIL_KNEE`] of the height), the second at the very top; between them it rises evenly ([`veil_at`]). A key
/// that names no backdrop gets the values of `"lake"`.
#[must_use]
pub fn veil(backdrop: &str, mode: Mode) -> [f32; 2] {
    match (backdrop, mode) {
        ("midday", Mode::Dark) => [0.45, 0.72],
        ("dusk", Mode::Light) => [0.2, 0.62],
        ("dusk", Mode::Dark) => [0.2, 0.5],
        ("night", Mode::Dark) => [0.1, 0.35],
        // The lake, the forest and the golden hour in the dark mode, and the night in the light one.
        ("night", Mode::Light) | (_, Mode::Dark) => [0.5, 0.82],
        (_, Mode::Light) => [0.0, 0.0],
    }
}

/// The veil's strength `down` the window (0 at the top, 1 at the bottom) when its two numbers are `strengths`
/// (the strength in the lower part, the strength at the top): the lower one from the bottom up to
/// [`VEIL_KNEE`] of the height, rising evenly from there to the higher one at the very top.
#[must_use]
pub fn veil_at(strengths: [f32; 2], down: f32) -> f32 {
    let rise = ((1.0 - down - VEIL_KNEE) / (1.0 - VEIL_KNEE)).clamp(0.0, 1.0);
    (strengths[0] + (strengths[1] - strengths[0]) * rise).clamp(0.0, 1.0)
}

/// The veil's strength over the backdrop `backdrop` in `mode`, `down` the window (0 at the top, 1 at the bottom).
#[must_use]
pub fn veil_strength(backdrop: &str, mode: Mode, down: f32) -> f32 {
    veil_at(veil(backdrop, mode), down)
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

impl Surface {
    /// Every surface.
    pub const ALL: [Surface; 4] = [Surface::Sidebar, Surface::Capsule, Surface::StatusBar, Surface::Panel];

    /// Whether the surface is glass, whose panes are baked into pictures of their own; a frosted one is drawn
    /// from the baker's sheet instead ([`crate::frost`]).
    #[must_use]
    pub fn is_glass(self) -> bool {
        match self {
            Surface::Sidebar | Surface::Capsule | Surface::StatusBar => true,
            Surface::Panel => false,
        }
    }
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
        assert_eq!(light.quiet, Color32::from_rgba_unmultiplied(0, 0, 0, 189));
        assert_eq!(dark.quiet, Color32::from_rgba_unmultiplied(255, 255, 255, 189));
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
        assert_eq!(veil("moon", Mode::Light), veil("lake", Mode::Light));
        assert_eq!(veil("moon", Mode::Dark), veil("lake", Mode::Dark)); // an unknown key is the default backdrop
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
        for surface in Surface::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                assert!(Material::read(&material_value(surface, mode)).is_some(), "{surface:?} {mode:?}");
            }
        }
    }

    #[test]
    fn a_surface_is_baked_exactly_when_its_material_is_glass() {
        for surface in Surface::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                assert_eq!(surface.is_glass(), params(surface, mode, (300.0, 200.0)).glass, "{surface:?} {mode:?}");
            }
        }
        assert!(!Surface::Panel.is_glass());
    }

    #[test]
    fn each_surface_names_its_material_words_exactly() {
        use material::{Appearance, Depth, Kind, Thickness, Variant};
        let glass = Kind::Glass(Variant::Regular);
        let want = [
            (Surface::Sidebar, glass, Some(Depth::Panel)),
            (Surface::Capsule, glass, None),
            (Surface::StatusBar, glass, Some(Depth::Bar)),
            (Surface::Panel, Kind::Frosted(Thickness::Thick), None),
        ];
        for (surface, kind, depth) in want {
            for (mode, appearance) in [(Mode::Light, Appearance::Light), (Mode::Dark, Appearance::Dark)] {
                let Some(read) = Material::read(&material_value(surface, mode)) else {
                    panic!("{surface:?} {mode:?} does not read");
                };
                assert_eq!(read.kind, kind, "{surface:?} {mode:?}");
                assert_eq!(read.depth, depth, "{surface:?} {mode:?}");
                assert_eq!(read.appearance, appearance, "{surface:?} {mode:?}");
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

    #[test]
    fn the_table_is_the_designs() {
        let want = [
            ("ROW_HEIGHT", ROW_HEIGHT, 44.0),
            ("HEAD_HEIGHT", HEAD_HEIGHT, 40.0),
            ("GROUP_HEIGHT", GROUP_HEIGHT, 36.0),
            ("ROW_PAD_LEFT", ROW_PAD_LEFT, 12.0),
            ("ROW_PAD_RIGHT", ROW_PAD_RIGHT, 8.0),
            ("COLUMN_GAP", COLUMN_GAP, 12.0),
            ("COLUMN_CHECK", COLUMN_CHECK, 24.0),
            ("COLUMN_FOLDER", COLUMN_FOLDER, 176.0),
            ("COLUMN_BRANCH", COLUMN_BRANCH, 136.0),
            ("COLUMN_CHANGES", COLUMN_CHANGES, 104.0),
            ("COLUMN_PULLS", COLUMN_PULLS, 134.0),
            ("COLUMN_ORIGIN", COLUMN_ORIGIN, 288.0),
            ("COLUMN_ACTION", COLUMN_ACTION, 32.0),
            ("COLUMN_NAME_LEAST", COLUMN_NAME_LEAST, 120.0),
            ("RULE_UNIT", RULE_UNIT, 8.0),
            ("RULE_HEIGHT", RULE_HEIGHT, 16.0),
            ("RULE_BAR", RULE_BAR, 6.0),
            ("RULE_BAR_TOP", RULE_BAR_TOP, 2.0),
            ("RULE_TICK", RULE_TICK, 4.0),
            ("RULE_TICK_FIFTH", RULE_TICK_FIFTH, 7.0),
            ("RULE_LABEL_GAP", RULE_LABEL_GAP, 12.0),
            ("RULE_TICK_OPACITY", RULE_TICK_OPACITY, 0.28),
            ("RULE_FIFTH_OPACITY", RULE_FIFTH_OPACITY, 0.5),
            ("CAPSULE_PAD", CAPSULE_PAD, 4.0),
            ("CAPSULE_GAP", CAPSULE_GAP, 2.0),
            ("ACTION_HEIGHT", ACTION_HEIGHT, 28.0),
            ("ACTION_GAP", ACTION_GAP, 6.0),
            ("FIELD_WIDTH", FIELD_WIDTH, 280.0),
            ("FIELD_PAD", FIELD_PAD, 12.0),
            ("CHECKBOX", CHECKBOX, 16.0),
            ("TOOLBAR_GAP", TOOLBAR_GAP, 8.0),
            ("LINE_OPACITY", LINE_OPACITY, 0.10),
            ("HEAD_FILL_OPACITY", HEAD_FILL_OPACITY, 0.06),
            ("GROUP_FILL_OPACITY", GROUP_FILL_OPACITY, 0.03),
            ("HOVER_OPACITY", HOVER_OPACITY, 0.05),
            ("CHECKBOX_STROKE_OPACITY", CHECKBOX_STROKE_OPACITY, 0.4),
            ("SELECTED_OPACITY_LIGHT", SELECTED_OPACITY_LIGHT, 0.14),
            ("SELECTED_OPACITY_DARK", SELECTED_OPACITY_DARK, 0.32),
            ("GROUP_ROW_GAP", GROUP_ROW_GAP, 8.0),
            ("HAIRLINE", HAIRLINE, 1.0),
            ("ICON_SMALL", ICON_SMALL, 14.0),
            ("CHECK_MARK", CHECK_MARK, 12.0),
            ("NAME_ICON_GAP", NAME_ICON_GAP, 8.0),
            ("CELL_ICON_GAP", CELL_ICON_GAP, 6.0),
            ("PULL_ICON_GAP", PULL_ICON_GAP, 4.0),
            ("PULL_GAP", PULL_GAP, 8.0),
            ("RULE_LEGEND_GAP", RULE_LEGEND_GAP, 8.0),
            ("RULE_BREAK_GAP", RULE_BREAK_GAP, 6.0),
            ("RULE_BREAK_END", RULE_BREAK_END, 14.0),
            ("PROGRESS_HEIGHT", PROGRESS_HEIGHT, 6.0),
            ("PROGRESS_LEAST", PROGRESS_LEAST, 6.0),
            ("PROGRESS_TRACK_OPACITY", PROGRESS_TRACK_OPACITY, 0.12),
        ];
        for (constant, got, expected) in want {
            assert!((got - expected).abs() < f32::EPSILON, "{constant} is {got}, the design has {expected}");
        }
        assert_eq!(RULE_HALF, 10);
        assert_eq!(ACTION_PAD, [8.0, 10.0]);
    }

    #[test]
    fn the_rule_is_twice_its_half_in_commits_and_the_middle_line() {
        let commits = f32::from(u16::try_from(2 * RULE_HALF).unwrap());
        assert!((RULE_WIDTH - (commits * RULE_UNIT + 1.0)).abs() < f32::EPSILON);
        assert!((RULE_WIDTH - 161.0).abs() < f32::EPSILON);
    }

    #[test]
    fn the_columns_fit_the_design_width() {
        let panel = 1148.0;
        let fixed = COLUMN_CHECK + COLUMN_FOLDER + COLUMN_BRANCH + COLUMN_CHANGES + COLUMN_PULLS + COLUMN_ORIGIN + COLUMN_ACTION;
        let columns = 8.0; // the seven fixed ones and the name
        let name = panel - fixed - (columns - 1.0) * COLUMN_GAP - ROW_PAD_LEFT - ROW_PAD_RIGHT;
        assert!(name >= COLUMN_NAME_LEAST, "the name gets {name} of the {panel} points, it needs {COLUMN_NAME_LEAST}");
    }

    #[test]
    fn the_selected_fill_is_the_accent_at_its_two_opacities() {
        for (mode, alpha) in [(Mode::Light, 36), (Mode::Dark, 82)] {
            let a = palette(mode).accent;
            assert_eq!(selected_fill(mode), Color32::from_rgba_unmultiplied(a.r(), a.g(), a.b(), alpha), "{mode:?}");
        }
    }
}
