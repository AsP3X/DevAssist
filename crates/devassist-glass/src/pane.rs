// Copied unchanged from Quill (pencraft) app/src/render/glass.rs at 6c6a0cc: `SHADER`, `BACKDROP`, `RIM`,
// `HAIRLINE`, `Shape` with `distance`, `CURVE_STEPS`, `Curve`, `to_linear`, `to_gamma`, `recode`, `linear`,
// `toward_light`, `frosted_pixel` and `program`; `RIM_BLUR` is made public. From app/src/render/bake.rs at
// 6c6a0cc: `REACH`, `sigma` (made public), `length` and `pixels_up`. `outward` is `Form::outward` for one shape that is not turned;
// `draw`'s glass part is `Builder::glass`, `frosted` is Quill's `frosted` with its `form` branch only.
// New code, not from Quill: `SHADER_FAILED`, `shader_allowed` and `shader_gave_up`, with which `draw` stops trying
// the shader after its first failure in a process.

//! One pane of glass or frost over a backdrop picture.
//!
//! The window's backdrop is prepared once ([`prepare`]): taken to linear light and blurred, once as the body
//! of a pane shows it and once, only lightly, for the rim of a glass pane. Each pane is then drawn as a picture
//! of its own rectangle ([`draw`]), through Quill's glass shader on the GPU, or frosted on the CPU.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use crate::color::Rgba;
use crate::gpu;
use crate::material::Params;
use crate::raster::{Bitmap, Pixel};
use crate::shader::{self, Program};

/// The glass shader's source.
const SHADER: &str = include_str!("glass.frag");
/// The shader's sampler for what is behind the layer.
const BACKDROP: &str = "u_backdrop";
/// The shader's sampler for the same, only lightly blurred, which the rim shows.
const RIM: &str = "u_rim";
/// The width of the bright line along a material's edge, in document units (the shader's `HAIRLINE`).
const HAIRLINE: f32 = 1.1;

/// One outline: a rounded box or an ellipse.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Shape {
    /// Its centre as an offset from the form's centre, in the form's own directions.
    offset: [f32; 2],
    /// Half its width and height.
    half: [f32; 2],
    /// Corner radii: top left, top right, bottom right, bottom left.
    radii: [f32; 4],
    /// Whether it is an ellipse.
    oval: bool,
}

impl Shape {
    /// The distance from `p` (an offset from the form's centre) to the outline; negative inside.
    // Agent: the same two formulas as `box_distance` and `oval_distance` in glass.frag; keep them alike.
    fn distance(&self, p: [f32; 2]) -> f32 {
        let p = [p[0] - self.offset[0], p[1] - self.offset[1]];
        let [hx, hy] = self.half;
        if self.oval {
            let k0 = (p[0] / hx).hypot(p[1] / hy);
            let k1 = (p[0] / (hx * hx)).hypot(p[1] / (hy * hy));
            return if k1 > 1e-6 { k0 * (k0 - 1.0) / k1 } else { -hx.min(hy) };
        }
        let radius = match (p[0] < 0.0, p[1] < 0.0) {
            (true, true) => self.radii[0],
            (false, true) => self.radii[1],
            (false, false) => self.radii[2],
            (true, false) => self.radii[3],
        };
        let q = [p[0].abs() - hx + radius, p[1].abs() - hy + radius];
        q[0].max(q[1]).min(0.0) + q[0].max(0.0).hypot(q[1].max(0.0)) - radius
    }
}

/// The way out of the pane at `p` (an offset from its centre), as a unit vector: the slope of the distance.
// Quill's `Form::outward` for one shape that is not turned.
fn outward(shape: &Shape, p: [f32; 2]) -> [f32; 2] {
    const STEP: f32 = 0.25;
    let slope = [
        shape.distance([p[0] + STEP, p[1]]) - shape.distance([p[0] - STEP, p[1]]),
        shape.distance([p[0], p[1] + STEP]) - shape.distance([p[0], p[1] - STEP]),
    ];
    let steep = slope[0].hypot(slope[1]);
    if steep < 1e-5 {
        return [0.0; 2];
    }
    [slope[0] / steep, slope[1] / steep]
}

/// How many steps the tables of the colour curves have between 0 and 1.
const CURVE_STEPS: usize = 4096;

/// A colour curve from 0 to 1, worked out once at [`CURVE_STEPS`] places and read between them.
// Human: every pixel of every material goes through both curves, and a power is slow to work out: the two
// were a sixth of the time a design with glass took to draw. Read from the table they are off by less than a
// ten-thousandth, a twentieth of the smallest step a colour on screen can take.
struct Curve([f32; CURVE_STEPS + 2]);

impl Curve {
    fn new(exact: fn(f32) -> f32) -> Curve {
        #[expect(clippy::cast_precision_loss, reason = "at most 4097, far below f32's exact range")]
        Curve(std::array::from_fn(|step| exact((step.min(CURVE_STEPS) as f32) / CURVE_STEPS as f32)))
    }

