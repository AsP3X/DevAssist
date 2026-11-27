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
use devassist_core::repos::forklines;
use devassist_core::repos::gitops::{ForkInfo, ForkList};
use devassist_core::repos::state::{MenuItem, MigrateItem, Msg, Repos};
use devassist_core::task::job::JobEvent;
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
    MIN_WINDOW, Mode, PANEL_TEXT_INSET, PROGRESS_HEIGHT, PROGRESS_TRACK_OPACITY, PUSH_SCROLL_CUE, Palette, RADIUS_PANEL, ROW_PAD_RIGHT, ROW_PADDING,
    RULE_LABEL_GAP, RULE_WIDTH, SIDEBAR_PADDING, SIDEBAR_TOP, STATUS_PADDING, Surface, TEXT, TEXT_SMALL, TEXT_TITLE, WINDOW,
};
use crate::{demo, forks_screen, frost, overlay, shell, table, widgets};

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

/// The rows of every backdrop in both modes, drawn as `rendering` draws them, the backdrops on a few threads
/// ([`on_workers`]).
fn measure_all(rendering: Rendering) -> Vec<Row> {
    on_workers(&BACKDROPS, |backdrop| measure(backdrop, rendering)).into_iter().flatten().collect()
}

/// How many takes the contrast tests draw and measure at once, in the whole process (both tests together):
/// `DEVASSIST_TEST_WORKERS` as given when it is set to a positive number (no more than the machine can run at once);
/// else half of what the machine can run at once, at least one and at most eight. A value that is no positive number
/// is said once on the test's output, and the default is taken. Each take renders and measures whole windows: one
/// thread a take, all at once, took every core of the machine (the user's request, 2026-11-27).
fn workers() -> usize {
    static UNREAD: std::sync::Once = std::sync::Once::new();
    let parallel = thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get);
    let fallback = (parallel / 2).clamp(1, 8);
    match std::env::var("DEVASSIST_TEST_WORKERS") {
        Err(_) => fallback,
        Ok(value) => match value.trim().parse::<usize>() {
            Ok(count) if count > 0 => count.min(parallel),
            _ => {
                UNREAD.call_once(|| eprintln!("DEVASSIST_TEST_WORKERS={value:?} is no positive number: {fallback} workers"));
                fallback
            }
        },
    }
}

/// The takes being drawn now, in the whole process, and the signal that one ended ([`workers`] at most).
static DRAWING: (std::sync::Mutex<usize>, std::sync::Condvar) = (std::sync::Mutex::new(0), std::sync::Condvar::new());

/// A place among the [`workers`] taken while one item is drawn, given back when it is dropped: also when the item's
/// job panics.
struct Slot;

impl Slot {
    /// Waits for a place among the [`workers`], and takes it.
    fn take() -> Slot {
        let (drawing, ended) = &DRAWING;
        let most = workers();
        let mut now = ended
            .wait_while(drawing.lock().unwrap_or_else(std::sync::PoisonError::into_inner), |now| *now >= most)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *now += 1;
        Slot
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let (drawing, ended) = &DRAWING;
        *drawing.lock().unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
        ended.notify_one();
    }
}

/// Stops the other workers of [`on_workers`] when it is dropped while its worker panics: they take no further item,
/// so the test fails soon.
struct StopOnPanic<'a>(&'a std::sync::atomic::AtomicBool);

impl Drop for StopOnPanic<'_> {
    fn drop(&mut self) {
        if thread::panicking() {
            self.0.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    }
}

/// `job` done to each of `items` on a few threads, each taking the next item not yet taken while it holds a place
/// among the process's [`workers`] ([`Slot`]); the results in the items' order, as one thread an item gave them. A
/// panic in one stops the others taking items, and fails the caller once they are done.
fn on_workers<T: Copy + Sync, R: Send>(items: &[T], job: impl Fn(T) -> R + Sync) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let stop = std::sync::atomic::AtomicBool::new(false);
    let results: Vec<std::sync::Mutex<Option<R>>> = items.iter().map(|_| std::sync::Mutex::new(None)).collect();
    thread::scope(|scope| {
        for _ in 0..workers().min(items.len()) {
            scope.spawn(|| {
                let _stop = StopOnPanic(&stop);
                loop {
                    let _slot = Slot::take();
                    if stop.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    let at = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let Some(&item) = items.get(at) else { break };
                    let done = job(item);
                    *results[at].lock().unwrap() = Some(done);
                }
            });
        }
    });
    results.into_iter().map(|result| result.into_inner().unwrap().expect("an item no worker took")).collect()
}

#[test]
fn the_workers_keep_the_items_order_and_stop_after_a_panic() {
    let squares = on_workers(&(0..40).collect::<Vec<u32>>(), |n| n * n);
    assert_eq!(squares, (0..40).map(|n| n * n).collect::<Vec<u32>>());
    // A panic on the first item: the others take no item after it, and the caller fails.
    let taken = std::sync::atomic::AtomicUsize::new(0);
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        on_workers(&(0..10_000).collect::<Vec<u32>>(), |n| {
            taken.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            assert!(n != 0, "the first item fails");
            std::thread::sleep(std::time::Duration::from_millis(1));
        })
    }));
    assert!(failed.is_err(), "the panic did not fail the caller");
    assert!(taken.load(std::sync::atomic::Ordering::SeqCst) < 1000, "the workers went on after the panic");
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
    /// Nothing, the pointer resting on the top row's control with these words, on the hover's fill, in every way the
    /// row fits (all its words, the capsule's icons alone, the main button's icon too): only the top row's controls
    /// are measured.
    TopHovered(&'static str),
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
    /// A dialog of the core's with a folder field, on its scrim, the field with the keyboard and its rows of folders
    /// shown (the Add dialog's folder field is given the keyboard as `--show add-folder` gives it).
    Rows(RowsShot),
    /// Nothing, the forks of a repository in the table's place: every text and graphic of that screen is measured,
    /// with the pointer where this says.
    Forks(Pointing),
}

/// Where the pointer rests in a shot of the forks screen.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Pointing {
    /// Nowhere on the window.
    Nowhere,
    /// On the row of the fork of this owner, which is drawn on the hover's fill.
    Row(&'static str),
    /// On the round button back to the repositories.
    Back,
    /// On the control of the top row or the toolbar with these words (the sort by its words), on the hover's fill.
    Control(&'static str),
    /// On a row's button: of the fork of this owner, the folder's (0) or GitHub's (1), on the hover's fill.
    RowButton(&'static str, usize),
    /// Nowhere, the keyboard in the filter (`/` typed). The field draws no ring for it (only egui's caret, which is not
    /// measured): its hint is measured with the keyboard there.
    Typing,
    /// Nowhere, in windows where the status bar's notes (the folder, the day of the fetch) give way to its sentence.
    Narrow,
}

/// What the rows of a dialog's folder field show in a shot over [`Over::Rows`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum RowsShot {
    /// The first lit by Down, the rest resting; the pointer nowhere on them.
    Lit,
    /// No row lit, the pointer resting on the first, on the hover's fill; the rest resting.
    Hovered,
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
        matches!(self, Over::Dialog | Over::Entered | Over::Hovering(_) | Over::Folder(_) | Over::Rows(_))
    }

    /// Nothing is over the screen.
    fn is_screen(self) -> bool {
        matches!(self, Over::Nothing | Over::MainHovered | Over::TopRow | Over::TopHovered(_) | Over::Forks(_))
    }
}

