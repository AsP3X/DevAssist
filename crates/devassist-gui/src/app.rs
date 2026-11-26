//! The window: opened as Quill opens its own, and at each frame the glass baked and the shell drawn over it.

use std::time::Instant;

use devassist_core::config::{self, Settings};
use devassist_glass::gpu;
use egui::{Color32, LayerId, Mesh};

use crate::assets;
use crate::baker::{Baker, Scene};
use crate::demo;
use crate::dialogs::{self, Dialogs};
use crate::launch::{self, Launch};
use crate::overlay;
use crate::repos_screen::{self, Screen};
use crate::repos_view::Status;
use crate::screenshot;
use crate::session::Session;
use crate::shell::{self, Page, Shown};
use crate::theme::{self, Mode, NOTICE_TIME, RESIZE_SETTLE, Surface};

/// The window's title, and the app's name to the system.
const TITLE: &str = "DevAssist";
/// The app's id to the system (Wayland and the like).
const APP_ID: &str = "dev.devassist.desktop";

/// The settings the window opens with: the demo's own for `--demo`, which reads no file (so the default look);
/// else the file `--config` names, else the ones a start of the terminal app without options reads (taking over
/// settings from older versions).
///
/// # Errors
/// A sentence saying why the settings file could not be read.
pub fn load_settings(launch: &Launch) -> Result<Settings, String> {
    if launch.demo {
        return Ok(demo::settings());
    }
    match &launch.config {
        Some(path) => config::load(path),
        None => config::load_default(),
    }
}

/// The line to tell the user when `settings` could not be read, the window then opening in the default look;
/// `None` when they were read.
#[must_use]
pub fn settings_warning(settings: &Result<Settings, String>) -> Option<String> {
    settings.as_ref().err().map(|e| format!("devassist-gui: {e}; opening with the default look"))
}

/// What the window says in its status bar when `settings` could not be read, in words for a person; `None` when
/// they were read. The console has its own line ([`settings_warning`]).
#[must_use]
pub fn window_warning(settings: &Result<Settings, String>) -> Option<String> {
    settings.as_ref().err().map(|e| format!("The settings could not be read, so the window opens in the default look: {e}"))
}

/// What the status bar says instead of the screen's own words: the settings warning `notice`, while less than
/// [`NOTICE_TIME`] has passed since the window opened (`elapsed`), as an error.
#[must_use]
pub fn notice_status(notice: Option<&str>, elapsed: std::time::Duration) -> Option<Status> {
    notice.filter(|_| elapsed < NOTICE_TIME).map(|text| Status { text: text.to_owned(), error: true, notes: Vec::new() })
}

/// Opens the window for `launch`, in the look of `settings` (from [`load_settings`]), and runs it until it is
/// closed.
///
/// # Errors
/// A sentence saying why the window could not be opened.
pub fn run(launch: Launch, settings: Result<Settings, String>) -> Result<(), String> {
    let mut viewport =
        egui::ViewportBuilder::default().with_title(TITLE).with_app_id(APP_ID).with_inner_size(theme::WINDOW).with_min_inner_size(theme::MIN_WINDOW);
    if cfg!(target_os = "macos") {
        // The top row is the title bar, with the window buttons inset at its start.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    } else {
        // The top row is the title bar: the system's own frame is left off, the row has the three window
        // buttons and moves the window (`shell::draw`), and the window's edges resize it (`resize_edges`).
        viewport = viewport.with_decorations(false);
    }
    if launch.screenshot.is_some() {
        // A run that only takes a picture keeps its window under every other window and out of the way.
        viewport = viewport.with_window_level(egui::WindowLevel::AlwaysOnBottom).with_active(false);
    }
    let options = eframe::NativeOptions { viewport, renderer: eframe::Renderer::Wgpu, ..Default::default() };
    eframe::run_native(TITLE, options, Box::new(move |cc| Ok(Box::new(App::new(cc, &launch, settings)))))
        .map_err(|e| format!("DevAssist couldn't open its window: {e}"))
}