    fn at(&self, c: f32) -> f32 {
        #[expect(clippy::cast_precision_loss, reason = "4096 is exact in f32")]
        let place = if c.is_nan() { 0.0 } else { c.clamp(0.0, 1.0) * CURVE_STEPS as f32 };
        #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "clamped to 0..=4096 just above")]
        let step = place as usize;
        #[expect(clippy::cast_precision_loss, reason = "at most 4096")]
        let part = place - step as f32;
        let (low, high) = (self.0[step.min(CURVE_STEPS)], self.0[(step + 1).min(CURVE_STEPS + 1)]);
        low + (high - low) * part
    }
}

/// A gamma-encoded channel in linear light.
fn to_linear(c: f32) -> f32 {
    static TABLE: std::sync::OnceLock<Curve> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| Curve::new(|c| if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) })).at(c)
}

/// A linear-light channel, gamma-encoded.
fn to_gamma(c: f32) -> f32 {
    static TABLE: std::sync::OnceLock<Curve> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| Curve::new(|c| if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 })).at(c)
}

/// A premultiplied pixel with its colour run through `curve`.
fn recode(pixel: Pixel, curve: fn(f32) -> f32) -> Pixel {
    let alpha = pixel[3];
    if alpha <= 1e-4 {
        return [0.0; 4];
    }
    [curve(pixel[0] / alpha) * alpha, curve(pixel[1] / alpha) * alpha, curve(pixel[2] / alpha) * alpha, alpha]
}

/// A colour in linear light, opaque.
fn linear(color: Rgba) -> [f32; 3] {
    [color.r, color.g, color.b].map(|c| to_linear(f32::from(c) / 255.0))
}

/// The unit vector toward a light `angle` degrees clockwise from straight above, in document space (y down).
fn toward_light(angle: f32) -> [f32; 2] {
    let (sin, cos) = angle.to_radians().sin_cos();
    [sin, -cos]
}

/// The compiled glass shader, or `None` when it doesn't compile (said once in the log).
fn program() -> Option<Arc<Program>> {
    static PROGRAM: OnceLock<Option<Arc<Program>>> = OnceLock::new();
    PROGRAM
        .get_or_init(|| match shader::compile(SHADER) {
            Ok(program) => Some(Arc::new(program)),
            Err(error) => {
                log::error!("the glass shader doesn't compile: {error}");
                None
            }
        })
        .clone()
}

/// Frost, tint and edge light for one pixel: `behind` in linear light, the edge's `cover` of the pixel, how far
/// `inside` the edge it is and which way is `outward` there (`None` for a shape without a worked-out edge).
// Agent: the CPU's side of glass.frag's frost and hairline; keep the two alike.
fn frosted_pixel(behind: Pixel, params: &Params, tint: [f32; 3], cover: f32, edge: Option<(f32, [f32; 2])>) -> Pixel {
    let grey = 0.2126 * behind[0] + 0.7152 * behind[1] + 0.0722 * behind[2];
    let alpha = behind[3];
    let mut rgb = [behind[0], behind[1], behind[2]].map(|c| (grey + (c - grey) * params.saturation).max(0.0));
    rgb = rgb.map(|c| (c + params.brightness * alpha).clamp(0.0, 1.0));
    let t = params.tint_opacity;
    let mut rgb = [0, 1, 2].map(|i| rgb[i] + (tint[i] - rgb[i]) * t);
    let mut alpha = alpha + (1.0 - alpha) * t;
    if let Some((inside, outward)) = edge {
        let along = (inside / HAIRLINE).clamp(0.0, 1.0);
        let hairline = 1.0 - along * along * (3.0 - 2.0 * along);
        let lit = if params.glass {
            let light = toward_light(params.light_angle);
            let facing = outward[0] * light[0] + outward[1] * light[1];
            0.16 + 0.84 * (facing.max(0.0).powi(2) + 0.45 * (-facing).max(0.0).powi(2))
        } else {
            1.0
        };
        let light = params.highlight * hairline * lit;
        rgb = rgb.map(|c| c + light * (1.0 - c));
        alpha += light * (1.0 - alpha);
    }
    if alpha <= 1e-4 {
        return [0.0; 4];
    }
    let out = alpha * cover;
    [to_gamma(rgb[0] / alpha) * out, to_gamma(rgb[1] / alpha) * out, to_gamma(rgb[2] / alpha) * out, out]
}

/// The standard deviation of the Gaussian an effect's `blur` or `radius` asks for (half of it), in the
/// radius's own units: the blur [`prepare`] gives a picture for a `radius`.
// Copied from Quill's app/src/render/bake.rs (made public here).
#[must_use]
pub fn sigma(radius: f32) -> f32 {
    radius.max(0.0) / 2.0
}

/// A count of pixels as a length.
// Copied from Quill's app/src/render/bake.rs.
#[expect(clippy::cast_precision_loss, reason = "pictures are at most a few thousand pixels a side")]
fn length(pixels: usize) -> f32 {
    pixels as f32
}

