//! Measures the design's promise that its text is readable on all six backdrops in both modes, on the pictures
//! the program makes: the wallpaper (scaled up as the screen shows it) with the shell's panes laid over it.
//!
//! Only where text can sit is measured: inside the padding the shell gives text in each pane, and only on pixels
//! where the pane's own picture is (all but) opaque. The rounded corners and the antialiased edge of a pane show
//! the bare wallpaper through it, and no text is ever drawn there. The panel is also measured only inside
//! `PANEL_TEXT_INSET`, the closest any text comes to its edge (its rim highlight is no place for text).
//!
//! Both renderings are measured: the glass panes frosted on the CPU, as a window draws them when the glass shader
//! cannot run, and, where there is a GPU adapter, the glass panes through the shader, as people with a GPU see
//! them. The frosted panel is the same in both: the baker's sheet in the panel's rounded rectangle with its edge,
//! tessellated by egui as the window paints it ([`crate::frost`]) and laid over the wallpaper.
//!
//! The Repositories table is measured on its panel, the one under the screen's toolbar, with the demo's lines at
//! the rows and columns the table gives them ([`crate::columns`], the rows' heights in [`crate::theme`]). Each ink
//! is measured in the cells it is drawn in, at the height of a line of its text, over each fill the table can put
//! under it: the head row's, a group's, a ticked row's, the highlighted line's, and the hover over a line without one of these. Every row
//! of the demo is measured in every state, because any row can be ticked, highlighted or under the pointer. The
//! fills are laid over the panel's pixels as the window lays them, in the order the table paints them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::thread;

use devassist_core::config::BACKDROPS;
use devassist_core::repos::state::{MenuItem, Repos};
use devassist_glass::raster::{self, Bitmap, Pixel};
use egui::epaint::text::LayoutSection;
use egui::epaint::{ClippedShape, Mesh, RectShape, Shape, TessellationOptions, Tessellator, TextShape};
use egui::{Color32, ColorImage, Context, FontFamily, FontId, Pos2, RawInput, Rect, StrokeKind, TextureId, pos2, vec2};

use crate::assets::{self, Icon};
use crate::baker::{Drawn, Scene, Sheet, SheetPicture, Wallpaper, Want, frosted_pane_picture, pane_picture, sheet_picture};
use crate::columns::{self, Span};
use crate::dialogs::{self, Dialogs};
use crate::picture::{self, over, wallpaper_window};
use crate::repos_screen::{self, Screen as ReposScreen};
use crate::repos_view::{self, Line};
use crate::session::Session;
use crate::shell::{Page, Shown as ShellShown};
use crate::theme::{
    self, ADD_ROW, BUTTON_OFF_OPACITY, CHECK_MARK, FOCUS_RING, GROUP_FILL_OPACITY, GROUP_ROW_GAP, HEAD_FILL_OPACITY, HEAD_HEIGHT, HOVER_OPACITY,
    MIN_WINDOW, Mode, PANEL_TEXT_INSET, PROGRESS_HEIGHT, PROGRESS_TRACK_OPACITY, Palette, RADIUS_PANEL, ROW_PAD_RIGHT, ROW_PADDING, RULE_LABEL_GAP,
    RULE_WIDTH, SIDEBAR_PADDING, SIDEBAR_TOP, STATUS_PADDING, Surface, TEXT, TEXT_SMALL, TEXT_TITLE, WINDOW,
};
use crate::{demo, frost, overlay, shell, table, widgets};

/// The least contrast of text at `TEXT` (WCAG's level AA for normal text).
const LEAST_TEXT: f64 = 4.5;
/// The least contrast of the quiet text at `TEXT_SMALL`.
const LEAST_QUIET: f64 = 6.0;
/// A pane's pixel counts as the pane (not its edge) from this alpha.
const SOLID: f32 = 0.99;
/// The fewest pixels a place may leave to measure: a mask or rectangle that leaves (almost) none would pass
/// without having checked anything. The smallest real place, a group label's line, has several thousand.
const LEAST_PIXELS: usize = 100;
/// The fewest pixels of a focus ring: its straight runs, two points wide, around a dialog's button of a few words.
const LEAST_RING_PIXELS: usize = 60;
/// The room, in points, kept above and below a line of text.
const LINE_SLACK: f32 = 2.0;
/// The least contrast of a graphic: the check in a ticked box (WCAG's level AA for non-text contrast).
const LEAST_GRAPHIC: f64 = 3.0;

/// Which colour a place's text is drawn in.
#[derive(Clone, Copy)]
enum Ink {
    /// `on_glass`, opaque.
    OnGlass,
    /// `quiet`, laid over the pixel it is on.
    Quiet,
    /// `warning`: the words of a repository that is behind, and the pull requests waiting.
    Warning,
    /// `danger`: what went wrong with a repository, as "Not found on GitHub".
    Danger,
    /// `on_dark_glass`: the check in a ticked box.
    OnDarkGlass,
}

impl Ink {
    fn colour(self, palette: Palette) -> Color32 {
        match self {
            Ink::OnGlass => palette.on_glass,
            Ink::Quiet => palette.quiet,
            Ink::Warning => palette.warning,
            Ink::Danger => palette.danger,
            Ink::OnDarkGlass => palette.on_dark_glass,
        }
    }
}

/// A fill the table paints under its text, as [`crate::table`] paints it.
#[derive(Clone, Copy)]
enum Layer {
    /// The head row's fill.
    Head,
    /// A group's header's fill.
    Group,
    /// A ticked row's fill, [`theme::selected_fill`].
    Ticked,
    /// The highlighted line's fill (the keyboard's place), the palette's `selection`.
    Highlighted,
    /// The pointer over a line.
    Hover,
    /// A ticked box, filled with the accent colour.
    Accent,
}

impl Layer {
    fn colour(self, palette: Palette, mode: Mode) -> Color32 {
        match self {
            Layer::Head => palette.on_glass.gamma_multiply(HEAD_FILL_OPACITY),
            Layer::Group => palette.on_glass.gamma_multiply(GROUP_FILL_OPACITY),
            Layer::Ticked => theme::selected_fill(mode),
            Layer::Highlighted => palette.selection,
            Layer::Hover => palette.on_glass.gamma_multiply(HOVER_OPACITY),
            Layer::Accent => palette.accent,
        }
    }
}

/// The fills a repository's row can have, bottom first, with the name its places get: the table paints one row
/// fill (the ticked one when a row is also highlighted), and the hover only over a row without one.
const ROW_FILLS: [(&str, &[Layer]); 4] =
    [("plain", &[]), ("ticked", &[Layer::Ticked]), ("highlighted", &[Layer::Highlighted]), ("hovered", &[Layer::Hover])];

/// The fills a group's header can have, bottom first, with the contrast its quiet note needs: the group's own,
/// the highlight over it, or the hover over it (never over a highlighted header). On a plain header the note needs
/// `LEAST_QUIET`, as small quiet text does everywhere. On a highlighted or a hovered header it needs `LEAST_TEXT`,
/// WCAG's level AA: the fill over the group's lightens the panel so far that even full-strength text cannot reach
/// 6.0 on a highlighted header in the dark mode (the user's ruling of 2026-11-25, for the hover too).
const GROUP_FILLS: [(&str, &[Layer], f64); 3] = [
    ("group", &[Layer::Group], LEAST_QUIET),
    ("group+highlighted", &[Layer::Group, Layer::Highlighted], LEAST_TEXT),
    ("group+hovered", &[Layer::Group, Layer::Hover], LEAST_TEXT),
];

/// Which of the two panels a place is on.
#[derive(Clone, Copy)]
enum Screen {
    /// The panel of a screen without a toolbar, as the shell's places measure it.
    Bare,
    /// The panel under the Repositories screen's toolbar, which holds the table.
    Table,
}

/// How the glass panes are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rendering {
    /// Every glass pane frosted on the CPU.
    Frosted,
    /// The glass panes through the glass shader on the GPU; the panel is drawn from its sheet in both.
    Glass,
}

/// A place text is on: its name, the panel it is on, the pane whose picture it is on (none: the bare wallpaper),
/// where text can sit, the fills under it (bottom first), its ink and the contrast it needs.
struct Place {
    name: String,
    screen: Screen,
    pane: Option<&'static str>,
    rect: Rect,
    under: &'static [Layer],
    ink: Ink,
    least: f64,
}

impl Place {
    /// A place of the shell, with no fill under it.
    fn shell(name: &str, pane: Option<&'static str>, rect: Rect, ink: Ink, least: f64) -> Place {
        Place { name: name.to_owned(), screen: Screen::Bare, pane, rect, under: &[], ink, least }
    }

    /// A place on the table's panel.
    fn table(name: String, rect: Rect, under: &'static [Layer], ink: Ink, least: f64) -> Place {
        Place { name, screen: Screen::Table, pane: Some("panel"), rect, under, ink, least }
    }
}

/// One pane's picture and where it lies in the window, in pixels.
struct Laid {
    id: &'static str,
    left: usize,
    top: usize,
    picture: Bitmap,
}

/// The window as the screen shows it, and the pictures of the panes it is made of.
struct Window {
    bitmap: Bitmap,
    panes: Vec<Laid>,
}

/// The window with each of the two panels.
struct Windows {
    bare: Window,
    table: Window,
}

impl Windows {
    fn on(&self, screen: Screen) -> &Window {
        match screen {
            Screen::Bare => &self.bare,
            Screen::Table => &self.table,
        }
    }
}