/// Windows narrower than the app allows (the sidebar shown), where the top row gives way: its capsule shows its icons
/// only ([`TOP_COMPACT`]), and the main button its icon too ([`TOP_BARE`]). The top row has all its words from 1056
/// wide (the sidebar shown); the smallest window the app allows ([`MIN_WINDOW`]) shows the capsule's icons alone too.
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
/// on the ticked api, its first line highlighted and its last (Remove, in the danger colour; Migrate resting, its key
/// and chevron), and with Migrate's side menu open (Migrate on the selection's fill, as it is highlighted, with its key
/// and chevron; each of the side menu's two lines highlighted and resting, with their keys: Migrate highlighted with
/// the side menu closed draws the same on the same place, and its key alone, 98 pixels, is under the 100 a place must
/// measure); the folder field in a
/// test dialog, with its rows (resting, under the pointer, lit) and empty with its hint; the Add dialog as the demo
/// opens it with an installed row highlighted (its ticks, the installed rows at half strength, "archived" and
/// "private", the status line, the note), searched so that a tick is hidden ("Also ticked") with the folder refused
/// (the danger note), and with GitHub failed and a plain word typed (the status line in the danger colour, "Retry",
/// the row that lists the account); the screen without repositories (its panel's two lines, the top row), the top row
/// where it gives way, and the Import dialog as the demo opens it and with a folder the core refused; the screen with
/// the pointer on the top row's "Add repository"; the Move dialog as the demo opens it (its rows, the note on another
/// drive), and with the note that a forks folder stays where it is ([`MOVE_FORKS_STAY`]), with rows that are there
/// already and that cannot move, with no usable folder (its quiet targets, the main
/// button off; why the folder cannot be used, in the place of the hint), and with a folder the core refused; the
/// Import dialog refusing a long path, its reason wrapped across the dialog's width; and the Push dialog as the demo
/// opens it (its label, address, note, facts with their checks and crosses, buttons), with no address (the field's
/// hint, "Push" off), and with an address the core refused (its reason in the danger colour where the note was); the
/// Add and the Import dialog with their folder field's rows shown on their own glass, the first row lit and under
/// the pointer; the progress dialog of a fetch of forks paused, counting, ended, and into a folder it cuts.
const SHOTS: [Shot; 73] = [
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
    // The fetch of the forks of api as `--show forks-fetching` opens it: its count, its bar and its pause (icon and
    // sentence), the line under the title, the output.
    Shot { name: "forks fetching", over: Over::Dialog, set_up: |repos| demo::show(repos, demo::FORKS_FETCHING).unwrap() },
    // The same once the fetch goes on: no pause.
    Shot {
        name: "forks fetching counting",
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, demo::FORKS_FETCHING).unwrap();
            repos.on_msg(Msg::Job(JobEvent::Line(forklines::going_on(), false)));
        },
    },
    // The same, ended while its dialog shows it (see `FETCH_FINISHED`): the last count, and Close.
    Shot { name: FETCH_FINISHED, over: Over::Dialog, set_up: |repos| demo::show(repos, demo::FORKS_FETCHING).unwrap() },
    // A fetch of the forks of three repositories ("Update the forks of all repositories"): the dialog's task list,
    // the count of the one being fetched.
    Shot { name: "forks fetching several", over: Over::Dialog, set_up: set_up_fetch_several },
    // A fetch into a folder so deep that the line under the title cuts it from its left.
    Shot { name: "forks fetching long folder", over: Over::Dialog, set_up: set_up_fetch_long_folder },
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
    // The forks' lines highlighted, with their icons and keys (the menu on api and web: "Clone the forks"). All three
    // are drawn by the same code with the same inks on the same fill; Show the forks' key alone, F, is 98 pixels, under
    // the 100 a place must measure, so the highlighted state is measured on Clone the forks (N, 126 pixels) and on the
    // all-forks line (A, 112 pixels). Resting, the three keys share the menu's place of keys on its glass.
    Shot {
        name: "menu on clone forks",
        over: Over::Menu,
        set_up: |repos| {
            demo::show(repos, "menu").unwrap();
            repos.highlight_menu(MenuItem::Forks);
        },
    },
    Shot {
        name: "menu on all forks",
        over: Over::Menu,
        set_up: |repos| {
            demo::show(repos, "menu").unwrap();
            repos.highlight_menu(MenuItem::AllForks);
        },
    },
    // The menu on api alone, whose forks the demo holds: "Update the forks", resting.
    Shot {
        name: "menu on api",
        over: Over::Menu,
        set_up: |repos| {
            repos.clear_marks();
            repos.pick("api");
            repos.enter_row();
        },
    },
    Shot {
        name: "menu on remove",
        over: Over::Menu,
        set_up: |repos| {
            demo::show(repos, "menu").unwrap();
            repos.highlight_menu(MenuItem::Remove);
        },
    },
    Shot { name: "migrate", over: Over::Menu, set_up: |repos| demo::show(repos, "migrate").unwrap() },
    Shot {
        name: "migrate on push",
        over: Over::Menu,
        set_up: |repos| {
            demo::show(repos, "migrate").unwrap();
            repos.highlight_migrate(MigrateItem::Service);
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
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "import").unwrap();
            // A file with a long path: the reason spans the dialog's width, and wraps. The held demo checks a folder by
            // its text alone, so the refusal a live core gives on Enter is set as it would stand.
            set_up_file_refused(repos);
        },
    },
    Shot { name: "add repository hovered", over: Over::MainHovered, set_up: |_| {} },
    Shot { name: "update all forks hovered", over: Over::TopHovered(UPDATE_ALL_FORKS), set_up: |_| {} },
    // The capsule's two others under the pointer, now that it has three items.
    Shot { name: "import hovered", over: Over::TopHovered("Import"), set_up: |_| {} },
    Shot { name: "update all hovered", over: Over::TopHovered("Update all"), set_up: |_| {} },
    Shot { name: "move", over: Over::Dialog, set_up: |repos| demo::show(repos, "move").unwrap() },
    // The same, with api's forks folder unable to follow (its note under the rows: see `MOVE_FORKS_STAY`).
    Shot { name: MOVE_FORKS_STAY, over: Over::Dialog, set_up: |repos| demo::show(repos, "move").unwrap() },
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
        over: Over::Dialog,
        set_up: |repos| {
            demo::show(repos, "move").unwrap();
            // A file: the held core tells the rows by the text; the refusal a live core gives on Enter is set.
            set_up_file_refused(repos);
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
    // The Add dialog's folder field at the demo's clone folder, with the keyboard: the demo's two folders that match.
    Shot { name: "add folder rows lit", over: Over::Rows(RowsShot::Lit), set_up: |repos| demo::show(repos, demo::ADD_FOLDER).unwrap() },
    Shot { name: "add folder rows hovered", over: Over::Rows(RowsShot::Hovered), set_up: |repos| demo::show(repos, demo::ADD_FOLDER).unwrap() },
    // The Import dialog's field (it has the keyboard as it opens) in the demo's made-up folder: its four folders.
    Shot { name: "import folder rows lit", over: Over::Rows(RowsShot::Lit), set_up: set_up_import_rows },
    Shot { name: "import folder rows hovered", over: Over::Rows(RowsShot::Hovered), set_up: set_up_import_rows },
    // The forks of api as `--show forks` opens them: bob (with changes) highlighted, every kind of cell resting.
    Shot { name: "forks", over: Over::Forks(Pointing::Nowhere), set_up: set_up_forks },
    // A fork without changes under the pointer.
    Shot { name: "forks hovered", over: Over::Forks(Pointing::Row("a-petrov")), set_up: set_up_forks },
    // The gone fork highlighted (followed into view), a fork with changes under the pointer. Filtered by "n" (mkoenig,
    // lin-wei, northwind-it, hanna-l, old-contrib), so that under the toolbar in the smallest window every row is in
    // view with it: forks with changes resting (a broken bar among them) and under the pointer.
    Shot {
        name: "forks gone highlighted",
        over: Over::Forks(Pointing::Row("lin-wei")),
        set_up: |repos| {
            set_up_forks(repos);
            repos.set_fork_filter("n");
            repos.pick_fork("old-contrib");
        },
    },
    // By name (the arrow in the Fork column's head): a fork without changes highlighted, the gone one under the pointer.
    Shot {
        name: "forks by name",
        over: Over::Forks(Pointing::Row("old-contrib")),
        set_up: |repos| {
            set_up_forks(repos);
            repos.set_fork_sort(devassist_core::repos::state::ForkSort::Name);
            repos.pick_fork("hanna-l");
        },
    },
    Shot { name: "forks back hovered", over: Over::Forks(Pointing::Back), set_up: set_up_forks },
    // The top row and the toolbar resting, "All" chosen (with the counts of `set_up_forks_counted`).
    Shot { name: "forks controls", over: Over::Forks(Pointing::Nowhere), set_up: set_up_forks_counted },
    // The top row's actions and its main button under the pointer, in each way it fits.
    Shot { name: "forks read again hovered", over: Over::Forks(Pointing::Control(FORKS_TOP[0])), set_up: set_up_forks_counted },
    Shot { name: "forks open hovered", over: Over::Forks(Pointing::Control(FORKS_TOP[1])), set_up: set_up_forks_counted },
    Shot { name: "forks fetch hovered", over: Over::Forks(Pointing::Control(FORKS_TOP[2])), set_up: set_up_forks_counted },
    // The toolbar: "With changes" under the pointer beside the chosen "All"; then "With changes" chosen, "All" under
    // the pointer; the sort under the pointer; the filter with text.
    Shot { name: "forks with changes hovered", over: Over::Forks(Pointing::Control("With changes")), set_up: set_up_forks_counted },
    Shot {
        name: "forks with changes",
        over: Over::Forks(Pointing::Control("All")),
        set_up: |repos| {
            set_up_forks_counted(repos);
            repos.set_fork_changed_only(true);
        },
    },
    Shot { name: "forks sort hovered", over: Over::Forks(Pointing::Control("Most changes first")), set_up: set_up_forks_counted },
    Shot {
        name: "forks filtered",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks_counted(repos);
            repos.set_fork_filter("o");
        },
    },
    // A row's buttons under the pointer: on a resting row, and on the highlighted one.
    Shot { name: "forks row button hovered", over: Over::Forks(Pointing::RowButton("a-petrov", 1)), set_up: set_up_forks },
    Shot { name: "forks highlighted row button hovered", over: Over::Forks(Pointing::RowButton("bob", 0)), set_up: set_up_forks },
    // The status bar: what the core said (the demo's refusal), and an error of a read while the rows show.
    Shot {
        name: "forks notice",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            repos.update_shown_forks();
        },
    },
    Shot {
        name: "forks read failed",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            repos.forks.as_mut().unwrap().error = Some("the folder could not be read".to_owned());
        },
    },
    // The sort capsule in its other two words, the arrow on the head of the column sorted by.
    Shot {
        name: "forks sort newest",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks_counted(repos);
            repos.set_fork_sort(devassist_core::repos::state::ForkSort::Newest);
        },
    },
    Shot {
        name: "forks sort by name",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks_counted(repos);
            repos.set_fork_sort(devassist_core::repos::state::ForkSort::Name);
        },
    },
    // The filter with the keyboard: its focus ring, and its hint while it is empty.
    Shot { name: "forks filter focused", over: Over::Forks(Pointing::Typing), set_up: set_up_forks_counted },
    // The status bar's sentence alone, its notes given way.
    Shot { name: "forks status narrow", over: Over::Forks(Pointing::Narrow), set_up: set_up_forks },
    // The panel's sentence for a filter nothing matches.
    Shot {
        name: "forks no match",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            repos.set_fork_filter("zzzz");
        },
    },
    // The panel's sentence instead of the rows: nothing in the folder, the folder being read, and an error.
    Shot {
        name: "forks empty",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            repos.forks.as_mut().unwrap().forks.clear();
        },
    },
    Shot {
        name: "forks loading",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            let view = repos.forks.as_mut().unwrap();
            view.forks.clear();
            view.loading = true;
        },
    },
    Shot {
        name: "forks error",
        over: Over::Forks(Pointing::Nowhere),
        set_up: |repos| {
            set_up_forks(repos);
            let view = repos.forks.as_mut().unwrap();
            view.forks.clear();
            view.error = Some("the folder could not be read".to_owned());
        },
    },
];

