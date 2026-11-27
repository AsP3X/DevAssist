//! The window: opened as Quill opens its own, and at each frame the glass baked and the shell drawn over it.

use std::time::Instant;

use devassist_core::config::{self, Settings};
use devassist_glass::gpu;
use egui::{Color32, LayerId, Mesh};

use crate::assets;
use crate::baker::{Baker, Scene, Sheet};
use crate::demo;
use crate::dialogs::{self, Dialogs};
use crate::forks_screen;
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
    notice.filter(|_| elapsed < NOTICE_TIME).map(|text| Status { text: text.to_owned(), error: true, ..Status::default() })
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
    /// The page the sidebar has chosen and what its screens keep.
    pages: Pages,
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
            baker: Baker::new(),
            visuals: None,
            shot: launch.screenshot.clone().map(screenshot::Job::new),
            start: Instant::now(),
            session: Session::start(settings, launch),
            pages: Pages { sidebar_shown: !launch.sidebar_hidden, ..Pages::default() },
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
        self.pages.page = dialogs::page_for(self.session.repos(), self.pages.page);
        let painter = ctx.layer_painter(LayerId::background());
        // The screen's glass is placed before the bake, so it is baked where the screen draws over it.
        let planned = self.pages.plan(&painter, size, &self.session);
        // The dialog or the menu the core's mode asks for, on any screen: opened now when the core's workers asked for
        // it. The menu stands under the row's button the screen placed in the last frame.
        let dialog = dialogs::before(&ctx, self.session.repos(), self.pages.menu_anchor(), &mut self.dialogs);
        let mut wants = planned.wants();
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
        egui::CentralPanel::default().frame(egui::Frame::new()).show(ui, |ui| {
            self.pages.draw(ui, &planned, &mut self.session, sheet.as_ref(), mode, notice);
        });
        // Over everything but the window's own top row: the dialog, also one the screen asked for in this frame.
        dialogs::after(&ctx, dialog, self.session.repos_mut(), &drawn, mode, &mut self.dialogs);
        if self.folder_focus && dialogs::focus_add_folder(&ctx, self.session.repos()) {
            self.folder_focus = false;
        }
    }
}

/// The page the sidebar has chosen and what its screens keep between frames: the Repositories screen, or the forks
/// of a repository in its place while the core has them open.
pub(crate) struct Pages {
    /// The page the sidebar has chosen.
    pub(crate) page: Page,
    /// Whether the sidebar is shown.
    pub(crate) sidebar_shown: bool,
    /// What the Repositories screen keeps between frames.
    screen: Screen,
    /// What the forks screen keeps between frames.
    forks: forks_screen::Screen,
}

impl Default for Pages {
    /// As the window starts: on the Repositories page, the sidebar shown.
    fn default() -> Pages {
        Pages { page: Page::default(), sidebar_shown: true, screen: Screen::default(), forks: forks_screen::Screen::default() }
    }
}

/// What a frame's screen draws, decided before the bake: the layout, the Repositories screen's controls or the
/// forks' plan (never both), and the shell's title and status words for the forks.
pub(crate) struct Planned {
    layout: shell::Layout,
    plan: Option<repos_screen::Plan>,
    forks: Option<forks_screen::Plan>,
    title: Option<String>,
    on_repositories: bool,
    status: Option<Status>,
}

impl Planned {
    /// The glass panes of the shell and the screen.
    pub(crate) fn wants(&self) -> shell::Wants {
        let mut wants = shell::wants(&self.layout);
        if let Some(plan) = &self.plan {
            plan.add_wants(&mut wants);
        }
        if let Some(forks) = &self.forks {
            forks.add_wants(&mut wants);
        }
        wants
    }

    /// The forks' plan, while they show.
    #[cfg(test)]
    pub(crate) fn forks(&self) -> Option<&forks_screen::Plan> {
        self.forks.as_ref()
    }
}

impl Pages {
    /// What the chosen page draws in a window of `size`, measured with `painter`'s faces: the forks of a repository
    /// while the core has them open and the page is Repositories, else the Repositories screen there.
    pub(crate) fn plan(&self, painter: &egui::Painter, size: egui::Vec2, session: &Session) -> Planned {
        let on_repositories = self.page == Page::Repositories;
        let view = session.repos().and_then(|repos| repos.forks.as_ref()).filter(|_| on_repositories);
        // The forks have their toolbar always; the table when there are repositories to filter, sort and tick.
        let toolbar = view.is_some() || (on_repositories && repos_screen::has_toolbar(session));
        let layout = shell::layout(size, self.sidebar_shown, toolbar);
        let plan = if on_repositories { repos_screen::plan(painter, &layout, session) } else { None };
        let repos = view.and(session.repos());
        let forks = view.map(|view| forks_screen::plan(view, &layout, painter));
        Planned {
            plan,
            // As the forks' plan shows it: cut when the top row's controls need the room.
            title: forks.as_ref().map(|forks| forks.title().to_owned()),
            forks,
            status: repos.and_then(forks_screen::status),
            layout,
            on_repositories,
        }
    }

    /// Where the menu stands under: the "…" of the row the Repositories screen opened it on, in the last frame.
    pub(crate) fn menu_anchor(&self) -> Option<egui::Rect> {
        repos_screen::menu_anchor(&self.screen)
    }

