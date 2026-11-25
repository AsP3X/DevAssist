//! The window's frame from the design: a glass sidebar, the top row with the title and the window's own
//! buttons, a screen's toolbar row, one frosted panel and the status bar. [`layout`] is the one place that gives
//! every rectangle; the glass panes are pictures the [`crate::baker`] makes from [`wants`] (to which a screen adds
//! its own before the bake); the panel is drawn from the baker's frosted sheet ([`crate::frost`]) by the screen
//! on it, or by [`placeholder`]; [`draw`] puts the text, icons and buttons of the frame over them all.

use devassist_core::update::VERSION;
use egui::{Align2, Color32, FontFamily, Painter, Pos2, Rect, Role, Sense, Ui, Vec2, ViewportCommand, WidgetInfo, pos2};

use crate::assets::{self, Icon};
use crate::baker::{Sheet, Want};
use crate::frost;
use crate::repos_view::Status;
use crate::theme::{
    self, BADGE_HEIGHT, BADGE_INSET, BADGE_PAD, BAR_GAP, BAR_HEIGHT, CAPTION_BUTTON, GROUP_GAP, HOVER_OPACITY, ICON, ICON_GAP, INSET, LABEL_GAP,
    LABEL_ROW, Mode, NAV_ROW, Palette, RADIUS_PANE, RADIUS_PANEL, RADIUS_ROW, RADIUS_SMALL, ROW_GAP, ROW_PADDING, SIDEBAR_PADDING, SIDEBAR_TOP,
    SIDEBAR_WIDTH, STATUS_GAP, STATUS_HEIGHT, STATUS_ICON, STATUS_ICON_GAP, STATUS_PADDING, Surface, TEXT, TEXT_SMALL, TEXT_TITLE, TITLE_ROOM,
    WORKSPACE_MARK,
};
use crate::widgets::{self, draw_icon, font};

/// The sidebar's pane.
const SIDEBAR: &str = "sidebar";
/// The round button that shows and hides the sidebar.
const SIDEBAR_TOGGLE: &str = "sidebar-toggle";
/// The status bar.
const STATUS: &str = "status";

/// The sentence the panel of a screen that is not built yet shows.
const PANEL_TEXT: &str = "The screens come next.";
/// What the status bar says when no screen has anything to say.
fn idle_status() -> String {
    format!("DevAssist {VERSION}")
}

/// A screen the sidebar goes to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    /// The start screen.
    Home,
    /// The repositories.
    #[default]
    Repositories,
    /// Docker.
    Docker,
    /// The tasks that wait.
    Queue,
    /// What the last update did.
    LastUpdate,
}

impl Page {
    /// Every page, in the sidebar's order.
    pub const ALL: [Page; 5] = [Page::Home, Page::Repositories, Page::Docker, Page::Queue, Page::LastUpdate];

    /// The page's name, in the sidebar and in the title.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Page::Home => "Home",
            Page::Repositories => "Repositories",
            Page::Docker => "Docker",
            Page::Queue => "Queue",
            Page::LastUpdate => "Last update",
        }
    }

    /// The page's icon in the sidebar.
    fn icon(self) -> Icon {
        match self {
            Page::Home => Icon::House,
            Page::Repositories => Icon::FolderGit2,
            Page::Docker => Icon::Container,
            Page::Queue => Icon::ListOrdered,
            Page::LastUpdate => Icon::ScrollText,
        }
    }
}

/// Every rectangle of the window's frame. One that is not shown is an empty rectangle (no width, no height) at
/// the place it would start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The sidebar's glass pane.
    pub sidebar: Rect,
    /// The round button that shows and hides the sidebar.
    pub toggle: Rect,
    /// The room of the title in the top row: its text starts at the left edge, centred on the height.
    pub title: Rect,
    /// The room in the top row between the title and the window's own buttons, for a screen's actions.
    pub actions: Rect,
    /// The window's own three buttons, at the right end of the top row.
    pub captions: Rect,
    /// The row of a screen's controls above its panel.
    pub toolbar: Rect,
    /// The frosted panel that holds a screen; its corners are [`RADIUS_PANEL`].
    pub panel: Rect,
    /// The status bar.
    pub status: Rect,
}

