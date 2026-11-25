//! The window's frame from the design: a glass sidebar, the top row with the title and the window's own
//! buttons, one frosted panel and the status bar. The glass panes are pictures the [`crate::baker`] makes from
//! [`wants`]; the panel, at [`panel_rect`], is drawn from the baker's frosted sheet ([`crate::frost`]); [`draw`]
//! paints the panel and puts the text, icons and buttons over them all.

use devassist_core::update::VERSION;
use egui::{Align2, Color32, FontFamily, FontId, Painter, Pos2, Rect, Sense, Ui, Vec2, ViewportCommand, pos2};

use crate::assets::{self, Icon};
use crate::baker::{Sheet, Want};
use crate::frost;
use crate::theme::{
    self, BAR_GAP, BAR_HEIGHT, CAPTION_BUTTON, GROUP_GAP, ICON, ICON_GAP, INSET, LABEL_GAP, LABEL_ROW, Mode, NAV_ROW, Palette, RADIUS_PANE,
    RADIUS_PANEL, RADIUS_ROW, RADIUS_SMALL, ROW_GAP, ROW_PADDING, SIDEBAR_PADDING, SIDEBAR_TOP, SIDEBAR_WIDTH, STATUS_HEIGHT, STATUS_PADDING,
    Surface, TEXT, TEXT_SMALL, TEXT_TITLE, TITLE_ROOM, WORKSPACE_MARK,
};

/// The sidebar's pane.
const SIDEBAR: &str = "sidebar";
/// The round button that shows and hides the sidebar.
const SIDEBAR_TOGGLE: &str = "sidebar-toggle";
/// The status bar.
const STATUS: &str = "status";

/// The sentence the empty panel shows.
const PANEL_TEXT: &str = "The screens come next.";
/// The screen the title names and the sidebar shows as chosen.
const SCREEN: &str = "Repositories";

/// The rectangle from `x0`, `y0` to `x1`, `y1` kept inside a window of `size` with at least a point a side,
/// however small the window.
fn kept_inside(size: Vec2, [x0, y0, x1, y1]: [f32; 4]) -> Rect {
    let (w, h) = (size.x, size.y);
    let (x0, y0) = (x0.min(w - 1.0).max(0.0), y0.min(h - 1.0).max(0.0));
    Rect::from_min_max(pos2(x0, y0), pos2(x1.min(w).max(x0 + 1.0), y1.min(h).max(y0 + 1.0)))
}

/// Where the content starts from the window's left: past the sidebar when it is shown.
fn content_left(sidebar_shown: bool) -> f32 {
    if sidebar_shown { INSET + SIDEBAR_WIDTH + INSET } else { INSET }
}

/// The top of the status bar in a window of `size`.
fn status_top(size: Vec2) -> f32 {
    size.y - INSET - STATUS_HEIGHT
}

/// The glass panes of the shell in a window of `size` points, the sidebar among them when `sidebar_shown`. Each
/// is kept inside the window with at least a point a side, however small the window. The frosted panel is not
/// among them: it is no picture of its own ([`panel_rect`]).
#[must_use]
pub fn wants(size: Vec2, sidebar_shown: bool) -> Vec<Want> {
    let (w, h) = (size.x, size.y);
    let rect = |x0: f32, y0: f32, x1: f32, y1: f32| kept_inside(size, [x0, y0, x1, y1]);
    let content = content_left(sidebar_shown);
    // Human: on a Mac the traffic lights sit at the window's top left, where the sidebar is when it is shown.
    let bar_left = if cfg!(target_os = "macos") && !sidebar_shown { content + theme::TRAFFIC_LIGHT_SPACE } else { content };
    let status_top = status_top(size);
    let mut wants = Vec::new();
    if sidebar_shown {
        wants.push(Want { id: SIDEBAR, rect: rect(INSET, INSET, INSET + SIDEBAR_WIDTH, h - INSET), radius: RADIUS_PANE, surface: Surface::Sidebar });
    }
    wants.extend([
        Want {
            id: SIDEBAR_TOGGLE,
            rect: rect(bar_left, INSET, bar_left + BAR_HEIGHT, INSET + BAR_HEIGHT),
            radius: BAR_HEIGHT / 2.0,
            surface: Surface::Capsule,
        },
        Want { id: STATUS, rect: rect(content, status_top, w - INSET, h - INSET), radius: RADIUS_PANEL, surface: Surface::StatusBar },
    ]);
    wants
}

