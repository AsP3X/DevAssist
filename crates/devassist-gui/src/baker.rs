//! The window's glass, drawn once into pictures and kept until something it depends on changes.
//!
//! Drawing a pane is slow (a blur of the whole window's backdrop and, for glass, a run of the glass shader),
//! so the window never draws one at every frame. The [`Baker`] draws the wallpaper (the backdrop photo under
//! its veil) and each pane into a picture, hands egui one texture per picture, and draws a picture again only
//! when what it shows changed: the pane's place, size, radius or surface, or the window's size, scale, mode,
//! backdrop or blur.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use devassist_glass::material::Params;
use devassist_glass::pane::{self, Behind, Pane, RIM_BLUR};
use devassist_glass::raster::{self, Bitmap};
use egui::{Color32, Context, Pos2, Rect, TextureHandle, TextureId, TextureOptions};
use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::assets;
use crate::theme::{self, BACKDROP_BLUR, Mode, PICTURE_KEEP_FRAMES, RESIZE_SETTLE, Surface, VEIL_KNEE};

/// The backdrop drawn for a key that names none.
const DEFAULT_BACKDROP: &str = "lake";
/// How far below a whole number a length of pixels may fall from rounding and still count as that number.
const PIXEL_SLACK: f32 = 1e-3;
/// The most pixels a picture may have along one side.
const MOST_PIXELS: f32 = 100_000.0;

/// What the pictures depend on besides the panes themselves.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// The window's width and height in points.
    pub size: [f32; 2],
    /// Pixels per point on the window's screen.
    pub pixels_per_point: f32,
    /// Light or dark.
    pub mode: Mode,
    /// The key of the backdrop photo, one of `devassist_core::config::BACKDROPS`; another is drawn as `"lake"`.
    pub backdrop: String,
    /// Whether the photo is blurred.
    pub blur: bool,
}

impl Scene {
    /// The window's width and height in whole pixels, or `None` for a window without area or scale.
    fn pixels(&self) -> Option<(usize, usize)> {
        let scale = self.pixels_per_point;
        if !(scale.is_finite() && scale > 0.0) {
            return None;
        }
        Some((whole_pixels(self.size[0] * scale)?, whole_pixels(self.size[1] * scale)?))
    }
}

/// One pane the shell wants drawn this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Want {
    /// The pane's name, the same at every frame; while the window is resized its last picture is found by it.
    pub id: &'static str,
    /// Where it is, in points.
    pub rect: Rect,
    /// The radius of its corners, in points.
    pub radius: f32,
    /// Its kind, which gives its material.
    pub surface: Surface,
}

/// A picture to draw and where.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Drawn {
    /// The picture's texture.
    pub texture: TextureId,
    /// Where to draw it, in points.
    pub rect: Rect,
}

/// A length in pixels as a count of whole pixels, rounded up; `None` for no length or one beyond any picture.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to be within 1..=MOST_PIXELS before the cast")]
fn whole_pixels(length: f32) -> Option<usize> {
    let rounded = (length - PIXEL_SLACK).ceil();
    (rounded.is_finite() && (1.0..=MOST_PIXELS).contains(&rounded)).then_some(rounded as usize)
}

/// A count of pixels as a length.
#[expect(clippy::cast_precision_loss, reason = "pictures are at most a few thousand pixels a side")]
fn length(pixels: usize) -> f32 {
    pixels as f32
}

/// Whether two window sizes are exactly the same numbers: a size the window keeps is given again unchanged.
fn same_size(a: [f32; 2], b: [f32; 2]) -> bool {
    a.map(f32::to_bits) == b.map(f32::to_bits)
}

/// The key of a backdrop that has a photo: `key` itself, or the default one.
fn known_backdrop(key: &str) -> &str {
    if assets::backdrop(key).is_some() { key } else { DEFAULT_BACKDROP }
}