/// A length as a count of whole pixels, rounded up; `None` for no length at all or one beyond any picture.
// Copied from Quill's app/src/render/bake.rs.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "checked to be within 1..=100000 before the cast")]
fn pixels_up(length: f32) -> Option<usize> {
    let rounded = (length - 1e-3).ceil();
    (rounded.is_finite() && (1.0..=100_000.0).contains(&rounded)).then_some(rounded as usize)
}

/// How far a blur reaches, in standard deviations: how far the backdrop is grown before it is blurred.
// Quill's `REACH` in app/src/render/bake.rs.
const REACH: f32 = 3.0;

/// Whether the glass shader has failed in this process. After its first failure it is not tried again: each try
/// can hold the calling thread for seconds, and a GPU that failed once is likely to fail again.
static SHADER_FAILED: AtomicBool = AtomicBool::new(false);

/// Whether the glass shader may be tried, `failed` being whether it has failed before.
fn shader_allowed(failed: &AtomicBool) -> bool {
    !failed.load(Ordering::Relaxed)
}

/// Notes in `failed` that the shader failed; true the first time, when the failure is to be said in the log.
fn shader_gave_up(failed: &AtomicBool) -> bool {
    !failed.swap(true, Ordering::Relaxed)
}

/// What lies behind every pane: the whole window's backdrop in linear light, blurred for the pane's body
/// (`soft`) and only lightly for its rim (`rim`), each with the window's top left at 0,0.
///
/// A blurred picture holds no detail finer than its blur, so each may be coarser than the pane's picture:
/// each has its own pixels per point, and the pane's picture is drawn at `picture_scale`.
pub struct Behind<'a> {
    /// The backdrop blurred as the pane's body shows it.
    pub soft: &'a Bitmap,
    /// The backdrop only lightly blurred, as the rim of a glass pane shows it.
    pub rim: &'a Bitmap,
    /// Pixels per point of `soft`.
    pub scale: f32,
    /// Pixels per point of `rim`.
    pub rim_scale: f32,
    /// Pixels per point of the pane's picture.
    pub picture_scale: f32,
}

/// One pane: its rectangle in points (x, y, width, height), its corner radius, and the numbers it is drawn with.
pub struct Pane {
    /// Left, top, width and height in points.
    pub rect: [f32; 4],
    /// The radius of its four corners in points.
    pub radius: f32,
    /// The material's numbers.
    pub params: Params,
}

/// The rim's blur as a share of the body's: Quill's `RIM_BLUR`.
pub const RIM_BLUR: f32 = 0.2;

impl Pane {
    /// Its picture's width and height in pixels at `scale`, or `None` when it has no area.
    fn pixels(&self, scale: f32) -> Option<(usize, usize)> {
        Some((pixels_up(self.rect[2] * scale)?, pixels_up(self.rect[3] * scale)?))
    }

    /// Its centre in points.
    fn centre(&self) -> [f32; 2] {
        [self.rect[0] + self.rect[2] / 2.0, self.rect[1] + self.rect[3] / 2.0]
    }

    /// Its outline around its centre. A corner can't be rounder than half the shorter side.
    fn shape(&self) -> Shape {
        let half = [self.rect[2] / 2.0, self.rect[3] / 2.0];
        let radius = self.radius.clamp(0.0, half[0].min(half[1]).max(0.0));
        Shape { offset: [0.0; 2], half, radii: [radius; 4], oval: false }
    }
}

/// The pane as a picture of `rect` at `picture_scale` pixels per point, premultiplied, in display (gamma)
/// colours: through the glass shader when `params.glass` and a GPU is there, else frosted on the CPU. Once the
/// shader has failed, every pane after it is frosted, for the rest of the process.
/// A pane with no area gives `None`.
#[must_use]
pub fn draw(behind: &Behind<'_>, pane: &Pane) -> Option<Bitmap> {
    let size = pane.pixels(behind.picture_scale)?;
    if pane.params.glass
        && shader_allowed(&SHADER_FAILED)
        && gpu::ready()
        && let Some(program) = program()
    {
        match glass(&program, behind, pane, size) {
            Ok(bitmap) => return Some(bitmap),
            Err(error) => {
                if shader_gave_up(&SHADER_FAILED) {
                    log::warn!("liquid glass: {error}; drawn frosted instead, from now on");
                }
            }
        }
    }
    frosted(behind, pane)
}