/// The frosted panel that holds a screen, in a window of `size` points: below the top row, above the status bar,
/// past the sidebar when `sidebar_shown`. Kept inside the window with at least a point a side. Its corners are
/// [`RADIUS_PANEL`].
#[must_use]
pub fn panel_rect(size: Vec2, sidebar_shown: bool) -> Rect {
    kept_inside(size, [content_left(sidebar_shown), INSET + BAR_HEIGHT + INSET, size.x - INSET, status_top(size) - INSET])
}

/// The room of the title in the top row whose sidebar button is at `toggle`: the title's text starts at its left
/// edge, centred on its height, and lies within it.
#[must_use]
pub fn title_rect(toggle: Rect) -> Rect {
    let left = toggle.right() + BAR_GAP;
    Rect::from_min_max(pos2(left, toggle.top()), pos2(left + TITLE_ROOM, toggle.bottom()))
}

/// The rows of the sidebar's two group labels, in the sidebar pane at `sidebar`: where [`draw`] puts their text.
#[must_use]
pub fn label_rows(sidebar: Rect) -> Vec<Rect> {
    placed_rows(sidebar).into_iter().filter(|(row, _)| matches!(row, Row::Label(_))).map(|(_, rect)| rect).collect()
}

/// What the shell shows that is not its layout.
#[derive(Clone, Copy, Debug)]
pub struct Shown<'a> {
    /// Light or dark.
    pub mode: Mode,
    /// The backdrop's key, named in the status bar.
    pub backdrop: &'a str,
    /// The frosted panel's sheet, from the baker; `None` before it has one, when the panel is not painted.
    pub sheet: Option<Sheet>,
}

/// Paints the frosted panel and draws the shell's text, icons and buttons over it and over the pictures of
/// `wants` (from [`wants`] for this frame and `sidebar_shown`). A press on the sidebar button flips
/// `sidebar_shown`; the new layout is drawn from the next frame on.
pub fn draw(ui: &mut Ui, wants: &[Want], sidebar_shown: &mut bool, shown: Shown<'_>) {
    let palette = theme::palette(shown.mode);
    let at = |id: &str| wants.iter().find(|want| want.id == id).map(|want| want.rect);
    let (Some(toggle), Some(status)) = (at(SIDEBAR_TOGGLE), at(STATUS)) else { return };
    let window = ui.ctx().content_rect();
    let panel = panel_rect(window.size(), *sidebar_shown);
    // Painted first, so everything else lies over it; over the window as the baker's wallpaper lies.
    if let Some(sheet) = &shown.sheet {
        frost::paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, window.size()), panel, RADIUS_PANEL, sheet);
    }

    // The empty part of the top row moves the window, as the title bar it replaces did; drawn first, so the
    // buttons over it take their own presses.
    let bar = Rect::from_min_max(pos2(toggle.left(), window.top()), pos2(window.right(), panel.top()));
    let drag = ui.interact(bar, ui.scope_id().with("window-drag"), Sense::click_and_drag());
    if drag.drag_started() {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
    if drag.double_clicked() {
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!maximized));
    }

    if let Some(sidebar) = at(SIDEBAR) {
        draw_sidebar(&ui.painter().with_clip_rect(sidebar), sidebar, palette);
    }

    let painter = ui.painter().clone();
    if ui.interact(toggle, ui.scope_id().with(SIDEBAR_TOGGLE), Sense::click()).on_hover_text("Show or hide the sidebar").clicked() {
        *sidebar_shown = !*sidebar_shown;
        ui.ctx().request_repaint();
    }
    draw_icon(&painter, toggle.center(), Icon::PanelLeft, palette.on_glass);
    let title = title_rect(toggle);
    painter.text(pos2(title.left(), title.center().y), Align2::LEFT_CENTER, SCREEN, font(TEXT_TITLE, assets::medium_family()), palette.on_glass);
    if !cfg!(target_os = "macos") {
        caption_buttons(ui, Rect::from_min_max(pos2(toggle.left(), toggle.top()), pos2(panel.right(), toggle.bottom())), palette);
    }

    painter.with_clip_rect(panel).text(panel.center(), Align2::CENTER_CENTER, PANEL_TEXT, font(TEXT, FontFamily::Proportional), palette.quiet);

    let status_painter = painter.with_clip_rect(status);
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let mode = match shown.mode {
        Mode::Light => "light",
        Mode::Dark => "dark",
    };
    status_painter.text(
        pos2(status.left() + STATUS_PADDING, status.center().y),
        Align2::LEFT_CENTER,
        format!("DevAssist {VERSION}"),
        small.clone(),
        palette.quiet,
    );
    status_painter.text(
        pos2(status.right() - STATUS_PADDING, status.center().y),
        Align2::RIGHT_CENTER,
        format!("{} · {mode}", shown.backdrop),
        small,
        palette.quiet,
    );
}

