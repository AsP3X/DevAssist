//! The window's own faces, icons and backdrop pictures, built into the program.

use std::sync::Arc;

use egui::{Context, FontData, FontDefinitions, FontFamily, FontId};

const BARLOW_REGULAR: &[u8] = include_bytes!("../assets/fonts/BarlowTabular-Regular.ttf");
const BARLOW_MEDIUM: &[u8] = include_bytes!("../assets/fonts/BarlowTabular-Medium.ttf");
const LUCIDE: &[u8] = include_bytes!("../assets/fonts/lucide.ttf");

const BACKDROP_LAKE: &[u8] = include_bytes!("../assets/backdrops/lake.jpg");
const BACKDROP_MIDDAY: &[u8] = include_bytes!("../assets/backdrops/midday.jpg");
const BACKDROP_GOLDEN: &[u8] = include_bytes!("../assets/backdrops/golden.jpg");
const BACKDROP_DUSK: &[u8] = include_bytes!("../assets/backdrops/dusk.jpg");
const BACKDROP_NIGHT: &[u8] = include_bytes!("../assets/backdrops/night.jpg");
const BACKDROP_FOREST: &[u8] = include_bytes!("../assets/backdrops/forest.jpg");

/// The name of the family that holds Barlow Medium.
const MEDIUM_FAMILY: &str = "medium";
/// The name of the family that holds the Lucide icon font.
const ICONS_FAMILY: &str = "icons";
/// The size, in points, at which glyphs are looked up and measured here.
const PROBE_SIZE: f32 = 14.0;

/// The family for medium-weight text.
#[must_use]
pub fn medium_family() -> FontFamily {
    FontFamily::Name(MEDIUM_FAMILY.into())
}

/// The family for icons; draw them with the characters [`icon`] returns.
#[must_use]
pub fn icons_family() -> FontFamily {
    FontFamily::Name(ICONS_FAMILY.into())
}

/// Install the window's faces into `ctx`: Barlow Regular as `Proportional`, Barlow Medium as the `"medium"`
/// family, Lucide as the `"icons"` family. `Monospace` stays egui's own. The faces egui ships stay behind
/// Barlow as fallbacks for characters it lacks.
pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();
    let fallbacks = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();

    fonts.font_data.insert("BarlowTabular-Regular".into(), Arc::new(FontData::from_static(BARLOW_REGULAR)));
    fonts.font_data.insert("BarlowTabular-Medium".into(), Arc::new(FontData::from_static(BARLOW_MEDIUM)));
    fonts.font_data.insert("Lucide".into(), Arc::new(FontData::from_static(LUCIDE)));

    let with_fallbacks = |first: &str| std::iter::once(first.to_owned()).chain(fallbacks.iter().cloned()).collect::<Vec<_>>();
    fonts.families.insert(FontFamily::Proportional, with_fallbacks("BarlowTabular-Regular"));
    fonts.families.insert(medium_family(), with_fallbacks("BarlowTabular-Medium"));
    fonts.families.insert(icons_family(), vec!["Lucide".to_owned()]);

    ctx.set_fonts(fonts);
}

/// Lists the icons once: each entry is a variant, its code point in the Lucide font (`lucide-static` 1.80.0) and
/// its Lucide name. The enum, [`Icon::ALL`], [`icon`] and [`Icon::name`] are all made from the list, so an icon
/// cannot be in one and left out of another.
macro_rules! icons {
    ($($variant:ident => $code:literal, $name:literal,)+) => {
        /// An icon of the Lucide font the window uses; the variant is the Lucide name in camel case.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Icon {
            $(
                #[doc = concat!("Lucide `", $name, "`.")]
                $variant,
            )+
        }

        impl Icon {
            /// Every icon, in the order of the enum.
            pub const ALL: [Icon; [$($name),+].len()] = [$(Icon::$variant),+];

            /// The icon's name in Lucide.
            #[must_use]
            pub fn name(self) -> &'static str {
                match self {
                    $(Icon::$variant => $name,)+
                }
            }
        }

        /// The character that draws `name` in the [`icons_family`]. Code points are from `lucide-static` 1.80.0.
        #[must_use]
        pub fn icon(name: Icon) -> char {
            match name {
                $(Icon::$variant => char::from_u32($code).unwrap_or(char::REPLACEMENT_CHARACTER),)+
            }
        }
    };
}