/// The shot of the fetch of the forks of api that ends while its dialog shows it: the run is ended after the first
/// frame drew it running (a dialog keeps the last count it showed, not one it never drew).
const FETCH_FINISHED: &str = "forks fetching finished";

/// The demo's forks of api open, with a held fetch of them into a folder so deep that the line under the dialog's title
/// cuts it, paused as `--show forks-fetching` has it.
fn set_up_fetch_long_folder(repos: &mut Repos) {
    set_up_forks(repos);
    let into = (0..12).fold(std::path::PathBuf::from(demo::CODE_DIR), |path, at| path.join(format!("a-rather-long-folder-{at}"))).join("api-fork");
    let lines = [
        (forklines::total(38, &into), false),
        (forklines::fetching(24, 38), true),
        (forklines::paused("The fetch timed out", 30), false),
        (forklines::countdown(12), true),
    ];
    let said: Vec<(&str, bool)> = lines.iter().map(|(line, progress)| (line.as_str(), *progress)).collect();
    repos.hold_fork_job("api", &said);
}

/// The repositories whose forks the shot of a fetch of several fetches, the first running.
const FETCH_SEVERAL: [&str; 3] = ["api", "web", "portal"];

/// The demo's forks of api open, with a held fetch of the forks of [`FETCH_SEVERAL`] counting the first's.
fn set_up_fetch_several(repos: &mut Repos) {
    set_up_forks(repos);
    let into = repos.forks.as_ref().unwrap().dest.clone();
    let lines = [(forklines::total(38, &into), false), (forklines::fetching(24, 38), true)];
    let said: Vec<(&str, bool)> = lines.iter().map(|(line, progress)| (line.as_str(), *progress)).collect();
    devassist_core::repos::state::testing::fork_job_of(repos, &FETCH_SEVERAL, &said);
}

/// The demo's forks of api open, as `--show forks` opens them.
fn set_up_forks(repos: &mut Repos) {
    demo::show(repos, demo::FORKS_SCREEN).unwrap();
}

/// How many made-up forks with a folder the shots of the forks' controls add to the demo's thirteen.
const MORE_CHANGED: usize = 6;

/// The demo's forks of api open, with [`MORE_CHANGED`] more forks that have a folder: for the shots that measure the
/// top row and the toolbar ([`FORKS_CONTROL_SHOTS`]). The demo's counts are "13" and "6"; a single digit of quiet
/// 12-point ink is fewer pixels than a place needs to be measured, so these shots count 19 forks, 12 with changes,
/// in the same words, faces, inks and fills. The demo itself (and its pictures) keeps the drawing's forks.
fn set_up_forks_counted(repos: &mut Repos) {
    set_up_forks(repos);
    let view = repos.forks.as_ref().unwrap();
    let mut forks = view.forks.clone();
    let made_up = forks.iter().find(|fork| fork.folder).cloned().unwrap();
    forks.extend((0..MORE_CHANGED).map(|index| ForkInfo { owner: format!("contrib-{index}"), ..made_up.clone() }));
    let list =
        ForkList { original: view.original.clone(), dest: view.dest.clone(), forks, base: view.base().map(str::to_owned), fetched: view.fetched() };
    repos.hold_forks("api", list);
    repos.load_forks();
}

/// The shots of the forks screen that measure its top row and its toolbar, besides its panel and status bar; the
/// others measure the panel, the status bar, the button back and the title.
const FORKS_CONTROL_SHOTS: [&str; 11] = [
    "forks controls",
    "forks sort newest",
    "forks sort by name",
    "forks filter focused",
    "forks read again hovered",
    "forks open hovered",
    "forks fetch hovered",
    "forks with changes hovered",
    "forks with changes",
    "forks sort hovered",
    "forks filtered",
];

/// The shot of the Move dialog noting that api's forks folder stays where it is. A held demo reads no folder, so its
/// core never finds a forks folder that cannot follow: the note is given to the dialog as a live core would have it
/// (`dialogs::note_forks_stay`) once the dialog is open.
const MOVE_FORKS_STAY: &str = "move forks stay";

/// The open folder question with this test's program typed, a file with a long path, and the core's refusal of it as
/// a live core gives it when Enter is pressed (`prompt::resolve`'s own words, asked of the disk here, by the test, not
/// by the held demo, which checks the text alone).
fn set_up_file_refused(repos: &mut Repos) {
    let file = std::env::current_exe().unwrap().display().to_string();
    repos.set_path_input(&file);
    let refused = devassist_core::prompt::resolve(&file).unwrap_err();
    repos.prompt.as_mut().unwrap().error = Some(refused);
}

/// The demo's Import dialog with its field in the demo's made-up folder, so it suggests the demo's four folders.
fn set_up_import_rows(repos: &mut Repos) {
    demo::show(repos, "import").unwrap();
    repos.set_path_input(&format!("{}{}", demo::CODE_DIR, std::path::MAIN_SEPARATOR));
}

/// The demo's Add dialog searched for "p", for the shots that point at its rows: acme/api (installed),
/// acme/artcraft (highlighted), acme/artcraft-plugins (ordinary), then more.
fn set_up_pointing(repos: &mut Repos) {
    demo::show(repos, "add").unwrap();
    repos.set_add_filter("p");
    let found: Vec<String> = repos.add_list().iter().map(devassist_core::repos::github::RemoteRepo::id).take(3).collect();
    assert_eq!(found, ["acme/api", "acme/artcraft", "acme/artcraft-plugins"], "the rows pointed at are not these");
    assert_eq!(repos.add_selected.as_deref(), Some("acme/artcraft"));
}