/// The frosted picture, always on the CPU.
#[must_use]
pub fn frosted(behind: &Behind<'_>, pane: &Pane) -> Option<Bitmap> {
    let (w, h) = pane.pixels(behind.picture_scale)?;
    let params = &pane.params;
    let tint = linear(params.tint);
    let shape = pane.shape();
    let centre = pane.centre();
    let (scale, soft_scale) = (behind.picture_scale, behind.scale);
    // The backdrop's edge pixel goes on past its centre, as the shader's mirrored sampler does, instead of fading
    // into the clear beyond it: half a pixel, which is several points of a coarse backdrop.
    let (most_x, most_y) = ((length(behind.soft.w) - 0.5).max(0.5), (length(behind.soft.h) - 0.5).max(0.5));
    let inside = |v: f32, most: f32| v.clamp(0.5, most);
    let mut out = Bitmap::new(w, h);
    for y in 0..out.h {
        for x in 0..out.w {
            let doc = [pane.rect[0] + (length(x) + 0.5) / scale, pane.rect[1] + (length(y) + 0.5) / scale];
            let p = [doc[0] - centre[0], doc[1] - centre[1]];
            let distance = shape.distance(p);
            let cover = (0.5 - distance * scale).clamp(0.0, 1.0);
            // Far from the edge there is no light to work out.
            let near = cover > 0.0 && -distance < HAIRLINE;
            let edge = Some(if near { ((-distance).max(0.0), outward(&shape, p)) } else { (HAIRLINE, [0.0; 2]) });
            if cover > 0.0
                && let Some(pixel) = out.px.get_mut(y * out.w + x)
            {
                *pixel = frosted_pixel(
                    behind.soft.sample(inside(doc[0] * soft_scale, most_x), inside(doc[1] * soft_scale, most_y)),
                    params,
                    tint,
                    cover,
                    edge,
                );
            }
        }
    }
    Some(out)
}

/// The backdrop for `Behind`: `picture` (display colours) taken to linear light and blurred by `radius` points.
#[must_use]
// The blur sees nothing beyond a picture's edge, which would thin the backdrop there; so the picture is first
// grown by the blur's reach on every side, filled with its own edge mirrored (as the GPU's sampler mirrors past
// it), blurred, and cut back to its size.
pub fn prepare(picture: &Bitmap, radius: f32, scale: f32) -> Bitmap {
    let (w, h) = (picture.w, picture.h);
    if w == 0 || h == 0 || picture.px.len() != w * h {
        return Bitmap::new(w, h);
    }
    let blur = sigma(radius) * scale;
    let pad = pixels_up(REACH * blur).unwrap_or(0);
    // Where a place `at` of the grown picture comes from in a row of `count` pixels: mirrored past either end, and
    // the nearest end where the picture has fewer pixels than the mirror needs.
    let mirrored = |at: usize, count: usize| match at.checked_sub(pad) {
        None => (pad - at - 1).min(count - 1),
        Some(inside) if inside < count => inside,
        Some(inside) => (2 * count).saturating_sub(inside + 1),
    };
    let (grown_w, grown_h) = (w + 2 * pad, h + 2 * pad);
    let mut grown = Bitmap::new(grown_w, grown_h);
    for (at, pixel) in grown.px.iter_mut().enumerate() {
        let (x, y) = (mirrored(at % grown_w, w), mirrored(at / grown_w, h));
        *pixel = recode(picture.px[y * w + x], to_linear);
    }
    grown.blur(blur);
    let mut bitmap = Bitmap::new(w, h);
    for (row, line) in bitmap.px.chunks_mut(w).enumerate() {
        let start = (row + pad) * grown_w + pad;
        line.copy_from_slice(&grown.px[start..start + w]);
    }
    bitmap
}

/// A premultiplied picture in linear light (as [`prepare`] returns it) in display (gamma) colours again.
#[must_use]
pub fn to_display(bitmap: &Bitmap) -> Bitmap {
    Bitmap { w: bitmap.w, h: bitmap.h, px: bitmap.px.iter().map(|pixel| recode(*pixel, to_gamma)).collect() }
}