icons! {
    House => 0xe0f5, "house",
    FolderGit2 => 0xe40a, "folder-git-2",
    Container => 0xe4d5, "container",
    ListOrdered => 0xe1d1, "list-ordered",
    ScrollText => 0xe45f, "scroll-text",
    Search => 0xe151, "search",
    Settings => 0xe154, "settings",
    CircleHelp => 0xe082, "circle-help",
    PanelLeft => 0xe12a, "panel-left",
    Wrench => 0xe1b1, "wrench",
    Minus => 0xe11c, "minus",
    Square => 0xe167, "square",
    X => 0xe1b2, "x",
    EllipsisVertical => 0xe0b7, "ellipsis-vertical",
    GitBranch => 0xe0e2, "git-branch",
    GitPullRequest => 0xe0e5, "git-pull-request",
    GitMerge => 0xe0e4, "git-merge",
    GitPullRequestClosed => 0xe35a, "git-pull-request-closed",
    Eye => 0xe0ba, "eye",
    ChevronDown => 0xe06d, "chevron-down",
    ChevronRight => 0xe06f, "chevron-right",
    ArrowUp => 0xe04a, "arrow-up",
    ArrowDown => 0xe042, "arrow-down",
    ArrowUpDown => 0xe37d, "arrow-up-down",
    Check => 0xe06c, "check",
    Ellipsis => 0xe0b6, "ellipsis",
    TriangleAlert => 0xe193, "triangle-alert",
    LoaderCircle => 0xe10a, "loader-circle",
    RotateCw => 0xe149, "rotate-cw",
    Clock => 0xe087, "clock",
    Group => 0xe464, "group",
    ArrowDownToLine => 0xe455, "arrow-down-to-line",
    FolderOpen => 0xe247, "folder-open",
    Folder => 0xe0d7, "folder",
    ExternalLink => 0xe0b9, "external-link",
    Copy => 0xe09e, "copy",
    Trash2 => 0xe18e, "trash-2",
    FolderInput => 0xe334, "folder-input",
    Plus => 0xe13d, "plus",
    ArrowRight => 0xe049, "arrow-right",
    HardDrive => 0xe0ed, "hard-drive",
    Link => 0xe102, "link",
    Truck => 0xe194, "truck",
    FolderOutput => 0xe337, "folder-output",
    CloudUpload => 0xe091, "cloud-upload",
    ArrowLeft => 0xe048, "arrow-left",
    CloudOff => 0xe08d, "cloud-off",
}

/// The JPEG bytes of the backdrop picture for `key`, one of `devassist_core::config::BACKDROPS`.
#[must_use]
pub fn backdrop(key: &str) -> Option<&'static [u8]> {
    match key {
        "lake" => Some(BACKDROP_LAKE),
        "midday" => Some(BACKDROP_MIDDAY),
        "golden" => Some(BACKDROP_GOLDEN),
        "dusk" => Some(BACKDROP_DUSK),
        "night" => Some(BACKDROP_NIGHT),
        "forest" => Some(BACKDROP_FOREST),
        _ => None,
    }
}

/// Check that what is built in is usable: every backdrop decodes and every face installs and has the glyphs
/// the window draws (digits in both weights, every [`Icon`] in the icon font).
///
/// # Errors
/// A message naming the first thing that is not usable.
pub fn check() -> Result<(), String> {
    for key in devassist_core::config::BACKDROPS {
        let bytes = backdrop(key).ok_or_else(|| format!("no backdrop picture for {key}"))?;
        image::load_from_memory(bytes).map_err(|e| format!("backdrop {key} does not decode: {e}"))?;
    }
    let ctx = ready_context();
    ctx.fonts_mut(|fonts| {
        for family in [FontFamily::Proportional, medium_family()] {
            if !fonts.has_glyph(&FontId::new(PROBE_SIZE, family.clone()), '0') {
                return Err(format!("the {family} face did not load"));
            }
        }
        let icons = FontId::new(PROBE_SIZE, icons_family());
        match Icon::ALL.into_iter().find(|&name| !fonts.has_glyph(&icons, icon(name))) {
            Some(name) => Err(format!("the icon font has no glyph for {name:?}")),
            None => Ok(()),
        }
    })
}

