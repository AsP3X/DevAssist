//! The window's glass, drawn once into pictures and kept until something it depends on changes.
//!
//! Drawing a glass pane is slow (a blur of the whole window's backdrop and a run of the glass shader), so the
//! window never draws one at every frame. The [`Baker`] draws the wallpaper (the backdrop photo under its veil)
//! and each glass pane into a picture, hands egui one texture per picture, and draws a picture again only when
//! what it shows changed: the pane's place, size, radius or surface, or the window's size, scale, mode, backdrop
//! or blur.
//!
//! Frosted panes have no picture of their own. With each wallpaper the baker makes one [`Sheet`] per frosted
//! surface: the whole window blurred and tinted as the middle of such a pane is. A frosted pane is a rounded
//! rectangle showing the part of the sheet under it ([`crate::frost`]), so it moves and scrolls without a bake.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use devassist_glass::material::Params;
use devassist_glass::pane::{self, Behind, Pane, RIM_BLUR, sigma};
use devassist_glass::raster::{self, Bitmap};
use egui::{Color32, Context, Pos2, Rect, Stroke, TextureHandle, TextureId, TextureOptions};
use image::RgbaImage;
use image::imageops::{self, FilterType};

use crate::assets;
use crate::theme::{self, BACKDROP_BLUR, COARSE_LEAST_PIXELS, COARSE_SIGMA_PX, Mode, PICTURE_KEEP_FRAMES, RESIZE_SETTLE, Surface};

/// The backdrop drawn for a key that names none.
const DEFAULT_BACKDROP: &str = "lake";
/// How far below a whole number a length of pixels may fall from rounding and still count as that number.
const PIXEL_SLACK: f32 = 1e-3;
/// The most pixels a picture may have along one side.
const MOST_PIXELS: f32 = 100_000.0;
/// The id of the wallpaper among what [`Baker::frame`] returns.
pub const WALLPAPER: &str = "wallpaper";

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

/// One glass pane the shell wants drawn this frame. [`Baker::frame`] bakes glass surfaces only; a frosted one is
/// drawn from its [`Sheet`] instead, and a want of one is left out (said once in the log).
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
    /// The id of the pane it is ([`Want::id`]), or [`WALLPAPER`]: a dialog's or a menu's pane is drawn on the
    /// overlay, over its scrim, not under the screen ([`crate::overlay::glass`]).
    pub id: &'static str,
    /// The picture's texture.
    pub texture: TextureId,
    /// Where to draw it, in points.
    pub rect: Rect,
    /// The part of the texture that belongs at `rect`: a picture is whole pixels, so it reaches a little past
    /// what it shows.
    pub uv: Rect,
}

/// A blurred, tinted picture of the whole window that frosted panes show a part of.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sheet {
    /// The picture's texture.
    pub texture: TextureId,
    /// The part of the texture that covers the window: the picture is whole pixels, so it reaches a little past
    /// the window's right and bottom.
    pub uv: Rect,
    /// The light along a frosted pane's edge, to stroke its outline with.
    pub edge: Stroke,
}

/// A [`Sheet`] as a picture, before egui has it.
pub struct SheetPicture {
    /// The picture in display colours, with the window's top left at 0,0.
    pub bitmap: Bitmap,
    /// Its pixels per point: as coarse as the material's blur allows.
    pub scale: f32,
    /// The part of it that covers the window.
    pub uv: Rect,
    /// The light along a frosted pane's edge.
    pub edge: Stroke,
}

/// The part of a picture of `w` by `h` pixels that shows `length` pixels from its top left.
fn uv_of(w: usize, h: usize, length_x: f32, length_y: f32) -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new((length_x / length(w)).min(1.0), (length_y / length(h)).min(1.0)))
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

/// `scale` (pixels per point) lowered as far as a picture of `size` points needs to have at most `limit` pixels
/// on each side: a texture larger than the device takes cannot be drawn.
fn fitting_scale(scale: f32, size: [f32; 2], limit: usize) -> f32 {
    let longest = size[0].max(size[1]);
    if !(longest.is_finite() && longest > 0.0) {
        return scale;
    }
    // Half a pixel short of the limit, so that rounding the picture's size up stays within it.
    scale.min((length(limit) - 0.5) / longest)
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

/// Lays the veil over `picture` (display colours): `colour` at `strengths[0]` from the bottom up to
/// [`VEIL_KNEE`](theme::VEIL_KNEE) of the height, rising evenly from there to `strengths[1]` at the very top
/// ([`theme::veil_at`]).
fn lay_veil(picture: &mut Bitmap, colour: Color32, strengths: [f32; 2]) {
    if picture.w == 0 || picture.h == 0 {
        return;
    }
    let veil = raster::pixel(colour);
    let height = length(picture.h);
    for (row, line) in picture.px.chunks_mut(picture.w).enumerate() {
        let strength = theme::veil_at(strengths, (length(row) + 0.5) / height);
        if strength <= 0.0 {
            continue;
        }
        for pixel in line {
            *pixel = [0, 1, 2, 3].map(|i| veil[i] * strength + pixel[i] * (1.0 - strength));
        }
    }
}

/// The pixels per point a picture blurred by a Gaussian of `sigma` points needs to show all it holds: at most
/// `finest`, and at least enough for [`COARSE_LEAST_PIXELS`] along the window's shorter side. 0 when `finest`
/// is no usable scale; `finest` when `sigma` is no usable blur.
fn coarse_scale(finest: f32, sigma: f32, size: [f32; 2]) -> f32 {
    if !(finest.is_finite() && finest > 0.0) {
        return 0.0;
    }
    if !(sigma.is_finite() && sigma > 0.0) {
        return finest;
    }
    let short = size[0].min(size[1]);
    let least = if short.is_finite() && short > 0.0 { (COARSE_LEAST_PIXELS / short).min(finest) } else { 0.0 };
    (COARSE_SIGMA_PX / sigma).clamp(least, finest)
}

/// For each of `count_out` pixels along a line of `count_in` pixels shrunk by `factor`: the pixels it averages
/// and the share each has in it.
fn spans(count_in: usize, count_out: usize, factor: f32) -> Vec<Vec<(usize, f32)>> {
    let end = length(count_in);
    (0..count_out)
        .map(|out| {
            let (from, to) = (length(out) / factor, (length(out + 1) / factor).min(end));
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "within 0..count_in, as from and to are")]
            let (first, last) = (from.floor() as usize, (to.ceil() as usize).min(count_in));
            let parts: Vec<(usize, f32)> = (first..last).map(|at| (at, (length(at + 1).min(to) - length(at).max(from)).max(0.0))).collect();
            let total: f32 = parts.iter().map(|(_, part)| part).sum();
            parts.into_iter().map(|(at, part)| (at, if total > 0.0 { part / total } else { 0.0 })).collect()
        })
        .collect()
}