/// The window sizes a shot is drawn at: the design's and the smallest the window allows (where a dialog lies over
/// the sidebar as well as the table); where the top row gives way, the windows narrower than that.
fn sizes(over: Over) -> &'static [[f32; 2]] {
    match over {
        Over::TopRow => &TOP_ROW_SIZES,
        // All the words in the design's window, the capsule's icons alone in the smallest the app allows and narrower,
        // the main button's icon too where the row gives way further.
        Over::TopHovered(_) => &[WINDOW, MIN_WINDOW, TOP_COMPACT, TOP_BARE],
        // The forks' top row has all its words in the design's window, its icons alone in the smallest the app
        // allows, and the main button's icon too where the top row gives way further.
        Over::Forks(Pointing::Control(words)) if FORKS_TOP.contains(&words) => &[WINDOW, MIN_WINDOW, TOP_BARE],
        // The status bar's notes give way to its sentence in the smallest window the app allows (narrower still, the
        // sentence is cut too).
        Over::Forks(Pointing::Narrow) => &[MIN_WINDOW],
        _ => &[WINDOW, MIN_WINDOW],
    }
}

/// Whether a text at `at` of a shot of the forks screen (`shot`, in `layout`) is measured: its every text on its panel,
/// the status bar, the button back and the title; in the shots of its controls ([`FORKS_CONTROL_SHOTS`]), the top
/// row's and the toolbar's too.
fn forks_measured(shot: &Shot, layout: &shell::Layout, at: Pos2) -> bool {
    let controls = FORKS_CONTROL_SHOTS.contains(&shot.name);
    [layout.panel, layout.status, layout.toggle, layout.title].iter().any(|place| place.contains(at))
        || (controls && [layout.actions, layout.toolbar].iter().any(|place| place.contains(at)))
}

/// The forks screen's top row, by its words: its capsule's two actions and its main button.
const FORKS_TOP: [&str; 3] = ["Read again", "Open the folder", "Fetch the forks"];

/// The top row's capsule action whose hover is measured: the one this plan adds.
const UPDATE_ALL_FORKS: &str = "Update all forks";

/// How many frames a shot is drawn for before the one measured: the overlay opens, its glass is placed and baked,
/// the menu finds its row's button, the faces are laid out.
const SETTLE: usize = 4;

/// The texture of the panel's sheet in the shots.
const SHEET_TEXTURE: TextureId = TextureId::User(1);
/// The textures of the overlay's glass panes in the shots, in the order the frame wants them: a dialog's or the
/// menu's, then the menu's side menu's.
const GLASS_TEXTURES: [TextureId; 2] = [TextureId::User(2), TextureId::User(3)];

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
        (p.chosen_count, "chosen count"),
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
/// dialog's glass (3.0), and the cue that says the Push dialog's facts go on out of view against their field (3.0).
/// Gives its name, the contrast it needs, its colour, its rectangle and which pixels count.
fn graphic(shape: &RectShape, palette: Palette) -> Option<(&'static str, f64, Color32, Rect, Inside)> {
    if shape.fill == palette.quiet && (shape.rect.width() - PUSH_SCROLL_CUE).abs() < 1e-3 {
        // The bar's straight part, a pixel in from its long edges: its edges and round ends are antialiased thin.
        let rect = shape.rect;
        let inner = Rect::from_min_max(
            pos2(rect.left() + 1.0, rect.top() + PUSH_SCROLL_CUE / 2.0),
            pos2(rect.right() - 1.0, rect.bottom() - PUSH_SCROLL_CUE / 2.0),
        );
        return Some((
            "graphic: the Push facts' scroll cue on the field",
            LEAST_GRAPHIC,
            shape.fill,
            rect,
            Box::new(move |at: Pos2| inner.contains(at)),
        ));
    }
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

/// The rule the row's menu draws between its blocks (the forks' three, Migrate, Remove): a hairline the menu's row
/// wide in the text's colour at [`theme::MENU_LINE_OPACITY`] on the menu's glass.
const MENU_RULE_PLACE: &str = "graphic: the menu's rule between its blocks (reported, not held)";

/// The menu's rule between its blocks, when `shape` is one: measured and reported, not held.
fn menu_rule(shape: &RectShape, palette: Palette) -> Option<(&'static str, f64, Color32, Rect, Inside)> {
    let rule = shape.fill == palette.on_glass.gamma_multiply(theme::MENU_LINE_OPACITY)
        && (shape.rect.height() - theme::HAIRLINE).abs() < 1e-3
        && (shape.rect.width() - (crate::menu::demo_size().x - 2.0 * (theme::MENU_PAD + theme::MENU_ROW_PAD))).abs() < 1e-3;
    // The controller's ruling of 2026-11-27, pending the user: the rule is a separator, a layout aid as the ahead rule's
    // small tick is (the blocks are set apart by their spacing too, and the rule before Remove has been there unmeasured
    // since the menu was built); so it is reported, not held. Measured: 1.30 to 1.31 in the light mode, 1.40 to 1.46 in
    // the dark, on every backdrop, both sizes, both renderings.
    rule.then(|| (MENU_RULE_PLACE, 1.0, shape.fill, shape.rect, Box::new(|_| true) as Inside))
}

/// A graphic measured on the overlay of a shot over `over` (`on_overlay`): the menu's rules in a shot of the menu, and
/// the overlay's other graphics ([`graphic`]).
fn overlay_graphic(over: Over, on_overlay: bool, shape: &RectShape, palette: Palette) -> Option<(&'static str, f64, Color32, Rect, Inside)> {
    if !on_overlay {
        return None;
    }
    if over == Over::Menu { menu_rule(shape, palette) } else { None }.or_else(|| graphic(shape, palette))
}

/// Where the screen's `plan` places the top row's capsule and main button, and the top row's control a shot over
/// [`Over::TopHovered`] points at (none in other shots), measured with `painter`'s faces.
fn top_places(over: Over, painter: &egui::Painter, plan: Option<&repos_screen::Plan>) -> (Option<(Rect, Rect)>, Option<Rect>) {
    let hovered = match (over, plan) {
        (Over::TopHovered(words), Some(plan)) => {
            repos_screen::control_places(painter, plan).into_iter().find(|(said, _)| said == words).map(|(_, place)| place)
        }
        _ => None,
    };
    (plan.and_then(repos_screen::top_row), hovered)
}

/// The forks table's ahead rule's small tick, at each commit.
const RULE_TICK_PLACE: &str = "graphic: ahead rule's tick (reported, not held)";
/// Its taller tick, at every fifth commit.
const RULE_FIFTH_PLACE: &str = "graphic: ahead rule's fifth tick";
/// Its origin mark.
const RULE_ORIGIN_PLACE: &str = "graphic: ahead rule's origin mark";
/// The fewest pixels a place of the forks table's rule's hairlines measures (its ticks and its origin mark, never its
/// bars, which keep the general least): the origin mark of one row is 9 pixels above its tick, so a row's few are
/// enough.
const LEAST_RULE_PIXELS: usize = 9;
/// The short piece of a broken bar, at the far end of the forks rule.
const RULE_BREAK_PLACE: &str = "graphic: ahead rule's broken bar, short piece";
/// The fewest pixels the short piece of a broken bar measures: all of one ([`theme::RULE_BREAK_END`] by
/// [`theme::RULE_BAR`]), as a screen may have one broken bar only; less is a piece measured short.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a piece of a few points")]
const LEAST_BREAK_PIXELS: usize = (theme::RULE_BREAK_END * theme::RULE_BAR) as usize;

/// A graphic of the forks table's ahead rule inside `panel`, as the table paints it ([`table::draw_forward`]): a
/// tick, a fifth's tick, the origin mark, the bar, and the short piece of a broken bar. Each is held to 3.0, as
/// graphics are, but the small tick. Gives its name, the contrast it needs, its colour, its rectangle and which pixels
/// count.
fn rule_graphic(shape: &RectShape, palette: Palette, panel: Rect) -> Option<(&'static str, f64, Color32, Rect, Inside)> {
    let near = |a: f32, b: f32| (a - b).abs() < 1e-3;
    let (rect, fill) = (shape.rect, shape.fill);
    if !panel.contains_rect(rect) {
        return None;
    }
    let (name, least, area) =
        if near(rect.width(), theme::HAIRLINE) && near(rect.height(), theme::RULE_TICK) && fill == table::soft(palette, theme::RULE_TICK_OPACITY) {
            // The controller's ruling of 2026-11-27, pending the user: the small tick (0.28 of the text colour) is a
            // scale aid, the bar, the origin mark and the number carry how far a fork is, and the Repositories table
            // has had the same ticks unmeasured since the first plan; so it is reported, not held. Measured: 1.91 to
            // 1.97 in the light mode, 2.00 to 2.07 in the dark, on every backdrop, both sizes, both renderings.
            (RULE_TICK_PLACE, 1.0, rect)
        } else if near(rect.width(), theme::HAIRLINE)
            && near(rect.height(), theme::RULE_TICK_FIFTH)
            && fill == table::soft(palette, theme::RULE_FIFTH_OPACITY)
        {
            (RULE_FIFTH_PLACE, LEAST_GRAPHIC, rect)
        } else if near(rect.width(), theme::RULE_ZERO_WIDTH) && near(rect.height(), theme::RULE_HEIGHT) && fill == palette.on_glass {
            // Measured above the fifth tick it is painted over: there it stands against the panel, below against the tick.
            let above = Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - theme::RULE_TICK_FIFTH));
            (RULE_ORIGIN_PLACE, LEAST_GRAPHIC, above)
        } else if near(rect.height(), theme::RULE_BAR) && fill == palette.on_glass {
            let name = if near(rect.width(), theme::RULE_BREAK_END) { RULE_BREAK_PLACE } else { "graphic: ahead rule's bar" };
            (name, LEAST_GRAPHIC, rect)
        } else {
            return None;
        };
    Some((name, least, fill, area, Box::new(|_| true)))
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
    /// The top row's control the pointer rests on in a shot over [`Over::TopHovered`], where the screen placed it.
    top_hovered: Option<Rect>,
    /// The words the panel shows instead of the table, when it shows none.
    state: Vec<String>,
    /// The folder field of the Add or the folder dialog, with the room its rows take; the test dialog's field.
    field: Option<Rect>,
    /// The menu's pane and its side menu's, when the menu is drawn.
    menu: Option<(Rect, Option<Rect>)>,
    /// The core's highlight in the menu and in its side menu, as places among their lines.
    menu_lit: (Option<usize>, Option<usize>),
    /// The row of the Add list the core highlights, when it is among the rows.
    add_lit: Option<Rect>,
    /// The forks screen's rows, whole, each with whether it is the highlighted one; and what the pointer rests on in
    /// a shot of it: a row, or the button back.
    forks: (Vec<(Rect, bool)>, Option<Rect>),
    /// The forks screen's chosen segment, where it was drawn.
    forks_chosen: Option<Rect>,
    /// The forks' sort showed its icon alone.
    forks_sort_compact: bool,
}