/// The photo of the backdrop `key` (the default one for an unknown key), decoded; `None`, said in the log,
/// when it does not decode.
fn decode(key: &str) -> Option<RgbaImage> {
    let key = known_backdrop(key);
    let bytes = assets::backdrop(key)?;
    match image::load_from_memory(bytes) {
        Ok(photo) => Some(photo.to_rgba8()),
        Err(error) => {
            log::error!("the backdrop {key} does not decode: {error}");
            None
        }
    }
}

/// `photo` scaled to cover `w` by `h` pixels and cut to them about its middle, in display colours.
fn cover(photo: &RgbaImage, w: usize, h: usize) -> Option<Bitmap> {
    let (photo_w, photo_h) = photo.dimensions();
    let (want_w, want_h) = (u32::try_from(w).ok()?, u32::try_from(h).ok()?);
    if photo_w == 0 || photo_h == 0 {
        return None;
    }
    let scale = (f64::from(want_w) / f64::from(photo_w)).max(f64::from(want_h) / f64::from(photo_h));
    #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a positive size of at most a few times the window's pixels")]
    let scaled = |side: u32, least: u32| ((f64::from(side) * scale).ceil() as u32).max(least);
    let (scaled_w, scaled_h) = (scaled(photo_w, want_w), scaled(photo_h, want_h));
    let resized = imageops::resize(photo, scaled_w, scaled_h, FilterType::Triangle);
    let cut = imageops::crop_imm(&resized, (scaled_w - want_w) / 2, (scaled_h - want_h) / 2, want_w, want_h).to_image();
    // A JPEG is opaque, so its colours are already premultiplied.
    Bitmap::from_bytes(w, h, cut.as_raw())
}

/// Lays the veil over `picture` (display colours): `colour` at `opacity[0]` from the top down to
/// [`VEIL_KNEE`] of the height, rising evenly from there to `opacity[1]` at the bottom.
fn lay_veil(picture: &mut Bitmap, colour: Color32, opacity: [f32; 2]) {
    if picture.w == 0 || picture.h == 0 {
        return;
    }
    let veil = raster::pixel(colour);
    let height = length(picture.h);
    for (row, line) in picture.px.chunks_mut(picture.w).enumerate() {
        let down = (length(row) + 0.5) / height;
        let rise = ((down - VEIL_KNEE) / (1.0 - VEIL_KNEE)).clamp(0.0, 1.0);
        let strength = (opacity[0] + (opacity[1] - opacity[0]) * rise).clamp(0.0, 1.0);
        if strength <= 0.0 {
            continue;
        }
        for pixel in line {
            *pixel = [0, 1, 2, 3].map(|i| veil[i] * strength + pixel[i] * (1.0 - strength));
        }
    }
}

/// The wallpaper of `scene` from its decoded `photo`.
fn wallpaper_from(photo: &RgbaImage, scene: &Scene) -> Option<Bitmap> {
    let (w, h) = scene.pixels()?;
    let covered = cover(photo, w, h)?;
    // `prepare` grows the picture by its own mirrored edge before it blurs, so the window's edges don't fade.
    let mut picture = if scene.blur { pane::to_display(&pane::prepare(&covered, BACKDROP_BLUR, scene.pixels_per_point)) } else { covered };
    lay_veil(&mut picture, theme::palette(scene.mode).veil, theme::veil(&scene.backdrop, scene.mode));
    Some(picture)
}

/// The whole window's backdrop in display colours, `size × pixels_per_point` pixels: the photo scaled to cover
/// the window, blurred when `scene.blur`, and the veil laid over it. `None` for a window without area.
#[must_use]
pub fn wallpaper(scene: &Scene) -> Option<Bitmap> {
    scene.pixels()?;
    wallpaper_from(&decode(&scene.backdrop)?, scene)
}

/// The wallpaper, made ready for panes of one blur: blurred in linear light for their body, and, once a glass
/// pane needs it, only lightly for its rim.
struct Prepared {
    soft: Bitmap,
    rim: Option<Bitmap>,
}