/// The open window.
pub struct App {
    /// The look while the system asks for light, and while it asks for dark: mode, backdrop key and blur.
    looks: [(Mode, String, bool); 2],
    /// Whether the sidebar is shown.
    sidebar_shown: bool,
    /// The glass's pictures.
    baker: Baker,
    /// The mode egui's own visuals were last set for.
    visuals: Option<Mode>,
    /// The picture to take, in a `--screenshot` run.
    shot: Option<screenshot::Job>,
    /// When the window opened; the baker's clock counts from here.
    start: Instant,
    /// The repositories, loaded and kept current.
    session: Session,
    /// The screen the sidebar has chosen.
    page: Page,
    /// What the Repositories screen keeps between frames.
    screen: Screen,
    /// What the dialogs keep between frames.
    dialogs: Dialogs,
    /// The settings warning, said in the status bar for the first [`NOTICE_TIME`]: a window started without a
    /// console has nowhere else to say it.
    notice: Option<String>,
    /// A picture of the demo's Add dialog with the keyboard in its folder field (`--show add-folder`): the focus
    /// goes there once the dialog is open.
    folder_focus: bool,
}

impl App {
    /// The window's state, with the faces installed and the window's GPU lent to the glass.
    fn new(cc: &eframe::CreationContext<'_>, launch: &Launch, settings: Result<Settings, String>) -> App {
        assets::install_fonts(&cc.egui_ctx);
        // The glass shader runs on the same GPU device the window is drawn with.
        if let Some(state) = &cc.wgpu_render_state {
            gpu::install(state.device.clone(), state.queue.clone());
        }
        let looks = [launch::look(settings.clone(), launch, false), launch::look(settings.clone(), launch, true)];
        let notice = window_warning(&settings);
        App {
            looks,
            sidebar_shown: !launch.sidebar_hidden,
            baker: Baker::new(),
            visuals: None,
            shot: launch.screenshot.clone().map(screenshot::Job::new),
            start: Instant::now(),
            session: Session::start(settings, launch),
            page: Page::default(),
            screen: Screen::default(),
            dialogs: Dialogs::default(),
            notice,
            folder_focus: launch.demo && launch.show.as_deref() == Some(demo::ADD_FOLDER),
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // What the core's workers sent is taken in once a frame, and the next frame is asked for in time.
        ctx.request_repaint_after(self.session.tick());
        if let Some(code) = self.shot.as_mut().and_then(|job| job.frame(ctx)) {
            // The picture is the whole job. The repositories are closed first (running git is asked to stop), then
            // the process leaves at once.
            self.session.close();
            std::process::exit(code);
        }
    }

    /// Once, as the window closes, however it was asked to (its own close button, the system's, a signal): stops
    /// the git the core started.
    fn on_exit(&mut self) {
        self.session.close();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        resize_edges(&ctx);
        // The system can change its look at any time (at sunset, say), so it is asked every frame.
        let system_dark = ctx.system_theme() == Some(egui::Theme::Dark);
        let (mode, backdrop, blur) = self.looks[usize::from(system_dark)].clone();
        if self.visuals != Some(mode) {
            self.visuals = Some(mode);
            // egui follows the system's theme by default; pinned to the window's mode, its own defaults match
            // what the window draws, whatever the system does. Pinned first, as the visuals are set for it.
            ctx.set_theme(if mode == Mode::Dark { egui::Theme::Dark } else { egui::Theme::Light });
            let mut visuals = if mode == Mode::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
            // The baked pictures are the window's background; egui's panels must not cover them.
            visuals.panel_fill = Color32::TRANSPARENT;
            ctx.set_visuals(visuals);
        }

        let size = ctx.content_rect().size();
        // A dialog is on the Repositories screen: one the core's workers opened brings its page back.
        self.page = dialogs::page_for(self.session.repos(), self.page);
        let on_repositories = self.page == Page::Repositories;
        let layout = shell::layout(size, self.sidebar_shown, on_repositories && repos_screen::has_toolbar(&self.session));
        let painter = ctx.layer_painter(LayerId::background());
        // The screen's glass is placed before the bake, so it is baked where the screen draws over it.
        let plan = if on_repositories { repos_screen::plan(&painter, &layout, &self.session) } else { None };
        // The dialog or the menu the core's mode asks for, on any screen: opened now when the core's workers asked for
        // it. The menu stands under the row's button the screen placed in the last frame.
        let dialog = dialogs::before(&ctx, self.session.repos(), repos_screen::menu_anchor(&self.screen), &mut self.dialogs);
        let mut wants = shell::wants(&layout);
        if let Some(plan) = &plan {
            plan.add_wants(&mut wants);
        }
        for want in dialog.iter().flat_map(dialogs::Plan::wants) {
            wants.add(want);
        }
        let scene = Scene { size: size.into(), pixels_per_point: ctx.pixels_per_point(), mode, backdrop, blur };
        // The wallpaper first, stretched over the window, then each glass pane; each shows only its own part of
        // its texture (`uv`), or the backdrop behind a pane would sit shifted against the wallpaper. A dialog's or a
        // menu's pane is not painted here, under the screen, but on its overlay, over the scrim (`overlay::glass`).
        let drawn = self.baker.frame(&ctx, &scene, wants.as_slice(), self.start.elapsed());
        for drawn in drawn.iter().filter(|drawn| !overlay::is_overlay_pane(drawn.id)) {
            let mut mesh = Mesh::with_texture(drawn.texture);
            mesh.add_rect_with_uv(drawn.rect, drawn.uv, Color32::WHITE);
            painter.add(mesh);
        }
        if self.baker.waiting() {
            ctx.request_repaint_after(RESIZE_SETTLE);
        }

        // The frosted panel is no picture: the screen paints it from the sheet, which costs nothing to move.
        let sheet = self.baker.sheet(Surface::Panel);
        let notice = notice_status(self.notice.as_deref(), self.start.elapsed());
        let status = notice.or_else(|| repos_screen::status(&self.session));
        let waiting = self.session.repos().map_or(0, |repos| repos.queue.len());
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            let shown = Shown { mode, waiting, status: status.as_ref() };
            shell::draw(ui, &layout, &mut self.sidebar_shown, &mut self.page, shown);
            if on_repositories {
                // Under an open overlay the screen has no keyboard and takes no press.
                let keys = !overlay::is_open(&ctx);
                repos_screen::show(ui, &layout, plan.as_ref(), &mut self.session, sheet.as_ref(), mode, &mut self.screen, keys);
            } else {
                shell::placeholder(ui, layout.panel, sheet.as_ref(), theme::palette(mode));
            }
        });
        // Over everything but the window's own top row: the dialog, also one the screen asked for in this frame.
        dialogs::after(&ctx, dialog, self.session.repos_mut(), &drawn, mode, &mut self.dialogs);
        if self.folder_focus && dialogs::focus_add_folder(&ctx, self.session.repos()) {
            self.folder_focus = false;
        }
    }
}