/// The lowest contrast found in one place, the pixel it is at and how many pixels were measured.
#[derive(Clone, Copy)]
struct Lowest {
    contrast: f64,
    at: (usize, usize),
    count: usize,
}

/// The light of a display-colour channel in 0..=1, linearised as WCAG does.
fn linear(channel: f32) -> f64 {
    let c = f64::from(channel.clamp(0.0, 1.0));
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// The relative luminance of an opaque pixel.
pub(crate) fn luminance(pixel: Pixel) -> f64 {
    0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
}

/// WCAG's contrast ratio of two luminances.
pub(crate) fn ratio(a: f64, b: f64) -> f64 {
    let (high, low) = if a > b { (a, b) } else { (b, a) };
    (high + 0.05) / (low + 0.05)
}

/// The frosted panel at `panel` in a window of `w` by `h` points at one pixel per point, painted as the window
/// paints it: the shapes [`frost::paint`] adds, tessellated by egui and filled from `sheet` read between its
/// pixels, as the GPU's linear sampler reads the texture. A picture of the whole window, clear outside the panel.
fn painted_panel(sheet: &SheetPicture, panel: Rect, w: usize, h: usize) -> Bitmap {
    let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
    // The texture's id does not matter here: the fill's mesh is painted from the sheet's picture.
    let painted = Sheet { texture: TextureId::User(0), uv: sheet.uv, edge: sheet.edge };
    let texture = sheet.bitmap.image();
    let mut tessellator = Tessellator::new(1.0, TessellationOptions::default(), [1, 1], Vec::new());
    let mut picture = Bitmap::new(w, h);
    for shape in frost::shapes(window, panel, RADIUS_PANEL, &painted) {
        let textured = shape.brush.is_some();
        let mut mesh = Mesh::default();
        tessellator.tessellate_shape(Shape::Rect(shape), &mut mesh);
        picture.triangles(&mesh, |p| [p.x, p.y], None, textured.then_some(&texture), false);
    }
    picture
}

/// A window of `WINDOW` points as pixels: the wallpaper scaled up with bilinear sampling, the shell's glass panes
/// and its frosted panel over it: once with the panel of a screen without a toolbar, once with the table's panel
/// under the toolbar. The glass panes are the same in both; the toolbar's own lie on no text of the panel.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel places on a 1440 by 900 window, far inside every range")]
fn windows(backdrop: &str, mode: Mode, rendering: Rendering) -> Windows {
    let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: backdrop.to_owned(), blur: true };
    let (wall, mut bitmap) = wallpaper_window(&scene);
    let (w, h) = (bitmap.w, bitmap.h);
    let mut panes = Vec::new();
    let mut shaded = false;
    for want in shell::wants(&shell::layout(vec2(WINDOW[0], WINDOW[1]), true, false)).as_slice().iter().cloned() {
        let frosted = frosted_pane_picture(&wall, &scene, &want).unwrap();
        let picture = match rendering {
            Rendering::Frosted => frosted,
            Rendering::Glass => {
                let picture = pane_picture(&wall, &scene, &want).unwrap();
                shaded |= picture.px != frosted.px;
                picture
            }
        };
        let (left, top) = (want.rect.min.x.round() as usize, want.rect.min.y.round() as usize);
        for y in 0..picture.h.min(h - top) {
            for x in 0..picture.w.min(w - left) {
                let at = (top + y) * w + left + x;
                bitmap.px[at] = over(bitmap.px[at], picture.px[y * picture.w + x]);
            }
        }
        panes.push(Laid { id: want.id, left, top, picture });
    }
    // Glass that fell back to frost would measure the frosted panes twice.
    assert!(rendering == Rendering::Frosted || shaded, "{backdrop} {mode:?}: no pane was drawn through the glass shader");
    let sheet = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
    let with_panel = |toolbar: bool, mut bitmap: Bitmap, mut panes: Vec<Laid>| {
        let painted = painted_panel(&sheet, shell::layout(vec2(WINDOW[0], WINDOW[1]), true, toolbar).panel, w, h);
        for (under, over_it) in bitmap.px.iter_mut().zip(&painted.px) {
            *under = over(*under, *over_it);
        }
        panes.push(Laid { id: "panel", left: 0, top: 0, picture: painted });
        Window { bitmap, panes }
    };
    // Every place of the table is on its panel, so the table's window keeps only the panel's picture.
    let table = with_panel(true, bitmap.clone(), Vec::new());
    Windows { bare: with_panel(false, bitmap, panes), table }
}

/// `rect` cut down to the band a line of text of `size` points occupies about its middle, with some room.
fn line_band(rect: Rect, size: f32) -> Rect {
    let half = size / 2.0 + LINE_SLACK;
    Rect::from_min_max(pos2(rect.left(), rect.center().y - half), pos2(rect.right(), rect.center().y + half))
}

/// The places to measure, from the shell's own geometry and the insets it gives its text.
fn places() -> Vec<Place> {
    let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, false);
    let wants = shell::wants(&layout);
    let rect = |id: &str| wants.as_slice().iter().find(|want| want.id == id).unwrap().rect;
    let sidebar = rect("sidebar");
    // The sidebar's rows lie inside `SIDEBAR_PADDING` at the sides and the foot and `SIDEBAR_TOP` below its top.
    let sidebar_text = Rect::from_min_max(
        pos2(sidebar.left() + SIDEBAR_PADDING, sidebar.top() + SIDEBAR_TOP),
        pos2(sidebar.right() - SIDEBAR_PADDING, sidebar.bottom() - SIDEBAR_PADDING),
    );
    // The status bar's text starts `STATUS_PADDING` in from each end and fills one line of `TEXT_SMALL`.
    let status = rect("status");
    let status_ends = Rect::from_min_max(pos2(status.left() + STATUS_PADDING, status.top()), pos2(status.right() - STATUS_PADDING, status.bottom()));
    // Text comes no closer to the panel's edge than `PANEL_TEXT_INSET`.
    let panel_text = layout.panel.shrink(PANEL_TEXT_INSET);
    let mut places = vec![
        Place::shell("sidebar", Some("sidebar"), sidebar_text, Ink::OnGlass, LEAST_TEXT),
        // The version line under the workspace's name is quiet text at `TEXT_SMALL`.
        Place::shell("sidebar-quiet", Some("sidebar"), sidebar_text, Ink::Quiet, LEAST_QUIET),
        // The panel's text is centred; its place is the panel inside `PANEL_TEXT_INSET`.
        Place::shell("panel", Some("panel"), panel_text, Ink::OnGlass, LEAST_TEXT),
        // The panel's sentence is quiet text at `TEXT` (normal size), as are the table's quiet cells.
        Place::shell("panel-quiet", Some("panel"), panel_text, Ink::Quiet, LEAST_TEXT),
        // The title can be longer than the one word now, so its whole room is measured, as one line.
        Place::shell("title", None, line_band(layout.title, TEXT_TITLE), Ink::OnGlass, LEAST_TEXT),
        Place::shell("status", Some("status"), line_band(status_ends, TEXT_SMALL), Ink::Quiet, LEAST_QUIET),
    ];
    // A group label's text starts `ROW_PADDING` in from its row's left and fills one line of `TEXT_SMALL`.
    for row in shell::label_rows(sidebar) {
        let text = Rect::from_min_max(pos2(row.left() + ROW_PADDING, row.top()), row.max);
        places.push(Place::shell("labels", Some("sidebar"), line_band(text, TEXT_SMALL), Ink::Quiet, LEAST_QUIET));
    }
    places.extend(table_places());
    places
}

/// The cell of `span` in the row at `row`, as the table places it.
fn cell(row: Rect, span: Span) -> Rect {
    Rect::from_min_max(pos2(row.left() + span.left, row.top()), pos2(row.left() + span.right(), row.bottom()))
}

/// The demo's lines on the table at `panel` as the table places them unscrolled, under its head row: each line's
/// row and whether it is a group's header.
fn demo_rows(panel: Rect) -> Vec<(Rect, bool)> {
    let mut top = table::body_rect(panel).top();
    repos_view::lines(&demo::repos())
        .iter()
        .map(|line| {
            let row = Rect::from_min_max(pos2(panel.left(), top), pos2(panel.right(), top + table::height(line)));
            top = row.bottom();
            (row, matches!(line, Line::Group(_)))
        })
        .collect()
}

/// The cells of `row` that `ink` is drawn in. `on_glass`: the name, the branch, "Uncommitted", the pull requests'
/// counts and the origin's plain words; `quiet`: those and the folder, "Clean" and the icons;
/// `warning`: the pull requests waiting and the words of a repository behind; `danger`: the origin's cell.
fn cells(row: Rect, columns: &columns::Columns, ink: Ink) -> Vec<Rect> {
    let words = || {
        let origin = cell(row, columns.origin);
        Rect::from_min_max(pos2(origin.left() + RULE_WIDTH + RULE_LABEL_GAP, origin.top()), origin.max)
    };
    let spans = match ink {
        Ink::OnGlass => vec![Some(columns.name), columns.branch, columns.changes, columns.pulls, Some(columns.origin)],
        Ink::Quiet => vec![Some(columns.name), columns.folder, columns.branch, columns.changes, columns.pulls, Some(columns.origin)],
        Ink::Warning => return columns.pulls.map(|span| cell(row, span)).into_iter().chain([words()]).collect(),
        Ink::Danger => vec![Some(columns.origin)],
        Ink::OnDarkGlass => Vec::new(),
    };
    spans.into_iter().flatten().map(|span| cell(row, span)).collect()
}