/// The wallpaper as panes drawn with `params` see it, from the ones `prepared` already, or prepared now and kept.
fn behind<'a>(prepared: &'a mut HashMap<u32, Prepared>, wallpaper: &Bitmap, params: &Params, scale: f32) -> Behind<'a> {
    let kept = prepared.entry(params.blur.to_bits()).or_insert_with(|| Prepared { soft: pane::prepare(wallpaper, params.blur, scale), rim: None });
    if params.glass && kept.rim.is_none() {
        kept.rim = Some(pane::prepare(wallpaper, params.blur * RIM_BLUR, scale));
    }
    // A frosted pane shows no rim; it is lent the body's picture, which it doesn't read.
    Behind { soft: &kept.soft, rim: kept.rim.as_ref().unwrap_or(&kept.soft), scale }
}

/// What `pane` draws `want` as in `scene`, or `None` for one without area.
fn pane_of(scene: &Scene, want: &Want) -> Option<Pane> {
    let size = want.rect.size();
    if !(want.rect.is_finite() && size.x > 0.0 && size.y > 0.0) {
        return None;
    }
    let params = theme::params(want.surface, scene.mode, (size.x, size.y));
    Some(Pane { rect: [want.rect.min.x, want.rect.min.y, size.x, size.y], radius: want.radius, params })
}

/// One pane drawn over `wallpaper`, with the prepared copies of it kept in `prepared`.
fn draw_pane(prepared: &mut HashMap<u32, Prepared>, wallpaper: &Bitmap, scale: f32, pane: &Pane) -> Option<Bitmap> {
    pane::draw(&behind(prepared, wallpaper, &pane.params, scale), pane)
}

/// The picture of the pane `want` over `wallpaper` (from [`wallpaper`] for the same `scene`), in display
/// colours, `want.rect × pixels_per_point` pixels. `None` for a pane or a window without area.
#[must_use]
pub fn pane_picture(wallpaper: &Bitmap, scene: &Scene, want: &Want) -> Option<Bitmap> {
    scene.pixels()?;
    draw_pane(&mut HashMap::new(), wallpaper, scene.pixels_per_point, &pane_of(scene, want)?)
}

/// What a pane's picture shows besides the scene: its rectangle in whole pixels, its radius and its surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct PaneKey {
    rect: [i64; 4],
    radius: u32,
    surface: Surface,
}

impl PaneKey {
    fn of(want: &Want, scale: f32) -> PaneKey {
        #[expect(clippy::cast_possible_truncation, reason = "a rounded place on a window, far inside the i64 range")]
        let pixel = |v: f32| (v * scale).round() as i64;
        let Rect { min, max } = want.rect;
        PaneKey { rect: [pixel(min.x), pixel(min.y), pixel(max.x), pixel(max.y)], radius: want.radius.to_bits(), surface: want.surface }
    }
}

/// A pane's picture and the last frame that wanted it.
struct Kept {
    texture: TextureHandle,
    wanted: u64,
}

/// The wallpaper of the last settled scene, with its texture and the copies of it prepared for panes.
struct Settled {
    scene: Scene,
    wallpaper: Bitmap,
    texture: TextureHandle,
    prepared: HashMap<u32, Prepared>,
}

/// Makes and keeps the window's pictures.
#[derive(Default)]
pub struct Baker {
    /// The backdrop photos decoded so far, by key (`None` for one that did not decode).
    photos: HashMap<String, Option<RgbaImage>>,
    /// The wallpaper the pane pictures were drawn over.
    settled: Option<Settled>,
    /// The pane pictures.
    panes: HashMap<PaneKey, Kept>,
    /// The picture each pane was last drawn with, by the pane's id.
    by_id: HashMap<&'static str, PaneKey>,
    /// While the window's size differs from the settled one: that size, and since when the window has had it.
    resizing: Option<([f32; 2], Duration)>,
    /// How many frames there have been.
    frame: u64,
    /// How many pane pictures and wallpapers have been made.
    counts: (usize, usize),
}

