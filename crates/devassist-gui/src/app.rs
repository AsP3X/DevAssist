//! The window: opened as Quill opens its own, and at each frame the glass baked and the shell drawn over it.

use std::time::Instant;

use devassist_core::config;
use devassist_glass::gpu;
use egui::{Color32, LayerId, Mesh};

use crate::assets;
use crate::baker::{Baker, Scene};
use crate::launch::{self, Launch};
use crate::screenshot;
use crate::shell::{self, Shown};
use crate::theme::{self, Mode, RESIZE_SETTLE};

/// The window's title, and the app's name to the system.
const TITLE: &str = "DevAssist";
/// The app's id to the system (Wayland and the like).
const APP_ID: &str = "dev.devassist.desktop";

/// Opens the window for `launch` and runs it until it is closed.
///
/// # Errors
/// A sentence saying why the window could not be opened.
pub fn run(launch: Launch) -> Result<(), String> {
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
    eframe::run_native(TITLE, options, Box::new(move |cc| Ok(Box::new(App::new(cc, launch)))))
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
}

impl App {
    /// The window's state, with the faces installed and the window's GPU lent to the glass.
    fn new(cc: &eframe::CreationContext<'_>, launch: Launch) -> App {
        assets::install_fonts(&cc.egui_ctx);
        // The glass shader runs on the same GPU device the window is drawn with.
        if let Some(state) = &cc.wgpu_render_state {
            gpu::install(state.device.clone(), state.queue.clone());
        }
        let settings = match &launch.config {
            Some(path) => config::load(path),
            None => config::load_default(),
        };
        if let Err(e) = &settings {
            log::warn!("settings: {e}; the window opens in the default look");
        }
        let looks = [launch::look(settings.clone(), &launch, false), launch::look(settings, &launch, true)];
        App {
            looks,
            sidebar_shown: !launch.sidebar_hidden,
            baker: Baker::new(),
            visuals: None,
            shot: launch.screenshot.map(screenshot::Job::new),
            start: Instant::now(),
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(job) = &mut self.shot {
            job.frame(ctx);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        resize_edges(&ctx);
        // The system can change its look at any time (at sunset, say), so it is asked every frame.
        let system_dark = ctx.system_theme() == Some(egui::Theme::Dark);
        let (mode, backdrop, blur) = self.looks[usize::from(system_dark)].clone();
        if self.visuals != Some(mode) {
            self.visuals = Some(mode);
            let mut visuals = if mode == Mode::Dark { egui::Visuals::dark() } else { egui::Visuals::light() };
            // The baked pictures are the window's background; egui's panels must not cover them.
            visuals.panel_fill = Color32::TRANSPARENT;
            ctx.set_visuals(visuals);
        }

        let size = ctx.content_rect().size();
        let scene = Scene { size: size.into(), pixels_per_point: ctx.pixels_per_point(), mode, backdrop: backdrop.clone(), blur };
        let wants = shell::wants(size, self.sidebar_shown);
        let painter = ctx.layer_painter(LayerId::background());
        // The wallpaper first, stretched over the window, then each pane; each shows only its own part of its
        // texture (`uv`), or the backdrop behind a pane would sit shifted against the wallpaper.
        for drawn in self.baker.frame(&ctx, &scene, &wants, self.start.elapsed()) {
            let mut mesh = Mesh::with_texture(drawn.texture);
            mesh.add_rect_with_uv(drawn.rect, drawn.uv, Color32::WHITE);
            painter.add(mesh);
        }
        if self.baker.waiting() {
            ctx.request_repaint_after(RESIZE_SETTLE);
        }

        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            shell::draw(ui, &wants, &mut self.sidebar_shown, Shown { mode, backdrop: &backdrop });
        });
    }
}

// Copied from Quill (pencraft) app/src/app.rs at 6c6a0cc: `edge_at` and `resize_edges`.
// One change: the edges resize the window on every system where it has no frame of its own (all but macOS),
// not on Windows only.

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
        .show(ctx, |ui| ui.allocate_exact_size(catch.size(), egui::Sense::click_and_drag()).1)
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
