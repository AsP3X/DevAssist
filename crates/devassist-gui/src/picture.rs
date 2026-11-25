//! For the tests: pictures of what egui paints, made on the CPU over the window's wallpaper, one pixel a point.
//! The contrast measurement lays panes over the wallpaper with them; the table's picture paints a whole frame.

use std::collections::HashMap;

use devassist_glass::raster::{Bitmap, Pixel};
use egui::epaint::textures::TexturesDelta;
use egui::epaint::{ClippedPrimitive, ImageData, Primitive};
use egui::{ColorImage, TextureId};

use crate::baker::{Scene, Wallpaper, wallpaper};

/// `over` (premultiplied) painted on `under`.
pub(crate) fn over(under: Pixel, over: Pixel) -> Pixel {
    let keep = 1.0 - over[3];
    [0, 1, 2, 3].map(|i| over[i] + under[i] * keep)
}

/// The wallpaper of `scene` and the window as the screen shows it, at one pixel a point: the wallpaper scaled up
/// with bilinear sampling.
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "pixel places on a window of a few thousand points, far inside every range"
)]
pub(crate) fn wallpaper_window(scene: &Scene) -> (Wallpaper, Bitmap) {
    let wall = wallpaper(scene).unwrap();
    let (w, h) = (scene.size[0] as usize, scene.size[1] as usize);
    let mut bitmap = Bitmap::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let (u, v) = ((x as f32 + 0.5) * wall.scale, (y as f32 + 0.5) * wall.scale);
            bitmap.px[y * w + x] = wall.bitmap.sample(u, v);
        }
    }
    (wall, bitmap)
}

/// Keeps the pictures `delta` sets in `textures` (a whole one replaces, a patch is written into the one it
/// patches), and marks the delta handled.
pub(crate) fn take_textures(mut delta: TexturesDelta, textures: &mut HashMap<TextureId, ColorImage>) {
    for (id, deltas) in std::mem::take(&mut delta.set) {
        for change in deltas {
            let ImageData::Color(image) = &change.image;
            match (change.pos, textures.get_mut(&id)) {
                (Some([left, top]), Some(whole)) => {
                    let [w, h] = image.size;
                    for y in 0..h {
                        for x in 0..w {
                            let at = (top + y) * whole.size[0] + left + x;
                            if let Some(pixel) = whole.pixels.get_mut(at) {
                                *pixel = image.pixels[y * w + x];
                            }
                        }
                    }
                }
                _ => {
                    textures.insert(id, ColorImage::clone(image));
                }
            }
        }
    }
    delta.clear();
}

/// Paints `primitives` (tessellated at one pixel a point) onto `bitmap`, each mesh in its clip and from its
/// picture in `textures`.
pub(crate) fn paint(bitmap: &mut Bitmap, primitives: &[ClippedPrimitive], textures: &HashMap<TextureId, ColorImage>) {
    for primitive in primitives {
        let Primitive::Mesh(mesh) = &primitive.primitive else { continue };
        let clip = primitive.clip_rect;
        bitmap.triangles(mesh, |p| [p.x, p.y], Some([clip.min.x, clip.min.y, clip.max.x, clip.max.y]), textures.get(&mesh.texture_id), false);
    }
}