// Copied from Quill (pencraft) app/src/app.rs at 6c6a0cc: `edge_at` and `resize_edges`.
// Two changes: the edges resize the window on every system where it has no frame of its own (all but macOS),
// not on Windows only; and the unseen area under the pointer senses clicks and drags but takes no keyboard focus
// (`Sense::CLICK | Sense::DRAG`, not `click_and_drag()`), so the filter stays the one control that holds it.

/// Which way a press at `at` resizes a window that covers `window`: `None` away from its edges.
// Human: within `WINDOW_EDGE` of an edge the window is resized that way; within `WINDOW_CORNER` of a corner
// along that edge, both ways at once.
fn edge_at(window: egui::Rect, at: egui::Pos2) -> Option<egui::ResizeDirection> {
    use egui::ResizeDirection as To;
    if !window.contains(at) {
        return None;
    }
    let (left, right) = (at.x - window.left(), window.right() - at.x);
    let (top, bottom) = (at.y - window.top(), window.bottom() - at.y);
    let near = left.min(right).min(top).min(bottom) < theme::WINDOW_EDGE;
    let reach = |edge: f32| edge < theme::WINDOW_CORNER;
    match (near, reach(top), reach(bottom), reach(left), reach(right)) {
        (false, ..) => None,
        (_, true, _, true, _) => Some(To::NorthWest),
        (_, true, _, _, true) => Some(To::NorthEast),
        (_, _, true, true, _) => Some(To::SouthWest),
        (_, _, true, _, true) => Some(To::SouthEast),
        _ if top < theme::WINDOW_EDGE => Some(To::North),
        _ if bottom < theme::WINDOW_EDGE => Some(To::South),
        _ if left < theme::WINDOW_EDGE => Some(To::West),
        _ => Some(To::East),
    }
}