    /// What the forks screen keeps, for the tests.
    #[cfg(test)]
    pub(crate) fn forks_screen(&self) -> &forks_screen::Screen {
        &self.forks
    }

    /// Draws the shell and the screen `planned` (this frame's [`Pages::plan`]) on `ui` and does what their presses
    /// ask of `session`; `notice` stands in the status bar over the screen's words while it is given.
    pub(crate) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        planned: &Planned,
        session: &mut Session,
        sheet: Option<&Sheet>,
        mode: Mode,
        notice: Option<Status>,
    ) {
        let ctx = ui.ctx().clone();
        let status = notice.or_else(|| planned.status.clone()).or_else(|| repos_screen::status(session));
        let waiting = session.repos().map_or(0, |repos| repos.queue.len());
        let shown = Shown { mode, waiting, status: status.as_ref(), back: planned.title.as_deref() };
        let back = shell::draw(ui, &planned.layout, &mut self.sidebar_shown, &mut self.page, shown);
        // Under an open overlay the screen has no keyboard and takes no press.
        let keys = !overlay::is_open(&ctx);
        if let Some(forks_plan) = &planned.forks {
            // The table's filter does not keep the keyboard under the forks, nor get it back after them.
            repos_screen::leave(&ctx, &mut self.screen);
            let asked = forks_screen::show(ui, &planned.layout, forks_plan, sheet, mode, &mut self.forks, keys);
            if let Some(repos) = session.repos_mut() {
                forks_screen::apply_all(if back { vec![forks_screen::Command::Back] } else { asked }, repos);
            }
        } else if planned.on_repositories {
            repos_screen::show(ui, &planned.layout, planned.plan.as_ref(), session, sheet, mode, &mut self.screen, keys);
        } else {
            shell::placeholder(ui, planned.layout.panel, sheet, theme::palette(mode));
        }
        // Closed, the forks leave nothing behind: the next forks opened start at the top, their highlight followed, and
        // their filter has no keyboard.
        if !session.repos().is_some_and(forks_screen::shown) {
            self.forks = forks_screen::Screen::default();
            forks_screen::leave(&ctx);
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

    /// Which screens one frame of the window's pages drew: the forks table's head ("Fork"), the Repositories table's
    /// head ("Repository"), the placeholder of a page not built yet.
    fn screens_drawn(ctx: &egui::Context, pages: &mut Pages, session: &mut Session, events: Vec<egui::Event>) -> [bool; 3] {
        let size = egui::vec2(1440.0, 900.0);
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        let sheet =
            Sheet { texture: egui::TextureId::User(0), uv: egui::Rect::from_min_max(egui::Pos2::ZERO, pos2(1.0, 1.0)), edge: egui::Stroke::NONE };
        let output = ctx.run_ui(input, |ui| {
            let planned = pages.plan(ui.painter(), size, session);
            pages.draw(ui, &planned, session, Some(&sheet), Mode::Light, None);
        });
        let said: Vec<String> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        ["Fork", "Repository", "The screens come next."].map(|words| said.iter().any(|said| said == words))
    }

    /// A click at `at`, over three frames.
    fn clicked(at: egui::Pos2) -> [Vec<egui::Event>; 3] {
        let button =
            |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        [vec![egui::Event::PointerMoved(at)], vec![button(true)], vec![button(false)]]
    }

    #[test]
    fn one_screen_a_frame_as_the_forks_open_close_and_the_page_changes() {
        const FORKS: [bool; 3] = [true, false, false];
        const TABLE: [bool; 3] = [false, true, false];
        const PLACEHOLDER: [bool; 3] = [false, false, true];
        let ctx = crate::widgets::tests::context();
        let mut repos = demo::repos();
        demo::show(&mut repos, "forks").unwrap();
        let mut session = Session::demo(repos);
        let mut pages = Pages { sidebar_shown: true, ..Pages::default() };
        let layout = shell::layout(egui::vec2(1440.0, 900.0), true, false);
        assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, Vec::new()), FORKS);
        // The press on back: its frame still draws the forks, the next the table alone.
        for events in clicked(layout.toggle.center()) {
            assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, events), FORKS);
        }
        assert!(session.repos().unwrap().forks.is_none(), "back closed nothing");
        assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, Vec::new()), TABLE);
        // Open again, then away to Docker and back: the forks, the placeholder, the forks.
        demo::show(session.repos_mut().unwrap(), "forks").unwrap();
        assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, Vec::new()), FORKS);
        let row = |page: Page| shell::page_rows(layout.sidebar).into_iter().find(|(at, _)| *at == page).unwrap().1.center();
        for events in clicked(row(Page::Docker)) {
            screens_drawn(&ctx, &mut pages, &mut session, events);
        }
        assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, Vec::new()), PLACEHOLDER);
        for events in clicked(row(Page::Repositories)) {
            screens_drawn(&ctx, &mut pages, &mut session, events);
        }
        assert_eq!(screens_drawn(&ctx, &mut pages, &mut session, Vec::new()), FORKS, "the forks are still open in the core");
        assert!(pages.sidebar_shown, "the sidebar kept its state");
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