/// Draws `shot` on `stage` as the window draws it, for [`SETTLE`] frames and one more, and gives the last; the
/// textures painted from go into `textures`.
fn draw_shot(shot: &Shot, stage: &Stage, textures: &mut HashMap<TextureId, ColorImage>) -> Frame {
    let (size, mode) = (vec2(stage.scene.size[0], stage.scene.size[1]), stage.scene.mode);
    let window = Rect::from_min_size(Pos2::ZERO, size);
    let sheet = Sheet { texture: SHEET_TEXTURE, uv: stage.sheet.uv, edge: stage.sheet.edge };
    textures.insert(SHEET_TEXTURE, stage.sheet.bitmap.image());
    let mut session = Session::demo(demo::repos());
    (shot.set_up)(session.repos_mut().unwrap());
    let ctx = shot_context(mode);
    let (mut screen, mut dialogs) = (ReposScreen::default(), Dialogs::default());
    // The overlay's glass panes, each where it was baked and its picture, in the order of GLASS_TEXTURES.
    let mut glass: Vec<(Rect, Rc<Bitmap>)> = Vec::new();
    let mut last = None;
    // The test dialog's folder field: its text and the folders it suggests, and where the pointer rests on it.
    let (mut folder_text, folder_suggestions) = test_folder(shot.over);
    let (mut pointer, mut found, mut reason, mut main, mut field, mut menu) = (None, None, None, None, None, None);
    let mut rows: Vec<(Rect, bool)> = Vec::new();
    let mut forks = ForksShot::default();
    for frame in 0..=SETTLE {
        if shot.name == FETCH_FINISHED && frame == 1 {
            session.repos_mut().unwrap().job.as_mut().unwrap().finish(Ok("Updated the forks of api.".to_owned()), false);
        }
        let mut drew = None;
        let events = [shot_events(shot.over, frame, pointer, found, main, field), forks.events(frame)].concat();
        let input = RawInput { screen_rect: Some(window), events, ..Default::default() };
        let mut output = ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            if shot.name == MOVE_FORKS_STAY {
                dialogs::note_forks_stay(&mut dialogs, &[("api", r"something is at E:\work\api-fork already")]);
            }
            let planned = dialogs::before(&ctx, session.repos(), repos_screen::menu_anchor(&screen), &mut dialogs);
            found = planned.as_ref().and_then(dialogs::add_found_box);
            reason = planned.as_ref().zip(session.repos()).and_then(|(plan, repos)| dialogs::reason_box(plan, repos, &dialogs));
            field = planned.as_ref().and_then(dialogs::folder_field_box);
            menu = planned.as_ref().and_then(dialogs::menu_panes);
            let wanted = if let Over::Folder(_) = shot.over {
                if frame == 0 {
                    overlay::open(&ctx, Some(widgets::tests::folder_id()));
                }
                let rows = widgets::folder_field_rows(&ctx, widgets::tests::folder_id(), folder_suggestions.len());
                pointer = Some(widgets::tests::folder_shot_pointer(ui.painter(), window, rows));
                field = Some(widgets::tests::folder_dialog_places(ui.painter(), window, rows).2);
                vec![overlay::want_dialog(widgets::tests::folder_dialog_places(ui.painter(), window, rows).0)]
            } else {
                planned.as_ref().map_or_else(Vec::new, dialogs::Plan::wants)
            };
            let overlay_glass = overlay_drawn(&wanted, &mut glass, stage, shot.name);
            let (layout, plan, forks_plan, wants) = screen_plans(ui.painter(), size, &session);
            draw_shell(ui, shot, &layout, &session, forks_plan.as_ref(), mode);
            let keys = !overlay::is_open(&ctx);
            if !forks.show(ui, shot, &layout, forks_plan.as_ref(), &sheet, mode, keys) {
                repos_screen::show(ui, &layout, plan.as_ref(), &mut session, Some(&sheet), mode, &mut screen, keys);
            }
            if let Over::Folder(_) = shot.over {
                let shown =
                    widgets::tests::show_folder_dialog(&ctx, window, &mut folder_text, &folder_suggestions, theme::palette(mode), &overlay_glass);
                assert!(!shown.closed && shown.did == widgets::FolderDid::default(), "{}: the folder field did something", shot.name);
            } else {
                dialogs::after(&ctx, planned, session.repos_mut(), &overlay_glass, mode, &mut dialogs);
                // As the window does for `--show add-folder`, once the Add dialog is open.
                let focus = matches!(shot.over, Over::Rows(_)) && frame == 0 && shot.name.starts_with("add");
                assert!(!focus || dialogs::focus_add_folder(&ctx, session.repos()), "{}: the Add dialog is not open", shot.name);
            }
            let count = ctx.graphics(|layers| layers.get(overlay::layer()).map_or(0, |list| list.next_idx().0));
            rows = match (found, session.repos()) {
                (Some(found), Some(repos)) => dialogs::add_rows(found, repos, &dialogs),
                _ => Vec::new(),
            };
            drew = Some((count, wants.as_slice().to_vec(), layout, top_places(shot.over, ui.painter(), plan.as_ref())));
        });
        picture::take_textures(std::mem::take(&mut output.textures_delta), textures);
        for ((_, picture), texture) in glass.iter().zip(GLASS_TEXTURES) {
            textures.insert(texture, picture.image());
        }
        let (overlay, wants, layout, (top, top_hovered)) = drew.unwrap();
        // The pointer goes to the control a shot over the top row points at, else to the main button.
        main = top_hovered.or(top.map(|(_, main)| main));
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
            top_hovered,
            state: state_words(&session),
            field,
            menu,
            menu_lit: menu_lit(&session),
            add_lit: add_lit(&session, &rows),
            forks: (forks.rows.clone(), forks.pointer.map(|(_, rect)| rect).or(forks.typing)),
            forks_chosen: forks.chosen,
            forks_sort_compact: forks.sort_compact,
        });
    }
    overlay_lands(shot, &ctx, glass.len(), &session);
    let last = last.unwrap();
    state_lands(shot, &last, size, mode);
    last
}

/// Checks that a shot of an overlay drew it, on its glass (`glass` panes baked), and a side menu open in the core of
/// `session` on a glass of its own, where its texts are measured.
fn overlay_lands(shot: &Shot, ctx: &Context, glass: usize, session: &Session) {
    if !shot.over.is_screen() {
        assert!(overlay::is_open(ctx) && glass > 0, "{}: its overlay is not drawn", shot.name);
        if session.repos().is_some_and(|repos| repos.migrate_highlight().is_some()) {
            assert_eq!(glass, 2, "{}: the side menu's glass is not baked", shot.name);
        }
    }
}