/// The places of the table's text on its panel, from the table's own geometry and the demo's lines.
fn table_places() -> Vec<Place> {
    let panel = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true).panel;
    let columns = columns::columns(panel.width());
    let rows = demo_rows(panel);
    assert!(rows.last().is_some_and(|(row, _)| row.bottom() <= panel.bottom()), "the demo's lines no longer fit the panel");
    let inks = [("on-glass", Ink::OnGlass), ("quiet", Ink::Quiet), ("warning", Ink::Warning), ("danger", Ink::Danger)];
    let mut places = Vec::new();

    // The head row's words, from the name's column to the end of "Ahead" in the origin's.
    let head = Rect::from_min_max(panel.min, pos2(panel.right(), panel.top() + HEAD_HEIGHT));
    let head_words = Rect::from_min_max(pos2(head.left() + columns.name.left, head.top()), pos2(head.left() + columns.origin.right(), head.bottom()));
    for (ink_name, ink) in inks {
        places.push(Place::table(format!("head-{ink_name}"), line_band(head_words, TEXT), &[Layer::Head], ink, LEAST_TEXT));
    }

    for &(row, group) in &rows {
        if group {
            // A group's name starts a gap after the fold arrow and keeps to the name's column; its note, quiet at
            // `TEXT_SMALL`, starts after the name, wherever that ends, and has the rest of the row.
            let left = row.left() + columns.check.right() + GROUP_ROW_GAP;
            let name = Rect::from_min_max(pos2(left, row.top()), pos2(row.left() + columns.name.right(), row.bottom()));
            let note = Rect::from_min_max(pos2(left, row.top()), pos2(row.right() - ROW_PAD_RIGHT, row.bottom()));
            for (fill_name, under, note_least) in GROUP_FILLS {
                places.push(Place::table(format!("group-name-{fill_name}"), line_band(name, TEXT), under, Ink::OnGlass, LEAST_TEXT));
                places.push(Place::table(format!("group-note-{fill_name}"), line_band(note, TEXT_SMALL), under, Ink::Quiet, note_least));
            }
            continue;
        }
        for (fill_name, under) in ROW_FILLS {
            for (ink_name, ink) in inks {
                for text in cells(row, &columns, ink) {
                    places.push(Place::table(format!("row-{ink_name}-{fill_name}"), line_band(text, TEXT), under, ink, LEAST_TEXT));
                }
            }
        }
        // The check in a ticked box: the mark's square at the box's centre, over the ticked row and the box.
        let tick = cell(row, columns.check);
        let mark = Rect::from_center_size(widgets::box_rect(tick).center(), vec2(CHECK_MARK, CHECK_MARK));
        places.push(Place::table("tick-check".to_owned(), mark, &[Layer::Ticked, Layer::Accent], Ink::OnDarkGlass, LEAST_GRAPHIC));
    }
    places
}

/// The pixels of `place` where text can sit: inside its rectangle and on a solid part of its pane's picture.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel places on a 1440 by 900 window, far inside every range")]
fn pixels<'a>(window: &'a Window, place: &'a Place) -> impl Iterator<Item = (usize, usize, Pixel)> + 'a {
    let pane = place.pane.map(|id| window.panes.iter().find(|pane| pane.id == id).unwrap_or_else(|| panic!("the shell has no pane {id}")));
    let bitmap = &window.bitmap;
    let (x0, x1) = (place.rect.left().floor() as usize, (place.rect.right().ceil() as usize).min(bitmap.w));
    let (y0, y1) = (place.rect.top().floor() as usize, (place.rect.bottom().ceil() as usize).min(bitmap.h));
    (y0..y1).flat_map(move |y| (x0..x1).map(move |x| (x, y))).filter_map(move |(x, y)| {
        let solid = pane.is_none_or(|pane| {
            let (px, py) = (x.checked_sub(pane.left), y.checked_sub(pane.top));
            matches!((px, py), (Some(px), Some(py)) if px < pane.picture.w && py < pane.picture.h && pane.picture.px[py * pane.picture.w + px][3] >= SOLID)
        });
        solid.then(|| (x, y, bitmap.px[y * bitmap.w + x]))
    })
}

/// The lowest contrast of `ink` against the pixels of `place`, with `fills` laid over each pixel first, bottom
/// first, as the table paints them.
fn lowest(window: &Window, place: &Place, fills: &[Color32], ink: Color32) -> Lowest {
    let ink = raster::pixel(ink);
    let fills: Vec<Pixel> = fills.iter().map(|&fill| raster::pixel(fill)).collect();
    let mut found = Lowest { contrast: f64::INFINITY, at: (0, 0), count: 0 };
    for (x, y, pixel) in pixels(window, place) {
        found.count += 1;
        let under = fills.iter().fold(pixel, |under, &fill| over(under, fill));
        let contrast = ratio(luminance(over(under, ink)), luminance(under));
        if contrast < found.contrast {
            found = Lowest { contrast, at: (x, y), count: found.count };
        }
    }
    found
}

/// One line of the table: a place on a backdrop in a mode and its lowest contrast.
struct Row {
    rendering: Rendering,
    backdrop: &'static str,
    mode: Mode,
    name: String,
    least: f64,
    found: Lowest,
}

impl Row {
    fn text(&self) -> String {
        format!(
            "{:<7} {:<7} {:<6} {:<38} lowest {:>6.2} (needs {:.1}) at x={} y={} over {} px{}\n",
            format!("{:?}", self.rendering),
            self.backdrop,
            format!("{:?}", self.mode),
            self.name,
            self.found.contrast,
            self.least,
            self.found.at.0,
            self.found.at.1,
            self.found.count,
            if self.found.contrast >= self.least { "" } else { "  FAIL" }
        )
    }
}

/// The rows of one backdrop in both modes; the places sharing a name (the two labels, a cell in every row of the
/// table) give their lowest.
fn measure(backdrop: &'static str, rendering: Rendering) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for mode in [Mode::Light, Mode::Dark] {
        let palette = theme::palette(mode);
        let windows = windows(backdrop, mode, rendering);
        let first = rows.len();
        for place in places() {
            let fills: Vec<Color32> = place.under.iter().map(|layer| layer.colour(palette, mode)).collect();
            let found = lowest(windows.on(place.screen), &place, &fills, place.ink.colour(palette));
            match rows[first..].iter_mut().find(|row| row.name == place.name) {
                Some(row) => {
                    let count = row.found.count + found.count;
                    row.found = if found.contrast < row.found.contrast { found } else { row.found };
                    row.found.count = count;
                }
                None => rows.push(Row { rendering, backdrop, mode, name: place.name, least: place.least, found }),
            }
        }
        for row in &rows[first..] {
            assert!(row.found.count >= LEAST_PIXELS, "{rendering:?} {backdrop} {mode:?} {}: only {} pixels measured", row.name, row.found.count);
        }
    }
    rows
}

/// The rows of every backdrop in both modes, drawn as `rendering` draws them.
fn measure_all(rendering: Rendering) -> Vec<Row> {
    thread::scope(|scope| {
        let jobs: Vec<_> = BACKDROPS.into_iter().map(|backdrop| scope.spawn(move || measure(backdrop, rendering))).collect();
        jobs.into_iter().flat_map(|job| job.join().unwrap()).collect()
    })
}

/// Whether the glass rendering is measured here. A machine without a GPU adapter skips it, and so does one whose
/// adapter is a software one (WARP on the Windows runners of CI): there a pane can take longer than the shader
/// waits, and the window then draws frost in its place. Either is said on the test's output. With
/// `DEVASSIST_REQUIRE_GPU` set, as it should be where the tests must not be skipped, a missing adapter is a failure
/// and a software one is measured.
fn shader_runs() -> bool {
    let ready = devassist_glass::gpu::install_own_device();
    let required = std::env::var_os("DEVASSIST_REQUIRE_GPU").is_some();
    assert!(ready || !required, "no GPU adapter was found for the glass rendering");
    if !ready {
        eprintln!("no GPU adapter: the glass rendering is not measured");
        return false;
    }
    if devassist_glass::gpu::software() && !required {
        eprintln!("a software adapter: the glass rendering is not measured");
        return false;
    }
    true
}

