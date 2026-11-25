//! The glass look of the `DevAssist` window: shared drawing pieces.
//!
//! Most of this crate is copied from Quill's glass renderer (see `SOURCE.md`); [`pane`] is its own and draws
//! one rounded pane over a backdrop picture with that material.

pub mod color;
pub mod gpu;
pub mod material;
pub mod pane;
pub mod raster;
pub mod shader;
pub mod sync;
