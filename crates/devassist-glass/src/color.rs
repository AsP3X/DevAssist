// Copied from Quill (pencraft) app/crates/pen-doc/src/color.rs and app/crates/pen-doc/src/value.rs at 6c6a0cc.
// Only the items the other copied files use: `Rgba` with `BLACK`, `WHITE` and `parse` (color.rs), and `narrow` (value.rs).
// Left out: `to_hex`, `with_opacity`, `lerp`, `luminance`, `contrast`, `unit_to_byte`, the rest of value.rs, and color.rs's test.

//! Colours as `.pen` files write them: `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.

/// A straight-alpha (not premultiplied) sRGB colour, each channel 0–255.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rgba {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
    /// Alpha, 255 opaque.
    pub a: u8,
}

impl Rgba {
    /// Opaque black.
    pub const BLACK: Rgba = Rgba { r: 0, g: 0, b: 0, a: 255 };
    /// Opaque white.
    pub const WHITE: Rgba = Rgba { r: 255, g: 255, b: 255, a: 255 };

    /// Reads a hex colour; `None` when it isn't one.
    #[must_use]
    pub fn parse(text: &str) -> Option<Rgba> {
        let hex = text.trim().strip_prefix('#')?;
        if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let nibble = |i: usize| u8::from_str_radix(hex.get(i..=i)?, 16).ok();
        let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
        // Human: one hex digit doubles (`f` → `ff`), as in CSS.
        let short = |i: usize| nibble(i).map(|n| n * 17);
        match hex.len() {
            3 => Some(Rgba { r: short(0)?, g: short(1)?, b: short(2)?, a: 255 }),
            4 => Some(Rgba { r: short(0)?, g: short(1)?, b: short(2)?, a: short(3)? }),
            6 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: 255 }),
            8 => Some(Rgba { r: byte(0)?, g: byte(2)?, b: byte(4)?, a: byte(6)? }),
            _ => None,
        }
    }
}

/// Document coordinates are stored as JSON doubles but laid out and drawn in `f32`.
// Human: Canvas positions stay far below f32's exact-integer range (16.7 million), so the narrowing loses
// nothing a designer could see.
// Agent: the ONE f64 → f32 conversion in the crate; NaN stays NaN and is rejected by callers that need finite values.
#[must_use]
#[expect(clippy::cast_possible_truncation, reason = "canvas coordinates fit f32; see the comment above")]
pub fn narrow(value: f64) -> f32 {
    value as f32
}