/// Lets the window's edges and corners resize it, where Quill draws the title bar itself (Windows) and the
/// system's frame, which did that, is gone. Not while the window is maximised or fills the screen.
// Agent: while the pointer is at an edge a small unseen area lies under it, above everything else, so the press
// that starts the resize goes to nothing beneath it.
fn resize_edges(ctx: &egui::Context) {
    use egui::{CursorIcon, ResizeDirection as To};
    let fixed = ctx.input(|i| i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false));
    let Some(at) = ctx.input(|i| i.pointer.latest_pos()).filter(|_| !cfg!(target_os = "macos") && !fixed) else { return };
    let Some(to) = edge_at(ctx.content_rect(), at) else { return };
    let catch = egui::Rect::from_center_size(at, egui::Vec2::splat(2.0 * theme::WINDOW_CORNER));
    let caught = egui::Area::new(egui::Id::unique("window-edge"))
        .order(egui::Order::Tooltip)
        .fixed_pos(catch.min)
        .show(ctx, |ui| ui.allocate_exact_size(catch.size(), egui::Sense::CLICK | egui::Sense::DRAG).1)
        .inner;
    ctx.set_cursor_icon(match to {
        To::North | To::South => CursorIcon::ResizeVertical,
        To::East | To::West => CursorIcon::ResizeHorizontal,
        To::NorthWest | To::SouthEast => CursorIcon::ResizeNwSe,
        To::NorthEast | To::SouthWest => CursorIcon::ResizeNeSw,
    });
    if caught.is_pointer_button_down_on() && ctx.input(|i| i.pointer.primary_pressed()) {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(to));
    }
}

#[cfg(test)]
mod tests {
    use egui::{ResizeDirection as To, pos2};

    use super::*;

    #[test]
    fn a_settings_file_that_could_not_be_read_is_said() {
        let broken = Err("line 1: theme is light, dark or system".to_owned());
        assert_eq!(
            settings_warning(&broken).as_deref(),
            Some("devassist-gui: line 1: theme is light, dark or system; opening with the default look")
        );
        let read = Ok(Settings::for_folder(Vec::new(), std::path::PathBuf::from("devassist.toml")));
        assert_eq!(settings_warning(&read), None);
        assert_eq!(
            window_warning(&broken).as_deref(),
            Some("The settings could not be read, so the window opens in the default look: line 1: theme is light, dark or system")
        );
        assert_eq!(window_warning(&read), None);
    }

    #[test]
    fn a_demo_reads_no_settings_file() {
        let path = std::env::temp_dir().join(format!("devassist-gui-broken-{}.toml", std::process::id()));
        std::fs::write(&path, "theme = 5\n[[[").unwrap();
        let named = Launch { config: Some(path.clone()), ..Launch::default() };
        assert!(load_settings(&named).is_err(), "the file has a mistake");
        // The demo does not read it: no warning, and the default look.
        let settings = load_settings(&Launch { demo: true, ..named });
        assert_eq!(window_warning(&settings), None);
        assert_eq!(settings.map(|settings| settings.look), Ok(devassist_core::config::Look::default()));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn the_settings_warning_is_the_status_for_its_first_seconds_only() {
        let notice = Some("The settings could not be read");
        let said = notice_status(notice, std::time::Duration::from_secs(3)).unwrap();
        assert_eq!((said.text.as_str(), said.error, said.notes.len()), ("The settings could not be read", true, 0));
        assert_eq!(notice_status(notice, NOTICE_TIME), None);
        assert_eq!(notice_status(notice, NOTICE_TIME + std::time::Duration::from_secs(1)), None);
        assert_eq!(notice_status(None, std::time::Duration::ZERO), None);
    }

    #[test]
    fn a_press_near_an_edge_or_corner_resizes_that_way() {
        let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1440.0, 900.0));
        assert_eq!(edge_at(window, pos2(700.0, 400.0)), None);
        assert_eq!(edge_at(window, pos2(700.0, 2.0)), Some(To::North));
        assert_eq!(edge_at(window, pos2(1438.0, 400.0)), Some(To::East));
        assert_eq!(edge_at(window, pos2(2.0, 898.0)), Some(To::SouthWest));
        assert_eq!(edge_at(window, pos2(10.0, 2.0)), Some(To::NorthWest));
        assert_eq!(edge_at(window, pos2(-5.0, 400.0)), None);
    }
}