#[test]
fn every_text_in_the_window_holds_its_contrast_on_all_six_backdrops() {
    // Frosted first; the frosted pictures are drawn on the CPU whatever device is installed.
    let mut renderings = vec![Rendering::Frosted];
    if shader_runs() {
        renderings.push(Rendering::Glass);
    }
    let rows: Vec<Row> = renderings.iter().flat_map(|&rendering| measure_all(rendering)).collect();
    let places = places();
    let mut names: Vec<&str> = places.iter().map(|place| place.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(rows.len(), names.len() * BACKDROPS.len() * 2 * renderings.len(), "one row per place, backdrop, mode and rendering");
    for &rendering in &renderings {
        for backdrop in BACKDROPS {
            for mode in [Mode::Light, Mode::Dark] {
                for name in &names {
                    assert!(
                        rows.iter().any(|row| row.rendering == rendering && row.backdrop == backdrop && row.mode == mode && row.name == *name),
                        "no row for {rendering:?} {backdrop} {mode:?} {name}"
                    );
                }
            }
        }
    }
    let table: String = rows.iter().map(Row::text).collect();
    eprintln!("{table}");
    let failures: String = rows.iter().filter(|row| row.found.contrast < row.least).map(Row::text).collect();
    assert!(
        failures.is_empty(),
        "text below its contrast:
{failures}"
    );
}

// The dialogs, the menu and the screen's new controls, measured on the frames the window draws.
//
// Each of these places is measured on a whole frame of the window, drawn by the window's own code on a demo set up
// as a person would see it: the wallpaper, the shell's and the toolbar's glass panes laid over it (frosted or
// through the shader, as the rendering draws them), then every shape the frame paints, in its order, from the
// frosted panel and the table's fills to the overlay's scrim, its baked glass and the fills on it. A text is
// measured where its galley lies (cut to its clip), against the pixels as they are when it is painted, so the
// stack under it is the real one; its ink is the colour it is painted in. The geometry is the code's own: nothing
// here places a rectangle. A place's name carries its stack: the part of the screen under it, then the fills
// painted over that (the scrim, the glass, an inset, a button's fill), then its ink and its face.

/// What a frame shows over the screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Over {
    /// Nothing: the screen's own controls are measured.
    Nothing,
    /// Nothing, the pointer resting on the top row's main button ("Add repository"): the screen's own controls are
    /// measured, that button on its hover's fill.
    MainHovered,
    /// Nothing, in windows narrower than the app allows, where the top row gives way ([`TOP_ROW_SIZES`]): only the
    /// top row's controls are measured.
    TopRow,
    /// A dialog, on its scrim.
    Dialog,
    /// A dialog, on its scrim, with Enter pressed once it is open: the Add dialog then says, under its folder field,
    /// why the core refused the folder.
    Entered,
    /// The Add dialog, on its scrim, with the pointer resting on its found row of this index.
    Hovering(usize),
    /// The row's menu, without a scrim.
    Menu,
    /// The test dialog with a folder field (`widgets::tests::show_folder_dialog`), on its scrim.
    Folder(FolderShot),
}

/// What the folder field of a shot shows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum FolderShot {
    /// `D:\code\work\` and its three folders: the first lit by Down, the pointer resting on the second (not filled),
    /// the third resting.
    Lit,
    /// The same, with no row lit: the second on the hover's fill.
    Hovered,
    /// Empty, showing its hint.
    Empty,
}

impl Over {
    /// A dialog is over the screen: one the core's mode asks for, or the test dialog with a folder field.
    fn is_dialog(self) -> bool {
        matches!(self, Over::Dialog | Over::Entered | Over::Hovering(_) | Over::Folder(_))
    }

    /// Nothing is over the screen.
    fn is_screen(self) -> bool {
        matches!(self, Over::Nothing | Over::MainHovered | Over::TopRow)
    }
}

/// Windows narrower than the app allows (the sidebar shown), where the top row gives way: its capsule shows its icons
/// only ([`TOP_COMPACT`]), and the main button its icon too ([`TOP_BARE`]). In every window the app allows, the top row
/// has all its words.
const TOP_COMPACT: [f32; 2] = [900.0, 600.0];
/// The narrower of the two.
const TOP_BARE: [f32; 2] = [780.0, 600.0];
/// Both.
const TOP_ROW_SIZES: [[f32; 2]; 2] = [TOP_COMPACT, TOP_BARE];

/// A demo set up as a person would see it, to draw: its name, what is over the screen, and what sets it up.
struct Shot {
    name: &'static str,
    over: Over,
    set_up: fn(&mut Repos),
}

/// The demo's states whose text is measured. The screen with the ticked api and web (the selection bar, "Update
/// all"), homelab highlighted (the "…" on a plain, a ticked and the highlighted row) and an error on the status
/// bar; the progress dialog of the drawing's run, and with its Cancel pressed ("Cancelling…", which cannot be
/// pressed); the remove dialog for a clean and a dirty repository ("clean", "uncommitted changes"); the group dialog
/// moving api and web with its name, and renaming home with none (its hint, and the danger note the core gives when the rename is pressed then); the menu
/// on the ticked api, its first line highlighted and its last (Remove, in the danger colour); the folder field in a
/// test dialog, with its rows (resting, under the pointer, lit) and empty with its hint; the Add dialog as the demo
/// opens it with an installed row highlighted (its ticks, the installed rows at half strength, "archived" and
/// "private", the status line, the note), searched so that a tick is hidden ("Also ticked") with the folder refused
/// (the danger note), and with GitHub failed and a plain word typed (the status line in the danger colour, "Retry",
/// the row that lists the account); the screen without repositories (its panel's two lines, the top row), the top row
/// where it gives way, and the Import dialog as the demo opens it and with a folder the core refused; the screen with
/// the pointer on the top row's "Add repository"; the Move dialog as the demo opens it (its rows, the note on another
/// drive), with rows that are there already and that cannot move, with no usable folder (its quiet targets, the main
/// button off; why the folder cannot be used, in the place of the hint), and with a folder the core refused; the
/// Import dialog refusing a long path, its reason wrapped across the dialog's width; and the Push dialog as the demo
/// opens it (its label, address, note, facts with their checks and crosses, buttons), with no address (the field's
/// hint, "Push" off), and with an address the core refused (its reason in the danger colour where the note was).
const SHOTS: [Shot; 30] = [
    Shot {
        name: "screen",
        over: Over::Nothing,
        set_up: |repos| {
            repos.pick("homelab");
            repos.ask_for_repositories_to_remove();
        },
    },
    Shot { name: "progress", over: Over::Dialog, set_up: |repos| demo::show(repos, "progress").unwrap() },
    Shot {
        name: "progress cancelling",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "progress").unwrap();
            repos.job.as_mut().unwrap().cancelling = true;
        },
    },
    Shot {
        name: "remove",
        over: Over::Dialog,
        set_up: |repos| {
            repos.clear_marks();
            repos.toggle_mark_of("api");
            repos.toggle_mark_of("portal");
            repos.begin_remove();
        },
    },
    Shot { name: "group", over: Over::Dialog, set_up: |repos| demo::show(repos, "group").unwrap() },
    Shot {
        name: "group refused",
        over: Over::Dialog,
        set_up: |repos| {
            // A rename of home with its name cleared: the core refuses a group without a name (a move with none
            // takes the rows out of their group, so it is no refusal).
            repos.clear_marks();
            repos.pick("group::home");
            repos.open_group_prompt();
            repos.set_group_name("");
            repos.accept_group();
            assert!(repos.group_prompt.as_ref().is_some_and(|prompt| prompt.error.is_some()), "the core did not refuse");
        },
    },
    Shot { name: "menu", over: Over::Menu, set_up: |repos| demo::show(repos, "menu").unwrap() },
    Shot {
        name: "menu on remove",
        over: Over::Menu,
        set_up: |repos| {
            demo::show(repos, "menu").unwrap();
            repos.highlight_menu(MenuItem::Remove);
        },
    },
    Shot { name: "folder field", over: Over::Folder(FolderShot::Lit), set_up: |_| {} },
    Shot { name: "folder field hovered", over: Over::Folder(FolderShot::Hovered), set_up: |_| {} },
    Shot { name: "folder field empty", over: Over::Folder(FolderShot::Empty), set_up: |_| {} },
    Shot {
        name: "add",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "add").unwrap();
            repos.pick_add("acme/api");
        },
    },
    Shot {
        // Searched for "p": api (installed, resting), artcraft (highlighted), artcraft-plugins (hovered), so all are
        // in view in the smallest window too. An ordinary row under the hover's fill.
        name: "add hovered",
        over: Over::Hovering(2),
        set_up: set_up_pointing,
    },
    Shot {
        // The same, the pointer resting on the installed api: it draws no hover, and its words are measured there.
        name: "add pointer on installed",
        over: Over::Hovering(0),
        set_up: set_up_pointing,
    },
    Shot {
        name: "add searched and refused",
        over: Over::Entered,
        set_up: |repos| {
            demo::show(repos, "add").unwrap();
            repos.set_add_filter("docs");
            // A relative folder: the core refuses it when Enter clones.
            repos.clone_dir = std::path::PathBuf::from("dev");
        },
    },
    Shot {
        name: "add failed",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "add").unwrap();
            repos.remotes_state = devassist_core::repos::state::RemoteState::Failed("GitHub answered 502 Bad Gateway".to_owned());
            repos.set_add_filter("zzzz");
        },
    },
    Shot {
        name: "empty",
        over: Over::Nothing,
        set_up: |repos| {
            repos.installed.clear();
            repos.marked.clear();
        },
    },
    Shot { name: "top row", over: Over::TopRow, set_up: |_| {} },
    Shot { name: "import", over: Over::Dialog, set_up: |repos| demo::show(repos, "import").unwrap() },
    Shot {
        name: "import refused",
        over: Over::Entered,
        set_up: |repos| {
            demo::show(repos, "import").unwrap();
            // A relative folder: the core refuses it when Enter imports.
            repos.set_path_input("code");
        },
    },
    Shot {
        name: "import refused long",
        over: Over::Entered,
        set_up: |repos| {
            demo::show(repos, "import").unwrap();
            // A file with a long path: the reason spans the dialog's width, and wraps.
            repos.set_path_input(&std::env::current_exe().unwrap().display().to_string());
        },
    },
    Shot { name: "add repository hovered", over: Over::MainHovered, set_up: |_| {} },
    Shot { name: "move", over: Over::Dialog, set_up: |repos| demo::show(repos, "move").unwrap() },
    Shot {
        // api and web are in the folder already; notes moves there.
        name: "move there already",
        over: Over::Dialog,
        set_up: |repos| {
            repos.marked = vec!["api".to_owned(), "web".to_owned(), "notes".to_owned()];
            demo::show(repos, "move").unwrap();
            repos.set_path_input(demo::CLONE_DIR);
        },
    },
    Shot {
        // Into api itself: api cannot move there, web can.
        name: "move cannot",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "move").unwrap();
            repos.set_path_input(&std::path::Path::new(demo::CLONE_DIR).join("api").display().to_string());
        },
    },
    Shot {
        // A folder that is no full path: the targets quiet, the main button off.
        name: "move without a folder",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "move").unwrap();
            repos.set_path_input("work");
        },
    },
    Shot {
        name: "move refused",
        over: Over::Entered,
        set_up: |repos| {
            demo::show(repos, "move").unwrap();
            // A file: the held core tells the rows by the text, then refuses the folder when Enter moves.
            repos.set_path_input(&std::env::current_exe().unwrap().display().to_string());
        },
    },
    Shot { name: "push", over: Over::Dialog, set_up: |repos| demo::show(repos, "push").unwrap() },
    Shot {
        name: "push without an address",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "push").unwrap();
            repos.set_service_input("");
        },
    },
    Shot {
        name: "push refused",
        over: Over::Entered,
        set_up: |repos| {
            demo::show(repos, "push").unwrap();
            // Words with a space: the core refuses them when Enter pushes, and looks at nothing to say so.
            repos.set_service_input("my own server");
        },
    },
];

