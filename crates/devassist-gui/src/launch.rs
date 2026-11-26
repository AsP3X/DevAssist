//! What the window is started with: its command line, and the look it opens in.

use std::path::PathBuf;

use devassist_core::config::{BACKDROPS, Look, Settings};

use crate::demo;
use crate::theme::Mode;

/// How the command line is written.
pub const USAGE: &str =
    "usage: devassist-gui [--config PATH] [--screenshot OUT.png] [--light|--dark] [--backdrop KEY] [--no-blur] [--no-sidebar] [--demo [--show NAME]]";

/// What the command line asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Launch {
    /// The settings file to read instead of the usual one.
    pub config: Option<PathBuf>,
    /// Where to save a picture of the window; the run ends once it is saved.
    pub screenshot: Option<PathBuf>,
    /// Light or dark, whatever the settings say.
    pub mode: Option<Mode>,
    /// The backdrop's key, whatever the settings say.
    pub backdrop: Option<String>,
    /// The backdrop is not blurred, whatever the settings say.
    pub no_blur: bool,
    /// The window opens with its sidebar hidden.
    pub sidebar_hidden: bool,
    /// The window shows the drawing's twelve repositories instead of the real ones, reading no disk and no network.
    pub demo: bool,
    /// The overlay the demo opens before anything is drawn (one of `demo::SHOWS`): for pictures of a dialog.
    pub show: Option<String>,
}

/// Reads the command line `args`, without the program's own name.
///
/// # Errors
/// A sentence naming the first option that is unknown or lacks its value.
pub fn parse(args: impl Iterator<Item = String>) -> Result<Launch, String> {
    let mut launch = Launch::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => launch.config = Some(PathBuf::from(args.next().ok_or("--config needs a file name")?)),
            "--screenshot" => launch.screenshot = Some(PathBuf::from(args.next().ok_or("--screenshot needs a file name")?)),
            "--light" => launch.mode = Some(Mode::Light),
            "--dark" => launch.mode = Some(Mode::Dark),
            "--backdrop" => launch.backdrop = Some(args.next().ok_or_else(|| format!("--backdrop needs a key: {}", BACKDROPS.join(", ")))?),
            "--no-blur" => launch.no_blur = true,
            "--no-sidebar" => launch.sidebar_hidden = true,
            "--demo" => launch.demo = true,
            "--show" => launch.show = Some(args.next().ok_or("--show needs the name of a dialog or menu")?),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    if let Some(name) = &launch.show {
        if !launch.demo {
            return Err("--show is for the demo: add --demo".to_owned());
        }
        if !demo::SHOWS.iter().any(|(known, _)| known == name) {
            return Err(demo::show_error(name));
        }
    }
    Ok(launch)
}

/// The mode, backdrop key and blur the window opens in: the settings' look (the default one when they could
/// not be read), with what `launch` asks for over it. `system_dark` is whether the system asks for dark. A
/// backdrop key that names no backdrop gives the default backdrop.
#[must_use]
pub fn look(settings: Result<Settings, String>, launch: &Launch, system_dark: bool) -> (Mode, String, bool) {
    let look = settings.map_or_default(|settings| settings.look);
    let mode = launch.mode.unwrap_or_else(|| Mode::of(look.theme, system_dark));
    let backdrop = launch.backdrop.clone().unwrap_or(look.backdrop);
    let backdrop = if BACKDROPS.contains(&backdrop.as_str()) { backdrop } else { Look::default().backdrop };
    (mode, backdrop, look.blur && !launch.no_blur)
}

#[cfg(test)]
mod tests {
    use devassist_core::config::Theme;

    use super::*;

    #[test]
    fn the_options_are_read() {
        let l = parse(["--screenshot", "a.png", "--dark", "--backdrop", "night", "--no-blur"].map(String::from).into_iter()).unwrap();
        assert_eq!(l.screenshot, Some(PathBuf::from("a.png")));
        assert_eq!((l.mode, l.backdrop.as_deref(), l.no_blur), (Some(Mode::Dark), Some("night"), true));
        assert_eq!(parse(["--screenshot".to_string()].into_iter()).unwrap_err(), "--screenshot needs a file name");
        assert_eq!(parse(["--frobnicate".to_string()].into_iter()).unwrap_err(), "unknown option --frobnicate");
    }

    #[test]
    fn the_other_options_are_read_too() {
        let l = parse(["--config", "c.toml", "--light", "--no-sidebar"].map(String::from).into_iter()).unwrap();
        assert_eq!((l.config, l.mode, l.sidebar_hidden, l.no_blur), (Some(PathBuf::from("c.toml")), Some(Mode::Light), true, false));
        assert!(parse(["--demo".to_string()].into_iter()).unwrap().demo);
        assert!(!l.demo);
        assert_eq!(parse(std::iter::empty()).unwrap(), Launch::default());
        assert_eq!(parse(["--config".to_string()].into_iter()).unwrap_err(), "--config needs a file name");
        assert!(parse(["--backdrop".to_string()].into_iter()).unwrap_err().starts_with("--backdrop needs a key"));
        assert_eq!(parse(["stray".to_string()].into_iter()).unwrap_err(), "unexpected argument stray");
    }

    #[test]
    fn show_opens_a_known_overlay_in_the_demo_only() {
        let args = |list: &[&str]| parse(list.iter().map(|arg| (*arg).to_owned()));
        assert_eq!(args(&["--demo", "--show", "progress"]).unwrap().show.as_deref(), Some("progress"));
        assert_eq!(args(&["--show", "progress", "--demo"]).unwrap().show.as_deref(), Some("progress"));
        assert_eq!(args(&["--show", "progress"]).unwrap_err(), "--show is for the demo: add --demo");
        assert_eq!(args(&["--demo", "--show", "menu"]).unwrap().show.as_deref(), Some("menu"));
        assert_eq!(args(&["--demo", "--show", "forks"]).unwrap_err(), "--show knows progress, remove, group, menu, add, not forks");
        assert_eq!(args(&["--demo", "--show"]).unwrap_err(), "--show needs the name of a dialog or menu");
        assert_eq!(args(&["--demo"]).unwrap().show, None);
    }

    #[test]
    fn a_broken_or_missing_settings_file_gives_the_default_look() {
        let none = Launch::default();
        assert_eq!(look(Err("line 1: theme is light, dark or system".into()), &none, false), (Mode::Light, "lake".to_string(), true));
        assert_eq!(look(Err("x".into()), &none, true), (Mode::Dark, "lake".to_string(), true));
    }

    #[test]
    fn the_settings_look_is_used_and_options_win_over_it() {
        let mut settings = Settings::for_folder(Vec::new(), PathBuf::from("devassist.toml"));
        settings.look = Look { theme: Theme::Dark, backdrop: "golden".to_owned(), blur: false };
        assert_eq!(look(Ok(settings.clone()), &Launch::default(), false), (Mode::Dark, "golden".to_owned(), false));
        let asked = Launch { mode: Some(Mode::Light), backdrop: Some("night".to_owned()), ..Launch::default() };
        assert_eq!(look(Ok(settings.clone()), &asked, false), (Mode::Light, "night".to_owned(), false));
        let unknown = Launch { backdrop: Some("moon".to_owned()), ..Launch::default() };
        assert_eq!(look(Ok(settings.clone()), &unknown, false).1, "lake");
        // `--no-blur` turns a blurring look's blur off.
        settings.look.blur = true;
        let sharp = Launch { no_blur: true, ..Launch::default() };
        assert_eq!(look(Ok(settings), &sharp, true), (Mode::Dark, "golden".to_owned(), false));
    }
}