/// A face at `size` points.
fn font(size: f32, family: FontFamily) -> FontId {
    FontId::new(size, family)
}

/// Draws the icon `name` centred on `center`.
fn draw_icon(painter: &Painter, center: Pos2, name: Icon, colour: Color32) {
    painter.text(center, Align2::CENTER_CENTER, assets::icon(name).to_string(), font(ICON, assets::icons_family()), colour);
}

/// A row of the sidebar.
enum Row {
    /// A screen to go to, with its icon; `true` for the chosen one.
    Nav(Icon, &'static str, bool),
    /// The label over a group of rows.
    Label(&'static str),
    /// The gap before a group.
    Gap,
}

/// The rows from the sidebar's top, below the workspace.
const TOP_ROWS: [Row; 10] = [
    Row::Gap,
    Row::Nav(Icon::House, "Home", false),
    Row::Gap,
    Row::Label("Tools"),
    Row::Nav(Icon::FolderGit2, SCREEN, true),
    Row::Nav(Icon::Container, "Docker", false),
    Row::Gap,
    Row::Label("Running"),
    Row::Nav(Icon::ListOrdered, "Queue", false),
    Row::Nav(Icon::ScrollText, "Last update", false),
];

/// The rows at the sidebar's foot, from the top.
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

/// Draws the sidebar's rows into the pane at `sidebar`. A click on a row does nothing yet.
fn draw_sidebar(painter: &Painter, sidebar: Rect, palette: Palette) {
    draw_workspace(painter, row_at(sidebar, sidebar.top() + SIDEBAR_TOP, WORKSPACE_MARK), palette);
    for (row, rect) in placed_rows(sidebar) {
        match *row {
            Row::Label(text) => {
                let small = font(TEXT_SMALL, FontFamily::Proportional);
                painter.text(pos2(rect.left() + ROW_PADDING, rect.center().y), Align2::LEFT_CENTER, text, small, palette.quiet);
            }
            Row::Nav(icon, text, chosen) => draw_nav(painter, rect, icon, text, chosen, palette),
            Row::Gap => {}
        }
    }

    let mut bottom = sidebar.bottom() - SIDEBAR_PADDING;
    for (icon, text) in FOOT_ROWS.into_iter().rev() {
        draw_nav(painter, row_at(sidebar, bottom - NAV_ROW, NAV_ROW), icon, text, false, palette);
        bottom -= NAV_ROW + ROW_GAP;
    }
}

/// Draws the workspace row: the wrench on an accent square, the app's name and its version.
fn draw_workspace(painter: &Painter, row: Rect, palette: Palette) {
    let mark = Rect::from_min_size(pos2(row.left() + ROW_PADDING, row.top()), Vec2::splat(WORKSPACE_MARK));
    painter.rect_filled(mark, RADIUS_SMALL, palette.accent);
    draw_icon(painter, mark.center(), Icon::Wrench, palette.on_dark_glass);
    let name = painter.layout_no_wrap("DevAssist".to_owned(), font(TEXT, assets::medium_family()), palette.on_glass);
    let version = painter.layout_no_wrap(format!("Version {VERSION}"), font(TEXT_SMALL, FontFamily::Proportional), palette.quiet);
    let x = mark.right() + ICON_GAP;
    let top = mark.center().y - f32::midpoint(name.size().y, version.size().y);
    let below = top + name.size().y;
    painter.galley(pos2(x, top), name, palette.on_glass);
    painter.galley(pos2(x, below), version, palette.quiet);
}

/// Draws a row of the sidebar's navigation at `row`, on the selection's fill when `chosen`.
fn draw_nav(painter: &Painter, row: Rect, icon: Icon, text: &str, chosen: bool, palette: Palette) {
    if chosen {
        painter.rect_filled(row, RADIUS_ROW, palette.selection);
    }
    let icon_center = pos2(row.left() + ROW_PADDING + ICON / 2.0, row.center().y);
    draw_icon(painter, icon_center, icon, palette.on_glass);
    painter.text(
        pos2(icon_center.x + ICON / 2.0 + ICON_GAP, row.center().y),
        Align2::LEFT_CENTER,
        text,
        font(TEXT, assets::medium_family()),
        palette.on_glass,
    );
}

/// The window's own three buttons at the right end of `row` (minimise, maximise or restore, close), where the
/// window has no system frame.
fn caption_buttons(ui: &Ui, row: Rect, palette: Palette) {
    let ctx = ui.ctx().clone();
    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
    let buttons = [
        (Icon::X, "Close", ViewportCommand::Close),
        (Icon::Square, if maximized { "Restore" } else { "Maximise" }, ViewportCommand::Maximized(!maximized)),
        (Icon::Minus, "Minimise", ViewportCommand::Minimized(true)),
    ];
    let mut right = row.right();
    for (icon, hint, command) in buttons {
        let rect = Rect::from_min_max(pos2(right - CAPTION_BUTTON, row.top()), pos2(right, row.bottom()));
        right = rect.left();
        let response = ui.interact(rect, ui.scope_id().with(hint), Sense::click()).on_hover_text(hint);
        let close = icon == Icon::X;
        let (fill, mark) = match (response.hovered(), close) {
            (true, true) => (palette.danger, palette.on_dark_glass),
            (true, false) => (palette.selection, palette.on_glass),
            (false, _) => (Color32::TRANSPARENT, palette.on_glass),
        };
        ui.painter().rect_filled(rect, RADIUS_ROW, fill);
        draw_icon(ui.painter(), rect.center(), icon, mark);
        if response.clicked() {
            ctx.send_viewport_cmd(command);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shell_wants_the_designs_panes() {
        let wants = wants(egui::vec2(1440.0, 900.0), true);
        let by = |id: &str| wants.iter().find(|w| w.id == id).unwrap();
        assert_eq!(by("sidebar").rect, egui::Rect::from_min_size(egui::pos2(12.0, 12.0), egui::vec2(256.0, 876.0)));
        assert_eq!((by("sidebar").radius, by("sidebar").surface), (20.0, Surface::Sidebar));
        assert_eq!(by("status").rect, egui::Rect::from_min_size(egui::pos2(280.0, 856.0), egui::vec2(1148.0, 32.0)));
        assert_eq!(by("sidebar-toggle").rect, egui::Rect::from_min_size(egui::pos2(280.0, 12.0), egui::vec2(36.0, 36.0)));
        let panel = panel_rect(egui::vec2(1440.0, 900.0), true);
        assert_eq!(panel.min, egui::pos2(280.0, 60.0));
        assert_eq!(panel.max, egui::pos2(1428.0, 844.0));
    }

    #[test]
    fn the_shell_wants_its_glass_panes_only() {
        for shown in [true, false] {
            let wants = wants(egui::vec2(1440.0, 900.0), shown);
            assert!(wants.iter().all(|w| w.surface.is_glass()), "{wants:?}");
            assert!(wants.iter().all(|w| w.id != "panel"));
            assert_eq!(wants.len(), if shown { 3 } else { 2 });
        }
    }

    #[test]
    fn without_the_sidebar_the_content_takes_its_room() {
        let wants = wants(egui::vec2(1440.0, 900.0), false);
        assert!(wants.iter().all(|w| w.id != "sidebar"));
        assert_eq!(panel_rect(egui::vec2(1440.0, 900.0), false).min.x, 12.0);
        assert_eq!(panel_rect(egui::vec2(1440.0, 900.0), false).max, panel_rect(egui::vec2(1440.0, 900.0), true).max);
    }

    #[test]
    fn a_tiny_window_gives_no_pane_with_negative_size() {
        for size in [egui::vec2(0.0, 0.0), egui::vec2(100.0, 40.0)] {
            assert!(wants(size, true).iter().all(|w| w.rect.width() > 0.0 && w.rect.height() > 0.0));
            for shown in [true, false] {
                let panel = panel_rect(size, shown);
                assert!(panel.width() > 0.0 && panel.height() > 0.0, "{panel:?} in {size:?}");
            }
        }
    }
}