/// The demo's Add dialog searched for "p", for the shots that point at its rows: acme/api (installed),
/// acme/artcraft (highlighted), acme/artcraft-plugins (ordinary), then more.
fn set_up_pointing(repos: &mut Repos) {
    demo::show(repos, "add").unwrap();
    repos.set_add_filter("p");
    let found: Vec<String> = repos.add_list().iter().map(devassist_core::repos::github::RemoteRepo::id).take(3).collect();
    assert_eq!(found, ["acme/api", "acme/artcraft", "acme/artcraft-plugins"], "the rows pointed at are not these");
    assert_eq!(repos.add_selected.as_deref(), Some("acme/artcraft"));
}

/// The window sizes a shot is drawn at: the design's, and for an overlay also the smallest the window allows, where
/// a dialog lies over the sidebar as well as the table.
fn sizes(over: Over) -> &'static [[f32; 2]] {
    match over {
        Over::Nothing | Over::MainHovered => &[WINDOW],
        Over::TopRow => &TOP_ROW_SIZES,
        _ => &[WINDOW, MIN_WINDOW],
    }
}

/// How many frames a shot is drawn for before the one measured: the overlay opens, its glass is placed and baked,
/// the menu finds its row's button, the faces are laid out.
const SETTLE: usize = 4;

/// The texture of the panel's sheet in the shots.
const SHEET_TEXTURE: TextureId = TextureId::User(1);
/// The texture of the overlay's glass in the shots.
const GLASS_TEXTURE: TextureId = TextureId::User(2);

/// One thing measured in a shot: its name, the contrast it needs, what it was (to check that the dark mode
/// measures the same things in the same order), its lowest contrast and every contrast measured.
struct Probe {
    name: String,
    least: f64,
    what: String,
    found: Lowest,
    all: Vec<f64>,
}

/// The name of a colour the window paints in `mode`, for a place's name; another colour by its value.
fn colour_name(colour: Color32, mode: Mode) -> String {
    let p = theme::palette(mode);
    let named = [
        (p.on_glass, "on-glass"),
        (p.on_dark_glass, "white"),
        (p.quiet, "quiet"),
        (p.danger, "danger"),
        (p.warning, "warning"),
        (p.accent, "accent"),
        (p.selection, "selection"),
        (p.field, "field"),
        (p.scrim, "scrim"),
        (p.danger_tint, "danger tint"),
        (p.selection.gamma_multiply(BUTTON_OFF_OPACITY), "off fill"),
        (p.on_glass.gamma_multiply(BUTTON_OFF_OPACITY), "off"),
        (p.installed, "installed"),
        (p.installed_quiet, "installed quiet"),
        (theme::selected_fill(mode), "ticked"),
        (p.on_glass.gamma_multiply(GROUP_FILL_OPACITY), "group"),
        (p.on_glass.gamma_multiply(HEAD_FILL_OPACITY), "head"),
        (p.on_glass.gamma_multiply(HOVER_OPACITY), "hover"),
        (p.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY), "track"),
    ];
    named.iter().find(|(known, _)| *known == colour).map_or_else(|| format!("{colour:?}"), |(_, name)| (*name).to_owned())
}

/// The lowest contrast of `ink` against `bitmap` at the pixels of `rect` whose centre `inside` takes, and every
/// contrast measured.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "pixel places on a window of a few thousand points"
)]
fn measured(bitmap: &Bitmap, rect: Rect, ink: Color32, inside: &dyn Fn(Pos2) -> bool) -> (Lowest, Vec<f64>) {
    let ink = raster::pixel(ink);
    let (x0, x1) = (rect.left().floor().max(0.0) as usize, (rect.right().ceil().max(0.0) as usize).min(bitmap.w));
    let (y0, y1) = (rect.top().floor().max(0.0) as usize, (rect.bottom().ceil().max(0.0) as usize).min(bitmap.h));
    let mut found = Lowest { contrast: f64::INFINITY, at: (0, 0), count: 0 };
    let mut all = Vec::new();
    for y in y0..y1 {
        for x in x0..x1 {
            if !inside(pos2(x as f32 + 0.5, y as f32 + 0.5)) {
                continue;
            }
            let under = bitmap.px[y * bitmap.w + x];
            let contrast = ratio(luminance(over(under, ink)), luminance(under));
            found.count += 1;
            all.push(contrast);
            if contrast < found.contrast {
                found = Lowest { contrast, at: (x, y), count: found.count };
            }
        }
    }
    (found, all)
}

/// A text shape's words, where its galley lies (cut to its clip), its ink and its face; `None` for an empty one.
fn text_of(text: &TextShape, clip: Rect) -> Option<(String, Rect, Color32, FontId)> {
    let words = text.galley.text().to_owned();
    let rect = text.galley.rect.translate(text.pos.to_vec2()).intersect(clip);
    if words.trim().is_empty() || !rect.is_positive() {
        return None;
    }
    let sections = &text.galley.job.sections;
    let colour = |section: &LayoutSection| {
        let colour = if section.format.color == Color32::PLACEHOLDER { text.fallback_color } else { section.format.color };
        text.override_text_color.unwrap_or(colour).gamma_multiply(text.opacity_factor)
    };
    let ink = colour(&sections[0]);
    assert!(sections.iter().all(|section| colour(section) == ink), "{words}: a text in more than one colour");
    Some((words, rect, ink, sections[0].format.font_id.clone()))
}

/// The contrast a text in `font` whose ink is named `ink` needs: a graphic's for an icon, and for the words of a
/// button that cannot be pressed ("Cancelling…": WCAG 1.4.3 leaves disabled controls out of the text rule, so they
/// are held to the graphics' 3.0 to stay seen); 6.0 for 12-point text; 4.5 for the rest. (The words in the Add
/// dialog's found repositories are held to 4.5 by their place, whatever their ink: see `measure_shot`.)
fn least_for(font: &FontId, ink: &str) -> f64 {
    if font.family == assets::icons_family() || ink == "off" {
        LEAST_GRAPHIC
    } else if font.size <= TEXT_SMALL {
        LEAST_QUIET
    } else {
        LEAST_TEXT
    }
}

/// A text's face in a place's name: its size, and whether it is an icon or monospaced.
fn face_name(font: &FontId) -> String {
    let kind = if font.family == assets::icons_family() {
        " icon"
    } else if font.family == FontFamily::Monospace {
        " mono"
    } else {
        ""
    };
    format!("{}pt{kind}", font.size)
}

/// Which pixels of a graphic's rectangle are measured.
type Inside = Box<dyn Fn(Pos2) -> bool>;

/// A graphic of the overlay that is measured, as the progress dialog and a dialog's button paint it: the progress
/// bar's fill against its track (it says how far a task is: held to 3.0), the track against the inset (reported, not
/// held: the fill carries how far, the track only shows where the bar would end), and the focus ring against the
/// dialog's glass (3.0). Gives its name, the contrast it needs, its colour, its rectangle and which pixels count.
fn graphic(shape: &RectShape, palette: Palette) -> Option<(&'static str, f64, Color32, Rect, Inside)> {
    let bar = (shape.rect.height() - PROGRESS_HEIGHT).abs() < 1e-3;
    if bar && shape.fill == palette.progress {
        return Some(("graphic: progress bar's fill on its track", LEAST_GRAPHIC, shape.fill, shape.rect, Box::new(|_| true)));
    }
    if bar && shape.fill == palette.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY) {
        return Some(("graphic: progress bar's track on the field (reported, not held)", 1.0, shape.fill, shape.rect, Box::new(|_| true)));
    }
    if shape.stroke.color == palette.focus && (shape.stroke.width - FOCUS_RING).abs() < 1e-3 && shape.stroke_kind == StrokeKind::Outside {
        // The ring's straight runs, outside the rectangle; its round corners are antialiased thin.
        let (inner, round) = (shape.rect, f32::from(shape.corner_radius.nw));
        let outer = inner.expand(FOCUS_RING);
        let inside = move |at: Pos2| {
            let across = at.x > inner.left() + round && at.x < inner.right() - round && (at.y < inner.top() || at.y > inner.bottom());
            let down = at.y > inner.top() + round && at.y < inner.bottom() - round && (at.x < inner.left() || at.x > inner.right());
            outer.contains(at) && (across || down)
        };
        return Some(("graphic: focus ring on the glass", LEAST_GRAPHIC, shape.stroke.color, outer, Box::new(inside)));
    }
    None
}