/// The rectangle from `x0`, `y0` to `x1`, `y1` kept inside a window of `size` with at least a point a side,
/// however small the window.
fn kept_inside(size: Vec2, [x0, y0, x1, y1]: [f32; 4]) -> Rect {
    let (w, h) = (size.x, size.y);
    let (x0, y0) = (x0.min(w - 1.0).max(0.0), y0.min(h - 1.0).max(0.0));
    Rect::from_min_max(pos2(x0, y0), pos2(x1.min(w).max(x0 + 1.0), y1.min(h).max(y0 + 1.0)))
}

/// An empty rectangle at `x`, `y`, kept inside a window of `size`.
fn nothing_at(size: Vec2, x: f32, y: f32) -> Rect {
    Rect::from_min_size(pos2(x.clamp(0.0, size.x.max(0.0)), y.clamp(0.0, size.y.max(0.0))), Vec2::ZERO)
}

/// Where the content starts from the window's left: past the sidebar when it is shown.
fn content_left(sidebar_shown: bool) -> f32 {
    if sidebar_shown { INSET + SIDEBAR_WIDTH + INSET } else { INSET }
}

/// The frame of a window of `size` points, the sidebar shown when `sidebar_shown`, with a screen's toolbar row
/// above the panel when `toolbar` (the panel then starts [`BAR_HEIGHT`] and [`BAR_GAP`] lower). Each shown
/// rectangle is kept inside the window with at least a point a side, however small the window.
#[must_use]
pub fn layout(size: Vec2, sidebar_shown: bool, toolbar: bool) -> Layout {
    let (w, h) = (size.x, size.y);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| kept_inside(size, [x0, y0, x1, y1]);
    let content = content_left(sidebar_shown);
    // Human: on a Mac the traffic lights sit at the window's top left, where the sidebar is when it is shown.
    let bar_left = if cfg!(target_os = "macos") && !sidebar_shown { content + theme::TRAFFIC_LIGHT_SPACE } else { content };
    let (top, bottom) = (INSET, INSET + BAR_HEIGHT);
    let status_top = h - INSET - STATUS_HEIGHT;

    let sidebar = if sidebar_shown { rect(INSET, INSET, INSET + SIDEBAR_WIDTH, h - INSET) } else { nothing_at(size, INSET, INSET) };
    let toggle = rect(bar_left, top, bar_left + BAR_HEIGHT, bottom);
    let captions = rect(w - INSET - 3.0 * CAPTION_BUTTON, top, w - INSET, bottom);
    // The title has its room unless the window's buttons come closer; the actions have what is left before them.
    let title_left = toggle.right() + BAR_GAP;
    let actions_right = (captions.left() - BAR_GAP).max(title_left);
    let title_right = (title_left + TITLE_ROOM).min(actions_right);
    let title = Rect::from_min_max(pos2(title_left, toggle.top()), pos2(title_right, toggle.bottom()));
    let actions = Rect::from_min_max(pos2(title_right, toggle.top()), pos2(actions_right, toggle.bottom()));

    let below = bottom + INSET;
    let (toolbar, panel_top) = if toolbar {
        (rect(content, below, w - INSET, below + BAR_HEIGHT), below + BAR_HEIGHT + BAR_GAP)
    } else {
        (nothing_at(size, content, below), below)
    };
    Layout {
        sidebar,
        toggle,
        title,
        actions,
        captions,
        toolbar,
        panel: rect(content, panel_top, w - INSET, status_top - INSET),
        status: rect(content, status_top, w - INSET, h - INSET),
    }
}

/// The glass panes the baker makes pictures of this frame: the shell's, then what a screen adds.
#[derive(Clone, Debug, Default)]
pub struct Wants(Vec<Want>);

impl Wants {
    /// Adds a glass pane.
    pub fn add(&mut self, want: Want) {
        self.0.push(want);
    }

    /// The panes, in the order they were added.
    #[must_use]
    pub fn as_slice(&self) -> &[Want] {
        &self.0
    }
}