impl Baker {
    /// A baker with no pictures yet.
    #[must_use]
    pub fn new() -> Baker {
        Baker::default()
    }

    /// The wallpaper and one picture per wanted pane, baking only what changed. `now` is the frame's time.
    /// While the window's size keeps changing, nothing is baked: the pictures of the last settled size are
    /// returned, stretched to the new rectangles. Returns the wallpaper first.
    pub fn frame(&mut self, ctx: &Context, scene: &Scene, wants: &[Want], now: Duration) -> Vec<Drawn> {
        if scene.pixels().is_none() {
            return Vec::new();
        }
        self.frame += 1;
        let resized = self.settled.as_ref().is_some_and(|settled| !same_size(settled.scene.size, scene.size));
        if resized {
            let since = match self.resizing {
                Some((size, since)) if same_size(size, scene.size) => since,
                _ => now,
            };
            self.resizing = Some((scene.size, since));
            if now.saturating_sub(since) < RESIZE_SETTLE {
                return self.drawn(scene, wants);
            }
        }
        self.resizing = None;
        self.bake(ctx, scene, wants);
        self.drawn(scene, wants)
    }

    /// How many pane pictures and how many wallpapers have been made since the start.
    #[must_use]
    pub fn counts(&self) -> (usize, usize) {
        self.counts
    }

    /// Whether a bake is waiting for the size to settle (the shell asks for another frame then).
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.resizing.is_some()
    }

    /// Makes the wallpaper of `scene` when it is not the settled one, and the picture of every wanted pane
    /// there is none of; drops the pictures no frame wanted for [`PICTURE_KEEP_FRAMES`].
    fn bake(&mut self, ctx: &Context, scene: &Scene, wants: &[Want]) {
        let start = Instant::now();
        let fresh = self.settled.as_ref().is_none_or(|settled| settled.scene != *scene);
        if fresh {
            let key = known_backdrop(&scene.backdrop);
            let photo = self.photos.entry(key.to_owned()).or_insert_with(|| decode(key));
            let Some(wallpaper) = photo.as_ref().and_then(|photo| wallpaper_from(photo, scene)) else { return };
            let texture = ctx.load_texture("glass-wallpaper", wallpaper.image(), TextureOptions::LINEAR);
            self.settled = Some(Settled { scene: scene.clone(), wallpaper, texture, prepared: HashMap::new() });
            self.panes.clear();
            self.by_id.clear();
            self.counts.1 += 1;
        }
        let Some(settled) = self.settled.as_mut() else { return };
        let scale = scene.pixels_per_point;
        let mut made = 0;
        for want in wants {
            let Some(pane) = pane_of(scene, want) else {
                self.by_id.remove(want.id);
                continue;
            };
            let key = PaneKey::of(want, scale);
            if let Some(kept) = self.panes.get_mut(&key) {
                kept.wanted = self.frame;
            } else if let Some(picture) = draw_pane(&mut settled.prepared, &settled.wallpaper, scale, &pane) {
                let texture = ctx.load_texture(format!("glass-{}", want.id), picture.image(), TextureOptions::LINEAR);
                self.panes.insert(key, Kept { texture, wanted: self.frame });
                self.counts.0 += 1;
                made += 1;
            }
            self.by_id.insert(want.id, key);
        }
        let frame = self.frame;
        self.panes.retain(|_, kept| frame - kept.wanted < PICTURE_KEEP_FRAMES);
        self.by_id.retain(|_, key| self.panes.contains_key(key));
        if fresh || made > 0 {
            log::info!("baked {made} panes in {} ms", start.elapsed().as_millis());
        }
    }

    /// The settled wallpaper over the whole window, then each wanted pane's latest picture at its rectangle.
    fn drawn(&self, scene: &Scene, wants: &[Want]) -> Vec<Drawn> {
        let Some(settled) = &self.settled else { return Vec::new() };
        let window = Drawn { texture: settled.texture.id(), rect: Rect::from_min_size(Pos2::ZERO, scene.size.into()) };
        let panes = wants.iter().filter_map(|want| {
            let kept = self.by_id.get(want.id).and_then(|key| self.panes.get(key))?;
            Some(Drawn { texture: kept.texture.id(), rect: want.rect })
        });
        std::iter::once(window).chain(panes).collect()
    }
}