/// A baked glass pane's id and the bits of its rectangle's corners.
type PaneKey = (&'static str, [u32; 4]);

/// The names of a shot's places from the light mode: each one's name, what it is, and the contrast it needs.
type Names = Vec<(String, String, f64)>;

/// What every shot on one backdrop, in one mode, at one size and drawn as one rendering draws it shares, made
/// once: the scene, its wallpaper, the window's picture of it, the panel's sheet, and each glass pane once baked.
struct Stage {
    scene: Scene,
    rendering: Rendering,
    wall: Wallpaper,
    window: Bitmap,
    sheet: SheetPicture,
    panes: RefCell<HashMap<PaneKey, Rc<Bitmap>>>,
}

impl Stage {
    fn new(scene: Scene, rendering: Rendering) -> Stage {
        let (wall, window) = wallpaper_window(&scene);
        let sheet = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
        Stage { scene, rendering, wall, window, sheet, panes: RefCell::new(HashMap::new()) }
    }

    /// The picture of the glass pane `want` as the rendering draws it, baked once.
    fn pane(&self, want: &Want) -> Rc<Bitmap> {
        let key = (want.id, [want.rect.min.x, want.rect.min.y, want.rect.max.x, want.rect.max.y].map(f32::to_bits));
        let baked = self.panes.borrow().get(&key).cloned();
        baked.unwrap_or_else(|| {
            let picture = Rc::new(match self.rendering {
                Rendering::Frosted => frosted_pane_picture(&self.wall, &self.scene, want).unwrap(),
                Rendering::Glass => pane_picture(&self.wall, &self.scene, want).unwrap(),
            });
            self.panes.borrow_mut().insert(key, Rc::clone(&picture));
            picture
        })
    }
}

/// The pictures of the glass panes `wants` laid over `bitmap`, as `stage` draws them.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel places on a window of a few thousand points")]
fn lay_panes(bitmap: &mut Bitmap, stage: &Stage, wants: &[Want]) {
    let (w, h) = (bitmap.w, bitmap.h);
    for want in wants {
        let picture = stage.pane(want);
        let (left, top) = (want.rect.min.x.round() as usize, want.rect.min.y.round() as usize);
        for y in 0..picture.h.min(h.saturating_sub(top)) {
            for x in 0..picture.w.min(w.saturating_sub(left)) {
                let at = (top + y) * w + left + x;
                bitmap.px[at] = over(bitmap.px[at], picture.px[y * picture.w + x]);
            }
        }
    }
}

/// A context with the window's faces and the visuals the window sets in `mode`.
fn shot_context(mode: Mode) -> Context {
    let ctx = widgets::tests::context();
    ctx.set_theme(if mode == Mode::Dark { egui::Theme::Dark } else { egui::Theme::Light });
    let mut visuals = if mode == Mode::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = Color32::TRANSPARENT;
    ctx.set_visuals(visuals);
    ctx
}

/// What the measured frame of a shot drew: its shapes in their order, how many of the last ones are the overlay's,
/// the glass panes under the screen, and the shell's layout.
struct Frame {
    ctx: Context,
    shapes: Vec<ClippedShape>,
    overlay: usize,
    wants: Vec<Want>,
    layout: shell::Layout,
    /// The box of the Add dialog's found repositories, when the Add dialog is drawn.
    found_box: Option<Rect>,
    /// The line under the folder dialog's field while it says why the folder cannot be used.
    reason_box: Option<Rect>,
    /// The rows of the Add dialog's found repositories, each with whether it is installed.
    rows: Vec<(Rect, bool)>,
    /// The top row's capsule and main button, when the screen placed them.
    top: Option<(Rect, Rect)>,
    /// The words the panel shows instead of the table, when it shows none.
    state: Vec<String>,
}

/// Draws `shot` on `stage` as the window draws it, for [`SETTLE`] frames and one more, and gives the last; the
/// textures painted from go into `textures`.
#[expect(clippy::cast_precision_loss, reason = "picture sizes of a few thousand pixels")]
fn draw_shot(shot: &Shot, stage: &Stage, textures: &mut HashMap<TextureId, ColorImage>) -> Frame {
    let (size, mode) = (vec2(stage.scene.size[0], stage.scene.size[1]), stage.scene.mode);
    let window = Rect::from_min_size(Pos2::ZERO, size);
    let sheet = Sheet { texture: SHEET_TEXTURE, uv: stage.sheet.uv, edge: stage.sheet.edge };
    textures.insert(SHEET_TEXTURE, stage.sheet.bitmap.image());
    let mut session = Session::demo(demo::repos());
    (shot.set_up)(session.repos_mut().unwrap());
    let ctx = shot_context(mode);
    let (mut screen, mut dialogs) = (ReposScreen::default(), Dialogs::default());
    let mut glass: Option<(Rect, Rc<Bitmap>)> = None;
    let mut last = None;
    // The test dialog's folder field: its text and the folders it suggests, and where the pointer rests on it.
    let (mut folder_text, folder_suggestions) = test_folder(shot.over);
    let mut pointer = None;
    let mut found = None;
    let mut reason = None;
    let mut main = None;
    let mut rows: Vec<(Rect, bool)> = Vec::new();
    for frame in 0..=SETTLE {
        let mut drew = None;
        let events = shot_events(shot.over, frame, pointer, found, main);
        let input = RawInput { screen_rect: Some(window), events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            let planned = dialogs::before(&ctx, session.repos(), repos_screen::menu_anchor(&screen), &mut dialogs);
            found = planned.as_ref().and_then(dialogs::add_found_box);
            reason = planned.as_ref().zip(session.repos()).and_then(|(plan, repos)| dialogs::reason_box(plan, repos, &dialogs));
            let mut overlay_glass = Vec::new();
            let wanted = if let Over::Folder(_) = shot.over {
                if frame == 0 {
                    overlay::open(&ctx, Some(widgets::tests::folder_id()));
                }
                let rows = widgets::folder_field_rows(&ctx, widgets::tests::folder_id(), folder_suggestions.len());
                pointer = Some(widgets::tests::folder_shot_pointer(ui.painter(), window, rows));
                Some(overlay::want_dialog(widgets::tests::folder_dialog_places(ui.painter(), window, rows).0))
            } else {
                planned.as_ref().map(dialogs::Plan::want)
            };
            if let Some(want) = wanted {
                if glass.as_ref().is_none_or(|(rect, _)| *rect != want.rect) {
                    glass = Some((want.rect, stage.pane(&want)));
                }
                let picture = &glass.as_ref().unwrap().1;
                let uv = Rect::from_min_max(Pos2::ZERO, pos2(want.rect.width() / picture.w as f32, want.rect.height() / picture.h as f32));
                overlay_glass.push(Drawn { id: want.id, texture: GLASS_TEXTURE, rect: want.rect, uv });
            }
            let layout = shell::layout(size, true, repos_screen::has_toolbar(&session));
            let plan = repos_screen::plan(ui.painter(), &layout, &session);
            let mut wants = shell::wants(&layout);
            if let Some(plan) = &plan {
                plan.add_wants(&mut wants);
            }
            if shot.over.is_screen() {
                let status = repos_screen::status(&session);
                let shown = ShellShown { mode, waiting: 0, status: status.as_ref() };
                shell::draw(ui, &layout, &mut true, &mut Page::Repositories, shown);
            }
            let keys = !overlay::is_open(&ctx);
            repos_screen::show(ui, &layout, plan.as_ref(), &mut session, Some(&sheet), mode, &mut screen, keys);
            if let Over::Folder(_) = shot.over {
                let shown =
                    widgets::tests::show_folder_dialog(&ctx, window, &mut folder_text, &folder_suggestions, theme::palette(mode), &overlay_glass);
                assert!(!shown.closed && shown.did == widgets::FolderDid::default(), "{}: the folder field did something", shot.name);
            } else {
                dialogs::after(&ctx, planned, session.repos_mut(), &overlay_glass, mode, &mut dialogs);
            }
            let count = ctx.graphics(|layers| layers.get(overlay::layer()).map_or(0, |list| list.next_idx().0));
            rows = match (found, session.repos()) {
                (Some(found), Some(repos)) => dialogs::add_rows(found, repos, &dialogs),
                _ => Vec::new(),
            };
            drew = Some((count, wants.as_slice().to_vec(), layout, plan.as_ref().and_then(repos_screen::top_row)));
        });
        picture::take_textures(std::mem::take(&mut output.textures_delta), textures);
        if let Some((_, picture)) = &glass {
            textures.insert(GLASS_TEXTURE, picture.image());
        }
        let (overlay, wants, layout, top) = drew.unwrap();
        main = top.map(|(_, main)| main);
        last = Some(Frame {
            ctx: ctx.clone(),
            shapes: std::mem::take(&mut output.shapes),
            overlay,
            wants,
            layout,
            found_box: found,
            reason_box: reason,
            rows: rows.clone(),
            top,
            state: state_words(&session),
        });
    }
    if !shot.over.is_screen() {
        assert!(overlay::is_open(&ctx) && glass.is_some(), "{}: its overlay is not drawn", shot.name);
    }
    let last = last.unwrap();
    hover_lands(shot, &last, size, mode);
    last
}