/// The glass panes of the shell in `layout`: the sidebar when it is shown, the sidebar button and the status bar.
/// The frosted panel is not among them: it is no picture of its own.
#[must_use]
pub fn wants(layout: &Layout) -> Wants {
    let mut wants = Wants::default();
    if layout.sidebar.is_positive() {
        wants.add(Want { id: SIDEBAR, rect: layout.sidebar, radius: RADIUS_PANE, surface: Surface::Sidebar });
    }
    wants.add(Want { id: SIDEBAR_TOGGLE, rect: layout.toggle, radius: BAR_HEIGHT / 2.0, surface: Surface::Capsule });
    wants.add(Want { id: STATUS, rect: layout.status, radius: RADIUS_PANEL, surface: Surface::StatusBar });
    wants
}

/// The rows of the sidebar's two group labels, in the sidebar pane at `sidebar`: where [`draw`] puts their text.
#[must_use]
pub fn label_rows(sidebar: Rect) -> Vec<Rect> {
    placed_rows(sidebar).into_iter().filter(|(row, _)| matches!(row, Row::Label(_))).map(|(_, rect)| rect).collect()
}

/// The row of each page in the sidebar pane at `sidebar`, in the sidebar's order.
#[must_use]
pub fn page_rows(sidebar: Rect) -> Vec<(Page, Rect)> {
    placed_rows(sidebar)
        .into_iter()
        .filter_map(|(row, rect)| match row {
            Row::Nav(page) => Some((*page, rect)),
            Row::Label(_) | Row::Gap => None,
        })
        .collect()
}

/// The count a page's row shows at its end: the tasks waiting, on the queue's row, when there are any.
#[must_use]
pub fn badge(page: Page, waiting: usize) -> Option<String> {
    (page == Page::Queue && waiting > 0).then(|| waiting.to_string())
}

/// What the shell shows that is not its layout.
#[derive(Clone, Copy, Debug)]
pub struct Shown<'a> {
    /// Light or dark.
    pub mode: Mode,
    /// How many tasks wait in the queue.
    pub waiting: usize,
    /// What the status bar says; `None` for the app's name and version.
    pub status: Option<&'a Status>,
}

/// Draws the frame's text, icons and buttons over the pictures of [`wants`] for `layout`. A press on the sidebar
/// button flips `sidebar_shown`, a press on a page's row in the sidebar chooses `page`; the new layout is drawn from
/// the next frame on. The panel is the screen's to draw.
pub fn draw(ui: &mut Ui, layout: &Layout, sidebar_shown: &mut bool, page: &mut Page, shown: Shown<'_>) {
    let palette = theme::palette(shown.mode);
    let window = ui.ctx().content_rect();

    // The empty part of the top row moves the window, as the title bar it replaces did; drawn first, so the
    // buttons over it take their own presses.
    let bar = Rect::from_min_max(pos2(layout.toggle.left(), window.top()), pos2(window.right(), layout.toggle.bottom() + INSET));
    let drag = ui.interact(bar, ui.scope_id().with("window-drag"), Sense::CLICK | Sense::DRAG);
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    if layout.sidebar.is_positive()
        && let Some(chosen) = draw_sidebar(ui, layout.sidebar, *page, shown.waiting, palette)
    {
        *page = chosen;
        ui.ctx().request_repaint();
    }

    let painter = ui.painter().clone();
    let toggle = layout.toggle;
    if ui.interact(toggle, ui.scope_id().with(SIDEBAR_TOGGLE), Sense::CLICK).on_hover_text("Show or hide the sidebar").clicked() {
        *sidebar_shown = !*sidebar_shown;
        ui.ctx().request_repaint();
    }
    draw_icon(&painter, toggle.center(), Icon::PanelLeft, ICON, palette.on_glass);
    let title = layout.title;
    painter.with_clip_rect(title).text(
        pos2(title.left(), title.center().y),
        Align2::LEFT_CENTER,
        page.name(),
        font(TEXT_TITLE, assets::medium_family()),
        palette.on_glass,
    );
    if !cfg!(target_os = "macos") {
        caption_buttons(ui, layout.captions, palette);
    }

    draw_status(ui, layout.status, shown.status, palette);
}

