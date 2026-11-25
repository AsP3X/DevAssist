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

use std::thread;

use devassist_core::config::BACKDROPS;
use devassist_glass::raster::{self, Bitmap, Pixel};
use egui::epaint::{Mesh, Shape, TessellationOptions, Tessellator};
use egui::{Color32, Pos2, Rect, TextureId, pos2, vec2};

use crate::baker::{Scene, Sheet, SheetPicture, frosted_pane_picture, pane_picture, sheet_picture};
use crate::columns::{self, Span};
use crate::picture::{over, wallpaper_window};
use crate::repos_view::{self, Line};
use crate::theme::{
    self, CHECK_MARK, GROUP_FILL_OPACITY, GROUP_ROW_GAP, HEAD_FILL_OPACITY, HEAD_HEIGHT, HOVER_OPACITY, Mode, PANEL_TEXT_INSET, Palette,
    RADIUS_PANEL, ROW_PAD_RIGHT, ROW_PADDING, RULE_LABEL_GAP, RULE_WIDTH, SIDEBAR_PADDING, SIDEBAR_TOP, STATUS_PADDING, Surface, TEXT, TEXT_SMALL,
    TEXT_TITLE, WINDOW,
};
use crate::{demo, frost, shell, table, widgets};

/// The least contrast of text at `TEXT` (WCAG's level AA for normal text).
const LEAST_TEXT: f64 = 4.5;
/// The least contrast of the quiet text at `TEXT_SMALL`.
const LEAST_QUIET: f64 = 6.0;
/// A pane's pixel counts as the pane (not its edge) from this alpha.
const SOLID: f32 = 0.99;
/// The fewest pixels a place may leave to measure: a mask or rectangle that leaves (almost) none would pass
/// without having checked anything. The smallest real place, a group label's line, has several thousand.
const LEAST_PIXELS: usize = 100;
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

/// Whether the glass shader can run here. A machine without a GPU adapter skips the glass rendering, said on
/// the test's output, unless `DEVASSIST_REQUIRE_GPU` is set, as it should be where the tests must not be skipped.
fn shader_runs() -> bool {
    let ready = devassist_glass::gpu::install_own_device();
    assert!(ready || std::env::var_os("DEVASSIST_REQUIRE_GPU").is_none(), "no GPU adapter was found for the glass rendering");
    if !ready {
        eprintln!("no GPU adapter: the glass rendering is not measured");
    }
    ready
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