/// What the screen of `session` places in a window of `size`, measured with `painter`'s faces, as the window's pages
/// place it: the layout (with the toolbar while the forks show, or while there are repositories), the Repositories
/// screen's controls or the forks' plan, and the glass panes to bake.
fn screen_plans(
    painter: &egui::Painter,
    size: egui::Vec2,
    session: &Session,
) -> (shell::Layout, Option<repos_screen::Plan>, Option<forks_screen::Plan>, shell::Wants) {
    let view = session.repos().and_then(|repos| repos.forks.as_ref());
    let layout = shell::layout(size, true, view.is_some() || repos_screen::has_toolbar(session));
    let plan = repos_screen::plan(painter, &layout, session);
    let forks_plan = view.map(|view| forks_screen::plan(view, &layout, painter));
    let mut wants = shell::wants(&layout);
    if let Some(plan) = &plan {
        plan.add_wants(&mut wants);
    }
    if let Some(plan) = &forks_plan {
        plan.add_wants(&mut wants);
    }
    (layout, plan, forks_plan, wants)
}

/// Draws the shell in `layout` under a shot of the screen (`shot`, of `session` in `mode`), as the window does: its
/// status bar and title the forks' while they show (`forks`, the title as their plan shows it), else the Repositories
/// screen's.
fn draw_shell(ui: &mut egui::Ui, shot: &Shot, layout: &shell::Layout, session: &Session, forks: Option<&forks_screen::Plan>, mode: Mode) {
    if !shot.over.is_screen() && shot.name != WHOLE_MENU {
        return;
    }
    let status = session.repos().and_then(forks_screen::status).or_else(|| repos_screen::status(session));
    let shown = ShellShown { mode, waiting: 0, status: status.as_ref(), back: forks.map(forks_screen::Plan::title) };
    shell::draw(ui, layout, &mut true, &mut Page::Repositories, shown);
}

/// The forks screen in a shot's frames: what it keeps, its rows as it placed them in the last frame (whole, each with
/// whether it is highlighted), and where the pointer rests in the next frame with what it rests on.
#[derive(Default)]
struct ForksShot {
    screen: forks_screen::Screen,
    rows: Vec<(Rect, bool)>,
    pointer: Option<(Pos2, Rect)>,
    /// The chosen segment, where it was drawn.
    chosen: Option<Rect>,
    /// The filter field, where it was drawn, in a shot that types `/` to give it the keyboard.
    typing: Option<Rect>,
    /// The sort showed its icon alone.
    sort_compact: bool,
}

impl ForksShot {
    /// The pointer's move onto what the shot rests it on, from the frame after the first.
    fn events(&self, frame: usize) -> Vec<egui::Event> {
        let mut events: Vec<egui::Event> = self.pointer.filter(|_| frame >= 1).map(|(at, _)| egui::Event::PointerMoved(at)).into_iter().collect();
        if self.typing.is_some() && frame == 1 {
            events.push(egui::Event::Text("/".to_owned()));
        }
        events
    }

    /// Draws the forks screen of `session` in `layout` when it shows, taking no press, and notes its rows and where
    /// `shot`'s pointer goes; whether it was drawn.
    #[expect(clippy::too_many_arguments, reason = "the screen's own entry, as a shot's frame calls it")]
    fn show(
        &mut self,
        ui: &mut egui::Ui,
        shot: &Shot,
        layout: &shell::Layout,
        plan: Option<&forks_screen::Plan>,
        sheet: &Sheet,
        mode: Mode,
        keys: bool,
    ) -> bool {
        let Some(plan) = plan else { return false };
        assert_eq!(forks_screen::show(ui, layout, plan, Some(sheet), mode, &mut self.screen, keys), [], "{}: a press", shot.name);
        let place = |owner: &str| forks_screen::row_rect(plan, self.screen.scroll(), owner);
        self.rows = plan.rows.iter().filter_map(|row| place(&row.owner).map(|rect| (rect, row.chosen))).collect();
        let controls = forks_screen::control_places(ui.painter(), plan);
        let control = |words: &str| controls.iter().find(|(said, _)| said == words).map(|(_, rect)| *rect);
        self.chosen = control(forks_screen::chosen_segment(plan));
        self.sort_compact = plan.toolbar.sort_compact;
        self.pointer = match shot.over {
            Over::Forks(Pointing::Control(words)) => {
                let rect = control(words).unwrap_or_else(|| panic!("{}: no {words}", shot.name));
                Some((rect.center(), rect))
            }
            Over::Forks(Pointing::RowButton(owner, which)) => {
                let button = forks_screen::row_buttons(plan, self.screen.scroll(), owner)[which];
                let rect = button.unwrap_or_else(|| panic!("{}: {owner} has no button {which}", shot.name));
                Some((rect.center(), rect))
            }
            Over::Forks(Pointing::Row(owner)) => {
                let rect = place(owner).unwrap_or_else(|| panic!("{}: no row of {owner}", shot.name));
                Some((rect.center(), rect))
            }
            Over::Forks(Pointing::Back) => Some((layout.toggle.center(), layout.toggle)),
            _ => None,
        };
        self.typing = (shot.over == Over::Forks(Pointing::Typing)).then(|| control("Filter by owner or name")).flatten();
        true
    }
}

/// On the forks screen, a fill that names what lies under the texts and graphics over it: a row's (cut to the
/// panel's round corners, a path) and the hover's round fill on the button back; where it is and its colour.
fn forks_fill(shape: &Shape) -> Option<(Rect, Color32)> {
    match shape {
        Shape::Path(path) if path.fill.a() > 0 => Some((path.visual_bounding_rect(), path.fill)),
        Shape::Circle(circle) if circle.fill.a() > 0 => Some((circle.visual_bounding_rect(), circle.fill)),
        _ => None,
    }
}