/// Paints the frosted panel at `panel` from `sheet` with the sentence of a screen that is not built yet.
pub fn placeholder(ui: &Ui, panel: Rect, sheet: Option<&Sheet>, palette: Palette) {
    if let Some(sheet) = sheet {
        frost::paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size()), panel, RADIUS_PANEL, sheet);
    }
    ui.painter().with_clip_rect(panel).text(panel.center(), Align2::CENTER_CENTER, PANEL_TEXT, font(TEXT, FontFamily::Proportional), palette.quiet);
}

/// Draws the status bar's text at `status`: the screen's message at the left (in the danger colour when it is an
/// error), what is switched on at the right, each with its icon.
fn draw_status(ui: &Ui, status: Rect, shown: Option<&Status>, palette: Palette) {
    let painter = ui.painter().with_clip_rect(status);
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let y = status.center().y;
    let mut right = status.right() - STATUS_PADDING;
    for (icon, words) in shown.map_or(&[][..], |status| status.notes.as_slice()).iter().rev() {
        let text = painter.text(pos2(right, y), Align2::RIGHT_CENTER, words, small.clone(), palette.quiet);
        widgets::draw_icon(&painter, pos2(text.left() - STATUS_ICON_GAP - STATUS_ICON / 2.0, y), *icon, STATUS_ICON, palette.quiet);
        right = text.left() - STATUS_ICON_GAP - STATUS_ICON - STATUS_GAP;
    }
    let left = status.left() + STATUS_PADDING;
    let (text, colour) = match shown {
        Some(status) => (status.text.clone(), if status.error { palette.danger } else { palette.quiet }),
        None => (idle_status(), palette.quiet),
    };
    let text = widgets::ellipsis(&text, &small, right - left, ui);
    painter.text(pos2(left, y), Align2::LEFT_CENTER, text, small, colour);
}