/// `bitmap` shrunk by `factor` (below 1) with each pixel the average of the area it covers, so that its pixels
/// per point are exactly `factor` times as many; the last pixel of a line, reaching past the picture, averages
/// the part it covers.
fn shrink(bitmap: &Bitmap, factor: f32) -> Option<Bitmap> {
    let (w, h) = (whole_pixels(length(bitmap.w) * factor)?, whole_pixels(length(bitmap.h) * factor)?);
    if bitmap.px.len() != bitmap.w * bitmap.h {
        return None;
    }
    let (across, down) = (spans(bitmap.w, w, factor), spans(bitmap.h, h, factor));
    let mut narrow = Bitmap::new(w, bitmap.h);
    for (row, line) in narrow.px.chunks_mut(w).enumerate() {
        let source = &bitmap.px[row * bitmap.w..(row + 1) * bitmap.w];
        for (pixel, span) in line.iter_mut().zip(&across) {
            *pixel = span.iter().fold([0.0; 4], |sum, &(at, share)| [0, 1, 2, 3].map(|c| sum[c] + source[at][c] * share));
        }
    }
    let mut out = Bitmap::new(w, h);
    for (row, span) in down.iter().enumerate() {
        for &(at, share) in span {
            let (to, from) = (&mut out.px[row * w..(row + 1) * w], &narrow.px[at * w..(at + 1) * w]);
            for (pixel, source) in to.iter_mut().zip(from) {
                *pixel = [0, 1, 2, 3].map(|c| pixel[c] + source[c] * share);
            }
        }
    }
    Some(out)
}

/// The window's backdrop in display colours, `bitmap` being `scale` pixels per point with the window's top
/// left at 0,0.
pub struct Wallpaper {
    /// The picture, `size × scale` pixels rounded up (it reaches a little past the window's right and bottom).
    pub bitmap: Bitmap,
    /// Its pixels per point: `pixels_per_point` when the photo is not blurred, else as coarse as the blur
    /// allows ([`wallpaper_scale`]).
    pub scale: f32,
}

/// The pixels per point of the wallpaper of `scene`; 0 for a scene without a usable `pixels_per_point`.
#[must_use]
pub fn wallpaper_scale(scene: &Scene) -> f32 {
    let finest = scene.pixels_per_point;
    if !(finest.is_finite() && finest > 0.0) {
        return 0.0;
    }
    if scene.blur { coarse_scale(finest, sigma(BACKDROP_BLUR), scene.size) } else { finest }
}

/// The wallpaper of `scene` from its decoded `photo`, at most `limit` pixels a side.
fn wallpaper_from(photo: &RgbaImage, scene: &Scene, limit: usize) -> Option<Wallpaper> {
    scene.pixels()?;
    let scale = fitting_scale(wallpaper_scale(scene), scene.size, limit);
    let covered = cover(photo, whole_pixels(scene.size[0] * scale)?, whole_pixels(scene.size[1] * scale)?)?;
    // `prepare` grows the picture by its own mirrored edge before it blurs, so the window's edges don't fade.
    let mut bitmap = if scene.blur { pane::to_display(&pane::prepare(&covered, BACKDROP_BLUR, scale)) } else { covered };
    lay_veil(&mut bitmap, theme::palette(scene.mode).veil, theme::veil(&scene.backdrop, scene.mode));
    Some(Wallpaper { bitmap, scale })
}

/// The whole window's backdrop in display colours: the photo scaled to cover the window, blurred when
/// `scene.blur`, and the veil laid over it. Unblurred it is `size × pixels_per_point` pixels; blurred it holds
/// no fine detail and is made at [`wallpaper_scale`], far fewer pixels, to be drawn stretched over the window.
/// `None` for a window without area.
#[must_use]
pub fn wallpaper(scene: &Scene) -> Option<Wallpaper> {
    scene.pixels()?;
    wallpaper_from(&decode(&scene.backdrop)?, scene, usize::MAX)
}

/// The wallpaper made ready for panes of one blur, each copy as coarse as what it shows allows: blurred in linear
/// light for their body, and, once a glass pane needs it, only lightly for its rim.
struct Prepared {
    soft: Bitmap,
    scale: f32,
    rim: Option<(Bitmap, f32)>,
}

/// `wallpaper` (display colours) blurred by `radius` points in linear light, at the scale the blur and what the
/// wallpaper already holds allow; with that scale.
fn prepare_coarse(wallpaper: &Wallpaper, scene: &Scene, radius: f32) -> (Bitmap, f32) {
    // What the wallpaper's own blur left, blurred again: the two blurs add up as squares.
    let already = if scene.blur { sigma(BACKDROP_BLUR) } else { 0.0 };
    let scale = coarse_scale(wallpaper.scale, already.hypot(sigma(radius)), scene.size);
    let factor = scale / wallpaper.scale;
    if factor < 1.0
        && let Some(small) = shrink(&wallpaper.bitmap, factor)
    {
        (pane::prepare(&small, radius, scale), scale)
    } else {
        (pane::prepare(&wallpaper.bitmap, radius, wallpaper.scale), wallpaper.scale)
    }
}