/// The overlay's panes `wanted` as a shot's frame draws them: each baked on `stage` into `glass` (kept while it
/// stands where it was baked), with its texture of [`GLASS_TEXTURES`]. `name` is the shot's.
#[expect(clippy::cast_precision_loss, reason = "picture sizes of a few thousand pixels")]
fn overlay_drawn(wanted: &[Want], glass: &mut Vec<(Rect, Rc<Bitmap>)>, stage: &Stage, name: &str) -> Vec<Drawn> {
    assert!(wanted.len() <= GLASS_TEXTURES.len(), "{name}: more overlay panes than textures for them");
    glass.truncate(wanted.len());
    let mut drawn = Vec::new();
    for (index, (want, texture)) in wanted.iter().zip(GLASS_TEXTURES).enumerate() {
        if glass.get(index).is_none_or(|(rect, _)| *rect != want.rect) {
            glass.truncate(index);
            glass.push((want.rect, stage.pane(want)));
        }
        let picture = &glass[index].1;
        let uv = Rect::from_min_max(Pos2::ZERO, pos2(want.rect.width() / picture.w as f32, want.rect.height() / picture.h as f32));
        drawn.push(Drawn { id: want.id, texture, rect: want.rect, uv });
    }
    drawn
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

/// The row among the Add list's `rows` that `session`'s core highlights, if any.
fn add_lit(session: &Session, rows: &[(Rect, bool)]) -> Option<Rect> {
    let repos = session.repos()?;
    let at = repos.add_list().iter().position(|remote| repos.add_selected.as_deref().is_some_and(|id| remote.id().eq_ignore_ascii_case(id)))?;
    rows.get(at).map(|(row, _)| *row)
}

/// Where the core's highlight of `session`'s menu and side menu stands among their lines, when they are open.
fn menu_lit(session: &Session) -> (Option<usize>, Option<usize>) {
    let Some(repos) = session.repos() else { return (None, None) };
    let menu = repos.menu_highlight().and_then(|item| crate::menu::shown().iter().position(|shown| *shown == item));
    let side = repos.migrate_highlight().and_then(|item| MigrateItem::ALL.iter().position(|known| *known == item));
    (menu, side)
}

/// The words the panel of `session` shows instead of the table: its title and the lines under it; none with a table.
fn state_words(session: &Session) -> Vec<String> {
    repos_screen::state_text(session).map_or_default(|(title, lines)| std::iter::once(title).chain(lines).collect())
}

/// What a person does in the frame `frame` of a shot over `over`: the folder field's keys and pointer at `pointer`,
/// Enter once the dialog is open, the pointer on a row of the Add list `found`, on the top row's main button `main` (over
/// [`Over::TopHovered`], the control it names),
/// or Down or the pointer on the first row of a dialog's folder field `field` (all from the last frame).
fn shot_events(over: Over, frame: usize, pointer: Option<Pos2>, found: Option<Rect>, main: Option<Rect>, field: Option<Rect>) -> Vec<egui::Event> {
    let key = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    match (over, pointer) {
        (Over::Rows(RowsShot::Lit), _) if frame == 2 => vec![key(egui::Key::ArrowDown)],
        (Over::Rows(RowsShot::Hovered), _) if frame >= 1 => {
            field.map_or_default(|field| vec![egui::Event::PointerMoved(widgets::tests::folder_row_centre(field, 0))])
        }
        (Over::MainHovered | Over::TopHovered(_), _) if frame >= 1 => main.map_or_default(|main| vec![egui::Event::PointerMoved(main.center())]),
        (Over::Hovering(index), _) if frame >= 1 => found.map_or_default(|found: Rect| {
            let row = f32::from(u16::try_from(index).unwrap_or(u16::MAX));
            vec![egui::Event::PointerMoved(pos2(found.center().x, found.top() + (row + 0.5) * ADD_ROW))]
        }),
        (Over::Folder(kind), Some(at)) if kind != FolderShot::Empty => widgets::tests::folder_shot_events(frame, at, kind == FolderShot::Lit),
        (Over::Entered, _) if frame == 2 => vec![key(egui::Key::Enter)],
        _ => Vec::new(),
    }
}

/// Checks that the frame `last` of `shot`, at the window `size` in `mode`, draws the states the shot says it
/// measures, so that each claimed state is what was measured: the row of the Add list the pointer rests on whole
/// inside the list, filled with the hover's fill when it is an ordinary row and not when it is installed (an
/// installed row shows no hover); the row of the Add list the core highlights on the selection's fill; the top row's
/// main button on the hover's fill; the menu's highlighted line and its side menu's on the selection's fill; a folder
/// field's lit row on the selection's fill (and the row under the pointer beside it unfilled), its row under the
/// pointer on the hover's fill.
fn state_lands(shot: &Shot, last: &Frame, size: egui::Vec2, mode: Mode) {
    let name = shot.name;
    let palette = theme::palette(mode);
    let hover = palette.on_glass.gamma_multiply(HOVER_OPACITY);
    let filled_with = |colour: Color32, place: Rect| {
        last.shapes.iter().any(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == colour && place.contains_rect(rect.rect)))
    };
    let filled = |place: Rect| filled_with(hover, place);
    // The row of a folder field's suggestion `index`, whole.
    let folder_row = |index: usize| {
        let field = last.field.unwrap_or_else(|| panic!("{name}: no folder field"));
        Rect::from_center_size(widgets::tests::folder_row_centre(field, index), vec2(field.width(), theme::FOLDER_ROW))
    };
    if let Some(row) = last.add_lit {
        assert!(
            filled_with(palette.selection, row),
            "{name} at {size:?} in {mode:?}: the highlighted row of the Add list is not on the selection's fill"
        );
    }
    if let (Some((pane, side)), (lit, side_lit)) = (last.menu, last.menu_lit) {
        let line = crate::menu::line_rects(pane)[lit.unwrap_or_else(|| panic!("{name}: no line of the menu is highlighted"))];
        assert!(filled_with(palette.selection, line.expand(0.5)), "{name} at {size:?} in {mode:?}: the menu's highlighted line is not lit");
        if let Some(side_lit) = side_lit {
            let side_pane = side.unwrap_or_else(|| panic!("{name}: the side menu is open in the core, not drawn"));
            let line = crate::menu::side_line_rects(side_pane)[side_lit];
            assert!(filled_with(palette.selection, line.expand(0.5)), "{name} at {size:?} in {mode:?}: the side menu's highlighted line is not lit");
        }
    }
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
        Over::TopHovered(words) => {
            let control = last.top_hovered.unwrap_or_else(|| panic!("{name}: no {words} in the top row"));
            assert!(
                last.shapes
                    .iter()
                    .any(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == hover && control.expand(0.5).contains_rect(rect.rect))),
                "{name} at {size:?} in {mode:?}: no hover fill drawn on {words}"
            );
        }
        Over::Rows(RowsShot::Lit) | Over::Folder(FolderShot::Lit) => {
            assert!(filled_with(palette.selection, folder_row(0)), "{name} at {size:?} in {mode:?}: the first folder is not lit");
            assert!(
                !filled(folder_row(0)) && !filled(folder_row(1)),
                "{name} at {size:?} in {mode:?}: a folder on the hover's fill beside the lit one"
            );
        }
        Over::Rows(RowsShot::Hovered) => {
            assert!(filled(folder_row(0)), "{name} at {size:?} in {mode:?}: the folder under the pointer is not on the hover's fill");
        }
        Over::Folder(FolderShot::Hovered) => {
            assert!(filled(folder_row(1)), "{name} at {size:?} in {mode:?}: the folder under the pointer is not on the hover's fill");
        }
        Over::Forks(pointing) => forks_state_lands(last, pointing, &format!("{name} at {size:?} in {mode:?}"), mode),
        _ => {}
    }
    claims_land(shot, last, &format!("{name} at {size:?} in {mode:?}"));
}

/// The words of the texts `last` drew whose middle lies in `room`.
fn texts_in(last: &Frame, room: Rect) -> Vec<String> {
    last.shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            Shape::Text(text) if room.contains(text.galley.rect.translate(text.pos.to_vec2()).center()) => Some(text.galley.text().to_owned()),
            _ => None,
        })
        .collect()
}

/// Checks that the frame `last` of `shot` draws what the shot's set-up makes the core say, so that the state the shot
/// names is what was measured: a fetch's count, its pause while the core reports one (and none once it goes on or the
/// run ended), Close and no Cancel once it ended, the first two tasks of a fetch of several (the list scrolls in the
/// smallest window); the forks' sentence for an empty panel, else the sort's arrow in the head row (none where the
/// sorted column has given way); in the shots of the controls the sort's words, unless it shows its icon alone; with
/// `/` typed, the keyboard in the filter; in a narrow window, the status bar's sentence without its notes.
fn claims_land(shot: &Shot, last: &Frame, what: &str) {
    let mut probe = demo::repos();
    (shot.set_up)(&mut probe);
    let window = Rect::from_min_size(Pos2::ZERO, vec2(f32::INFINITY, f32::INFINITY));
    let all = texts_in(last, window);
    if let Some(progress) = probe.fork_progress().filter(|_| shot.over == Over::Dialog) {
        let finished = shot.name == FETCH_FINISHED;
        // The pause's sentence under the count, not the output's line that said the pause began.
        let paused = all.iter().any(|words| words.contains("going on in"));
        match progress.pause.filter(|_| !finished) {
            Some(pause) => assert!(all.contains(&pause), "{what}: the pause {pause:?} is not drawn"),
            None => assert!(!paused, "{what}: a pause is drawn"),
        }
        let count = format!("{} of {} forks, {} at a time", progress.done, progress.total, progress.at_a_time);
        assert!(all.contains(&count), "{what}: the count {count:?} is not drawn");
        if finished {
            assert!(all.iter().any(|words| words == "Close") && !all.iter().any(|words| words == "Cancel"), "{what}: the run is not drawn ended");
        }
        if shot.name == "forks fetching several" {
            // The list scrolls in the smallest window: the running task and the one after it are in view.
            for name in &FETCH_SEVERAL[..2] {
                assert!(all.iter().any(|words| words == name), "{what}: the task list has no {name}");
            }
        }
    }
    let Over::Forks(pointing) = shot.over else { return };
    let view = probe.forks.as_ref().unwrap_or_else(|| panic!("{what}: the forks are not open"));
    let panel = texts_in(last, last.layout.panel);
    if let Some(empty) = view.empty_message() {
        assert!(panel.iter().any(|words| words == empty.trim()), "{what}: the panel does not say {empty:?}");
    } else {
        let arrow = assets::icon(Icon::ArrowDown).to_string();
        let arrows = texts_in(last, table::head_rect(last.layout.panel)).into_iter().filter(|words| *words == arrow).count();
        // Sorted by the newest commit in a panel too narrow for that column, the arrow has no head to stand after
        // (the column gives way first): none is drawn.
        let gone =
            view.sort == devassist_core::repos::state::ForkSort::Newest && crate::columns::fork_columns(last.layout.panel.width()).newest.is_none();
        assert_eq!(arrows, usize::from(!gone), "{what}: not the sort's arrow in the head row");
    }
    if FORKS_CONTROL_SHOTS.contains(&shot.name) {
        let sort = widgets::capitalised(view.sort.label());
        // Where the toolbar gives way, the sort shows its icon alone (the forks' plan says so).
        assert!(last.forks_sort_compact || texts_in(last, last.layout.toolbar).contains(&sort), "{what}: the sort does not say {sort:?}");
    }
    match pointing {
        Pointing::Typing => assert!(forks_screen::filter_focused(&last.ctx), "{what}: the filter has no keyboard"),
        Pointing::Narrow => {
            let bar = texts_in(last, last.layout.status);
            assert!(bar.contains(&view.summary()), "{what}: the status bar does not say the forks' sentence: {bar:?}");
            assert!(!bar.iter().any(|words| words.contains("api-fork") || words.starts_with("Fetched")), "{what}: the notes stayed: {bar:?}");
        }
        _ => {}
    }
}

