# Where this crate's code comes from

Most of `devassist-glass` is copied from Quill (pencraft), the design editor whose glass material the
DevAssist window uses, at commit **6c6a0cc**. Paths below are relative to Quill's `app` folder. Quill itself
is not changed.

To compare with Quill later: diff each file against the same path at that commit.

## Copied files

| Here | Quill | Edits |
| --- | --- | --- |
| `src/material.rs` | `src/material.rs` | `pen_doc::color::Rgba` is `crate::color::Rgba`; `pen_doc::value::narrow` is `crate::color::narrow`. |
| `src/raster.rs` | `src/raster.rs` | None. |
| `src/shader.rs` | `src/shader.rs` | `pen_doc::color::Rgba` is `crate::color::Rgba`; `pen_doc::value::narrow` is `crate::color::narrow`. Removed: `load` (reads a shader file from disk by path and again when it changed), its `STORE`, the type `Loaded` only it used, and the imports only it used (`Path`, `PathBuf`, `Arc`, `Mutex`, `OnceLock`, `SystemTime`). Added: `#[expect]` on `Type::len` (`clippy::len_without_is_empty`) and on `declarations` and `uniform` (`clippy::map_unwrap_or`), lints this workspace's clippy raises on Quill's unchanged code. Built and checked with rustc 1.100.0 and clippy 0.1.100 (0ef1bc3860 2026-11-09); a clippy that doesn't raise these lints fails the build on the unfulfilled `#[expect]`s, which are then removed. |
| `src/gpu.rs` | `src/gpu.rs` | `crate::app::lock` is `crate::sync::lock` (rustfmt moved the import below `crate::shader`). `install_for_tests` is `install_own_device`, public and no longer `#[cfg(test)]`, so the tests and measurements of both crates can make a device of their own (no code outside them calls it; the window lends its own device with `install`). New code, not from Quill: `ADAPTER`, the kind of adapter `install_own_device` made its device on, and `software()`, whether that is a software adapter (the CPU); the glass test `the_panes_match_the_kept_pictures` compares the glass pictures with the kept ones only when it is not. |
| `src/glass.frag` | `src/render/glass.frag` | None (a header comment above `#version`, which GLSL allows). |
| `src/color.rs` | `crates/pen-doc/src/color.rs`, `crates/pen-doc/src/value.rs` | Only `Rgba` with `BLACK`, `WHITE` and `parse`, and `narrow`. |
| `src/sync.rs` | `src/app.rs` | Only `lock`. |

Every copied file starts with a comment that names its source and its edits. Their test modules came along
unchanged: 6 tests in `material.rs`, 12 in `raster.rs`, 5 in `shader.rs`. No test needed `pen_doc` beyond the
two items above, so none was dropped from those files.

## `src/pane.rs`

New: draws one rounded pane over the window's backdrop. Copied into it unchanged:

- from `src/render/glass.rs`: `SHADER`, `BACKDROP`, `RIM`, `RIM_BLUR` (made public), `HAIRLINE`, `Shape` with
  `distance`, `CURVE_STEPS`, `Curve`, `to_linear`, `to_gamma`, `recode`, `linear`, `toward_light`,
  `frosted_pixel`, `program`; the tests `the_glass_shader_compiles_and_has_every_uniform_the_renderer_sets`
  (with its eight outlines written out, as `MOST_SHAPES` is not copied) and
  `the_colour_curves_undo_each_other_and_keep_the_alpha`;
- from `src/render/bake.rs`: `REACH`, `sigma`, `length` and `pixels_up`. `sigma` is made public, so the
  window works out the blur of its own pictures with the same rule (half the radius).

Rewritten for one pane that is a single rounded box and never turned:

- `outward` is `Form::outward` for one shape, with the turn left out (it is the identity).
- `draw`'s glass part (`glass`) is `Builder::glass` (`src/render/glass.rs`): `u_count` 1, `u_merge` 0, `u_turn`
  the identity, `u_box0` and `u_round0` from the pane, the frame the pane's centre in its own picture, and
  each sampler's map from an offset from that centre in points to a place on its backdrop picture. The other
  uniforms are filled as Quill fills them.
