//! Measures the design's promise that its text is readable on all six backdrops in both modes, on the pictures
//! the program makes: the wallpaper (scaled up as the screen shows it) with the shell's panes laid over it.
//!
//! Only where text can sit is measured: inside the padding the shell gives text in each pane, and only on pixels
//! where the pane's own picture is (all but) opaque. The rounded corners and the antialiased edge of a pane show
//! the bare wallpaper through it, and no text is ever drawn there.

use std::thread;

use devassist_core::config::BACKDROPS;
use devassist_glass::raster::{self, Bitmap, Pixel};
use egui::{Color32, Rect, pos2, vec2};

use crate::baker::{Scene, pane_picture, wallpaper};
use crate::shell;
use crate::theme::{self, Mode, ROW_PADDING, SIDEBAR_PADDING, SIDEBAR_TOP, STATUS_PADDING, TEXT_SMALL, TEXT_TITLE, WINDOW};

/// The least contrast of text at `TEXT` (WCAG's level AA for normal text).
const LEAST_TEXT: f64 = 4.5;
/// The least contrast of the quiet text at `TEXT_SMALL`.
const LEAST_QUIET: f64 = 6.0;
/// A pane's pixel counts as the pane (not its edge) from this alpha.
const SOLID: f32 = 0.99;
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

/// The lowest contrast found in one place and the pixel it is at.
#[derive(Clone, Copy)]
struct Lowest {
    contrast: f64,
    at: (usize, usize),
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

/// `over` (premultiplied) painted on `under`.
fn over(under: Pixel, over: Pixel) -> Pixel {
    let keep = 1.0 - over[3];
    [0, 1, 2, 3].map(|i| over[i] + under[i] * keep)
}

/// A window of `WINDOW` points as pixels: the wallpaper scaled up with bilinear sampling, the shell's panes over it.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "pixel places on a 1440 by 900 window, far inside every range"
)]
fn window(backdrop: &str, mode: Mode) -> Window {
    let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: backdrop.to_owned(), blur: true };
    let wall = wallpaper(&scene).unwrap();
    let (w, h) = (WINDOW[0] as usize, WINDOW[1] as usize);
    let mut bitmap = Bitmap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) * wall.scale, (y as f32 + 0.5) * wall.scale);
            bitmap.px[y * w + x] = wall.bitmap.sample(u, v);
        }
    }
    let mut panes = Vec::new();
    for want in shell::wants(vec2(WINDOW[0], WINDOW[1]), true) {
        let picture = pane_picture(&wall, &scene, &want).unwrap();
        let (left, top) = (want.rect.min.x.round() as usize, want.rect.min.y.round() as usize);
        for y in 0..picture.h.min(h - top) {
            for x in 0..picture.w.min(w - left) {
                let at = (top + y) * w + left + x;
                bitmap.px[at] = over(bitmap.px[at], picture.px[y * picture.w + x]);
            }
        }
        panes.push(Laid { id: want.id, left, top, picture });
    }
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
    let mut places = vec![
        Place { name: "sidebar", pane: Some("sidebar"), rect: sidebar_text, ink: Ink::OnGlass, least: LEAST_TEXT },
        // The panel's text is centred and the shell gives it no padding: only its own edge is left out.
        Place { name: "panel", pane: Some("panel"), rect: rect("panel"), ink: Ink::OnGlass, least: LEAST_TEXT },
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
    let pane = place.pane.and_then(|id| window.panes.iter().find(|pane| pane.id == id));
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
    let mut found = Lowest { contrast: f64::INFINITY, at: (0, 0) };
    for (x, y, under) in pixels(window, place) {
        let contrast = ratio(luminance(over(under, ink)), luminance(under));
        if contrast < found.contrast {
            found = Lowest { contrast, at: (x, y) };
        }
    }
    found
}

/// One line of the table: a place on a backdrop in a mode and its lowest contrast.
struct Row {
    backdrop: &'static str,
    mode: Mode,
    name: &'static str,
    least: f64,
    found: Lowest,
}

impl Row {
    fn text(&self) -> String {
        format!(
            "{:<7} {:<6} {:<7} lowest {:>6.2} (needs {:.1}) at x={} y={}{}\n",
            self.backdrop,
            format!("{:?}", self.mode),
            self.name,
            self.found.contrast,
            self.least,
            self.found.at.0,
            self.found.at.1,
            if self.found.contrast >= self.least { "" } else { "  FAIL" }
        )
    }
}

/// The rows of one backdrop in both modes; the places sharing a name (the two labels) give their lowest.
fn measure(backdrop: &'static str) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for mode in [Mode::Light, Mode::Dark] {
        let palette = theme::palette(mode);
        let window = window(backdrop, mode);
        let first = rows.len();
        for place in places() {
            let ink = match place.ink {
                Ink::OnGlass => palette.on_glass,
                Ink::Quiet => palette.quiet,
            };
            let found = lowest(&window, &place, ink);
            match rows[first..].iter_mut().find(|row| row.name == place.name) {
                Some(row) if found.contrast < row.found.contrast => row.found = found,
                Some(_) => {}
                None => rows.push(Row { backdrop, mode, name: place.name, least: place.least, found }),
            }
        }
    }
    rows
}

#[test]
fn every_text_on_the_shell_holds_its_contrast_on_all_six_backdrops() {
    let rows: Vec<Row> = thread::scope(|scope| {
        let jobs: Vec<_> = BACKDROPS.into_iter().map(|backdrop| scope.spawn(move || measure(backdrop))).collect();
        jobs.into_iter().flat_map(|job| job.join().unwrap()).collect()
    });
    let table: String = rows.iter().map(Row::text).collect();
    eprintln!("{table}");
    let failures: String = rows.iter().filter(|row| row.found.contrast < row.least).map(Row::text).collect();
    assert!(failures.is_empty(), "text below its contrast:\n{failures}");
}