/// The wallpaper as panes drawn with `params` at `picture_scale` pixels per point see it, from the ones
/// `prepared` already, or prepared now and kept.
fn behind<'a>(prepared: &'a mut HashMap<u32, Prepared>, wallpaper: &Wallpaper, scene: &Scene, params: &Params, picture_scale: f32) -> Behind<'a> {
    let kept = prepared.entry(params.blur.to_bits()).or_insert_with(|| {
        let (soft, scale) = prepare_coarse(wallpaper, scene, params.blur);
        Prepared { soft, scale, rim: None }
    });
    if params.glass && kept.rim.is_none() {
        kept.rim = Some(prepare_coarse(wallpaper, scene, params.blur * RIM_BLUR));
    }
    // A frosted pane shows no rim; it is lent the body's picture, which it doesn't read.
    let (rim, rim_scale) = kept.rim.as_ref().map_or((&kept.soft, kept.scale), |(rim, scale)| (rim, *scale));
    Behind { soft: &kept.soft, rim, scale: kept.scale, rim_scale, picture_scale }
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

/// One pane drawn over `wallpaper` at `picture_scale` pixels per point, with the prepared copies of it kept in
/// `prepared`.
fn draw_pane(prepared: &mut HashMap<u32, Prepared>, wallpaper: &Wallpaper, scene: &Scene, pane: &Pane, picture_scale: f32) -> Option<Bitmap> {
    pane::draw(&behind(prepared, wallpaper, scene, &pane.params, picture_scale), pane)
}

/// The picture of the pane `want` over `wallpaper` (from [`wallpaper`] for the same `scene`), in display
/// colours, `want.rect × pixels_per_point` pixels rounded up. `None` for a pane or a window without area.
#[must_use]
pub fn pane_picture(wallpaper: &Wallpaper, scene: &Scene, want: &Want) -> Option<Bitmap> {
    scene.pixels()?;
    draw_pane(&mut HashMap::new(), wallpaper, scene, &pane_of(scene, want)?, scene.pixels_per_point)
}

/// The picture of the pane `want` as [`pane_picture`] makes it, but always frosted on the CPU, as a window draws
/// it when the glass shader cannot run, whether or not there is a GPU device now.
#[must_use]
pub fn frosted_pane_picture(wallpaper: &Wallpaper, scene: &Scene, want: &Want) -> Option<Bitmap> {
    scene.pixels()?;
    let pane = pane_of(scene, want)?;
    pane::frosted(&behind(&mut HashMap::new(), wallpaper, scene, &pane.params, scene.pixels_per_point), &pane)
}

/// The sheet of the frosted `surface` over `wallpaper` (from [`wallpaper`] for the same `scene`), with the copies
/// of the wallpaper prepared for panes kept in `prepared`; `None` for a glass surface.
fn make_sheet(prepared: &mut HashMap<u32, Prepared>, wallpaper: &Wallpaper, scene: &Scene, surface: Surface) -> Option<SheetPicture> {
    if surface.is_glass() {
        return None;
    }
    // A frosted material's numbers do not depend on the pane's size; the window's is as good as any.
    let params = theme::params(surface, scene.mode, (scene.size[0], scene.size[1]));
    let behind = behind(prepared, wallpaper, scene, &params, scene.pixels_per_point);
    let bitmap = pane::frost_sheet(behind.soft, &params);
    let uv = uv_of(bitmap.w, bitmap.h, scene.size[0] * behind.scale, scene.size[1] * behind.scale);
    let (colour, width) = pane::frost_edge(&params);
    let edge = Stroke::new(width, Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a));
    Some(SheetPicture { scale: behind.scale, bitmap, uv, edge })
}