- `frosted` is Quill's `frosted` with its `form` branch only. New here: it clamps where it samples the backdrop
  to the outermost pixel centres, so the backdrop's edge pixel goes on to its edge (as the shader's mirrored
  sampler does) instead of fading into the clear past it; with a coarse backdrop that band would be several
  points wide. Its test is `a_frosted_pane_at_the_corner_of_a_coarse_backdrop_does_not_fade`.
- `prepare` is the part of Quill's `Builder::pane` that takes the backdrop to linear light and blurs it. Quill
  paints the backdrop a blur's reach (`REACH` standard deviations) beyond the pane; here the window's picture is
  grown by as much with its own edge mirrored (as the GPU sampler's `MirrorRepeat`), blurred and cut back, so
  it stays opaque up to the window's edge.
- `SHADER_FAILED`, `shader_allowed` and `shader_gave_up` are new code, not from Quill: after the glass shader's
  first failure in a process `draw` no longer tries it and draws every pane frosted, with one warning in the
  log (Quill warns at each failure; here each try can hold the window's thread for seconds). Its test is
  `after_the_shaders_first_failure_it_is_not_tried_again_and_said_once`.
- `to_display` is new code, not from Quill: it takes a picture `prepare` returned (linear light) back to
  display colours with the copied `recode` and `to_gamma`, so the window can blur its own wallpaper with
  `prepare`'s mirrored edges. Its test is `a_prepared_picture_comes_back_to_display_colours`.
- `Behind`'s `rim_scale` and `picture_scale` are new code, not from Quill: a blurred backdrop holds no
  detail finer than its blur, so the window makes `soft` and `rim` coarse, each at its own pixels per point
  (`scale`, `rim_scale`), while the pane's picture is drawn at `picture_scale`. `draw`, `frosted` and the
  shader's frame, `u_scale` and picture size use `picture_scale`; each sampler's map and the frosted sampling
  use the scale of the bitmap they read. With all three equal everything draws as before (the existing tests
  and golden pictures are unchanged). Its test is `a_pane_over_a_backdrop_at_half_scale_looks_the_same`
  (frosted over `golden/behind.png`, mean difference per channel under 2/255).
- The test `the_rim_bends_what_is_behind_it_and_the_middle_does_not` follows Quill's test of that name
  (`src/render/glass/drawn.rs`), over the photo instead of a checkerboard; `matches_golden` and `shader_runs`
  follow the functions of those names there, with this crate's environment variables (`DEVASSIST_BLESS`,
  `DEVASSIST_REQUIRE_GPU`), at most 0.5 % of pixels more than 6 off. On a software adapter (`gpu::software()`) the glass
  pictures are drawn and checked for their size and for not being frosted, but not compared with the kept
  ones, which were drawn on a real GPU.

## Left out, and why

- `Form`, `smooth_min`, `MOST_SHAPES`, glass groups, turned layers and ellipses: a pane here is one rounded
  box that is not turned. (`Shape` keeps its ellipse branch, as it is copied unchanged.)
- `lightness` (`src/render/glass.rs`), `levels`, `DARK_BELOW`: they decide the `auto` appearance from what is
  behind a layer. Nothing here does, so `lightness` would be dead code, which this crate denies.
- `Behind` (Quill's) and `Grid`: the backdrop here is the whole window at one scale with its top left at 0,0,
  so a pane's place on it is its rectangle times the scale.
- Shadows (`CONTACT`, `Shadow`, `drop_shadow`), the baked-picture cache and its keys, `COARSE_FROM`: the window
  draws and keeps its own pictures.
- Quill's other `glass.rs` tests (`a_box_measures…`, `an_oval…`, `a_turned_layer…`, `outlines_closer…`,
  `frost_keeps_grey_grey…`, `the_light_comes_from…`, `levels_give…`) test `Form`, `levels` and Quill's
  document; the parts copied here are covered by the tests above.
- `color.rs`'s own test: it also tests `to_hex`, which is not copied. `parse` is tested through
  `material.rs`'s and `shader.rs`'s tests.

## Golden pictures

`golden/behind.png` is a 320 by 200 crop (left 1000, top 190) of
`docs/design/glass/backdrops/alpine-lake-sunrise.png`: sky, snowy peaks, mist and the edge of a forest. The
other pictures in `golden/` are the panes `the_panes_match_the_kept_pictures` draws over it, as premultiplied
RGBA; run the tests with `DEVASSIST_BLESS=1` to make them again.