/// Checks that a shot of the forks screen draws what it claims: with rows, one highlighted row whole on the
/// selection's fill (the panel's rows are cut to its round corners, so their fill is a path), in view; the row the
/// pointer rests on, whole in the panel, on the hover's fill and not the selection's; the button back, under the
/// pointer, on the hover's fill.
fn forks_state_lands(last: &Frame, pointing: Pointing, what: &str, mode: Mode) {
    let palette = theme::palette(mode);
    let hover = palette.on_glass.gamma_multiply(HOVER_OPACITY);
    let filled_path = |colour: Color32, place: Rect| {
        last.shapes.iter().any(|clipped| {
            matches!(&clipped.shape, Shape::Path(path) if path.fill == colour && place.expand(0.5).contains_rect(path.visual_bounding_rect()))
        })
    };
    let filled_rect = |colour: Color32, place: Rect| {
        last.shapes
            .iter()
            .any(|clipped| matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == colour && place.expand(0.5).contains_rect(rect.rect)))
    };
    let lit: Vec<Rect> = last.forks.0.iter().filter(|(_, chosen)| *chosen).map(|(rect, _)| *rect).collect();
    if !last.forks.0.is_empty() {
        assert_eq!(lit.len(), 1, "{what}: not one highlighted fork");
        assert!(table::body_rect(last.layout.panel).contains_rect(lit[0]), "{what}: the highlighted fork is not in view");
        assert!(filled_path(theme::selected_fill(mode), lit[0]), "{what}: the highlighted fork is not on the selection's fill");
    }
    match pointing {
        Pointing::Row(_) => {
            let row = last.forks.1.unwrap_or_else(|| panic!("{what}: no row under the pointer"));
            assert!(table::body_rect(last.layout.panel).contains_rect(row), "{what}: the row under the pointer is not in view");
            assert!(
                filled_path(hover, row) && !filled_path(theme::selected_fill(mode), row),
                "{what}: the row under the pointer is not on the hover's fill"
            );
        }
        Pointing::Back => {
            let back = last.forks.1.unwrap_or_else(|| panic!("{what}: no button back"));
            let circled = last
                .shapes
                .iter()
                .any(|clipped| matches!(&clipped.shape, Shape::Circle(circle) if circle.fill == hover && back.contains(circle.center)));
            assert!(circled, "{what}: the button back is not on the hover's fill");
        }
        Pointing::Control(words) => {
            let control = last.forks.1.unwrap_or_else(|| panic!("{what}: no {words}"));
            assert!(filled_rect(hover, control), "{what}: {words} is not on the hover's fill");
        }
        Pointing::RowButton(owner, _) => {
            let button = last.forks.1.unwrap_or_else(|| panic!("{what}: no button of {owner}"));
            assert!(filled_rect(hover, button), "{what}: the button of {owner} is not on the hover's fill");
        }
        // Checked by what the set-up makes the core say (`claims_land`).
        Pointing::Nowhere | Pointing::Typing | Pointing::Narrow => {}
    }
    if let Some(chosen) = last.forks_chosen {
        assert!(filled_rect(palette.selection, chosen), "{what}: the chosen segment is not on the selection's fill");
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
    let top_only = matches!(shot.over, Over::TopRow | Over::TopHovered(_));
    let ids: &[&str] = if top_only { &["actions"] } else { &["selection", "actions", "status"] };
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
                        Over::TopRow | Over::TopHovered(_) => controls.iter().any(|(_, control)| control.contains(rect.center())),
                        Over::Dialog | Over::Entered | Over::Hovering(_) | Over::Folder(_) | Over::Rows(_) => on_overlay,
                        Over::Menu => on_overlay || words == dots,
                        Over::Forks(_) => forks_measured(shot, &frame.layout, rect.center()),
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
                        // Danger text on the status bar the shell draws, on any screen (not on an overlay over it), keyed
                        // by that place alone. The user's decision, 2026-11-27: the status bar's danger text is held to
                        // 4.5 on every screen (measured on the forks screen in light: 4.57 at its lowest).
                        let danger_status = !on_overlay && region(centre) == "status" && ink_name == "danger";
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
                // Only what is painted of it: a row scrolled under the head row is cut to the rows' room.
                let forks_graphic = if let Over::Forks(_) = shot.over { rule_graphic(rect, palette, frame.layout.panel) } else { None }
                    .map(|(name, least, ink, area, inside)| (name, least, ink, area.intersect(clipped.clip_rect), inside))
                    .filter(|(.., area, _)| area.is_positive());
                if let Some((name, least, ink, area, inside)) = forks_graphic.or_else(|| overlay_graphic(shot.over, on_overlay, rect, palette)) {
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
            Shape::Mesh(mesh) if GLASS_TEXTURES.contains(&mesh.texture_id) => {
                let glass = if shot.over == Over::Menu { "menu glass" } else { "dialog glass" };
                fills.push((mesh.calc_bounds(), glass.to_owned(), on_overlay));
            }
            shape => {
                if let Some((area, fill)) = forks_fill(shape).filter(|_| matches!(shot.over, Over::Forks(_))) {
                    fills.push((area.intersect(clipped.clip_rect), colour_name(fill, mode), on_overlay));
                }
            }
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
            row.row.found.count
                >= if row.row.name.contains("focus ring") {
                    LEAST_RING_PIXELS
                } else if row.row.name.ends_with(RULE_BREAK_PLACE) {
                    LEAST_BREAK_PIXELS
                } else if [RULE_TICK_PLACE, RULE_FIFTH_PLACE, RULE_ORIGIN_PLACE].iter().any(|place| row.row.name.ends_with(place)) {
                    LEAST_RULE_PIXELS
                } else {
                    LEAST_PIXELS
                },
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

/// The rows of every shot on every backdrop in both modes, drawn as each of `renderings` draws them: the takes drawn
/// on a few threads ([`on_workers`]).
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
    let mut measured_takes: Vec<Taken> = takes.iter().copied().zip(on_workers(&takes, measure_take)).collect();
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
/// The picture of the demo's menu (`--show menu`) with the window around it, its top row and status bar: no shot,
/// as the shots of the menu draw no shell under it.
const WHOLE_MENU: &str = "menu in the whole window";

/// Not a check: pictures of `--demo --show menu` in the smallest window the app allows, light and dark, the shell
/// drawn under the menu as the window draws it, drawn as the shots are (the glass frosted on the CPU, on the first
/// backdrop), into the folder the environment variable `DEVASSIST_PICTURES` names as `f8-menu-960-light.png` and
/// `f8-menu-960-dark.png`. Run by hand with `--ignored`; without the variable it draws nothing.
#[test]
#[ignore = "draws pictures for a person to look at"]
fn pictures_of_the_menu_in_the_smallest_window() {
    let Ok(folder) = std::env::var("DEVASSIST_PICTURES") else { return };
    let shot = Shot { name: WHOLE_MENU, over: Over::Menu, set_up: |repos| demo::show(repos, "menu").unwrap() };
    for mode in [Mode::Light, Mode::Dark] {
        let scene = Scene { size: MIN_WINDOW, pixels_per_point: 1.0, mode, backdrop: BACKDROPS[0].to_owned(), blur: true };
        let stage = Stage::new(scene, Rendering::Frosted);
        let mut textures = HashMap::new();
        let frame = draw_shot(&shot, &stage, &mut textures);
        let mut bitmap = stage.window.clone();
        lay_panes(&mut bitmap, &stage, &frame.wants);
        picture::paint(&mut bitmap, &frame.ctx.tessellate(frame.shapes, 1.0), &textures);
        let name = format!("f8-menu-960-{}.png", if mode == Mode::Dark { "dark" } else { "light" });
        crate::screenshot::write_png(&std::path::Path::new(&folder).join(name), &bitmap.image()).unwrap();
        if let Some((pane, _)) = frame.menu {
            eprintln!("{mode:?}: the menu's pane {pane:?}, the top row {:?}, the window's buttons {:?}", frame.layout.actions, frame.layout.captions);
        }
    }
}

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
