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

use std::thread;

use devassist_core::config::BACKDROPS;
use devassist_glass::raster::{self, Bitmap, Pixel};
use egui::epaint::{Mesh, Shape, TessellationOptions, Tessellator};
use egui::{Color32, Pos2, Rect, TextureId, pos2, vec2};

use crate::baker::{Scene, Sheet, SheetPicture, frosted_pane_picture, pane_picture, sheet_picture};
use crate::picture::{over, wallpaper_window};
use crate::theme::{
    self, Mode, PANEL_TEXT_INSET, RADIUS_PANEL, ROW_PADDING, SIDEBAR_PADDING, SIDEBAR_TOP, STATUS_PADDING, Surface, TEXT_SMALL, TEXT_TITLE, WINDOW,
};
use crate::{frost, shell};

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

/// Which colour a place's text is drawn in.
#[derive(Clone, Copy)]
enum Ink {
    /// `on_glass`, opaque.
    OnGlass,
    /// `quiet`, laid over the pixel it is on.
    Quiet,
}

/// How the glass panes are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rendering {
    /// Every glass pane frosted on the CPU.
    Frosted,
    /// The glass panes through the glass shader on the GPU; the panel is drawn from its sheet in both.
    Glass,
}

/// A place text is on: its name, the pane whose picture it is on (none: the bare wallpaper), where text can sit,
/// its ink and the contrast it needs.
struct Place {
    name: &'static str,
    pane: Option<&'static str>,
    rect: Rect,
    ink: Ink,
    least: f64,
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
fn luminance(pixel: Pixel) -> f64 {
    0.2126 * linear(pixel[0]) + 0.7152 * linear(pixel[1]) + 0.0722 * linear(pixel[2])
}

/// WCAG's contrast ratio of two luminances.
fn ratio(a: f64, b: f64) -> f64 {
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
/// and its frosted panel over it.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixel places on a 1440 by 900 window, far inside every range")]
fn window(backdrop: &str, mode: Mode, rendering: Rendering) -> Window {
    let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: backdrop.to_owned(), blur: true };
    let (wall, mut bitmap) = wallpaper_window(&scene);
    let (w, h) = (bitmap.w, bitmap.h);
    let mut panes = Vec::new();
    let mut shaded = false;
    for want in shell::wants(vec2(WINDOW[0], WINDOW[1]), true) {
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
    let sheet = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
    let painted = painted_panel(&sheet, shell::panel_rect(vec2(WINDOW[0], WINDOW[1]), true), w, h);
    for (under, over_it) in bitmap.px.iter_mut().zip(&painted.px) {
        *under = over(*under, *over_it);
    }
    panes.push(Laid { id: "panel", left: 0, top: 0, picture: painted });
    // Glass that fell back to frost would measure the frosted panes twice.
    assert!(rendering == Rendering::Frosted || shaded, "{backdrop} {mode:?}: no pane was drawn through the glass shader");
    Window { bitmap, panes }
}

/// `rect` cut down to the band a line of text of `size` points occupies about its middle, with some room.
fn line_band(rect: Rect, size: f32) -> Rect {
    let half = size / 2.0 + LINE_SLACK;
    Rect::from_min_max(pos2(rect.left(), rect.center().y - half), pos2(rect.right(), rect.center().y + half))
}

/// The places to measure, from the shell's own geometry and the insets it gives its text.
fn places() -> Vec<Place> {
    let wants = shell::wants(vec2(WINDOW[0], WINDOW[1]), true);
    let rect = |id: &str| wants.iter().find(|want| want.id == id).unwrap().rect;
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
    let panel_text = shell::panel_rect(vec2(WINDOW[0], WINDOW[1]), true).shrink(PANEL_TEXT_INSET);
    let mut places = vec![
        Place { name: "sidebar", pane: Some("sidebar"), rect: sidebar_text, ink: Ink::OnGlass, least: LEAST_TEXT },
        // The version line under the workspace's name is quiet text at `TEXT_SMALL`.
        Place { name: "sidebar-quiet", pane: Some("sidebar"), rect: sidebar_text, ink: Ink::Quiet, least: LEAST_QUIET },
        // The panel's text is centred; its place is the panel inside `PANEL_TEXT_INSET`.
        Place { name: "panel", pane: Some("panel"), rect: panel_text, ink: Ink::OnGlass, least: LEAST_TEXT },
        // The panel's sentence is quiet text at `TEXT` (normal size), as will be a table's quiet cells.
        Place { name: "panel-quiet", pane: Some("panel"), rect: panel_text, ink: Ink::Quiet, least: LEAST_TEXT },
        // The title can be longer than the one word now, so its whole room is measured, as one line.
        Place {
            name: "title",
            pane: None,
            rect: line_band(shell::title_rect(rect("sidebar-toggle")), TEXT_TITLE),
            ink: Ink::OnGlass,
            least: LEAST_TEXT,
        },
        Place { name: "status", pane: Some("status"), rect: line_band(status_ends, TEXT_SMALL), ink: Ink::Quiet, least: LEAST_QUIET },
    ];
    // A group label's text starts `ROW_PADDING` in from its row's left and fills one line of `TEXT_SMALL`.
    for row in shell::label_rows(sidebar) {
        let text = Rect::from_min_max(pos2(row.left() + ROW_PADDING, row.top()), row.max);
        places.push(Place { name: "labels", pane: Some("sidebar"), rect: line_band(text, TEXT_SMALL), ink: Ink::Quiet, least: LEAST_QUIET });
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

/// The lowest contrast of `ink` against the pixels of `place`.
fn lowest(window: &Window, place: &Place, ink: Color32) -> Lowest {
    let ink = raster::pixel(ink);
    let mut found = Lowest { contrast: f64::INFINITY, at: (0, 0), count: 0 };
    for (x, y, under) in pixels(window, place) {
        found.count += 1;
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
    name: &'static str,
    least: f64,
    found: Lowest,
}

impl Row {
    fn text(&self) -> String {
        format!(
            "{:<7} {:<7} {:<6} {:<13} lowest {:>6.2} (needs {:.1}) at x={} y={} over {} px{}\n",
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

/// The rows of one backdrop in both modes; the places sharing a name (the two labels) give their lowest.
fn measure(backdrop: &'static str, rendering: Rendering) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for mode in [Mode::Light, Mode::Dark] {
        let palette = theme::palette(mode);
        let window = window(backdrop, mode, rendering);
        let first = rows.len();
        for place in places() {
            let ink = match place.ink {
                Ink::OnGlass => palette.on_glass,
                Ink::Quiet => palette.quiet,
            };
            let found = lowest(&window, &place, ink);
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
fn every_text_on_the_shell_holds_its_contrast_on_all_six_backdrops() {
    // Frosted first; the frosted pictures are drawn on the CPU whatever device is installed.
    let mut renderings = vec![Rendering::Frosted];
    if shader_runs() {
        renderings.push(Rendering::Glass);
    }
    let rows: Vec<Row> = renderings.iter().flat_map(|&rendering| measure_all(rendering)).collect();
    let mut names: Vec<&str> = places().iter().map(|place| place.name).collect();
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