/// The glass shader run over `behind`, as a picture of `size` pixels of the pane.
// Quill's `Builder::glass` for one rounded box that is not turned: the frame is the pane's centre in its own
// picture, and each map turns an offset from that centre in points into a place on its backdrop picture.
fn glass(program: &Program, behind: &Behind<'_>, pane: &Pane, size: (usize, usize)) -> Result<Bitmap, String> {
    let whole = |pixels: usize| u32::try_from(pixels).map_err(|e| e.to_string());
    let params = &pane.params;
    let scale = behind.picture_scale;
    let shape = pane.shape();
    let centre = pane.centre();
    // Each backdrop picture at its own pixels per point.
    let map = |bitmap: &Bitmap, bitmap_scale: f32| {
        let (w, h) = (length(bitmap.w) / bitmap_scale, length(bitmap.h) / bitmap_scale);
        [1.0 / w, 1.0 / h, centre[0] / w, centre[1] / h]
    };
    let tint = linear(params.tint);
    let light = toward_light(params.light_angle);
    let maps = [(shader::map_member(BACKDROP), map(behind.soft, behind.scale)), (shader::map_member(RIM), map(behind.rim, behind.rim_scale))];
    // A rim can't be wider than half the narrowest outline.
    let narrowest = shape.half[0].min(shape.half[1]);
    let frame = [pane.rect[2] / 2.0 * scale, pane.rect[3] / 2.0 * scale, scale, 0.0];
    let block = program.block(frame, &maps, |uniform| match uniform.name.as_str() {
        "u_box0" => [shape.offset[0], shape.offset[1], shape.half[0], shape.half[1]],
        "u_round0" => shape.radii,
        "u_count" => [1.0, 0.0, 0.0, 0.0],
        "u_merge" => [0.0, 0.0, 0.0, 0.0],
        "u_turn" => [1.0, 0.0, 0.0, 1.0],
        "u_scale" => [scale, 0.0, 0.0, 0.0],
        "u_bevel" => [params.bevel.min(narrowest), 0.0, 0.0, 0.0],
        "u_thickness" => [params.thickness, 0.0, 0.0, 0.0],
        "u_refraction" => [params.refraction, 0.0, 0.0, 0.0],
        "u_dispersion" => [params.dispersion, 0.0, 0.0, 0.0],
        "u_saturation" => [params.saturation, 0.0, 0.0, 0.0],
        "u_brightness" => [params.brightness, 0.0, 0.0, 0.0],
        "u_tint" => [tint[0], tint[1], tint[2], params.tint_opacity],
        "u_highlight" => [params.highlight, 0.0, 0.0, 0.0],
        "u_light" => [light[0], light[1], 0.0, 0.0],
        _ => [0.0; 4],
    });
    let encode = |bitmap: &Bitmap| Bitmap { w: bitmap.w, h: bitmap.h, px: bitmap.px.iter().map(|p| recode(*p, to_gamma)).collect() }.bytes();
    let (soft, sharp) = (encode(behind.soft), encode(behind.rim));
    let inputs = [
        gpu::Input { rgba: &soft, w: whole(behind.soft.w)?, h: whole(behind.soft.h)?, mirror: true },
        gpu::Input { rgba: &sharp, w: whole(behind.rim.w)?, h: whole(behind.rim.h)?, mirror: true },
    ];
    let bytes = gpu::run(program, (whole(size.0)?, whole(size.1)?), &block, &inputs)?;
    Bitmap::from_bytes(size.0, size.1, &bytes).ok_or_else(|| "the GPU returned a picture of the wrong size".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use serde_json::json;

    use super::*;
    use crate::material::{Lookup, Material};

    /// A backdrop and its two blurs, kept together so a test can lend them out as a [`Behind`].
    struct Backdrop {
        soft: Bitmap,
        rim: Bitmap,
        scale: f32,
    }

    impl Backdrop {
        fn as_behind(&self) -> Behind<'_> {
            Behind { soft: &self.soft, rim: &self.rim, scale: self.scale, rim_scale: self.scale, picture_scale: self.scale }
        }
    }

    /// A flat mid-grey window of `w` by `h` points at one pixel per point.
    fn grey_behind(w: usize, h: usize) -> Backdrop {
        let mut picture = Bitmap::new(w, h);
        picture.fill([0.5, 0.5, 0.5, 1.0]);
        Backdrop { soft: prepare(&picture, 8.0, 1.0), rim: prepare(&picture, 8.0 * RIM_BLUR, 1.0), scale: 1.0 }
    }

    #[test]
    fn a_prepared_opaque_picture_stays_opaque_up_to_its_corners() {
        let mut picture = Bitmap::new(40, 30);
        picture.fill([0.5, 0.5, 0.5, 1.0]);
        // A blur far wider than the picture, at two scales: the padding is mirrored as far as there are pixels.
        for (radius, scale) in [(24.0, 1.0), (60.0, 2.0)] {
            let prepared = prepare(&picture, radius, scale);
            assert_eq!((prepared.w, prepared.h), (40, 30));
            for (x, y) in [(0, 0), (39, 0), (0, 29), (39, 29), (20, 0), (20, 15)] {
                let pixel = prepared.px[y * prepared.w + x];
                assert!((pixel[3] - 1.0).abs() < 1e-3, "alpha {} at {x}, {y} after a blur of {radius}", pixel[3]);
                assert!((pixel[0] - to_linear(0.5)).abs() < 1e-3, "colour {} at {x}, {y}", pixel[0]);
            }
        }
    }

    #[test]
    fn a_prepared_picture_comes_back_to_display_colours() {
        let colours = [[0.5, 0.5, 0.5, 1.0], [0.9, 0.2, 0.05, 1.0], [0.0, 0.0, 0.0, 1.0], [0.2, 0.1, 0.05, 0.5], [0.0; 4]];
        let picture = Bitmap { w: colours.len(), h: 1, px: colours.to_vec() };
        // Without a blur `prepare` only takes the picture to linear light; `to_display` takes it back.
        let linear = prepare(&picture, 0.0, 1.0);
        assert!((linear.px[0][0] - to_linear(0.5)).abs() < 1e-4, "prepare is linear light");
        let back = to_display(&linear);
        assert_eq!((back.w, back.h), (picture.w, picture.h));
        for (was, is) in picture.px.iter().zip(&back.px) {
            for (a, b) in was.iter().zip(is) {
                assert!((a - b).abs() < 1e-3, "{was:?} came back as {is:?}");
            }
        }
    }

    #[test]
    fn a_picture_without_pixels_is_prepared_empty() {
        for (w, h) in [(0, 0), (0, 5), (5, 0)] {
            let prepared = prepare(&Bitmap::new(w, h), 24.0, 1.0);
            assert_eq!((prepared.w, prepared.h, prepared.px.len()), (w, h, 0));
        }
    }

    /// A pane of material `kind` (`frosted` or `liquid_glass`) in `appearance`, resolved with no variables.
    fn material_pane(rect: [f32; 4], radius: f32, kind: &str, appearance: &str) -> Pane {
        let material = Material::read(&json!({"type": kind, "appearance": appearance})).unwrap();
        let params = material.resolve((rect[2], rect[3]), false, &Lookup { number: &|_| None, color: &|_| None });
        Pane { rect, radius, params }
    }

    /// A light pane: liquid glass when `glass`, else frosted.
    fn pane(rect: [f32; 4], radius: f32, glass: bool) -> Pane {
        material_pane(rect, radius, if glass { "liquid_glass" } else { "frosted" }, "light")
    }

    fn alpha(picture: &Bitmap, x: usize, y: usize) -> f32 {
        picture.px[y * picture.w + x][3]
    }

    // Quill's test of the same name, with its eight outlines written out here.
    #[test]
    fn the_glass_shader_compiles_and_has_every_uniform_the_renderer_sets() {
        let program = shader::compile(SHADER).unwrap();
        let names: Vec<&str> = program.uniforms.iter().map(|u| u.name.as_str()).collect();
        let mut wanted: Vec<String> = [
            BACKDROP,
            RIM,
            "u_count",
            "u_merge",
            "u_turn",
            "u_scale",
            "u_bevel",
            "u_thickness",
            "u_refraction",
            "u_dispersion",
            "u_saturation",
            "u_brightness",
            "u_tint",
            "u_highlight",
            "u_light",
        ]
        .map(str::to_owned)
        .to_vec();
        for index in 0..8 {
            wanted.extend([format!("u_box{index}"), format!("u_round{index}")]);
        }
        for name in &wanted {
            assert!(names.contains(&name.as_str()), "{name} is missing from the shader");
        }
        assert_eq!(names.len(), wanted.len());
    }

    // Quill's test of the same name, unchanged.
    #[test]
    fn the_colour_curves_undo_each_other_and_keep_the_alpha() {
        for v in [0.0, 0.002, 0.2, 0.5, 1.0] {
            assert!((to_gamma(to_linear(v)) - v).abs() < 1e-4);
        }
        // Read from their tables the curves are where the formulas put them, also at the steep dark end.
        for step in 0..=1000u16 {
            let v = f32::from(step) / 1000.0;
            let (linear, gamma) = (if v <= 0.040_45 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }, 1.055 * v.powf(1.0 / 2.4) - 0.055);
            assert!((to_linear(v) - linear).abs() < 1e-4, "{v}");
            assert!(v <= 0.003_130_8 || (to_gamma(v) - gamma).abs() < 2e-4, "{v}");
        }
        let half = recode([0.25, 0.25, 0.25, 0.5], to_linear);
        assert!((half[3] - 0.5).abs() < 1e-6 && (half[0] - to_linear(0.5) * 0.5).abs() < 1e-6);
        assert_eq!(recode([0.0; 4], to_linear), [0.0; 4]);
    }

    #[test]
    fn a_pane_without_area_is_not_drawn() {
        let behind = grey_behind(64, 64);
        assert!(frosted(&behind.as_behind(), &pane([8.0, 8.0, 0.0, 20.0], 4.0, false)).is_none());
        assert!(draw(&behind.as_behind(), &pane([8.0, 8.0, 20.0, -1.0], 4.0, true)).is_none());
    }

    #[test]
    fn after_the_shaders_first_failure_it_is_not_tried_again_and_said_once() {
        let failed = AtomicBool::new(false);
        assert!(shader_allowed(&failed), "tried until it fails");
        assert!(shader_gave_up(&failed), "the first failure is said");
        assert!(!shader_allowed(&failed), "not tried after a failure");
        assert!(!shader_gave_up(&failed), "a later failure is not said again");
        assert!(!shader_allowed(&failed));
    }

    #[test]
    fn a_frosted_pane_is_clear_outside_its_round_corners_and_solid_inside() {
        let behind = grey_behind(64, 64);
        let picture = frosted(&behind.as_behind(), &pane([0.0, 0.0, 40.0, 40.0], 12.0, false)).unwrap();
        assert_eq!((picture.w, picture.h), (40, 40));
        assert!(alpha(&picture, 0, 0) < 0.05); // the corner's tip
        assert!(alpha(&picture, 20, 20) > 0.95); // the middle
    }

    #[test]
    fn glass_without_a_gpu_is_drawn_frosted() {
        let behind = grey_behind(64, 64);
        let glass = pane([4.0, 4.0, 40.0, 24.0], 12.0, true);
        let drawn = draw(&behind.as_behind(), &glass).unwrap();
        // Another test of this binary may install a device at any moment; once there it stays, so a device
        // missing after the drawing was missing during it too. With one, this test has nothing to check.
        if crate::gpu::ready() {
            return;
        }
        assert_eq!(drawn.px, frosted(&behind.as_behind(), &glass).unwrap().px);
    }

    /// The window the kept pictures are drawn over: `golden/behind.png`, 320 by 200 points, at `scale` pixels
    /// per point (each pixel of the photo repeated), in display colours.
    fn photo(scale: usize) -> Bitmap {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden").join("behind.png");
        let image = image::open(&path).unwrap().to_rgba8();
        let (width, height) = (usize::try_from(image.width()).unwrap(), usize::try_from(image.height()).unwrap());
        let mut picture = Bitmap::new(width * scale, height * scale);
        let across = picture.w;
        for (at, pixel) in picture.px.iter_mut().enumerate() {
            let (column, row) = (at % across / scale, at / across / scale);
            let colour = image.get_pixel(u32::try_from(column).unwrap(), u32::try_from(row).unwrap()).0.map(|c| f32::from(c) / 255.0);
            let alpha = colour[3];
            *pixel = [colour[0] * alpha, colour[1] * alpha, colour[2] * alpha, alpha];
        }
        picture
    }

    /// Compares `picture` with the kept picture `name`; with `DEVASSIST_BLESS=1` it keeps the new one instead.
    // Human: Quill's `matches_golden`. GPUs differ in the last bits, so a pixel may be a little off, and a few
    // pixels (an edge that falls on the other side of a pixel's centre) a lot.
    fn matches_golden(picture: &Bitmap, name: &str) {
        const NEAR: u8 = 6;
        const MOST_FAR: f64 = 0.005;
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden").join(format!("{name}.png"));
        let (w, h) = (u32::try_from(picture.w).unwrap(), u32::try_from(picture.h).unwrap());
        let bytes = picture.bytes();
        if std::env::var_os("DEVASSIST_BLESS").is_some() {
            image::RgbaImage::from_raw(w, h, bytes).unwrap().save(&path).unwrap();
            return;
        }
        let kept =
            image::open(&path).unwrap_or_else(|error| panic!("{}: {error}; run the tests once with DEVASSIST_BLESS=1", path.display())).to_rgba8();
        assert_eq!((kept.width(), kept.height()), (w, h), "{name} has another size");
        let (old, new) = (kept.as_raw().as_chunks::<4>().0, bytes.as_chunks::<4>().0);
        let far = old.iter().zip(new).filter(|(a, b)| a.iter().zip(*b).any(|(a, b)| a.abs_diff(*b) > NEAR)).count();
        let share = f64::from(u32::try_from(far).unwrap()) / f64::from(w * h);
        assert!(share <= MOST_FAR, "{name}: {far} pixels ({:.2} %) differ from the kept picture by more than {NEAR}", share * 100.0);
    }

    /// Whether the glass shader can run here. A machine without a GPU adapter skips the glass pictures, unless
    /// `DEVASSIST_REQUIRE_GPU` is set, as it should be where the tests must not be skipped.
    fn shader_runs() -> bool {
        let ready = crate::gpu::install_own_device();
        assert!(ready || std::env::var_os("DEVASSIST_REQUIRE_GPU").is_none(), "no GPU adapter was found for the glass shader's tests");
        ready
    }

    // After Quill's test of the same name, over the photo instead of a checkerboard.
    #[test]
    fn the_rim_bends_what_is_behind_it_and_the_middle_does_not() {
        if !shader_runs() {
            return;
        }
        let picture = photo(2);
        let (soft, rim) = (prepare(&picture, 0.0, 2.0), prepare(&picture, 0.0, 2.0));
        let behind = Behind { soft: &soft, rim: &rim, scale: 2.0, rim_scale: 2.0, picture_scale: 2.0 };
        let clear = |refraction: u8| {
            let material = Material::read(&json!({"type": "liquid_glass", "variant": "clear", "appearance": "light",
                "advanced": {"refraction": refraction, "highlight": 0, "blur": 0}}))
            .unwrap();
            let params = material.resolve((240.0, 100.0), false, &Lookup { number: &|_| None, color: &|_| None });
            draw(&behind, &Pane { rect: [40.0, 50.0, 240.0, 100.0], radius: 50.0, params }).unwrap()
        };
        let (bent, flat) = (clear(1), clear(0));
        // How many pixels in a box (in the pane picture's own pixels) differ clearly between the two.
        let differing = |x0: usize, y0: usize, x1: usize, y1: usize| {
            let mut count = 0;
            for y in y0..y1 {
                for x in x0..x1 {
                    let (a, b) = (bent.px[y * bent.w + x], flat.px[y * flat.w + x]);
                    count += usize::from(a.iter().zip(b).any(|(a, b)| (a - b).abs() > 8.0 / 255.0));
                }
            }
            count
        };
        // The band along the top edge shows what lies further in; the middle shows the same in both.
        assert!(differing(100, 2, 380, 16) > 500, "the rim doesn't bend: {}", differing(100, 2, 380, 16));
        assert_eq!(differing(160, 80, 320, 120), 0);
    }

    #[test]
    fn a_frosted_pane_at_the_corner_of_a_coarse_backdrop_does_not_fade() {
        // A flat opaque backdrop of 100 points a side at a tenth of a pixel per point.
        let mut backdrop = Bitmap::new(10, 10);
        backdrop.fill([0.5, 0.5, 0.5, 1.0]);
        let soft = prepare(&backdrop, 0.0, 0.1);
        let behind = Behind { soft: &soft, rim: &soft, scale: 0.1, rim_scale: 0.1, picture_scale: 1.0 };
        let picture = frosted(&behind, &pane([0.0, 0.0, 30.0, 30.0], 0.0, false)).unwrap();
        let middle = picture.px[15 * picture.w + 15];
        for (x, y) in [(0, 0), (1, 1), (0, 10), (10, 0), (2, 2), (2, 15), (15, 2)] {
            let pixel = picture.px[y * picture.w + x];
            assert!((pixel[3] - 1.0).abs() < 1e-4, "alpha {} at {x}, {y}", pixel[3]);
            // Off the hairline along the edge, the colour is the middle's.
            if x >= 2 && y >= 2 {
                assert!((pixel[0] - middle[0]).abs() < 1e-3, "{pixel:?} at {x}, {y}; {middle:?} in the middle");
            }
        }
    }

    #[test]
    fn a_pane_over_a_backdrop_at_half_scale_looks_the_same() {
        const RECT: [f32; 4] = [30.0, 30.0, 260.0, 140.0];
        let picture = photo(1);
        let half = picture.halved();
        for appearance in ["light", "dark"] {
            let pane = material_pane(RECT, 32.0, "frosted", appearance);
            let (blur, rim_blur) = (pane.params.blur, pane.params.blur * RIM_BLUR);
            let (soft, rim) = (prepare(&picture, blur, 1.0), prepare(&picture, rim_blur, 1.0));
            let full = frosted(&Behind { soft: &soft, rim: &rim, scale: 1.0, rim_scale: 1.0, picture_scale: 1.0 }, &pane).unwrap();
            let (soft, rim) = (prepare(&half, blur, 0.5), prepare(&half, rim_blur, 0.5));
            let coarse = frosted(&Behind { soft: &soft, rim: &rim, scale: 0.5, rim_scale: 0.5, picture_scale: 1.0 }, &pane).unwrap();
            assert_eq!((coarse.w, coarse.h), (full.w, full.h), "the picture keeps its own scale");
            let channels = f64::from(u32::try_from(full.px.len() * 4).unwrap());
            let total: f64 = full.px.iter().zip(&coarse.px).flat_map(|(a, b)| a.iter().zip(b).map(|(a, b)| f64::from((a - b).abs()))).sum();
            let mean = total / channels;
            assert!(mean < 2.0 / 255.0, "{appearance}: a mean difference of {:.2} / 255", mean * 255.0);
        }
    }

    #[test]
    fn the_panes_match_the_kept_pictures() {
        const RECT: [f32; 4] = [30.0, 30.0, 260.0, 140.0];
        const RADIUS: f32 = 32.0;
        let glass_runs = shader_runs();
        for scale in [1_u8, 2] {
            let picture = photo(usize::from(scale));
            let at = f32::from(scale);
            for (kind, name) in [("frosted", "frosted"), ("liquid_glass", "glass")] {
                if kind == "liquid_glass" && !glass_runs {
                    continue;
                }
                for appearance in ["light", "dark"] {
                    let pane = material_pane(RECT, RADIUS, kind, appearance);
                    let (soft, rim) = (prepare(&picture, pane.params.blur, at), prepare(&picture, pane.params.blur * RIM_BLUR, at));
                    let behind = Behind { soft: &soft, rim: &rim, scale: at, rim_scale: at, picture_scale: at };
                    let drawn = draw(&behind, &pane).unwrap();
                    assert_eq!((drawn.w, drawn.h), (260 * usize::from(scale), 140 * usize::from(scale)));
                    let cpu = frosted(&behind, &pane).unwrap();
                    if kind == "frosted" {
                        assert_eq!(drawn.px, cpu.px, "a frosted pane is drawn on the CPU");
                    } else {
                        // The shader really drew it: the glass picture is not the frosted one.
                        assert_ne!(drawn.px, cpu.px, "glass fell back to frost");
                        // The kept glass pictures were drawn on a real GPU; a software adapter draws a little
                        // otherwise, so its pictures are only checked for their size and for being glass.
                        if crate::gpu::software() {
                            eprintln!("{name}-{appearance}-{scale}x: drawn on a software adapter, not compared with the kept picture");
                            continue;
                        }
                    }
                    matches_golden(&drawn, &format!("{name}-{appearance}-{scale}x"));
                }
            }
        }
    }
}