/// The test dialog's folder field in a shot over `over`: `D:\code\work\` and the folders it suggests while its rows are
/// shown; empty otherwise.
fn test_folder(over: Over) -> (String, Vec<String>) {
    match over {
        Over::Folder(FolderShot::Lit | FolderShot::Hovered) => {
            let text = widgets::tests::path(&["D:", "code", "work", ""]);
            let suggestions = widgets::tests::matching(&widgets::tests::folders(), &text);
            (text, suggestions)
        }
        _ => (String::new(), Vec::new()),
    }
}

/// The words the panel of `session` shows instead of the table: its title and the lines under it; none with a table.
fn state_words(session: &Session) -> Vec<String> {
    repos_screen::state_text(session).map_or_default(|(title, lines)| std::iter::once(title).chain(lines).collect())
}

/// What a person does in the frame `frame` of a shot over `over`: the folder field's keys and pointer at `pointer`,
/// Enter once the dialog is open, the pointer on a row of the Add list `found`, or on the top row's main button
/// `main` (both from the last frame).
fn shot_events(over: Over, frame: usize, pointer: Option<Pos2>, found: Option<Rect>, main: Option<Rect>) -> Vec<egui::Event> {
    match (over, pointer) {
        (Over::MainHovered, _) if frame >= 1 => main.map_or_default(|main| vec![egui::Event::PointerMoved(main.center())]),
        (Over::Hovering(index), _) if frame >= 1 => found.map_or_default(|found: Rect| {
            let row = f32::from(u16::try_from(index).unwrap_or(u16::MAX));
            vec![egui::Event::PointerMoved(pos2(found.center().x, found.top() + (row + 0.5) * ADD_ROW))]
        }),
        (Over::Folder(kind), Some(at)) if kind != FolderShot::Empty => widgets::tests::folder_shot_events(frame, at, kind == FolderShot::Lit),
        (Over::Entered, _) if frame == 2 => {
            vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }]
        }
        _ => Vec::new(),
    }
}

/// Checks, for a shot that points at something, that the frame `last` of `shot`, at the window `size` in `mode`,
/// shows the pointer where the shot says: the row of the Add list it rests on whole inside the list, filled with the
/// hover's fill when it is an ordinary row and not when it is installed (an installed row shows no hover); the top
/// row's main button filled with it. The shot measures what it says.
fn hover_lands(shot: &Shot, last: &Frame, size: egui::Vec2, mode: Mode) {
    let name = shot.name;
    let hover = theme::palette(mode).on_glass.gamma_multiply(HOVER_OPACITY);
    let filled = |place: Rect| {
        last.shapes.iter().any(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == hover && place.contains_rect(rect.rect)))
    };
    match shot.over {
        Over::Hovering(index) => {
            let found_box = last.found_box.unwrap_or_else(|| panic!("{name}: no Add list"));
            let (row, installed) = last.rows[index];
            assert!(found_box.contains_rect(row), "{name} at {size:?}: the pointed row {row:?} is not inside the list {found_box:?}");
            if installed {
                assert!(!filled(row), "{name} at {size:?}: a hover fill drawn on an installed row");
            } else {
                assert!(filled(row), "{name} at {size:?}: no hover fill drawn on the hovered row");
            }
        }
        Over::MainHovered => {
            let main = last.top.map_or_else(|| panic!("{name}: no top row"), |(_, main)| main);
            assert!(filled(main), "{name} at {size:?} in {mode:?}: no hover fill drawn on \"Add repository\"");
        }
        _ => {}
    }
}

/// Measures the shot `shot` on `stage`: every text and measured graphic of its overlay, and, on the screen, the
/// selection bar's, "Update all"'s and the status bar's text and every row's "…". Names its places in the stage's
/// mode's colours; `tag` tells the window's size.
fn measure_shot(shot: &Shot, stage: &Stage, tag: &str) -> Vec<Probe> {
    let mode = stage.scene.mode;
    let palette = theme::palette(mode);
    let mut textures = HashMap::new();
    let frame = draw_shot(shot, stage, &mut textures);
    let mut bitmap = stage.window.clone();
    lay_panes(&mut bitmap, stage, &frame.wants);
    let dots = assets::icon(Icon::Ellipsis).to_string();
    // The screen's controls whose text is measured: the selection bar, the top row's capsule and main button, and the
    // status bar; in the windows where the top row gives way, the top row's alone.
    let ids: &[&str] = if shot.over == Over::TopRow { &["actions"] } else { &["selection", "actions", "status"] };
    let mut controls: Vec<(&str, Rect)> = frame.wants.iter().filter(|want| ids.contains(&want.id)).map(|want| (want.id, want.rect)).collect();
    controls.extend(frame.top.map(|(_, main)| ("main", main)));
    // What part of the screen lies under a place.
    let region = |at: Pos2| {
        if let Some((id, _)) = frame.wants.iter().map(|want| (want.id, want.rect)).find(|(_, rect)| rect.contains(at)) {
            id
        } else if frame.layout.panel.contains(at) {
            "table panel"
        } else {
            "wallpaper"
        }
    };
    let overlay_from = frame.shapes.len() - frame.overlay;
    // The fills painted so far, where they are and by name: the table's (on the screen) and the overlay's.
    let mut fills: Vec<(Rect, String, bool)> = Vec::new();
    let mut probes = Vec::new();
    for (index, clipped) in frame.shapes.into_iter().enumerate() {
        let on_overlay = index >= overlay_from;
        let stack = |at: Pos2, fills: &[(Rect, String, bool)]| {
            let mut stack = vec![region(at).to_owned()];
            stack.extend(fills.iter().filter(|(rect, _, overlaid)| *overlaid == on_overlay && rect.contains(at)).map(|(_, name, _)| name.clone()));
            stack.join(" > ")
        };
        match &clipped.shape {
            Shape::Text(text) => {
                if let Some((words, rect, ink, font)) = text_of(text, clipped.clip_rect) {
                    let wanted = match shot.over {
                        Over::Nothing | Over::MainHovered => {
                            words == dots || frame.state.contains(&words) || controls.iter().any(|(_, control)| control.contains(rect.center()))
                        }
                        Over::TopRow => controls.iter().any(|(_, control)| control.contains(rect.center())),
                        Over::Dialog | Over::Entered | Over::Hovering(_) | Over::Folder(_) => on_overlay,
                        Over::Menu => on_overlay || words == dots,
                    };
                    if wanted {
                        let ink_name = colour_name(ink, mode);
                        let name = format!("{}{tag}: {}: {ink_name} {}", shot.name, stack(rect.center(), &fills), face_name(&font));
                        let (found, all) = measured(&bitmap, rect, ink, &|_| true);
                        let centre = rect.center();
                        // 12-point text on a dialog's field inset ("clean", "uncommitted changes", a task's state, the
                        // output) and the 12-point danger status are held to 4.5, WCAG's level AA, not to the 6.0 this
                        // project holds small text to elsewhere: there even white cannot reach 6.0 on every backdrop
                        // (the controller's ruling of 2026-11-26).
                        let on_inset =
                            shot.over.is_dialog() && fills.iter().any(|(fill, name, overlaid)| *overlaid && name == "field" && fill.contains(centre));
                        let danger_status = shot.over == Over::Nothing && region(centre) == "status" && ink_name == "danger";
                        // The words of an installed row of the Add list (its name, description and "Installed") are
                        // set back yet still information (the row can be highlighted): held to 4.5, whatever their
                        // size, by their place (the controller's ruling of 2026-11-26). Every other text keeps its bar.
                        let in_installed = on_overlay
                            && frame.found_box.is_some_and(|found_box| found_box.contains(centre))
                            && frame.rows.iter().any(|(row, installed)| *installed && row.contains(centre));
                        // The folder dialog's reason under its field (the core's refusal in the danger colour, or why
                        // a move's folder cannot be used, quiet), 12 point on the dialog's glass, is held to 4.5 by its
                        // place, as the danger status is (the controller's ruling of 2026-11-26, Task 6 fix round 1).
                        let in_reason = on_overlay && frame.reason_box.is_some_and(|reason| reason.contains(centre));
                        let least = match least_for(&font, &ink_name) {
                            _ if in_installed || in_reason => LEAST_TEXT,
                            least if least == LEAST_QUIET && (on_inset || danger_status) => LEAST_TEXT,
                            least => least,
                        };
                        probes.push(Probe { name, least, what: words, found, all });
                        // A measured text is not painted: what comes after it is measured against what is under it.
                        continue;
                    }
                }
            }
            Shape::Rect(rect) => {
                if on_overlay && let Some((name, least, ink, area, inside)) = graphic(rect, palette) {
                    let (found, all) = measured(&bitmap, area, ink, &inside);
                    probes.push(Probe {
                        // Named by what is under its top edge's middle, where it is drawn (a ring's middle is its button).
                        name: format!("{}{tag}: {}: {name}", shot.name, stack(pos2(area.center().x, area.top() + 0.5), &fills)),
                        least,
                        what: name.to_owned(),
                        found,
                        all,
                    });
                }
                if rect.brush.is_none() && rect.fill.a() > 0 {
                    // The light mode's track and selection are the same colour: a bar's height tells the track.
                    let track =
                        (rect.rect.height() - PROGRESS_HEIGHT).abs() < 1e-3 && rect.fill == palette.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY);
                    let name = if track { "track".to_owned() } else { colour_name(rect.fill, mode) };
                    fills.push((rect.rect.intersect(clipped.clip_rect), name, on_overlay));
                }
            }
            Shape::Mesh(mesh) if mesh.texture_id == GLASS_TEXTURE => {
                let glass = if shot.over == Over::Menu { "menu glass" } else { "dialog glass" };
                fills.push((mesh.calc_bounds(), glass.to_owned(), on_overlay));
            }
            _ => {}
        }
        picture::paint(&mut bitmap, &frame.ctx.tessellate(vec![clipped], 1.0), &textures);
    }
    probes
}