/// A row of the sidebar.
enum Row {
    /// A page to go to.
    Nav(Page),
    /// The label over a group of rows.
    Label(&'static str),
    /// The gap before a group.
    Gap,
}

/// The rows from the sidebar's top, below the workspace.
const TOP_ROWS: [Row; 10] = [
    Row::Gap,
    Row::Nav(Page::Home),
    Row::Gap,
    Row::Label("Tools"),
    Row::Nav(Page::Repositories),
    Row::Nav(Page::Docker),
    Row::Gap,
    Row::Label("Running"),
    Row::Nav(Page::Queue),
    Row::Nav(Page::LastUpdate),
];

/// The rows at the sidebar's foot, from the top. They do nothing yet, so they take no press.
const FOOT_ROWS: [(Icon, &str); 3] = [(Icon::Search, "Go to a tool"), (Icon::Settings, "Settings"), (Icon::CircleHelp, "Help")];

/// A row of the sidebar's top part (not its gaps) with the rectangle it fills, in the pane at `sidebar`.
fn placed_rows(sidebar: Rect) -> Vec<(&'static Row, Rect)> {
    let mut placed = Vec::new();
    let mut top = sidebar.top() + SIDEBAR_TOP + WORKSPACE_MARK;
    let mut after_nav = false;
    for row in &TOP_ROWS {
        match row {
            Row::Gap => top += GROUP_GAP,
            Row::Label(_) => {
                placed.push((row, row_at(sidebar, top, LABEL_ROW)));
                top += LABEL_ROW + LABEL_GAP;
            }
            Row::Nav(..) => {
                if after_nav {
                    top += ROW_GAP;
                }
                placed.push((row, row_at(sidebar, top, NAV_ROW)));
                top += NAV_ROW;
            }
        }
        after_nav = matches!(row, Row::Nav(..));
    }
    placed
}

/// The row of `height` at `top` across the sidebar pane at `sidebar`, inside its padding.
fn row_at(sidebar: Rect, top: f32, height: f32) -> Rect {
    Rect::from_min_max(pos2(sidebar.left() + SIDEBAR_PADDING, top), pos2(sidebar.right() - SIDEBAR_PADDING, top + height))
}

/// Draws the sidebar's rows into the pane at `sidebar`, `page` chosen and `waiting` tasks in the queue; gives
/// back the page whose row was pressed.
fn draw_sidebar(ui: &Ui, sidebar: Rect, page: Page, waiting: usize, palette: Palette) -> Option<Page> {
    let painter = ui.painter().with_clip_rect(sidebar);
    draw_workspace(&painter, row_at(sidebar, sidebar.top() + SIDEBAR_TOP, WORKSPACE_MARK), palette);
    let mut pressed = None;
    for (row, rect) in placed_rows(sidebar) {
        match *row {
            Row::Label(text) => {
                let small = font(TEXT_SMALL, FontFamily::Proportional);
                painter.text(pos2(rect.left() + ROW_PADDING, rect.center().y), Align2::LEFT_CENTER, text, small, palette.quiet);
            }
            Row::Nav(target) => {
                let response = ui.interact(rect.intersect(sidebar), ui.scope_id().with(("page", target.name())), Sense::CLICK);
                response.widget_info(|| WidgetInfo::selected(Role::Button, true, target == page, target.name()));
                let fill = if target == page {
                    palette.selection
                } else if response.hovered() {
                    palette.on_glass.gamma_multiply(HOVER_OPACITY)
                } else {
                    Color32::TRANSPARENT
                };
                draw_nav(&painter, rect, target.icon(), target.name(), fill, palette);
                if let Some(count) = badge(target, waiting) {
                    draw_badge(&painter, rect, count, palette);
                }
                if response.clicked() {
                    pressed = Some(target);
                }
            }
            Row::Gap => {}
        }
    }

    let mut bottom = sidebar.bottom() - SIDEBAR_PADDING;
    for (icon, text) in FOOT_ROWS.into_iter().rev() {
        draw_nav(&painter, row_at(sidebar, bottom - NAV_ROW, NAV_ROW), icon, text, Color32::TRANSPARENT, palette);
        bottom -= NAV_ROW + ROW_GAP;
    }
    pressed
}

/// Draws the workspace row: the wrench on an accent square, the app's name and its version.
fn draw_workspace(painter: &Painter, row: Rect, palette: Palette) {
    let mark = Rect::from_min_size(pos2(row.left() + ROW_PADDING, row.top()), Vec2::splat(WORKSPACE_MARK));
    painter.rect_filled(mark, RADIUS_SMALL, palette.accent);
    draw_icon(painter, mark.center(), Icon::Wrench, ICON, palette.on_dark_glass);
    let name = painter.layout_no_wrap("DevAssist".to_owned(), font(TEXT, assets::medium_family()), palette.on_glass);
    let version = painter.layout_no_wrap(format!("Version {VERSION}"), font(TEXT_SMALL, FontFamily::Proportional), palette.quiet);
    let x = mark.right() + ICON_GAP;
    let top = mark.center().y - f32::midpoint(name.size().y, version.size().y);
    let below = top + name.size().y;
    painter.galley(pos2(x, top), name, palette.on_glass);
    painter.galley(pos2(x, below), version, palette.quiet);
}

/// Draws a row of the sidebar's navigation at `row` on `fill`.
fn draw_nav(painter: &Painter, row: Rect, icon: Icon, text: &str, fill: Color32, palette: Palette) {
    painter.rect_filled(row, RADIUS_ROW, fill);
    let icon_center = pos2(row.left() + ROW_PADDING + ICON / 2.0, row.center().y);
    draw_icon(painter, icon_center, icon, ICON, palette.on_glass);
    painter.text(
        pos2(icon_center.x + ICON / 2.0 + ICON_GAP, row.center().y),
        Align2::LEFT_CENTER,
        text,
        font(TEXT, assets::medium_family()),
        palette.on_glass,
    );
}

/// Draws the badge with `count` at the right end of the sidebar row at `row`.
fn draw_badge(painter: &Painter, row: Rect, count: String, palette: Palette) {
    let small = font(TEXT_SMALL, assets::medium_family());
    let galley = painter.layout_no_wrap(count, small, palette.on_glass);
    let right = row.right() - BADGE_INSET;
    let badge = Rect::from_min_max(
        pos2(right - galley.size().x - 2.0 * BADGE_PAD, row.center().y - BADGE_HEIGHT / 2.0),
        pos2(right, row.center().y + BADGE_HEIGHT / 2.0),
    );
    painter.rect_filled(badge, RADIUS_SMALL, palette.selection);
    painter.galley(badge.center() - galley.size() / 2.0, galley, palette.on_glass);
}

/// The window's own three buttons in `captions`, from its right end (close, maximise or restore, minimise), where
/// the window has no system frame.
fn caption_buttons(ui: &Ui, captions: Rect, palette: Palette) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let buttons = [
        (Icon::X, "Close", ViewportCommand::Close),
        (Icon::Square, if maximized { "Restore" } else { "Maximise" }, ViewportCommand::Maximized(!maximized)),
        (Icon::Minus, "Minimise", ViewportCommand::Minimized(true)),
    ];
    let mut right = captions.right();
    for (icon, hint, command) in buttons {
        let rect = Rect::from_min_max(pos2(right - CAPTION_BUTTON, captions.top()), pos2(right, captions.bottom()));
        right = rect.left();
        let response = ui.interact(rect, ui.scope_id().with(hint), Sense::CLICK).on_hover_text(hint);
        let close = icon == Icon::X;
        let (fill, mark) = match (response.hovered(), close) {
            (true, true) => (palette.danger, palette.on_dark_glass),
            (true, false) => (palette.selection, palette.on_glass),
            (false, _) => (Color32::TRANSPARENT, palette.on_glass),
        };
        ui.painter().rect_filled(rect, RADIUS_ROW, fill);
        draw_icon(ui.painter(), rect.center(), icon, ICON, mark);
        if response.clicked() {
            ctx.send_viewport_cmd(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_layout_is_the_designs_at_1440_by_900() {
        let l = layout(egui::vec2(1440.0, 900.0), true, true); // sidebar shown, with a toolbar
        assert_eq!(l.sidebar, egui::Rect::from_min_size(egui::pos2(12.0, 12.0), egui::vec2(256.0, 876.0)));
        assert_eq!(l.toolbar, egui::Rect::from_min_size(egui::pos2(280.0, 60.0), egui::vec2(1148.0, 36.0)));
        assert_eq!(l.panel.min, egui::pos2(280.0, 108.0));
        assert_eq!(l.panel.max, egui::pos2(1428.0, 844.0));
        assert_eq!(l.captions.max.x, 1428.0);
        assert_eq!(l.captions.width(), 3.0 * theme::CAPTION_BUTTON);
        assert!(l.actions.max.x <= l.captions.min.x - theme::BAR_GAP);
        assert!(l.title.max.x <= l.actions.min.x);
    }

    #[test]
    fn nothing_in_the_layout_overlaps_at_any_size() {
        for width in (960..=2000).step_by(40) {
            for height in (600..=1200).step_by(60) {
                #[expect(clippy::cast_precision_loss, reason = "window sizes of a few thousand points")]
                let size = egui::vec2(width as f32, height as f32);
                let window = egui::Rect::from_min_size(Pos2::ZERO, size);
                for sidebar in [true, false] {
                    for toolbar in [true, false] {
                        let l = layout(size, sidebar, toolbar);
                        let named = [
                            ("sidebar", l.sidebar),
                            ("toggle", l.toggle),
                            ("title", l.title),
                            ("actions", l.actions),
                            ("captions", l.captions),
                            ("toolbar", l.toolbar),
                            ("panel", l.panel),
                            ("status", l.status),
                        ];
                        for (name, rect) in named {
                            assert!(rect.width() >= 0.0 && rect.height() >= 0.0, "{name} {rect:?} at {size:?}");
                            assert!(window.contains_rect(rect), "{name} {rect:?} leaves the window {size:?}");
                        }
                        for (at, (a, first)) in named.iter().enumerate() {
                            for (b, second) in &named[at + 1..] {
                                assert!(!first.intersect(*second).is_positive(), "{a} {first:?} and {b} {second:?} overlap at {size:?}");
                            }
                        }
                        assert_eq!(l.sidebar.is_positive(), sidebar);
                        assert_eq!(l.toolbar.is_positive(), toolbar);
                        assert!(l.toggle.is_positive());
                    }
                }
            }
        }
    }

    #[test]
    fn a_toolbar_moves_the_panel_down_by_its_row() {
        let size = egui::vec2(1440.0, 900.0);
        let (with, without) = (layout(size, true, true), layout(size, true, false));
        assert_eq!(with.panel.top() - without.panel.top(), BAR_HEIGHT + BAR_GAP);
        assert_eq!(with.panel.bottom(), without.panel.bottom());
        assert_eq!(without.panel, Rect::from_min_size(pos2(280.0, 60.0), egui::vec2(1148.0, 784.0)));
    }

    #[test]
    fn each_page_has_its_row_and_its_name() {
        let names: Vec<&str> = Page::ALL.iter().map(|page| page.name()).collect();
        assert_eq!(names, ["Home", "Repositories", "Docker", "Queue", "Last update"]);
        let sidebar = layout(egui::vec2(1440.0, 900.0), true, false).sidebar;
        let rows = page_rows(sidebar);
        assert_eq!(rows.len(), Page::ALL.len());
        for (page, rect) in &rows {
            assert!(sidebar.contains_rect(*rect), "{page:?}");
        }
        assert_eq!(rows.iter().find(|(page, _)| *page == Page::Repositories).map(|(_, rect)| rect.top()), Some(164.0));
    }

    #[test]
    fn the_queue_badge_is_there_only_when_something_waits() {
        assert_eq!(badge(Page::Queue, 0), None);
        assert_eq!(badge(Page::Queue, 2), Some("2".to_owned()));
        assert_eq!(badge(Page::Repositories, 2), None);
    }

    #[test]
    fn the_shell_wants_the_designs_panes() {
        let l = layout(egui::vec2(1440.0, 900.0), true, false);
        let wants = wants(&l);
        let by = |id: &str| wants.as_slice().iter().find(|w| w.id == id).unwrap();
        assert_eq!(by("sidebar").rect, egui::Rect::from_min_size(egui::pos2(12.0, 12.0), egui::vec2(256.0, 876.0)));
        assert_eq!((by("sidebar").radius, by("sidebar").surface), (20.0, Surface::Sidebar));
        assert_eq!(by("status").rect, egui::Rect::from_min_size(egui::pos2(280.0, 856.0), egui::vec2(1148.0, 32.0)));
        assert_eq!(by("sidebar-toggle").rect, egui::Rect::from_min_size(egui::pos2(280.0, 12.0), egui::vec2(36.0, 36.0)));
        assert_eq!((l.panel.min, l.panel.max), (egui::pos2(280.0, 60.0), egui::pos2(1428.0, 844.0)));
        assert_eq!((by("status").rect, by("sidebar").rect, by("sidebar-toggle").rect), (l.status, l.sidebar, l.toggle));
    }

    #[test]
    fn the_shell_wants_its_glass_panes_only() {
        for shown in [true, false] {
            let wants = wants(&layout(egui::vec2(1440.0, 900.0), shown, true));
            assert!(wants.as_slice().iter().all(|w| w.surface.is_glass()), "{wants:?}");
            assert!(wants.as_slice().iter().all(|w| w.id != "panel"));
            assert_eq!(wants.as_slice().len(), if shown { 3 } else { 2 });
        }
    }

    #[test]
    fn without_the_sidebar_the_content_takes_its_room() {
        let size = egui::vec2(1440.0, 900.0);
        let wants = wants(&layout(size, false, false));
        assert!(wants.as_slice().iter().all(|w| w.id != "sidebar"));
        assert_eq!(layout(size, false, false).panel.min.x, 12.0);
        assert_eq!(layout(size, false, true).toolbar.min.x, 12.0);
        assert_eq!(layout(size, false, false).panel.max, layout(size, true, false).panel.max);
    }

    #[test]
    fn a_tiny_window_gives_no_pane_with_negative_size() {
        for size in [egui::vec2(0.0, 0.0), egui::vec2(100.0, 40.0)] {
            for shown in [true, false] {
                for toolbar in [true, false] {
                    let l = layout(size, shown, toolbar);
                    assert!(wants(&l).as_slice().iter().all(|w| w.rect.width() > 0.0 && w.rect.height() > 0.0));
                    assert!(l.panel.width() > 0.0 && l.panel.height() > 0.0, "{:?} in {size:?}", l.panel);
                    assert!(l.title.width() >= 0.0 && l.actions.width() >= 0.0 && l.toolbar.width() >= 0.0);
                }
            }
        }
    }
}