/// The sheet of the frosted `surface` over `wallpaper` (from [`wallpaper`] for the same `scene`), as [`Baker`]
/// makes it. `None` for a glass surface or a window without area.
#[must_use]
pub fn sheet_picture(wallpaper: &Wallpaper, scene: &Scene, surface: Surface) -> Option<SheetPicture> {
    scene.pixels()?;
    make_sheet(&mut HashMap::new(), wallpaper, scene, surface)
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

/// A pane's picture, the part of it that shows the pane, and the last frame that wanted it.
struct Kept {
    texture: TextureHandle,
    uv: Rect,
    wanted: u64,
}

/// A frosted surface's sheet, as egui has it.
struct KeptSheet {
    surface: Surface,
    texture: TextureHandle,
    uv: Rect,
    edge: Stroke,
}

/// The wallpaper of the last settled scene, with its texture, the copies of it prepared for panes and the sheets
/// of the frosted surfaces. When the photo is blurred, all of it is coarse; nothing at the window's full
/// resolution is kept.
struct Settled {
    scene: Scene,
    wallpaper: Wallpaper,
    texture: TextureHandle,
    uv: Rect,
    prepared: HashMap<u32, Prepared>,
    sheets: Vec<KeptSheet>,
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
    /// Whether the log has been told that a frosted want was left out.
    told_frosted: bool,
}

impl Baker {
    /// A baker with no pictures yet.
    #[must_use]
    pub fn new() -> Baker {
        Baker::default()
    }

    /// The wallpaper and one picture per wanted glass pane, baking only what changed, and the sheets of the
    /// frosted surfaces with each new wallpaper ([`Baker::sheet`]). `now` is the frame's time. While the window's
    /// size keeps changing, nothing is baked: the pictures of the last settled size are returned, stretched to
    /// the new rectangles. Returns the wallpaper first. A want of a frosted surface is left out: it is drawn from
    /// its sheet.
    pub fn frame(&mut self, ctx: &Context, scene: &Scene, wants: &[Want], now: Duration) -> Vec<Drawn> {
        if scene.pixels().is_none() {
            return Vec::new();
        }
        if !self.told_frosted
            && let Some(want) = wants.iter().find(|want| !want.surface.is_glass())
        {
            self.told_frosted = true;
            log::warn!("the pane {} is frosted, which is drawn from its sheet, not baked; it is left out", want.id);
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

    /// The sheet for a frosted surface in the last scene `frame` saw; `None` before the first frame, and for a
    /// glass surface. While the window is resized it is the last settled size's, to be stretched over the window
    /// as the wallpaper is.
    #[must_use]
    pub fn sheet(&self, surface: Surface) -> Option<Sheet> {
        let kept = self.settled.as_ref()?.sheets.iter().find(|kept| kept.surface == surface)?;
        Some(Sheet { texture: kept.texture.id(), uv: kept.uv, edge: kept.edge })
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
    /// there is none of; drops the pictures no frame wanted for [`PICTURE_KEEP_FRAMES`]. A picture that would be
    /// larger than the device's textures is made at fewer pixels per point, and drawn stretched.
    fn bake(&mut self, ctx: &Context, scene: &Scene, wants: &[Want]) {
        let start = Instant::now();
        let limit = ctx.input(|i| i.max_texture_side);
        let fresh = self.settled.as_ref().is_none_or(|settled| settled.scene != *scene);
        if fresh {
            let key = known_backdrop(&scene.backdrop);
            let photo = self.photos.entry(key.to_owned()).or_insert_with(|| decode(key));
            let Some(wallpaper) = photo.as_ref().and_then(|photo| wallpaper_from(photo, scene, limit)) else { return };
            let texture = ctx.load_texture("glass-wallpaper", wallpaper.bitmap.image(), TextureOptions::LINEAR);
            let uv = uv_of(wallpaper.bitmap.w, wallpaper.bitmap.h, scene.size[0] * wallpaper.scale, scene.size[1] * wallpaper.scale);
            let mut prepared = HashMap::new();
            let sheets = Surface::ALL
                .into_iter()
                .filter_map(|surface| {
                    let sheet = make_sheet(&mut prepared, &wallpaper, scene, surface)?;
                    let texture = ctx.load_texture(format!("frost-{surface:?}"), sheet.bitmap.image(), TextureOptions::LINEAR);
                    Some(KeptSheet { surface, texture, uv: sheet.uv, edge: sheet.edge })
                })
                .collect();
            self.settled = Some(Settled { scene: scene.clone(), wallpaper, texture, uv, prepared, sheets });
            self.panes.clear();
            self.by_id.clear();
            self.counts.1 += 1;
        }
        let Some(settled) = self.settled.as_mut() else { return };
        let scale = scene.pixels_per_point;
        let mut made = 0;
        for want in wants.iter().filter(|want| want.surface.is_glass()) {
            let Some(pane) = pane_of(scene, want) else {
                self.by_id.remove(want.id);
                continue;
            };
            let key = PaneKey::of(want, scale);
            if let Some(kept) = self.panes.get_mut(&key) {
                kept.wanted = self.frame;
            } else if let picture_scale = fitting_scale(scale, [pane.rect[2], pane.rect[3]], limit)
                && let Some(picture) = draw_pane(&mut settled.prepared, &settled.wallpaper, scene, &pane, picture_scale)
            {
                let texture = ctx.load_texture(format!("glass-{}", want.id), picture.image(), TextureOptions::LINEAR);
                let uv = uv_of(picture.w, picture.h, pane.rect[2] * picture_scale, pane.rect[3] * picture_scale);
                self.panes.insert(key, Kept { texture, uv, wanted: self.frame });
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

    /// The settled wallpaper over the whole window, then each wanted glass pane's latest picture at its rectangle.
    fn drawn(&self, scene: &Scene, wants: &[Want]) -> Vec<Drawn> {
        let Some(settled) = &self.settled else { return Vec::new() };
        let window = Drawn { id: WALLPAPER, texture: settled.texture.id(), rect: Rect::from_min_size(Pos2::ZERO, scene.size.into()), uv: settled.uv };
        let panes = wants.iter().filter(|want| want.surface.is_glass()).filter_map(|want| {
            let kept = self.by_id.get(want.id).and_then(|key| self.panes.get(key))?;
            Some(Drawn { id: want.id, texture: kept.texture.id(), rect: want.rect, uv: kept.uv })
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

    /// The shell's three glass panes in a window of `size`, laid out as the design lays them out at 1440 by 900
    /// (where the sidebar is 12, 12, 256 × 876), each kept inside the window with at least a point a side. The
    /// frosted panel is no pane the baker makes.
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
    fn a_dialog_and_a_menu_are_baked_like_any_pane_and_named_in_what_is_drawn() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let mut wants = shell_wants();
        wants.push(crate::overlay::want_dialog(Rect::from_min_size(pos2(160.0, 150.0), vec2(480.0, 300.0))));
        wants.push(crate::overlay::want_menu(Rect::from_min_size(pos2(500.0, 60.0), vec2(256.0, 200.0))));
        let drawn = baker.frame(&ctx, &scene(SIZE), &wants, at(0));
        assert_eq!(baker.counts(), (wants.len(), 1), "every pane, the dialog's and the menu's among them, has its picture");
        let ids: Vec<&str> = drawn.iter().map(|drawn| drawn.id).collect();
        assert_eq!(ids, ["wallpaper", "sidebar", "sidebar-toggle", "status-bar", "dialog", "menu"]);
        assert_eq!(drawn[4].rect, wants[3].rect);
        assert_eq!((wants[3].surface, wants[4].surface), (Surface::Dialog, Surface::Menu));
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
    fn a_frosted_pane_that_moves_bakes_nothing() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (scene, wants) = (scene(SIZE), shell_wants());
        assert!(baker.sheet(Surface::Panel).is_none(), "no sheet before the first frame");
        baker.frame(&ctx, &scene, &wants, at(0));
        let counts = baker.counts();
        let Some(sheet) = baker.sheet(Surface::Panel) else { panic!("no sheet for the panel") };
        let window = whole(SIZE);
        for ms in 1..=100 {
            baker.frame(&ctx, &scene, &wants, at(ms * 16));
            // The panel moves and grows at every frame; it is the same sheet, only another part of it.
            #[expect(clippy::cast_precision_loss, reason = "a frame count below 101")]
            let step = ms as f32;
            let panel = Rect::from_min_size(pos2(280.0 + step, 60.0 + step), vec2(300.0 + step, 300.0));
            assert_eq!(baker.sheet(Surface::Panel), Some(sheet));
            let uv = crate::frost::uv_under(window, panel, sheet.uv);
            assert!(uv.min.x > sheet.uv.min.x && uv.max.x < sheet.uv.max.x && uv.min.y > sheet.uv.min.y && uv.max.y < sheet.uv.max.y, "{uv:?}");
        }
        assert_eq!(baker.counts(), counts);
    }

    #[test]
    fn the_sheet_follows_the_theme_and_the_backdrop() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (mut scene, wants) = (scene(SIZE), shell_wants());
        baker.frame(&ctx, &scene, &wants, at(0));
        let light = baker.sheet(Surface::Panel).map(|sheet| sheet.texture);
        let picture = |scene: &Scene| sheet_picture(&wallpaper(scene).unwrap(), scene, Surface::Panel).map(|sheet| sheet.bitmap.px);
        let light_picture = picture(&scene);
        scene.mode = Mode::Dark;
        baker.frame(&ctx, &scene, &wants, at(16));
        let dark = baker.sheet(Surface::Panel).map(|sheet| sheet.texture);
        assert!(light.is_some() && dark.is_some());
        assert_ne!(dark, light);
        assert_ne!(picture(&scene), light_picture, "the dark sheet is another picture");
        scene.backdrop = "night".to_owned();
        baker.frame(&ctx, &scene, &wants, at(32));
        let night = baker.sheet(Surface::Panel).map(|sheet| sheet.texture);
        assert!(night.is_some());
        assert_ne!(night, dark);
        // Only frosted surfaces have a sheet.
        for surface in [Surface::Sidebar, Surface::Capsule, Surface::StatusBar] {
            assert!(baker.sheet(surface).is_none(), "{surface:?}");
        }
    }

    #[test]
    fn a_frosted_want_is_not_baked() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let (scene, mut wants) = (scene(SIZE), shell_wants());
        let glass = wants.len();
        wants.push(Want {
            id: "panel",
            rect: Rect::from_min_size(pos2(280.0, 60.0), vec2(500.0, 400.0)),
            radius: RADIUS_PANEL,
            surface: Surface::Panel,
        });
        let drawn = baker.frame(&ctx, &scene, &wants, at(0));
        assert_eq!(baker.counts(), (glass, 1), "only the glass panes are baked");
        assert_eq!(drawn.len(), glass + 1, "and only they are drawn, after the wallpaper");
        assert!(drawn.iter().all(|drawn| drawn.rect != wants[glass].rect));
        baker.frame(&ctx, &scene, &wants, at(16));
        assert_eq!(baker.counts(), (glass, 1));
    }

    #[test]
    fn a_sheet_covers_the_window_at_the_coarse_scale_of_its_blur() {
        let mut scene = scene(SIZE);
        scene.pixels_per_point = 2.0;
        for blur in [true, false] {
            scene.blur = blur;
            let Some(wallpaper) = wallpaper(&scene) else { panic!("no wallpaper") };
            let Some(sheet) = sheet_picture(&wallpaper, &scene, Surface::Panel) else { panic!("no sheet") };
            // As coarse as the frosted panel's own blur on top of the backdrop's allows; the backdrop's blur
            // switched off, the panel still blurs by its own.
            let already = if blur { sigma(BACKDROP_BLUR) } else { 0.0 };
            let own = sigma(theme::params(Surface::Panel, scene.mode, (SIZE[0], SIZE[1])).blur);
            assert!((sheet.scale - (COARSE_SIGMA_PX / already.hypot(own)).min(wallpaper.scale)).abs() < 1e-6, "{} with blur {blur}", sheet.scale);
            assert!(sheet.scale < 0.2, "{}", sheet.scale);
            let (w, h) = (sheet.bitmap.w, sheet.bitmap.h);
            assert_eq!((w, h), (whole_pixels(800.0 * sheet.scale).unwrap(), whole_pixels(600.0 * sheet.scale).unwrap()));
            // Its uv is the part that covers the window: the last pixel may be partial.
            assert!((sheet.uv.max.x - 800.0 * sheet.scale / length(w)).abs() < 1e-6, "{:?}", sheet.uv);
            assert!((sheet.uv.max.y - 600.0 * sheet.scale / length(h)).abs() < 1e-6, "{:?}", sheet.uv);
            assert_eq!(sheet.uv.min, Pos2::ZERO);
            assert!(sheet.uv.max.x > 1.0 - 1.0 / length(w) && sheet.uv.max.x <= 1.0);
            assert!(sheet.bitmap.px.iter().all(|p| (p[3] - 1.0).abs() < 1e-3), "opaque with blur {blur}");
            // The edge is the frosted material's.
            let (colour, width) = pane::frost_edge(&theme::params(Surface::Panel, scene.mode, (SIZE[0], SIZE[1])));
            assert_eq!(sheet.edge, egui::Stroke::new(width, Color32::from_rgba_unmultiplied(colour.r, colour.g, colour.b, colour.a)));
        }
        assert!(sheet_picture(&wallpaper(&scene).unwrap(), &scene, Surface::Sidebar).is_none(), "glass has no sheet");
    }

    #[test]
    fn a_resize_waits_until_the_size_is_still() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let first = baker.frame(&ctx, &scene(SIZE), &shell_wants(), at(0));
        let settled = baker.counts();
        assert!(!baker.waiting());
        // Each change comes before the last one has settled, though the first is long past the settle time:
        // the wait starts again at every change.
        let step = RESIZE_SETTLE * 2 / 3;
        let changes = [(810.0, step), (820.0, step * 2), (830.0, step * 3), (830.0, step * 3 + RESIZE_SETTLE.saturating_sub(at(1)))];
        assert!(changes[3].1.saturating_sub(changes[0].1) > RESIZE_SETTLE);
        for (width, time) in changes {
            let size = [width, SIZE[1]];
            let wants = shell_wants_at(size);
            let drawn = baker.frame(&ctx, &scene(size), &wants, time);
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
        let drawn = baker.frame(&ctx, &scene(size), &wants, step * 3 + RESIZE_SETTLE);
        assert_eq!(baker.counts(), (settled.0 + wants.len(), settled.1 + 1));
        assert!(!baker.waiting());
        assert_eq!(drawn[0].rect, whole(size));
        assert_ne!(drawn[0].texture, first[0].texture);
    }

    #[test]
    fn going_back_to_the_settled_size_while_waiting_bakes_nothing() {
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let first = baker.frame(&ctx, &scene(SIZE), &shell_wants(), at(0));
        let settled = baker.counts();
        baker.frame(&ctx, &scene([810.0, SIZE[1]]), &shell_wants_at([810.0, SIZE[1]]), at(16));
        assert!(baker.waiting());
        let back = baker.frame(&ctx, &scene(SIZE), &shell_wants(), at(32));
        assert!(!baker.waiting());
        assert_eq!(baker.counts(), settled);
        assert_eq!(back, first);
        // Nor later: the settled pictures are the ones for this size.
        baker.frame(&ctx, &scene(SIZE), &shell_wants(), at(32) + RESIZE_SETTLE * 2);
        assert_eq!(baker.counts(), settled);
        assert!(!baker.waiting());
    }

    #[test]
    fn a_scene_without_a_usable_scale_has_no_wallpaper_scale_and_no_panic() {
        for blur in [true, false] {
            for pixels_per_point in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, -1.0] {
                let mut odd = scene(SIZE);
                (odd.pixels_per_point, odd.blur) = (pixels_per_point, blur);
                let scale = wallpaper_scale(&odd);
                assert!(scale.to_bits() == 0.0_f32.to_bits() || scale.is_finite() && scale > 0.0, "{scale} at {pixels_per_point}, blur {blur}");
            }
        }
        // Nor for odd blurs or window sizes.
        for (finest, sigma, size) in [
            (1.0, f32::NAN, SIZE),
            (1.0, f32::INFINITY, SIZE),
            (1.0, -3.0, SIZE),
            (1.0, 12.0, [0.0, 0.0]),
            (1.0, 12.0, [f32::NAN, 600.0]),
            (1.0, 12.0, [-800.0, 600.0]),
            (1.0, 12.0, [f32::INFINITY, f32::INFINITY]),
            (f32::NAN, 12.0, SIZE),
        ] {
            let scale = coarse_scale(finest, sigma, size);
            assert!(
                scale.to_bits() == 0.0_f32.to_bits() || scale.is_finite() && scale > 0.0 && scale <= 1.0,
                "{scale} for {finest}, {sigma}, {size:?}"
            );
        }
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
        for (size, pixels_per_point) in [
            ([-5.0, 600.0], 1.0),
            ([800.0, f32::NAN], 1.0),
            ([f32::INFINITY, 600.0], 1.0),
            (SIZE, 0.0),
            (SIZE, -1.0),
            (SIZE, f32::NAN),
            (SIZE, f32::INFINITY),
        ] {
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
            assert_eq!(wallpaper(&small_moon).map(|w| w.bitmap.px), wallpaper(&lake).map(|w| w.bitmap.px));
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
            let Some(Wallpaper { bitmap: picture, scale }) = wallpaper(&big) else { panic!("no wallpaper") };
            // Unblurred, the window's own pixels; blurred, coarser (drawn stretched over the window).
            assert_eq!(scale, wallpaper_scale(&big));
            assert!(if blur { scale < 2.0 } else { (scale - 2.0).abs() < f32::EPSILON }, "{scale} with blur {blur}");
            assert_eq!((picture.w, picture.h), (whole_pixels(200.0 * scale).unwrap(), whole_pixels(120.0 * scale).unwrap()));
            for pixel in &picture.px {
                assert!((pixel[3] - 1.0).abs() < 1e-3, "a pixel of alpha {} with blur {blur}", pixel[3]);
            }
        }
    }

    #[test]
    fn the_veil_keeps_its_low_strength_up_from_the_bottom_to_the_knee_then_rises_to_the_top() {
        let (w, h) = (4, 100);
        let mut picture = Bitmap::new(w, h);
        picture.fill([0.0, 0.0, 0.0, 1.0]);
        lay_veil(&mut picture, Color32::WHITE, [0.2, 0.8]);
        let row = |y: usize| picture.px[y * w];
        assert!((row(99)[0] - 0.2).abs() < 1e-3, "{:?}", row(99));
        assert!((row(60)[0] - 0.2).abs() < 1e-3, "{:?}", row(60));
        assert!((row(0)[0] - 0.8).abs() < 0.01, "{:?}", row(0));
        assert!(row(40)[0] > 0.2 && row(40)[0] < row(20)[0] && row(20)[0] < row(0)[0]);
        assert!(picture.px.iter().all(|p| (p[3] - 1.0).abs() < 1e-6));
    }

    #[test]
    fn the_veil_is_as_strong_as_the_designs_at_each_height() {
        // Measured on the design's export for the lake in the dark mode at 1440 by 900: 0.79 at 6 % from the top,
        // 0.66 at 28 %, 0.51 from 56 % down.
        for (down, want) in [(0.06, 0.79), (0.28, 0.66), (0.93, 0.51)] {
            let strength = theme::veil_strength("lake", Mode::Dark, down);
            assert!((strength - want).abs() <= 0.03, "{strength} at {down} of the height, the design has {want}");
        }
        // The two ends are the two numbers: the strength at the top and the strength in the lower part.
        let [low, high] = theme::veil("lake", Mode::Dark);
        assert!((theme::veil_strength("lake", Mode::Dark, 0.0) - high).abs() < 1e-6);
        assert!((theme::veil_strength("lake", Mode::Dark, 1.0) - low).abs() < 1e-6);
        // The wallpaper itself is veiled that way: over a black picture the night-blue veil's blue channel is
        // the strength times the veil's own blue.
        let (w, h) = (2, 900);
        let mut picture = Bitmap::new(w, h);
        picture.fill([0.0, 0.0, 0.0, 1.0]);
        let colour = theme::palette(Mode::Dark).veil;
        lay_veil(&mut picture, colour, theme::veil("lake", Mode::Dark));
        let blue = raster::pixel(colour)[2];
        for (down, want) in [(0.06, 0.79), (0.28, 0.66), (0.93, 0.51)] {
            #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a row of a 900-pixel picture")]
            let row = (down * 900.0) as usize;
            let strength = picture.px[row * w][2] / blue;
            assert!((strength - want).abs() <= 0.03, "{strength} at row {row}, the design has {want}");
        }
    }

    #[test]
    fn shrinking_averages_the_area_each_pixel_covers() {
        // Left half white, right half black, ten pixels wide: halved, the middle pixel covers one of each.
        let mut picture = Bitmap::new(10, 4);
        for (at, pixel) in picture.px.iter_mut().enumerate() {
            *pixel = if at % 10 < 5 { [1.0; 4] } else { [0.0, 0.0, 0.0, 1.0] };
        }
        let Some(half) = shrink(&picture, 0.5) else { panic!("nothing shrunk") };
        assert_eq!((half.w, half.h), (5, 2));
        let reds: Vec<f32> = half.px[..5].iter().map(|p| p[0]).collect();
        for (red, want) in reds.iter().zip([1.0, 1.0, 0.5, 0.0, 0.0]) {
            assert!((red - want).abs() < 1e-5, "{reds:?}");
        }
        // A share that does not divide the picture: the last pixel averages the part it covers, still opaque.
        let Some(third) = shrink(&picture, 0.3) else { panic!("nothing shrunk") };
        assert_eq!((third.w, third.h), (3, 2));
        assert!(third.px.iter().all(|p| (p[3] - 1.0).abs() < 1e-5));
        assert!(third.px[2][0].abs() < 1e-5 && (third.px[0][0] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn the_panes_see_a_backdrop_only_as_fine_as_its_blur() {
        let mut scene = scene(SIZE);
        scene.pixels_per_point = 2.0;
        let Some(blurred) = wallpaper(&scene) else { panic!("no wallpaper") };
        assert!(blurred.scale < 1.0, "a blurred wallpaper is coarse: {}", blurred.scale);
        assert_eq!((blurred.bitmap.w, blurred.bitmap.h), (whole_pixels(800.0 * blurred.scale).unwrap(), 200));
        let mut prepared = HashMap::new();
        for want in shell_wants() {
            let pane = pane_of(&scene, &want).unwrap();
            let behind = behind(&mut prepared, &blurred, &scene, &pane.params, scene.pixels_per_point);
            assert_eq!(behind.picture_scale, 2.0, "the pane's picture stays sharp");
            assert!(behind.scale <= blurred.scale && behind.rim_scale <= blurred.scale, "{}", want.id);
            // The body copy is as fine as its blur on top of the wallpaper's shows, and no finer.
            let shows = sigma(BACKDROP_BLUR).hypot(sigma(pane.params.blur));
            assert!((behind.scale - (COARSE_SIGMA_PX / shows).min(blurred.scale)).abs() < 1e-6, "{}", want.id);
            // It covers the window, reaching at most two of its pixels past the edge.
            let points = |pixels: usize| length(pixels) / behind.scale;
            assert!(points(behind.soft.w) >= 800.0 - 1e-3 && points(behind.soft.w - 2) < 800.0, "{}: {}", want.id, behind.soft.w);
            assert!(points(behind.soft.h) >= 600.0 - 1e-3 && points(behind.soft.h - 2) < 600.0, "{}: {}", want.id, behind.soft.h);
        }
        // Unblurred, the wallpaper is the window's own pixels.
        scene.blur = false;
        let Some(sharp) = wallpaper(&scene) else { panic!("no wallpaper") };
        assert_eq!((sharp.scale, sharp.bitmap.w, sharp.bitmap.h), (2.0, 1600, 1200));
    }

    /// Milliseconds since `start`.
    fn ms(start: Instant) -> f64 {
        start.elapsed().as_secs_f64() * 1000.0
    }

    /// Times each step of baking the shell at the design's size, at one and two pixels per point, and counts the
    /// bytes kept after it.
    fn measure(label: &str) {
        let wants = shell_wants_at(WINDOW);
        for (pixels_per_point, blur) in [(1.0, true), (2.0, true), (1.0, false), (2.0, false)] {
            let mut scene = scene(WINDOW);
            (scene.pixels_per_point, scene.blur) = (pixels_per_point, blur);
            let scale = wallpaper_scale(&scene);
            let start = Instant::now();
            let photo = decode(&scene.backdrop).unwrap();
            let decoding = ms(start);
            let start = Instant::now();
            let covered = cover(&photo, whole_pixels(WINDOW[0] * scale).unwrap(), whole_pixels(WINDOW[1] * scale).unwrap()).unwrap();
            let covering = ms(start);
            let start = Instant::now();
            let mut picture = if blur { pane::to_display(&pane::prepare(&covered, BACKDROP_BLUR, scale)) } else { covered };
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
                behind(&mut prepared, &whole, &scene, &pane.params, scene.pixels_per_point);
            }
            let preparing = ms(start);
            let start = Instant::now();
            let pictures: Vec<Bitmap> =
                panes.iter().map(|pane| draw_pane(&mut prepared, &whole, &scene, pane, scene.pixels_per_point).unwrap()).collect();
            let drawing = ms(start);
            // The frosted panel is no pane: one sheet of the whole window, made with the wallpaper.
            let start = Instant::now();
            let sheet = make_sheet(&mut prepared, &whole, &scene, Surface::Panel).unwrap();
            let sheeting = ms(start);
            let ctx = egui::Context::default();
            let start = Instant::now();
            let _textures: Vec<TextureHandle> = std::iter::once(&whole.bitmap)
                .chain(&pictures)
                .chain(std::iter::once(&sheet.bitmap))
                .map(|picture| ctx.load_texture("measured", picture.image(), TextureOptions::LINEAR))
                .collect();
            let uploading = ms(start);
            let start = Instant::now();
            Baker::new().frame(&egui::Context::default(), &scene, &wants, at(0));
            let first_frame = ms(start);
            let bytes = |bitmap: &Bitmap| bitmap.px.len() * size_of::<[f32; 4]>();
            let rims = prepared.values().filter_map(|kept| kept.rim.as_ref());
            let kept_bytes = bytes(&whole.bitmap)
                + bytes(&sheet.bitmap)
                + prepared.values().map(|kept| bytes(&kept.soft)).sum::<usize>()
                + rims.map(|(rim, _)| bytes(rim)).sum::<usize>();
            let scales: Vec<String> =
                prepared.values().map(|kept| format!("{:.2}/{:.2}", kept.scale, kept.rim.as_ref().map_or(f32::NAN, |(_, scale)| *scale))).collect();
            #[expect(clippy::cast_precision_loss, reason = "a byte count printed in megabytes")]
            let megabytes = kept_bytes as f64 / 1e6;
            println!(
                "{label} at {pixels_per_point}x, blur {blur} (wallpaper {} x {} px at {scale:.2}): wallpaper {wallpaper_ms:.1} ms (decode {decoding:.1}, \
                 cover {covering:.1}, blur {blurring:.1}, veil {veiling:.1}); panes {:.1} ms (prepare {preparing:.1} at body/rim scales {scales:?}, \
                 draw {drawing:.1}); the panel's sheet {sheeting:.1} ms ({} x {} px at {:.2}); textures {uploading:.1} ms; a whole first frame \
                 {first_frame:.1} ms; kept {megabytes:.1} MB",
                whole.bitmap.w,
                whole.bitmap.h,
                preparing + drawing,
                sheet.bitmap.w,
                sheet.bitmap.h,
                sheet.scale,
            );
        }
    }

    #[test]
    #[ignore = "a measurement: cargo test -p devassist-gui --release -- --ignored --nocapture measure_a_glass_bake"]
    fn measure_a_glass_bake() {
        // The test shell at the design's size is the design's own layout.
        let rects: Vec<Rect> = shell_wants_at(WINDOW).iter().map(|want| want.rect).collect();
        let design = [(12.0, 12.0, 256.0, 876.0), (280.0, 12.0, 36.0, 36.0), (280.0, 856.0, 1148.0, 32.0)];
        assert_eq!(rects, design.map(|(x, y, w, h)| Rect::from_min_size(pos2(x, y), vec2(w, h))));
        // The frosted panel, drawn from the sheet, is the design's too.
        assert_eq!(crate::shell::layout(WINDOW.into(), true, false).panel, Rect::from_min_size(pos2(280.0, 60.0), vec2(1148.0, 784.0)));
        // Frosted first: once a device is installed it stays for the rest of the run.
        measure("frosted");
        if devassist_glass::gpu::install_own_device() {
            measure("glass");
        } else {
            println!("no GPU adapter: glass not measured");
        }
    }

    #[test]
    fn every_picture_fits_the_largest_texture_the_device_takes() {
        // The least egui takes: its font atlas needs 1024 pixels a side.
        const LIMIT: usize = 1024;
        let (ctx, mut baker) = (egui::Context::default(), Baker::new());
        let mut scene = scene(SIZE);
        // Unblurred at two pixels per point the wallpaper would be 1600 by 1200 pixels, the sidebar 512 by 1152.
        (scene.blur, scene.pixels_per_point) = (false, 2.0);
        let wants = shell_wants();
        let mut drawn = Vec::new();
        let input = egui::RawInput { max_texture_side: Some(LIMIT), ..Default::default() };
        let mut output = ctx.run_pass(input, |ctx| drawn = baker.frame(ctx, &scene, &wants, at(0)));
        // No renderer takes the textures here.
        output.textures_delta.clear();
        assert_eq!(drawn.len(), wants.len() + 1, "every picture is made");
        let Some(settled) = &baker.settled else { panic!("no wallpaper") };
        let fits = |[w, h]: [usize; 2]| w <= LIMIT && h <= LIMIT;
        assert!(fits(settled.texture.size()), "the wallpaper is {:?}", settled.texture.size());
        assert_eq!(baker.panes.len(), wants.len());
        for kept in baker.panes.values() {
            assert!(fits(kept.texture.size()), "a pane's picture is {:?}", kept.texture.size());
        }
        // Each picture still shows all of what it is drawn over, only at fewer pixels.
        for drawn in &drawn {
            assert!(drawn.uv.max.x > 0.98 && drawn.uv.max.y > 0.98 && drawn.uv.max.x <= 1.0 && drawn.uv.max.y <= 1.0, "{:?}", drawn.uv);
        }
        // A pane that fits keeps the window's own pixels: the sidebar toggle is 36 points a side.
        let toggle = baker.by_id.get("sidebar-toggle").and_then(|key| baker.panes.get(key)).map(|kept| kept.texture.size());
        assert_eq!(toggle, Some([72, 72]));
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