/// One place of the shots on a backdrop in a mode, drawn as one rendering draws it: the lowest contrast of the
/// texts and graphics sharing its name, and every contrast measured there.
struct ShotRow {
    row: Row,
    all: Vec<f64>,
}

/// A shot's name and its probes.
type ShotProbes = (&'static str, Vec<Probe>);

/// A take and what its shots measured.
type Taken = (Take, Vec<ShotProbes>);

/// One drawing of the shots: on a backdrop, as a rendering draws it, at a window size, in a mode.
#[derive(Clone, Copy)]
struct Take {
    backdrop: &'static str,
    rendering: Rendering,
    size: [f32; 2],
    mode: Mode,
}

/// What every shot drawn at `take`'s size measures in `take`: its name and its probes, in the shots' order.
fn measure_take(take: Take) -> Vec<ShotProbes> {
    let tag = if take.size == WINDOW { String::new() } else { format!(" at {}x{}", take.size[0], take.size[1]) };
    let scene = Scene { size: take.size, pixels_per_point: 1.0, mode: take.mode, backdrop: take.backdrop.to_owned(), blur: true };
    let stage = Stage::new(scene, take.rendering);
    SHOTS.iter().filter(|shot| sizes(shot.over).contains(&take.size)).map(|shot| (shot.name, measure_shot(shot, &stage, &tag))).collect()
}

/// The rows of the takes on one backdrop drawn as one rendering draws it, `light` and `dark` their two modes at the
/// same sizes in the same order. The places are named in the light mode; the dark mode measures the same texts in
/// the same order under those names. Places sharing a name give their lowest.
fn rows_of(light: Vec<Taken>, dark: Vec<Taken>) -> Vec<ShotRow> {
    let mut rows: Vec<ShotRow> = Vec::new();
    let mut light_names: HashMap<(&str, u32), Names> = HashMap::new();
    for (take, shots) in light.into_iter().chain(dark) {
        let first = if take.mode == Mode::Light { 0 } else { rows.iter().take_while(|row| row.row.mode == Mode::Light).count() };
        for (shot, probes) in shots {
            let names = light_names
                .entry((shot, take.size[0].to_bits()))
                .or_insert_with(|| probes.iter().map(|probe| (probe.name.clone(), probe.what.clone(), probe.least)).collect());
            let said: Vec<&str> = probes.iter().map(|probe| probe.what.as_str()).collect();
            let whats: Vec<&str> = names.iter().map(|(_, what, _)| what.as_str()).collect();
            assert_eq!(said, whats, "{shot} {} {:?}: the dark mode drew other things than the light", take.backdrop, take.rendering);
            for (probe, (name, _, least)) in probes.into_iter().zip(names.iter()) {
                match rows[first..].iter_mut().find(|row| row.row.name == *name && row.row.mode == take.mode) {
                    Some(row) => {
                        let count = row.row.found.count + probe.found.count;
                        if probe.found.contrast < row.row.found.contrast {
                            row.row.found = probe.found;
                        }
                        row.row.found.count = count;
                        row.all.extend(probe.all);
                    }
                    None => rows.push(ShotRow {
                        row: Row {
                            rendering: take.rendering,
                            backdrop: take.backdrop,
                            mode: take.mode,
                            name: name.clone(),
                            least: *least,
                            found: probe.found,
                        },
                        all: probe.all,
                    }),
                }
            }
        }
    }
    for row in &rows {
        assert!(
            row.row.found.count >= if row.row.name.contains("focus ring") { LEAST_RING_PIXELS } else { LEAST_PIXELS },
            "{:?} {} {:?} {}: only {} pixels measured",
            row.row.rendering,
            row.row.backdrop,
            row.row.mode,
            row.row.name,
            row.row.found.count
        );
    }
    rows
}

/// The rows of every shot on every backdrop in both modes, drawn as each of `renderings` draws them: every take
/// drawn at once, each on a thread of its own.
fn measure_all_shots(renderings: &[Rendering]) -> Vec<ShotRow> {
    let takes: Vec<Take> = renderings
        .iter()
        .flat_map(|&rendering| {
            BACKDROPS.into_iter().flat_map(move |backdrop| {
                [WINDOW, MIN_WINDOW, TOP_COMPACT, TOP_BARE]
                    .into_iter()
                    .flat_map(move |size| [Mode::Light, Mode::Dark].map(|mode| Take { backdrop, rendering, size, mode }))
            })
        })
        .collect();
    let mut measured_takes: Vec<Taken> = thread::scope(|scope| {
        let jobs: Vec<_> = takes.iter().map(|&take| (take, scope.spawn(move || measure_take(take)))).collect();
        jobs.into_iter().map(|(take, job)| (take, job.join().unwrap())).collect()
    });
    let mut rows = Vec::new();
    for &rendering in renderings {
        for backdrop in BACKDROPS {
            let (mine, rest): (Vec<_>, Vec<_>) =
                measured_takes.into_iter().partition(|(take, _)| take.rendering == rendering && take.backdrop == backdrop);
            measured_takes = rest;
            let (light, dark) = mine.into_iter().partition(|(take, _)| take.mode == Mode::Light);
            rows.extend(rows_of(light, dark));
        }
    }
    rows
}

/// The value a share `at` (0 to 1) of `values` lies at or under: 0.05 for the 5th percentile, 0.5 for the median.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_precision_loss, reason = "an index into a list of pixels")]
fn share(values: &[f64], at: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[((sorted.len() - 1) as f64 * at).round() as usize]
}

#[test]
fn every_text_of_the_dialogs_the_menu_and_the_selection_bar_holds_its_contrast_on_all_six_backdrops() {
    let mut renderings = vec![Rendering::Frosted];
    if shader_runs() {
        renderings.push(Rendering::Glass);
    }
    let rows = measure_all_shots(&renderings);
    // Every place on every backdrop, in both modes and renderings.
    let mut names: Vec<&str> = rows.iter().map(|row| row.row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(rows.len(), names.len() * BACKDROPS.len() * 2 * renderings.len(), "one row per place, backdrop, mode and rendering");
    let table: String = rows.iter().map(|row| row.row.text()).collect();
    eprintln!("{table}");
    eprintln!("{} places", names.len());
    let failures: Vec<String> = rows
        .iter()
        .filter(|row| row.row.found.contrast < row.row.least)
        .map(|row| format!("{}    5th percentile {:.2}, median {:.2}", row.row.text().trim_end(), share(&row.all, 0.05), share(&row.all, 0.5)))
        .collect();
    let failures = failures.join("\n");
    assert!(failures.is_empty(), "text below its contrast:\n{failures}");
}

/// Not a check: pictures of the demo's screen at the smallest window the app allows (the top row with all its words)
/// and where the top row gives way, in windows narrower than that ([`TOP_ROW_SIZES`]), light and dark, drawn as the shots are (the glass frosted on the CPU, on the first backdrop),
/// into the folder the environment variable `DEVASSIST_PICTURES` names. Run by hand with `--ignored`; without the
/// variable it draws nothing.
#[test]
#[ignore = "draws pictures for a person to look at"]
fn pictures_of_the_top_row_where_it_gives_way() {
    let Ok(folder) = std::env::var("DEVASSIST_PICTURES") else { return };
    let shot = Shot { name: "top row", over: Over::TopRow, set_up: |_| {} };
    for size in [MIN_WINDOW].into_iter().chain(TOP_ROW_SIZES) {
        for mode in [Mode::Light, Mode::Dark] {
            let scene = Scene { size, pixels_per_point: 1.0, mode, backdrop: BACKDROPS[0].to_owned(), blur: true };
            let stage = Stage::new(scene, Rendering::Frosted);
            let mut textures = HashMap::new();
            let frame = draw_shot(&shot, &stage, &mut textures);
            let mut bitmap = stage.window.clone();
            lay_panes(&mut bitmap, &stage, &frame.wants);
            picture::paint(&mut bitmap, &frame.ctx.tessellate(frame.shapes, 1.0), &textures);
            let name = format!("t5-top-row-{}x{}-{}.png", size[0], size[1], if mode == Mode::Dark { "dark" } else { "light" });
            crate::screenshot::write_png(&std::path::Path::new(&folder).join(name), &bitmap.image()).unwrap();
        }
    }
}