#[cfg(test)]
mod tests {
    use egui::{pos2, vec2};

    use super::*;
    use crate::theme::{BAR_HEIGHT, INSET, RADIUS_PANE, RADIUS_PANEL, SIDEBAR_WIDTH, STATUS_HEIGHT, WINDOW};

    /// The window most tests run in.
    const SIZE: [f32; 2] = [800.0, 600.0];

    fn scene(size: [f32; 2]) -> Scene {
        Scene { size, pixels_per_point: 1.0, mode: Mode::Light, backdrop: "lake".to_owned(), blur: true }
    }

    fn at(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    /// The shell's four panes in a window of `size`, laid out as the design lays them out at 1440 by 900 (where
    /// the sidebar is 12, 12, 256 × 876 and the panel 280, 60 to 1428, 844), each kept inside the window with at
    /// least a point a side.
    fn shell_wants_at(size: [f32; 2]) -> Vec<Want> {
        let [w, h] = size;
        let content = INSET + SIDEBAR_WIDTH + INSET;
        let status_top = h - INSET - STATUS_HEIGHT;
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
            let (x0, y0) = (x0.clamp(0.0, w - 1.0), y0.clamp(0.0, h - 1.0));
            Rect::from_min_max(pos2(x0, y0), pos2(x1.clamp(x0 + 1.0, w), y1.clamp(y0 + 1.0, h)))
        };
        vec![
            Want { id: "sidebar", rect: rect(INSET, INSET, INSET + SIDEBAR_WIDTH, h - INSET), radius: RADIUS_PANE, surface: Surface::Sidebar },
            Want {
                id: "sidebar-toggle",
                rect: rect(content, INSET, content + BAR_HEIGHT, INSET + BAR_HEIGHT),
                radius: BAR_HEIGHT / 2.0,
                surface: Surface::Capsule,
            },
            Want {
                id: "panel",
                rect: rect(content, INSET + BAR_HEIGHT + INSET, w - INSET, status_top - INSET),
                radius: RADIUS_PANEL,
                surface: Surface::Panel,
            },
            Want { id: "status-bar", rect: rect(content, status_top, w - INSET, h - INSET), radius: RADIUS_PANEL, surface: Surface::StatusBar },
        ]
    }

    fn shell_wants() -> Vec<Want> {
        shell_wants_at(SIZE)
    }

    fn whole(size: [f32; 2]) -> Rect {
        Rect::from_min_size(Pos2::ZERO, size.into())
    }

