# Assets of the window

Everything here is built into the program with `include_bytes!` (see `src/assets.rs`). All downloads were made on 2026-11-25.

## Fonts (`fonts/`)

### Barlow Tabular Regular and Medium (`BarlowTabular-Regular.ttf`, `BarlowTabular-Medium.ttf`)

Modified copies of Barlow, under the SIL Open Font License 1.1 (`OFL.txt`, copyright 2017 The Barlow Project Authors,
https://github.com/jpt/barlow). The licence declares no Reserved Font Name.

Originals (not shipped, removed from the repository):

| File | Source | SHA-256 | Size |
| --- | --- | --- | --- |
| `Barlow-Regular.ttf` | https://raw.githubusercontent.com/google/fonts/main/ofl/barlow/Barlow-Regular.ttf (branch `main`; the last commit touching it is `89f5431ff0db41bd2fe3f7ba21a723a01622428b`, 2018-12-05) | `95aa02c7c43096e0dd44d787ba6216864a67157e402adab59b35572e0c1577ea` | 104068 |
| `Barlow-Medium.ttf` | https://raw.githubusercontent.com/google/fonts/main/ofl/barlow/Barlow-Medium.ttf (same) | `f8906f762cb73dca441da034bc363b2d8e2e68bc10d5c05e58717646c20cc4b4` | 103536 |

What was changed, and only this: the cmap entries for U+0030 to U+0039 point at the font's own tabular figures
(the glyphs of its GSUB `tnum` feature, `zero.tf` to `nine.tf`), so every digit has the same width in egui, which does
not apply OpenType features; and the family is renamed from "Barlow" to "Barlow Tabular" (name IDs 1, 3, 4, 6, 16;
PostScript name `BarlowTabular-...`).

To reproduce (fonttools 4.68.0, in a scratch virtual environment):

```
python make-tabular.py Barlow-Regular.ttf BarlowTabular-Regular.ttf
python make-tabular.py Barlow-Medium.ttf BarlowTabular-Medium.ttf
```

| Shipped file | SHA-256 | Size |
| --- | --- | --- |
| `BarlowTabular-Regular.ttf` | `f35fc31702c3ddefb140298d1569c46681a65d19f2f90b87f58dc92ddf8724f0` | 103464 |
| `BarlowTabular-Medium.ttf` | `5eeb910bcd531a446b1fab699b413a6ad9fe3f659fbaa855caf6fd5ef87e4565` | 102904 |
| `OFL.txt` | `186d750eb496a4c17a76385f82be6aea2ac1cf2de074a811d63786cf374ea73f` | 4377 |
| `make-tabular.py` | `ad68b478d18aced7e74c408935ebeed17ae500ee24863a9ceee6ac12c52e083e` | 2215 |

### Lucide icons (`lucide.ttf`, `LUCIDE-LICENSE.txt`)

From the npm package `lucide-static` version **1.80.0** (the `latest` tag on 2026-11-25), via
`https://unpkg.com/lucide-static@1.80.0/font/lucide.ttf` and `.../LICENSE`; code points from `.../font/info.json`
(not stored). The version was found with one plain GET to `https://registry.npmjs.org/lucide-static`. Licence: ISC.

| File | SHA-256 | Size |
| --- | --- | --- |
| `lucide.ttf` | `4067ac045c28dd64c23131a119ffd8a3531ff7dffc34a6cd3edb66e26b5828fe` | 1032408 |
| `LUCIDE-LICENSE.txt` | `b495047bd93a9b06913511076f504daba17d5bbeb3e0650f3bb53a4220329c57` | 3208 |

The code points of the icons added for the repositories table were read on 2026-11-25 from
`https://unpkg.com/lucide-static@1.80.0/font/info.json` (version 1.80.0, the `encodedCode` of each name; the list is
not stored). The file is long, so it was read in four successive parts of that one URL. Its `house` (`e0f5`) and
`folder-git-2` (`e40a`) agree with `src/assets.rs`.

| Name | Code point | Name | Code point |
| --- | --- | --- | --- |
| `git-branch` | `e0e2` | `arrow-up-down` | `e37d` |
| `git-pull-request` | `e0e5` | `check` | `e06c` |
| `git-merge` | `e0e4` | `ellipsis` | `e0b6` |
| `git-pull-request-closed` | `e35a` | `triangle-alert` | `e193` |
| `eye` | `e0ba` | `loader-circle` | `e10a` |
| `chevron-down` | `e06d` | `rotate-cw` | `e149` |
| `chevron-right` | `e06f` | `clock` | `e087` |
| `arrow-up` | `e04a` | `arrow-down` | `e042` |

## Backdrops (`backdrops/`)

JPEG, quality 88, 1440 by 900, converted from the PNGs in `docs/design/glass/backdrops/` (the conversion script is not kept):
`alpine-lake-sunrise` to `lake`, `sea-coast-midday` to `midday`, `oak-hills-golden-hour` to `golden`,
`fjord-blue-hour` to `dusk`, `desert-night` to `night`, `beech-forest-foggy-morning` to `forest`.
All stayed at quality 88 (each under 600 KB).

| File | SHA-256 | Size |
| --- | --- | --- |
| `lake.jpg` | `eb949ae48e450f5791e3e10202407459977111283b0847393283b77054c1b7c5` | 200552 |
| `midday.jpg` | `c83c915ff68366911c6ad69a0f02c58849737fa0716401bf48528f09864b9e1e` | 265993 |
| `golden.jpg` | `288d751ddd8a2cc35f573be75b754ba22cfcce3dd6d675c07bf437c64817e7a9` | 222068 |
| `dusk.jpg` | `1b97dcfe1b1f2d813f809e28804becd8e59158adaeaae6374abc8ee8ecc2b3bd` | 128539 |
| `night.jpg` | `5dfa17d33e0cbb5eae9f557cbd21dc5dd4ab0ba7fcce215459b82c192537cada` | 222985 |
| `forest.jpg` | `e5a9c4be314aa53ba4d82cdec511474faf791a4026f12c7457d373a80fe83825` | 281822 |