/// A context with the window's faces installed and one empty frame run, which is what makes them available
/// to measure.
fn ready_context() -> Context {
    let ctx = Context::default();
    install_fonts(&ctx);
    // Nothing paints this frame, so its texture changes are dropped on purpose (egui panics if they are not).
    ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
    ctx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_backdrop_the_settings_know_has_a_picture_of_the_right_size() {
        for key in devassist_core::config::BACKDROPS {
            let bytes = backdrop(key).unwrap();
            let picture = image::load_from_memory(bytes).unwrap();
            assert_eq!((picture.width(), picture.height()), (1440, 900), "{key}");
        }
        assert!(backdrop("moon").is_none());
    }

    #[test]
    fn the_face_has_figures_of_one_width() {
        let ctx = ready_context();
        for family in [FontFamily::Proportional, medium_family()] {
            let id = FontId::new(PROBE_SIZE, family.clone());
            let widths: Vec<f32> = ctx.fonts_mut(|fonts| "0123456789".chars().map(|c| fonts.glyph_width(&id, c)).collect());
            assert!(widths[0] > 0.0, "{family}: the figure 0 has no width");
            for (digit, width) in widths.iter().enumerate() {
                assert!((width - widths[0]).abs() < 0.01, "{family}: figure {digit} is {width} wide; all: {widths:?}");
            }
            // The digits were remapped in the font; the letters must still be their own, different widths.
            let (i, m, l) = ctx.fonts_mut(|fonts| (fonts.glyph_width(&id, 'i'), fonts.glyph_width(&id, 'm'), fonts.glyph_width(&id, 'l')));
            assert!(0.0 < i && i + 1.0 < m, "{family}: i is {i} wide, m is {m}");
            assert!(l < m, "{family}: l is {l} wide, m is {m}");
        }
    }

    #[test]
    fn every_icon_has_a_glyph() {
        let ctx = ready_context();
        let id = FontId::new(PROBE_SIZE, icons_family());
        for name in Icon::ALL {
            let c = icon(name);
            let (has, width) = ctx.fonts_mut(|fonts| (fonts.has_glyph(&id, c), fonts.glyph_width(&id, c)));
            assert!(has, "{name:?} (U+{:04X}) is not in the icon font", u32::from(c));
            assert!(width > 0.0, "{name:?} has no width");
            assert_ne!(c, char::REPLACEMENT_CHARACTER, "{name:?}");
        }
        let distinct: std::collections::BTreeSet<char> = Icon::ALL.into_iter().map(icon).collect();
        assert_eq!(distinct.len(), Icon::ALL.len(), "two icons share a character");
    }

    #[test]
    fn the_icons_are_listed_once_without_repeats() {
        let codes: std::collections::BTreeSet<char> = Icon::ALL.into_iter().map(icon).collect();
        let names: std::collections::BTreeSet<&str> = Icon::ALL.into_iter().map(Icon::name).collect();
        assert_eq!(codes.len(), Icon::ALL.len(), "two icons share a code point");
        assert_eq!(names.len(), Icon::ALL.len(), "two icons share a name");
        assert!(!codes.contains(&char::REPLACEMENT_CHARACTER), "an icon has no valid code point");
        // The README lists 16 icons beyond the 14 the sidebar and bar already had, the group dialog's, and the
        // row menu's five, the folder field's, the top row's two, the Move dialog's arrow and drive, and the Push dialog's
        // link, the Migrate line's truck and its side menu's two, and the forks screen's back button and gone state.
        assert_eq!(Icon::ALL.len(), 47);
        assert_eq!((Icon::Truck.name(), Icon::FolderOutput.name(), Icon::CloudUpload.name()), ("truck", "folder-output", "cloud-upload"));
        assert_eq!(Icon::X.name(), "x");
        assert_eq!(Icon::FolderGit2.name(), "folder-git-2");
    }
}