    #[test]
    fn an_unchanged_frame_bakes_nothing() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (scene, wants) = (scene(SIZE), shell_wants());
        let drawn = baker.frame(&ctx, &scene, &wants, at(0));
        let first = baker.counts();
        assert_eq!(first, (wants.len(), 1));
        assert_eq!(drawn.len(), wants.len() + 1);
        for ms in 1..100 {
            let again = baker.frame(&ctx, &scene, &wants, at(ms * 16));
            assert_eq!(again.iter().map(|d| (d.texture, d.rect)).collect::<Vec<_>>(), drawn.iter().map(|d| (d.texture, d.rect)).collect::<Vec<_>>());
        }
        assert_eq!(baker.counts(), first);
        assert!(!baker.waiting());
    }

    #[test]
    fn moving_one_pane_bakes_that_pane_only() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (scene, mut wants) = (scene(SIZE), shell_wants());
        let before = baker.frame(&ctx, &scene, &wants, at(0));
        let counts = baker.counts();
        wants[0].rect = wants[0].rect.translate(vec2(10.0, 0.0));
        let after = baker.frame(&ctx, &scene, &wants, at(16));
        assert_eq!(baker.counts(), (counts.0 + 1, counts.1));
        assert_eq!(after.len(), wants.len() + 1);
        assert_eq!(after[1].rect, wants[0].rect);
        assert_ne!(after[1].texture, before[1].texture);
        // The wallpaper and the other panes are the same pictures as before.
        assert_eq!(after[0].texture, before[0].texture);
        for index in 2..after.len() {
            assert_eq!(after[index].texture, before[index].texture);
        }
    }

    #[test]
    fn a_resize_waits_until_the_size_is_still() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let first = baker.frame(&ctx, &scene(SIZE), &shell_wants(), at(0));
        let settled = baker.counts();
        assert!(!baker.waiting());
        for (width, ms) in [(810.0, 16), (820.0, 32), (830.0, 48)] {
            let size = [width, SIZE[1]];
            let wants = shell_wants_at(size);
            let drawn = baker.frame(&ctx, &scene(size), &wants, at(ms));
            assert_eq!(baker.counts(), settled, "baked while the size changed, at {width}");
            assert!(baker.waiting());
            assert_eq!(drawn.len(), wants.len() + 1);
            // The settled pictures, stretched to this frame's rectangles.
            assert_eq!((drawn[0].texture, drawn[0].rect), (first[0].texture, whole(size)));
            for ((drawn, want), old) in drawn[1..].iter().zip(&wants).zip(&first[1..]) {
                assert_eq!((drawn.texture, drawn.rect), (old.texture, want.rect), "{}", want.id);
            }
        }
        let size = [830.0, SIZE[1]];
        let wants = shell_wants_at(size);
        let drawn = baker.frame(&ctx, &scene(size), &wants, at(48) + RESIZE_SETTLE);
        assert_eq!(baker.counts(), (settled.0 + wants.len(), settled.1 + 1));
        assert!(!baker.waiting());
        assert_eq!(drawn[0].rect, whole(size));
        assert_ne!(drawn[0].texture, first[0].texture);
    }

    #[test]
    fn another_theme_or_backdrop_bakes_everything_once() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (mut scene, wants) = (scene(SIZE), shell_wants());
        baker.frame(&ctx, &scene, &wants, at(0));
        let light = baker.counts();
        scene.mode = Mode::Dark;
        baker.frame(&ctx, &scene, &wants, at(16));
        baker.frame(&ctx, &scene, &wants, at(32));
        let dark = baker.counts();
        assert_eq!(dark, (light.0 + wants.len(), light.1 + 1));
        assert!(!baker.waiting());
        scene.backdrop = "night".to_owned();
        baker.frame(&ctx, &scene, &wants, at(48));
        baker.frame(&ctx, &scene, &wants, at(64));
        assert_eq!(baker.counts(), (dark.0 + wants.len(), dark.1 + 1));
    }

    #[test]
    fn a_window_without_size_bakes_nothing_and_returns_nothing() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        assert_eq!(baker.frame(&ctx, &scene([0.0, 0.0]), &shell_wants(), at(0)), Vec::new());
        assert_eq!(baker.counts(), (0, 0));
        let mut odd = scene(SIZE);
        for (size, pixels_per_point) in [([-5.0, 600.0], 1.0), ([800.0, f32::NAN], 1.0), ([f32::INFINITY, 600.0], 1.0), (SIZE, 0.0), (SIZE, -1.0)] {
            (odd.size, odd.pixels_per_point) = (size, pixels_per_point);
            assert_eq!(baker.frame(&ctx, &odd, &shell_wants(), at(0)), Vec::new(), "{size:?} at {pixels_per_point}");
            assert!(wallpaper(&odd).is_none());
        }
        assert_eq!(baker.counts(), (0, 0));
    }

    #[test]
    fn an_unknown_backdrop_is_drawn_as_the_default_one() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (mut moon, wants) = (scene(SIZE), shell_wants());
        moon.backdrop = "moon".to_owned();
        let drawn = baker.frame(&ctx, &moon, &wants, at(0));
        assert_eq!(drawn.len(), wants.len() + 1);
        assert_eq!(baker.counts(), (wants.len(), 1));
        // The same picture as the default backdrop's, in both modes.
        for mode in [Mode::Light, Mode::Dark] {
            let (mut small_moon, mut lake) = (scene([64.0, 40.0]), scene([64.0, 40.0]));
            (small_moon.backdrop, small_moon.mode, lake.mode) = ("moon".to_owned(), mode, mode);
            assert_eq!(wallpaper(&small_moon).map(|w| w.px), wallpaper(&lake).map(|w| w.px));
        }
    }

    #[test]
    fn a_picture_no_frame_wants_is_kept_a_while_then_dropped() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (scene, wants) = (scene(SIZE), shell_wants());
        let without_toggle: Vec<Want> = wants.iter().filter(|w| w.surface != Surface::Capsule).cloned().collect();
        baker.frame(&ctx, &scene, &wants, at(0));
        let counts = baker.counts();
        let mut ms = 0;
        let mut frames = |baker: &mut Baker, wants: &[Want], count: u64| {
            for _ in 0..count {
                ms += 16;
                baker.frame(&ctx, &scene, wants, at(ms));
            }
        };
        // Gone for fewer frames than the keep: wanted again, it is still there.
        frames(&mut baker, &without_toggle, PICTURE_KEEP_FRAMES - 1);
        frames(&mut baker, &wants, 1);
        assert_eq!(baker.counts(), counts);
        // Gone for longer: it was dropped and is made again.
        frames(&mut baker, &without_toggle, PICTURE_KEEP_FRAMES + 1);
        frames(&mut baker, &wants, 1);
        assert_eq!(baker.counts(), (counts.0 + 1, counts.1));
    }

    #[test]
    fn the_wallpaper_covers_the_window_in_pixels_and_is_opaque_to_its_edges() {
        let mut big = scene([200.0, 120.0]);
        big.pixels_per_point = 2.0;
        for blur in [true, false] {
            big.blur = blur;
            let Some(picture) = wallpaper(&big) else { panic!("no wallpaper") };
            assert_eq!((picture.w, picture.h), (400, 240));
            for pixel in &picture.px {
                assert!((pixel[3] - 1.0).abs() < 1e-3, "a pixel of alpha {} with blur {blur}", pixel[3]);
            }
        }
    }

    #[test]
    fn the_veil_keeps_its_top_opacity_to_the_knee_then_rises_to_the_bottom() {
        let (w, h) = (4, 100);
        let mut picture = Bitmap::new(w, h);
        picture.fill([0.0, 0.0, 0.0, 1.0]);
        lay_veil(&mut picture, Color32::WHITE, [0.2, 0.8]);
        let row = |y: usize| picture.px[y * w];
        assert!((row(0)[0] - 0.2).abs() < 1e-3, "{:?}", row(0));
        assert!((row(40)[0] - 0.2).abs() < 1e-3, "{:?}", row(40));
        assert!((row(99)[0] - 0.8).abs() < 0.01, "{:?}", row(99));
        assert!(row(60)[0] > 0.2 && row(60)[0] < row(80)[0] && row(80)[0] < row(99)[0]);
        assert!(picture.px.iter().all(|p| (p[3] - 1.0).abs() < 1e-6));
    }

    /// Milliseconds since `start`.
    fn ms(start: Instant) -> f64 {
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// Times each step of baking the shell at the design's size, at one and two pixels per point.
    fn measure(label: &str) {
        let wants = shell_wants_at(WINDOW);
        for pixels_per_point in [1.0, 2.0] {
            let mut scene = scene(WINDOW);
            scene.pixels_per_point = pixels_per_point;
            let (w, h) = scene.pixels().unwrap();
            let start = Instant::now();
            let photo = decode(&scene.backdrop).unwrap();
            let decoding = ms(start);
            let start = Instant::now();
            let covered = cover(&photo, w, h).unwrap();
            let covering = ms(start);
            let start = Instant::now();
            let mut picture = pane::to_display(&pane::prepare(&covered, BACKDROP_BLUR, pixels_per_point));
            let blurring = ms(start);
            let start = Instant::now();
            lay_veil(&mut picture, theme::palette(scene.mode).veil, theme::veil(&scene.backdrop, scene.mode));
            let veiling = ms(start);
            let start = Instant::now();
            let whole = wallpaper(&scene).unwrap();
            let wallpaper_ms = ms(start);
            let panes: Vec<Pane> = wants.iter().map(|want| pane_of(&scene, want).unwrap()).collect();
            let mut prepared = HashMap::new();
            let start = Instant::now();
            for pane in &panes {
                behind(&mut prepared, &whole, &pane.params, pixels_per_point);
            }
            let preparing = ms(start);
            let start = Instant::now();
            let pictures: Vec<Bitmap> = panes.iter().map(|pane| draw_pane(&mut prepared, &whole, pixels_per_point, pane).unwrap()).collect();
            let drawing = ms(start);
            let ctx = egui::Context::default();
            let start = Instant::now();
            let _textures: Vec<TextureHandle> = std::iter::once(&whole)
                .chain(&pictures)
                .map(|picture| ctx.load_texture("measured", picture.image(), TextureOptions::LINEAR))
                .collect();
            let uploading = ms(start);
            let start = Instant::now();
            Baker::new().frame(&egui::Context::default(), &scene, &wants, at(0));
            let first_frame = ms(start);
            println!(
                "{label} at {pixels_per_point}x ({w} x {h} px): wallpaper {wallpaper_ms:.0} ms (decode {decoding:.0}, cover {covering:.0}, \
                 blur {blurring:.0}, veil {veiling:.0}); panes {:.0} ms (prepare {} blurs {preparing:.0}, draw {drawing:.0}); \
                 textures {uploading:.0} ms; a whole first frame {first_frame:.0} ms",
                preparing + drawing,
                prepared.len(),
            );
        }
    }

    #[test]
    #[ignore = "a measurement: cargo test -p devassist-gui --release -- --ignored --nocapture measure_a_glass_bake"]
    fn measure_a_glass_bake() {
        // The test shell at the design's size is the design's own layout.
        let rects: Vec<Rect> = shell_wants_at(WINDOW).iter().map(|want| want.rect).collect();
        let design = [(12.0, 12.0, 256.0, 876.0), (280.0, 12.0, 36.0, 36.0), (280.0, 60.0, 1148.0, 784.0), (280.0, 856.0, 1148.0, 32.0)];
        assert_eq!(rects, design.map(|(x, y, w, h)| Rect::from_min_size(pos2(x, y), vec2(w, h))));
        // Frosted first: once a device is installed it stays for the rest of the run.
        measure("frosted");
        if devassist_glass::gpu::install_own_device() {
            measure("glass");
        } else {
            println!("no GPU adapter: glass not measured");
        }
    }

    #[test]
    fn a_pane_picture_is_its_rectangle_in_pixels() {
        let mut scene = scene([200.0, 120.0]);
        scene.pixels_per_point = 2.0;
        let Some(wallpaper) = wallpaper(&scene) else { panic!("no wallpaper") };
        let want = Want { id: "panel", rect: Rect::from_min_size(pos2(10.0, 20.0), vec2(60.0, 40.0)), radius: 8.0, surface: Surface::Panel };
        let Some(picture) = pane_picture(&wallpaper, &scene, &want) else { panic!("no picture") };
        assert_eq!((picture.w, picture.h), (120, 80));
        assert!(picture.px[40 * 120 + 60][3] > 0.95, "the middle is solid");
        assert!(picture.px[0][3] < 0.05, "the corner is clear");
        let flat = Want { rect: Rect::from_min_size(pos2(10.0, 20.0), vec2(60.0, 0.0)), ..want };
        assert!(pane_picture(&wallpaper, &scene, &flat).is_none());
    }
}
