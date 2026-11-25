//! The repositories table: a head row and the lines in view on a frosted panel, each cell drawn from what
//! [`crate::repos_view`] decided, at the places [`crate::columns`] gives. It draws and says what was clicked; it
//! changes nothing itself.
//!
//! The lines are of two heights and are placed by hand ([`in_view`]); only those in view are drawn, so a table of
//! a thousand repositories costs a frame no more than one of twelve. The wheel over the table moves it.

use std::f32::consts::{FRAC_PI_2, PI};

use egui::epaint::CornerRadiusF32;
use egui::{Align2, Color32, FontFamily, Painter, Pos2, Rect, Role, Sense, Shape, Stroke, Ui, WidgetInfo, pos2, vec2};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::columns::{self, Columns, Span};
use crate::frost;
use crate::repos_view::{Changes, GroupLine, Line, Origin, Pulls, RepoLine, Tone};
use crate::theme::{
    self, CELL_ICON_GAP, GROUP_FILL_OPACITY, GROUP_HEIGHT, GROUP_ROW_GAP, HAIRLINE, HEAD_FILL_OPACITY, HEAD_HEIGHT, HOVER_OPACITY, ICON, ICON_SMALL,
    LINE_OPACITY, Mode, NAME_ICON_GAP, PROGRESS_HEIGHT, PROGRESS_LEAST, PROGRESS_TRACK_OPACITY, PULL_GAP, PULL_ICON_GAP, Palette, RADIUS_PANEL,
    ROW_HEIGHT, ROW_PAD_RIGHT, RULE_BAR, RULE_BAR_TOP, RULE_BREAK_END, RULE_BREAK_GAP, RULE_FIFTH_OPACITY, RULE_HEIGHT, RULE_LABEL_GAP,
    RULE_LEGEND_GAP, RULE_TICK, RULE_TICK_FIFTH, RULE_TICK_OPACITY, RULE_WIDTH, TEXT, TEXT_SMALL,
};
use crate::widgets::{self, Tick, draw_icon, font};

/// The head over the names.
const REPOSITORY: &str = "Repository";
/// The head over the folders.
const FOLDER: &str = "Folder";
/// The head over the branches.
const BRANCH: &str = "Branch";
/// The head over whether a checkout has uncommitted changes.
const CHANGES: &str = "Changes";
/// The head over the pull requests.
const PULL_REQUESTS: &str = "Pull requests";
/// The head over the rule's left half.
const BEHIND: &str = "Behind";
/// The head over the rule's right half.
const AHEAD: &str = "Ahead";
/// A checkout without uncommitted changes.
const CLEAN: &str = "Clean";
/// A checkout with uncommitted changes.
const UNCOMMITTED: &str = "Uncommitted";
/// A repository GitHub knows no pull requests of.
const NO_PULLS: &str = "None";
/// A repository not read yet.
const READING: &str = "…";
/// What a screen reader says for the head's checkbox.
const SELECT_ALL: &str = "Select all repositories";
/// Before the number of pull requests on the remote that the clone has not saved.
const NEW_MARK: &str = "+";
/// Before the number of saved pull requests that changed on the remote.
const CHANGED_MARK: &str = "~";

/// What a screen reader says for a repository's checkbox.
fn select(name: &str) -> String {
    format!("Select {name}")
}

/// What a person did to the table this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Did {
    /// Chose the repository with this key (a click on its row).
    Picked(String),
    /// Ticked or unticked the repository with this key (a click on its box).
    Ticked(String),
    /// Folded or unfolded the group with this key (a click on its header).
    Folded(String),
    /// Ticked or unticked every repository shown (a click on the head's box).
    TickedAll,
    /// Nothing.
    None,
}

/// Which lines are in view: the first line's index, how many, and the first one's top, for lines of the two heights.
///
/// `heights` are the lines' heights from the top, `scroll` how far the table is moved up and `room` the height
/// the lines have. The top is measured from the room's top, so it is zero or less (a line partly scrolled away).
/// No room, or a scroll past the end, is no line in view.
#[must_use]
pub fn in_view(heights: &[f32], scroll: f32, room: f32) -> (usize, usize, f32) {
    let scroll = if scroll.is_finite() { scroll.max(0.0) } else { 0.0 };
    let mut top = 0.0;
    let Some(first) = heights.iter().position(|height| {
        let past = top + height <= scroll;
        if past {
            top += height;
        }
        !past
    }) else {
        return (heights.len(), 0, 0.0);
    };
    let first_top = top - scroll;
    let mut count = 0;
    let mut at = first_top;
    for height in &heights[first..] {
        if at >= room {
            break;
        }
        count += 1;
        at += height;
    }
    (first, count, first_top)
}

/// Draws the head row and the lines in view on a frosted panel in `rect`, and says what was clicked.
///
/// `sheet` is the baker's frosted sheet for the panel, `scroll` how far the lines are moved up: the wheel over the
/// table changes it, and it is held to the content at every frame, also when the lines got fewer.
pub fn show(ui: &mut Ui, rect: Rect, lines: &[Line], sheet: &Sheet, palette: Palette, mode: Mode, scroll: &mut f32) -> Did {
    let heights: Vec<f32> = lines.iter().map(height).collect();
    let body = body_rect(rect);
    if ui.rect_contains_pointer(rect) {
        *scroll -= ui.input(|input| input.smooth_scroll_delta().y);
    }
    *scroll = held(*scroll, heights.iter().sum(), body.height());

    let window = Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size());
    frost::paint(ui.painter(), window, rect, RADIUS_PANEL, sheet);
    let columns = columns::columns(rect.width());
    let mut did = Did::None;

    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(rect.intersect(outer_clip));
    let head = head_rect(rect);
    draw_head(ui, head, &columns, palette);
    if widgets::checkbox(ui, tick_rect(head, &columns), SELECT_ALL, head_tick(lines), palette, SELECT_ALL).clicked() {
        did = Did::TickedAll;
    }

    // The lines are clipped to the body, and their fills and lines also to the panel's round corners.
    ui.set_clip_rect(body.intersect(outer_clip));
    for (index, row) in placed(body, &heights, *scroll) {
        let shown = row.intersect(body);
        let hovered = ui.rect_contains_pointer(shown) && takes_hover(&lines[index]);
        let fill = Fill { row, shown, panel: rect, under_line: index + 1 < lines.len(), hovered };
        match &lines[index] {
            Line::Group(group) => {
                let response = ui.interact(shown, ui.scope_id().with(("table-group", &group.key)), Sense::CLICK);
                response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &group.name));
                let highlight = if group.chosen { palette.selection } else { Color32::TRANSPARENT };
                draw_fill(ui.painter(), &fill, &[soft(palette, GROUP_FILL_OPACITY), highlight], palette);
                draw_group(ui, row, &columns, group, palette);
                if response.clicked() {
                    did = Did::Folded(group.key.clone());
                }
            }
            Line::Repo(repo) => {
                let response = ui.interact(shown, ui.scope_id().with(("table-row", &repo.key)), Sense::CLICK);
                response.widget_info(|| WidgetInfo::labeled(Role::Row, true, &repo.name));
                draw_fill(ui.painter(), &fill, &[repo_fill(repo, palette, mode)], palette);
                draw_repo(ui, row, &columns, repo, palette);
                let tick = if repo.ticked { Tick::On } else { Tick::Off };
                // Placed on the whole row, so the box keeps to the row's centre line; the clip keeps its press to the
                // part shown.
                if widgets::checkbox(ui, tick_rect(row, &columns), &repo.key, tick, palette, &select(&repo.name)).clicked() {
                    did = Did::Ticked(repo.key.clone());
                } else if response.clicked() {
                    did = Did::Picked(repo.key.clone());
                }
            }
        }
    }
    ui.set_clip_rect(outer_clip);
    did
}

/// The height of a line.
fn height(line: &Line) -> f32 {
    match line {
        Line::Group(_) => GROUP_HEIGHT,
        Line::Repo(_) => ROW_HEIGHT,
    }
}

/// The head row of the table at `rect`.
fn head_rect(rect: Rect) -> Rect {
    Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + HEAD_HEIGHT.min(rect.height())))
}

/// Where the lines are, in the table at `rect`: all of it below the head row.
fn body_rect(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.left(), head_rect(rect).bottom()), rect.max)
}

/// The lines in view in the room `body`, each with its index and the whole row it fills (which may reach past
/// `body` at the top or the bottom).
fn placed(body: Rect, heights: &[f32], scroll: f32) -> Vec<(usize, Rect)> {
    let (first, count, top) = in_view(heights, scroll, body.height());
    let mut y = body.top() + top;
    (first..first + count)
        .map(|index| {
            let row = Rect::from_min_max(pos2(body.left(), y), pos2(body.right(), y + heights[index]));
            y = row.bottom();
            (index, row)
        })
        .collect()
}

/// The part of the row at `row` that takes a press on its checkbox: the check column, the row's whole height.
fn tick_rect(row: Rect, columns: &Columns) -> Rect {
    Rect::from_min_max(pos2(row.left() + columns.check.left, row.top()), pos2(row.left() + columns.check.right(), row.bottom()))
}

/// `scroll` held between the top and the furthest the lines of `content` points can be moved up in `room`.
fn held(scroll: f32, content: f32, room: f32) -> f32 {
    let furthest = (content - room).max(0.0);
    if scroll.is_nan() { 0.0 } else { scroll.clamp(0.0, furthest) }
}

/// The head's checkbox: on when every repository shown is ticked, off when none is (or none is shown), some
/// otherwise.
fn head_tick(lines: &[Line]) -> Tick {
    let (mut ticked, mut repos) = (0, 0);
    for line in lines {
        if let Line::Repo(repo) = line {
            repos += 1;
            ticked += usize::from(repo.ticked);
        }
    }
    match ticked {
        0 => Tick::Off,
        all if all == repos => Tick::On,
        _ => Tick::Some,
    }
}

/// The pieces of a bar from `left`, `width` long: one piece, or, when `over` (more commits than the rule shows),
/// two with a gap near the bar's outer end, which is its left end when `outer_left`.
fn bar_pieces(left: f32, width: f32, over: bool, outer_left: bool) -> Vec<(f32, f32)> {
    let inner = width - RULE_BREAK_END - RULE_BREAK_GAP;
    if !over || inner <= 0.0 {
        return vec![(left, width)];
    }
    if outer_left {
        vec![(left, RULE_BREAK_END), (left + RULE_BREAK_END + RULE_BREAK_GAP, inner)]
    } else {
        vec![(left, inner), (left + width - RULE_BREAK_END, RULE_BREAK_END)]
    }
}

/// The text colour at `opacity`.
fn soft(palette: Palette, opacity: f32) -> Color32 {
    palette.on_glass.gamma_multiply(opacity)
}

/// The cell of `span` in the row at `row`.
fn cell(row: Rect, span: Span) -> Rect {
    Rect::from_min_max(pos2(row.left() + span.left, row.top()), pos2(row.left() + span.right(), row.bottom()))
}

/// A repository row's fill: the ticked rows filled as the drawing fills them, the highlighted one (the keyboard's
/// place) in the selection colour; a row that is both shows the ticked fill.
fn repo_fill(repo: &RepoLine, palette: Palette, mode: Mode) -> Color32 {
    if repo.ticked {
        theme::selected_fill(mode)
    } else if repo.chosen {
        palette.selection
    } else {
        Color32::TRANSPARENT
    }
}

/// Whether the hover is painted over a line under the pointer: not over a ticked or highlighted row, nor over a
/// highlighted group's header, whose own fill already marks it (and the hover over it would leave its text too
/// little contrast).
fn takes_hover(line: &Line) -> bool {
    match line {
        Line::Group(group) => !group.chosen,
        Line::Repo(repo) => !repo.ticked && !repo.chosen,
    }
}

/// Where a line's fill goes.
struct Fill {
    /// The whole row.
    row: Rect,
    /// The part of it shown.
    shown: Rect,
    /// The panel, whose round corners the fill keeps inside.
    panel: Rect,
    /// A line is drawn along the row's bottom.
    under_line: bool,
    /// The pointer is over it.
    hovered: bool,
}

/// Paints a line's fills, one over the other, the hover's over them, and the line under it; each cut to the
/// panel's round corners.
fn draw_fill(painter: &Painter, fill: &Fill, fills: &[Color32], palette: Palette) {
    let hover = if fill.hovered { soft(palette, HOVER_OPACITY) } else { Color32::TRANSPARENT };
    for &colour in fills.iter().chain([&hover]) {
        fill_inside(painter, fill.shown, fill.panel, colour);
    }
    if fill.under_line {
        let line = Rect::from_min_max(pos2(fill.row.left(), fill.row.bottom() - HAIRLINE), fill.row.max).intersect(fill.shown);
        fill_inside(painter, line, fill.panel, soft(palette, LINE_OPACITY));
    }
}

/// Paints the line along the bottom of `row`.
fn draw_line_under(painter: &Painter, row: Rect, palette: Palette) {
    painter.rect_filled(Rect::from_min_max(pos2(row.left(), row.bottom() - HAIRLINE), row.max), 0.0, soft(palette, LINE_OPACITY));
}

/// Paints the part of `shape` that is inside the panel at `panel` with its round corners in `colour`.
fn fill_inside(painter: &Painter, shape: Rect, panel: Rect, colour: Color32) {
    if colour == Color32::TRANSPARENT || !shape.is_positive() {
        return;
    }
    let outline = inside_panel(shape, panel);
    if outline.len() >= 3 {
        painter.add(Shape::convex_polygon(outline, colour, Stroke::NONE));
    }
}

/// The outline of the part of `shape` inside the panel at `panel` with its corners round at [`RADIUS_PANEL`]: a
/// convex polygon whose corners lie on or inside the panel's outline; empty when nothing of `shape` is inside.
fn inside_panel(shape: Rect, panel: Rect) -> Vec<Pos2> {
    let mut points = vec![shape.left_top(), shape.right_top(), shape.right_bottom(), shape.left_bottom()];
    let outline = panel_outline(panel);
    let centre = panel.center();
    for (index, &from) in outline.iter().enumerate() {
        let to = outline[(index + 1) % outline.len()];
        // Which side of the edge from `from` to `to` a point is on; the panel's centre is inside.
        let side = |p: Pos2| (to.x - from.x) * (p.y - from.y) - (to.y - from.y) * (p.x - from.x);
        let inside = |p: Pos2| side(p) * side(centre) >= 0.0;
        let mut kept = Vec::with_capacity(points.len() + 1);
        for (at, &here) in points.iter().enumerate() {
            let next = points[(at + 1) % points.len()];
            if inside(here) {
                kept.push(here);
            }
            if inside(here) != inside(next) {
                let part = side(here) / (side(here) - side(next));
                kept.push(here + (next - here) * part);
            }
        }
        points = kept;
        if points.is_empty() {
            break;
        }
    }
    points
}

/// The outline of the panel at `panel`, clockwise: its four round corners, each cut into [`theme::CORNER_STEPS`]
/// straight pieces whose ends lie on a round [`theme::CORNER_SLACK`] wider than the panel's, so that the pieces
/// stay inside the panel as egui paints it (its own round is cut into pieces too, and its edge is softened).
fn panel_outline(panel: Rect) -> Vec<Pos2> {
    let radius = (RADIUS_PANEL + theme::CORNER_SLACK).min(panel.width() / 2.0).min(panel.height() / 2.0).max(0.0);
    let corners = [
        (pos2(panel.right() - radius, panel.top() + radius), -FRAC_PI_2),
        (pos2(panel.right() - radius, panel.bottom() - radius), 0.0),
        (pos2(panel.left() + radius, panel.bottom() - radius), FRAC_PI_2),
        (pos2(panel.left() + radius, panel.top() + radius), PI),
    ];
    let steps = f32::from(theme::CORNER_STEPS);
    corners
        .into_iter()
        .flat_map(|(centre, start)| {
            (0..=theme::CORNER_STEPS).map(move |step| {
                let angle = start + FRAC_PI_2 * f32::from(step) / steps;
                centre + vec2(angle.cos(), angle.sin()) * radius
            })
        })
        .collect()
}

/// Paints the head row's fill, its line and its words (its checkbox is drawn by the caller).
fn draw_head(ui: &Ui, head: Rect, columns: &Columns, palette: Palette) {
    let painter = ui.painter();
    let corners = CornerRadiusF32 { nw: RADIUS_PANEL, ne: RADIUS_PANEL, sw: 0.0, se: 0.0 };
    painter.rect_filled(head, corners, soft(palette, HEAD_FILL_OPACITY));
    draw_line_under(painter, head, palette);
    let medium = font(TEXT, assets::medium_family());
    let y = head.center().y;
    let word = |span: Span, text: &str| {
        painter.text(pos2(head.left() + span.left, y), Align2::LEFT_CENTER, text, medium.clone(), palette.on_glass);
    };
    let name = painter.text(pos2(head.left() + columns.name.left, y), Align2::LEFT_CENTER, REPOSITORY, medium.clone(), palette.on_glass);
    draw_icon(painter, pos2(name.right() + CELL_ICON_GAP + ICON_SMALL / 2.0, y), Icon::ArrowUp, ICON_SMALL, palette.quiet);
    for (span, text) in [(columns.folder, FOLDER), (columns.branch, BRANCH), (columns.changes, CHANGES), (columns.pulls, PULL_REQUESTS)] {
        if let Some(span) = span {
            word(span, text);
        }
    }
    let zero = head.left() + columns.origin.left + columns::rule(0, 0).zero;
    painter.text(pos2(zero - RULE_LEGEND_GAP, y), Align2::RIGHT_CENTER, BEHIND, medium.clone(), palette.on_glass);
    painter.text(pos2(zero + HAIRLINE + RULE_LEGEND_GAP, y), Align2::LEFT_CENTER, AHEAD, medium, palette.on_glass);
}

/// Paints a group's header: its fold arrow, its name and its note.
/// The name keeps to the name column; the note has what is left of the row after it.
fn draw_group(ui: &Ui, row: Rect, columns: &Columns, group: &GroupLine, palette: Palette) {
    let painter = ui.painter();
    let y = row.center().y;
    let arrow = if group.collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
    draw_icon(painter, pos2(row.left() + columns.check.left + ICON / 2.0, y), arrow, ICON, palette.on_glass);
    let medium = font(TEXT, assets::medium_family());
    let left = row.left() + columns.check.right() + GROUP_ROW_GAP;
    let name = widgets::ellipsis(&group.name, &medium, row.left() + columns.name.right() - left, ui);
    let right = draw_text(painter, left, y, name, true, palette.on_glass);
    let small = font(TEXT_SMALL, FontFamily::Proportional);
    let note_left = right + GROUP_ROW_GAP;
    let note = widgets::ellipsis(&group.note, &small, row.right() - ROW_PAD_RIGHT - note_left, ui);
    if !note.is_empty() {
        painter.text(pos2(note_left, y), Align2::LEFT_CENTER, note, small, palette.quiet);
    }
}

/// Paints `text` at the left of `x`, centred on `y`, unless it is empty; gives back where its right end is.
fn draw_text(painter: &Painter, x: f32, y: f32, text: String, medium: bool, colour: Color32) -> f32 {
    if text.is_empty() {
        return x;
    }
    let family = if medium { assets::medium_family() } else { FontFamily::Proportional };
    painter.text(pos2(x, y), Align2::LEFT_CENTER, text, font(TEXT, family), colour).right()
}

/// Paints a repository's cells (its checkbox is drawn by the caller).
fn draw_repo(ui: &Ui, row: Rect, columns: &Columns, repo: &RepoLine, palette: Palette) {
    let painter = ui.painter();
    let y = row.center().y;
    let medium = font(TEXT, assets::medium_family());
    let regular = font(TEXT, FontFamily::Proportional);

    let eye_room = if repo.watched { NAME_ICON_GAP + ICON_SMALL } else { 0.0 };
    let name = widgets::ellipsis(&repo.name, &medium, columns.name.width - eye_room, ui);
    let right = draw_text(painter, row.left() + columns.name.left, y, name, true, palette.on_glass);
    if repo.watched {
        draw_icon(painter, pos2(right + NAME_ICON_GAP + ICON_SMALL / 2.0, y), Icon::Eye, ICON_SMALL, palette.quiet);
    }

    if let Some(span) = columns.folder {
        let folder = widgets::path_to_fit(&repo.folder, &regular, span.width, ui);
        draw_text(painter, row.left() + span.left, y, folder, false, palette.quiet);
    }
    if let Some(span) = columns.branch {
        let left = row.left() + span.left;
        draw_icon(painter, pos2(left + ICON / 2.0, y), Icon::GitBranch, ICON, palette.quiet);
        let branch = widgets::ellipsis(&repo.branch, &regular, span.width - ICON - CELL_ICON_GAP, ui);
        draw_text(painter, left + ICON + CELL_ICON_GAP, y, branch, false, palette.on_glass);
    }
    if let Some(span) = columns.changes {
        let (words, colour) = match repo.changes {
            Changes::Clean => (CLEAN, palette.quiet),
            Changes::Uncommitted => (UNCOMMITTED, palette.on_glass),
            Changes::Unknown => ("", palette.quiet),
        };
        draw_text(painter, row.left() + span.left, y, words.to_owned(), false, colour);
    }
    if let Some(span) = columns.pulls {
        let clipped = painter.with_clip_rect(cell(row, span).intersect(painter.clip_rect()));
        draw_pulls(&clipped, row.left() + span.left, y, repo.pulls, palette);
    }
    draw_origin(ui, cell(row, columns.origin), &repo.origin, palette);
    let action = cell(row, columns.action);
    draw_icon(painter, action.center(), Icon::Ellipsis, ICON, palette.quiet);
}

/// Paints the pull requests cell from `left`: open, merged and closed each with its icon (a state with none left
/// out), then what the clone has not saved; "None" when GitHub knows none and nothing waits; nothing when GitHub
/// has not answered and nothing waits.
fn draw_pulls(painter: &Painter, left: f32, y: f32, pulls: Pulls, palette: Palette) {
    let mut x = left;
    for (count, icon) in [(pulls.open, Icon::GitPullRequest), (pulls.merged, Icon::GitMerge), (pulls.closed, Icon::GitPullRequestClosed)] {
        if count > 0 {
            draw_icon(painter, pos2(x + ICON_SMALL / 2.0, y), icon, ICON_SMALL, palette.quiet);
            x = draw_text(painter, x + ICON_SMALL + PULL_ICON_GAP, y, count.to_string(), false, palette.on_glass) + PULL_GAP;
        }
    }
    for (count, mark) in [(pulls.new, NEW_MARK), (pulls.changed, CHANGED_MARK)] {
        if count > 0 {
            x = draw_text(painter, x, y, format!("{mark}{count}"), true, palette.warning) + PULL_GAP;
        }
    }
    let none = [pulls.open, pulls.merged, pulls.closed, pulls.new, pulls.changed].iter().all(|&count| count == 0);
    if pulls.known && none {
        draw_text(painter, left, y, NO_PULLS.to_owned(), false, palette.quiet);
    }
}

/// Paints how the repository stands to its remote in its cell.
fn draw_origin(ui: &Ui, cell: Rect, origin: &Origin, palette: Palette) {
    let painter = ui.painter();
    let y = cell.center().y;
    let regular = font(TEXT, FontFamily::Proportional);
    let words_left = cell.left() + RULE_WIDTH + RULE_LABEL_GAP;
    let words_room = cell.right() - words_left;
    match origin {
        Origin::Reading => {
            draw_text(painter, cell.left(), y, READING.to_owned(), false, palette.quiet);
        }
        Origin::Busy { share, text } => {
            let track = Rect::from_min_size(pos2(cell.left(), y - PROGRESS_HEIGHT / 2.0), vec2(RULE_WIDTH, PROGRESS_HEIGHT));
            painter.rect_filled(track, PROGRESS_HEIGHT / 2.0, soft(palette, PROGRESS_TRACK_OPACITY));
            let filled = (RULE_WIDTH * share.clamp(0.0, 1.0)).round().clamp(PROGRESS_LEAST, RULE_WIDTH);
            painter.rect_filled(Rect::from_min_size(track.min, vec2(filled, PROGRESS_HEIGHT)), PROGRESS_HEIGHT / 2.0, palette.accent);
            draw_text(painter, words_left, y, widgets::ellipsis(text, &regular, words_room, ui), false, palette.on_glass);
        }
        Origin::Failed(reason) => {
            draw_icon(painter, pos2(cell.left() + ICON / 2.0, y), Icon::TriangleAlert, ICON, palette.danger);
            let left = cell.left() + ICON + CELL_ICON_GAP;
            draw_text(painter, left, y, widgets::ellipsis(reason, &regular, cell.right() - left, ui), false, palette.danger);
        }
        Origin::Measured { behind, ahead, text, tone } => {
            draw_rule(painter, cell.left(), y, *behind, *ahead, palette);
            let (colour, medium) = match tone {
                Tone::Quiet => (palette.quiet, false),
                Tone::Plain => (palette.on_glass, true),
                Tone::Warning => (palette.warning, true),
            };
            let family = if medium { assets::medium_family() } else { FontFamily::Proportional };
            draw_text(painter, words_left, y, widgets::ellipsis(text, &font(TEXT, family), words_room, ui), medium, colour);
        }
    }
}

/// Paints the rule from `left`, centred on `y`: its ticks, its zero line, and the bars of the commits behind (in
/// the warning colour) and ahead.
fn draw_rule(painter: &Painter, left: f32, y: f32, behind: u32, ahead: u32, palette: Palette) {
    let top = y - RULE_HEIGHT / 2.0;
    let bottom = top + RULE_HEIGHT;
    for (x, fifth) in columns::ticks() {
        let (tall, opacity) = if fifth { (RULE_TICK_FIFTH, RULE_FIFTH_OPACITY) } else { (RULE_TICK, RULE_TICK_OPACITY) };
        painter.rect_filled(Rect::from_min_max(pos2(left + x, bottom - tall), pos2(left + x + HAIRLINE, bottom)), 0.0, soft(palette, opacity));
    }
    let rule = columns::rule(behind, ahead);
    painter.rect_filled(Rect::from_min_max(pos2(left + rule.zero, top), pos2(left + rule.zero + HAIRLINE, bottom)), 0.0, palette.on_glass);
    let bars = [(rule.behind, rule.behind_over, true, palette.warning), (rule.ahead, rule.ahead_over, false, palette.on_glass)];
    for (bar, over, outer_left, colour) in bars {
        let Some((start, width)) = bar else { continue };
        for (piece, length) in bar_pieces(left + start, width, over, outer_left) {
            painter.rect_filled(Rect::from_min_size(pos2(piece, top + RULE_BAR_TOP), vec2(length, RULE_BAR)), 0.0, colour);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use egui::{Event, FullOutput, PointerButton, RawInput, Stroke, TextureId};

    use super::*;
    use devassist_glass::raster::Bitmap;

    use crate::baker::{Scene, sheet_picture};
    use crate::picture;
    use crate::repos_view::{self, RepoLine};
    use crate::theme::Surface;
    use crate::widgets::tests::context;

    /// The window the tests draw in, the design's.
    const WINDOW: [f32; 2] = [1440.0, 900.0];

    /// The table's place in the design's window.
    fn table_rect() -> Rect {
        Rect::from_min_size(pos2(280.0, 108.0), vec2(1148.0, 736.0))
    }

    fn sheet() -> Sheet {
        Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
    }

    fn demo() -> Vec<Line> {
        repos_view::lines(&crate::demo::repos())
    }

    /// `count` lines: the demo's sixteen again and again, the first round as the demo has them and each later
    /// round with keys and names of its own.
    fn many(count: usize) -> Vec<Line> {
        let all = demo();
        (0..count)
            .map(|index| {
                let (round, line) = (index / all.len(), all[index % all.len()].clone());
                if round == 0 {
                    return line;
                }
                match line {
                    Line::Group(group) => {
                        Line::Group(GroupLine { key: format!("{}#{round}", group.key), name: format!("{}#{round}", group.name), ..group })
                    }
                    Line::Repo(repo) => Line::Repo(RepoLine { key: format!("{}#{round}", repo.key), name: format!("{}#{round}", repo.name), ..repo }),
                }
            })
            .collect()
    }

    /// Runs one frame of the table at `rect` with these lines and pointer events; gives back what it said and the
    /// frame's output.
    fn frame(ctx: &egui::Context, rect: Rect, lines: &[Line], events: Vec<Event>, scroll: &mut f32) -> (Did, FullOutput) {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), events, ..Default::default() };
        let mut did = Did::None;
        let output = ctx.run_ui(input, |ui| {
            did = show(ui, rect, lines, &sheet(), theme::palette(Mode::Light), Mode::Light, scroll);
        });
        (did, output)
    }

    /// How many shapes a settled frame of the table at `rect` with these lines paints.
    fn shapes(rect: Rect, lines: &[Line]) -> usize {
        let ctx = context();
        let mut scroll = 0.0;
        // The first frame lays the faces out; the second is a frame like any other.
        frame(&ctx, rect, lines, Vec::new(), &mut scroll).1.drop_without_applying_deltas();
        let (_, output) = frame(&ctx, rect, lines, Vec::new(), &mut scroll);
        let count = output.shapes.len();
        output.drop_without_applying_deltas();
        count
    }

    /// What the table says to a press and release at `at`, after the pointer has come there.
    fn click(lines: &[Line], at: Pos2) -> Vec<Did> {
        click_scrolled(lines, at, 0.0)
    }

    /// What the table, its lines moved up by `scroll`, says to a press and release at `at`.
    fn click_scrolled(lines: &[Line], at: Pos2, scroll: f32) -> Vec<Did> {
        let ctx = context();
        let mut scroll = scroll;
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: egui::Modifiers::default() };
        [Vec::new(), vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()]
            .into_iter()
            .map(|events| {
                let (did, output) = frame(&ctx, table_rect(), lines, events, &mut scroll);
                output.drop_without_applying_deltas();
                did
            })
            .collect()
    }

    /// The row of the line at `index` in the table at `table_rect`, unscrolled.
    fn row_of(lines: &[Line], index: usize) -> Rect {
        row_at(lines, index, 0.0)
    }

    /// The row of the line at `index` in the table at `table_rect`, its lines moved up by `scroll`.
    fn row_at(lines: &[Line], index: usize, scroll: f32) -> Rect {
        let heights: Vec<f32> = lines.iter().map(height).collect();
        placed(body_rect(table_rect()), &heights, scroll).into_iter().find(|(at, _)| *at == index).map(|(_, row)| row).unwrap()
    }

    /// The only thing said among `said`, when it is said once on the release and nothing on the other frames.
    fn said_once(said: &[Did]) -> Did {
        let some: Vec<&Did> = said.iter().filter(|did| **did != Did::None).collect();
        assert_eq!(some.len(), 1, "{said:?}");
        assert_eq!(&said[3], some[0], "said on the release: {said:?}");
        some[0].clone()
    }

    #[test]
    fn only_the_lines_in_view_are_counted() {
        let heights: Vec<f32> = (0..1000).map(|i| if i % 10 == 0 { 36.0 } else { 44.0 }).collect();
        let (first, count, top) = in_view(&heights, 0.0, 600.0);
        assert_eq!((first, top), (0, 0.0));
        assert!(count <= 16);
        let (first, count, top) = in_view(&heights, 4320.0, 600.0); // 100 lines down: 10 groups and 90 rows
        assert_eq!((first, top), (100, 0.0));
        assert!(count <= 16);
        assert_eq!(in_view(&[], 50.0, 600.0), (0, 0, 0.0));
        assert_eq!(in_view(&heights, 1.0e9, 600.0).1, 0); // scrolled past the end: nothing, no panic
    }

    #[test]
    fn a_line_partly_scrolled_away_is_in_view_with_its_top_above() {
        let heights = [36.0, 44.0, 44.0, 44.0];
        // 50 down: the header is gone, the first row shows its lower 30 points.
        assert_eq!(in_view(&heights, 50.0, 60.0), (1, 2, -14.0));
        // The lines in view fill the room and no more: 30 + 44 reaches past 60, a third does not start in it.
        assert_eq!(in_view(&heights, 0.0, 80.0), (0, 2, 0.0));
        assert_eq!(in_view(&heights, 0.0, 0.0).1, 0);
        assert_eq!(in_view(&heights, f32::NAN, 60.0).0, 0);
    }

    #[test]
    fn the_scroll_is_held_to_the_content() {
        assert_eq!(held(-5.0, 1000.0, 600.0), 0.0);
        assert_eq!(held(300.0, 1000.0, 600.0), 300.0);
        assert_eq!(held(900.0, 1000.0, 600.0), 400.0);
        assert_eq!(held(900.0, 300.0, 600.0), 0.0); // content shorter than the room does not move
        assert_eq!(held(f32::NAN, 1000.0, 600.0), 0.0);
    }

    #[test]
    fn a_table_whose_lines_got_fewer_scrolls_back_into_them() {
        let ctx = context();
        let mut scroll = 1.0e6;
        let lines = many(100);
        let (_, output) = frame(&ctx, table_rect(), &lines, Vec::new(), &mut scroll);
        output.drop_without_applying_deltas();
        let content: f32 = lines.iter().map(height).sum();
        assert!((scroll - (content - body_rect(table_rect()).height())).abs() < 1e-3, "{scroll}");
        // The demo's sixteen lines fit the room: back to the top.
        let (_, output) = frame(&ctx, table_rect(), &demo(), Vec::new(), &mut scroll);
        output.drop_without_applying_deltas();
        assert_eq!(scroll, 0.0);
    }

    #[test]
    fn a_wheel_over_the_table_moves_it_and_one_elsewhere_does_not() {
        let lines = many(100);
        let wheel = |at: Pos2| {
            let ctx = context();
            let mut scroll = 0.0;
            let mut events = vec![vec![Event::PointerMoved(at)]];
            events.extend((0..20).map(|_| {
                vec![Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: vec2(0.0, -40.0),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::default(),
                }]
            }));
            events.extend((0..20).map(|_| Vec::new()));
            for events in events {
                frame(&ctx, table_rect(), &lines, events, &mut scroll).1.drop_without_applying_deltas();
            }
            scroll
        };
        assert!(wheel(table_rect().center()) > 0.0, "the wheel over the table did not move it");
        assert_eq!(wheel(pos2(100.0, 100.0)), 0.0, "a wheel beside the table moved it");
    }

    #[test]
    fn a_frame_of_a_thousand_lines_lays_out_no_more_text_than_a_frame_of_twelve() {
        let rect = Rect::from_min_size(pos2(280.0, 108.0), vec2(1148.0, 700.0));
        let twelve = shapes(rect, &demo());
        let thousand = shapes(rect, &many(1000));
        assert!(twelve > 50, "the demo's frame painted only {twelve} shapes");
        assert!(thousand <= twelve + 20, "a thousand lines painted {thousand} shapes, twelve painted {twelve}");
    }

    #[test]
    fn a_click_on_a_row_picks_it_and_a_click_on_its_box_ticks_it() {
        let lines = demo();
        let columns = columns::columns(table_rect().width());
        // The drawing's second repository is homelab, under the header of home.
        let index = lines.iter().enumerate().filter(|(_, line)| matches!(line, Line::Repo(_))).nth(1).map(|(at, _)| at).unwrap();
        let Line::Repo(second) = &lines[index] else { unreachable!() };
        assert_eq!(second.key, "homelab");
        let row = row_of(&lines, index);
        let name = pos2(row.left() + columns.name.left + columns.name.width / 2.0, row.center().y);
        assert_eq!(said_once(&click(&lines, name)), Did::Picked("homelab".to_owned()));
        let tick = widgets::box_rect(tick_rect(row, &columns)).center();
        assert_eq!(said_once(&click(&lines, tick)), Did::Ticked("homelab".to_owned()));
        let Line::Group(home) = &lines[0] else { panic!("the demo starts with a header") };
        let header = row_of(&lines, 0);
        assert_eq!(said_once(&click(&lines, pos2(header.center().x, header.center().y))), Did::Folded(home.key.clone()));
        let head = head_rect(table_rect());
        assert_eq!(said_once(&click(&lines, widgets::box_rect(tick_rect(head, &columns)).center())), Did::TickedAll);
        // A click on the head's words does nothing yet.
        assert!(click(&lines, pos2(head.left() + columns.name.left + 10.0, head.center().y)).iter().all(|did| *did == Did::None));
    }

    #[test]
    fn the_heads_box_is_on_some_or_off() {
        let lines = demo();
        assert_eq!(head_tick(&lines), Tick::Some, "api and web of twelve");
        let tick_all = |ticked: bool| -> Vec<Line> {
            lines
                .iter()
                .cloned()
                .map(|line| match line {
                    Line::Repo(repo) => Line::Repo(RepoLine { ticked, ..repo }),
                    group @ Line::Group(_) => group,
                })
                .collect()
        };
        assert_eq!(head_tick(&tick_all(true)), Tick::On);
        assert_eq!(head_tick(&tick_all(false)), Tick::Off);
        assert_eq!(head_tick(&[]), Tick::Off);
    }

    #[test]
    fn a_bar_past_the_scale_is_broken_near_its_outer_end() {
        assert_eq!(bar_pieces(48.0, 32.0, false, true), [(48.0, 32.0)]);
        // Behind: the outer end is at the left.
        assert_eq!(bar_pieces(0.0, 80.0, true, true), [(0.0, 14.0), (20.0, 60.0)]);
        // Ahead: the outer end is at the right, at the rule's end.
        assert_eq!(bar_pieces(81.0, 80.0, true, false), [(81.0, 60.0), (147.0, 14.0)]);
    }

    /// What a picture shows over the wallpaper.
    #[derive(Clone, Copy, PartialEq)]
    enum Drawn {
        /// Nothing: the bare wallpaper.
        Nothing,
        /// The frosted panel alone, without the table on it.
        Panel,
        /// The whole table.
        Table,
    }

    /// A picture of the window over the lake in `mode` at one pixel a point, with `drawn` where the design has the
    /// table: `lines` moved up by `scroll`, the pointer at `pointer`.
    fn render(lines: &[Line], pointer: Option<Pos2>, mode: Mode, scroll: f32, drawn: Drawn) -> Bitmap {
        let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: "lake".to_owned(), blur: true };
        let (wall, mut bitmap) = picture::wallpaper_window(&scene);
        if drawn == Drawn::Nothing {
            return bitmap;
        }
        let panel = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
        let sheet = Sheet { texture: TextureId::User(1), uv: panel.uv, edge: panel.edge };
        let ctx = context();
        let mut textures = HashMap::from([(sheet.texture, panel.bitmap.image())]);
        let mut scroll = scroll;
        let mut shapes = Vec::new();
        // The first frame lays the faces out; the second is the one painted.
        for _ in 0..2 {
            let events = pointer.map(Event::PointerMoved).into_iter().collect();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                if drawn == Drawn::Table {
                    show(ui, table_rect(), lines, &sheet, theme::palette(mode), mode, &mut scroll);
                } else {
                    let window = Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]));
                    frost::paint(ui.painter(), window, table_rect(), RADIUS_PANEL, &sheet);
                }
            });
            picture::take_textures(std::mem::take(&mut output.textures_delta), &mut textures);
            shapes = std::mem::take(&mut output.shapes);
        }
        picture::paint(&mut bitmap, &ctx.tessellate(shapes, 1.0), &textures);
        bitmap
    }

    /// Writes `bitmap` to `target/shots/{name}`.
    fn write(bitmap: &Bitmap, name: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots").join(name);
        crate::screenshot::write_png(&path, &bitmap.image()).unwrap();
        eprintln!("wrote {}", path.display());
    }

    /// Draws a table of `lines` where the design has it, over the lake in `mode`, with the pointer at `pointer`, and
    /// writes the picture to `target/shots/{name}`.
    fn draw_picture(lines: &[Line], pointer: Option<Pos2>, mode: Mode, name: &str) {
        write(&render(lines, pointer, mode, 0.0, Drawn::Table), name);
    }

    /// The demo's lines again and again, moved up so that the bottom of the ticked `api` of the second round is
    /// `above_bottom` points over the panel's bottom edge (below it when negative), with the pointer over the
    /// panel's last points: (lines, scroll, pointer).
    fn overflowing(above_bottom: f32) -> (Vec<Line>, f32, Pos2) {
        let lines = many(100);
        let index = 16 + lines.iter().position(|line| matches!(line, Line::Repo(repo) if repo.name == "api")).unwrap();
        let Line::Repo(api) = &lines[index] else { unreachable!() };
        assert!(api.ticked);
        let bottom_of_api = body_rect(table_rect()).top() + lines[..=index].iter().map(height).sum::<f32>();
        let scroll = bottom_of_api - (table_rect().bottom() - above_bottom);
        let pointer = pos2(table_rect().left() + RADIUS_PANEL * 3.0, table_rect().bottom() - 2.0);
        (lines, scroll, pointer)
    }

    /// Whether two pixels are exactly the same.
    fn same(a: [f32; 4], b: [f32; 4]) -> bool {
        a.map(f32::to_bits) == b.map(f32::to_bits)
    }

    #[test]
    fn nothing_of_the_table_shows_outside_the_panels_round_corners() {
        let rect = table_rect();
        let wall = render(&[], None, Mode::Light, 0.0, Drawn::Nothing);
        let panel = render(&[], None, Mode::Light, 0.0, Drawn::Panel);
        // The ticked row across the edge with its line 6 points above it (the row below 6 points high, under the
        // pointer), and the ticked row's fill reaching 8 points past the edge.
        for above_bottom in [6.0, 12.5, -8.0] {
            let (lines, scroll, pointer) = overflowing(above_bottom);
            let table = render(&lines, Some(pointer), Mode::Light, scroll, Drawn::Table);
            let (mut outside, mut changed) = (0, Vec::new());
            for corner_left in [rect.left(), rect.right() - RADIUS_PANEL] {
                #[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "pixels of a 1440 by 900 window")]
                let (x0, y0) = ((corner_left - 2.0) as usize, (rect.bottom() - RADIUS_PANEL - 2.0) as usize);
                for y in y0..y0 + 20 {
                    for x in x0..x0 + 20 {
                        let at = y * wall.w + x;
                        // Outside the panel's outline: where the panel alone leaves the wallpaper as it is.
                        if same(panel.px[at], wall.px[at]) {
                            outside += 1;
                            if !same(table.px[at], wall.px[at]) {
                                changed.push((x, y));
                            }
                        }
                    }
                }
            }
            assert!(outside > 50, "only {outside} pixels outside the corners were looked at");
            assert!(changed.is_empty(), "{above_bottom}: the table painted outside the panel at {changed:?}");
        }
    }

    #[test]
    #[ignore = "writes pictures for a person to look at; run by hand"]
    fn draw_the_demo_table_to_pictures() {
        let (lines, scroll, pointer) = overflowing(6.0);
        write(&render(&lines, Some(pointer), Mode::Light, scroll, Drawn::Table), "table-overflow-light.png");
        draw_picture(&demo(), None, Mode::Light, "table-light.png");
        draw_picture(&demo(), None, Mode::Dark, "table-dark.png");
        // The states the demo does not show: a highlighted row, the pointer over another, a fetch running, bars past the
        // scale, GitHub knowing no pull requests, a folded group.
        let states: Vec<Line> = demo()
            .into_iter()
            .map(|line| match line {
                Line::Repo(repo) => Line::Repo(match repo.name.as_str() {
                    "portal" => RepoLine { chosen: true, ..repo },
                    "customer-portal-i18n" => RepoLine { origin: Origin::Busy { share: 0.5, text: "Receiving, 50%".to_owned() }, ..repo },
                    "DevAssist" => RepoLine {
                        origin: Origin::Measured { behind: 25, ahead: 12, text: "12 ahead, 25 behind".to_owned(), tone: Tone::Plain },
                        ..repo
                    },
                    "homelab" | "recipes" => RepoLine { pulls: Pulls { known: true, ..repo.pulls }, ..repo },
                    "notes" => RepoLine { origin: Origin::Reading, changes: Changes::Unknown, ..repo },
                    _ => repo,
                }),
                Line::Group(group) if group.name == "No group" => Line::Group(GroupLine { collapsed: true, ..group }),
                group @ Line::Group(_) => group,
            })
            .collect();
        let pointer = row_of(&states, 2).center();
        draw_picture(&states, Some(pointer), Mode::Light, "table-states-light.png");
        draw_picture(&states, Some(pointer), Mode::Dark, "table-states-dark.png");
    }

    #[test]
    fn a_fill_is_cut_to_the_panels_round_corners_only_where_it_reaches_them() {
        let panel = table_rect();
        let inner = Rect::from_min_max(pos2(panel.left(), panel.top() + 100.0), pos2(panel.right(), panel.top() + 144.0));
        let outline = inside_panel(inner, panel);
        assert_eq!(outline.len(), 4);
        for corner in [inner.left_top(), inner.right_top(), inner.right_bottom(), inner.left_bottom()] {
            assert!(outline.iter().any(|p| (*p - corner).length() < 1e-3), "{corner:?} lost: {outline:?}");
        }
        // A row across the bottom: every corner of its outline lies inside the panel's round.
        let bottom = Rect::from_min_max(pos2(panel.left(), panel.bottom() - 30.0), pos2(panel.right(), panel.bottom()));
        let cut = inside_panel(bottom, panel);
        assert!(cut.len() > 4, "{cut:?}");
        for p in &cut {
            for centre in
                [pos2(panel.left() + RADIUS_PANEL, panel.bottom() - RADIUS_PANEL), pos2(panel.right() - RADIUS_PANEL, panel.bottom() - RADIUS_PANEL)]
            {
                let beyond =
                    (p.x - centre.x).abs() >= 0.0 && p.y > centre.y && (p.x < panel.left() + RADIUS_PANEL || p.x > panel.right() - RADIUS_PANEL);
                if beyond && (p.x - centre.x).abs() <= RADIUS_PANEL {
                    assert!((*p - centre).length() <= RADIUS_PANEL + 1e-3, "{p:?} is outside the corner at {centre:?}");
                }
            }
        }
        // Nothing of a rectangle below the panel is inside it.
        assert_eq!(inside_panel(Rect::from_min_size(pos2(panel.left(), panel.bottom() + 1.0), vec2(100.0, 10.0)), panel), Vec::<Pos2>::new());
    }

    #[test]
    fn the_ticked_rows_are_filled_and_the_highlighted_one_has_the_selection() {
        let Some(Line::Repo(api)) = demo().into_iter().find(|line| matches!(line, Line::Repo(repo) if repo.name == "api")) else { panic!("no api") };
        for mode in [Mode::Light, Mode::Dark] {
            let palette = theme::palette(mode);
            assert_eq!(repo_fill(&RepoLine { ticked: true, chosen: false, ..api.clone() }, palette, mode), theme::selected_fill(mode));
            assert_eq!(repo_fill(&RepoLine { ticked: false, chosen: true, ..api.clone() }, palette, mode), palette.selection);
            assert_eq!(repo_fill(&RepoLine { ticked: true, chosen: true, ..api.clone() }, palette, mode), theme::selected_fill(mode));
            assert_eq!(repo_fill(&RepoLine { ticked: false, chosen: false, ..api.clone() }, palette, mode), Color32::TRANSPARENT);
        }
    }

    #[test]
    fn the_hover_is_painted_only_over_a_line_without_a_fill_of_its_own() {
        let lines = demo();
        let Some(Line::Repo(api)) = lines.iter().find(|line| matches!(line, Line::Repo(repo) if repo.name == "api")).cloned() else {
            panic!("no api")
        };
        let Some(Line::Group(group)) = lines.first().cloned() else { panic!("the demo starts with a header") };
        let repo = |ticked, chosen| Line::Repo(RepoLine { ticked, chosen, ..api.clone() });
        assert!(takes_hover(&repo(false, false)));
        assert!(!takes_hover(&repo(true, false)));
        assert!(!takes_hover(&repo(false, true)));
        assert!(!takes_hover(&repo(true, true)));
        assert!(takes_hover(&Line::Group(GroupLine { chosen: false, ..group.clone() })));
        assert!(!takes_hover(&Line::Group(GroupLine { chosen: true, ..group })));
    }

    #[test]
    fn a_row_half_under_the_head_keeps_its_box_on_its_centre_line() {
        let lines = many(100);
        let columns = columns::columns(table_rect().width());
        // The first repository, dotfiles, half under the head row.
        let scroll = GROUP_HEIGHT + ROW_HEIGHT / 2.0;
        let row = row_at(&lines, 1, scroll);
        assert_eq!(row.center().y, body_rect(table_rect()).top());
        let ctx = context();
        let mut moved = scroll;
        frame(&ctx, table_rect(), &lines, Vec::new(), &mut moved).1.drop_without_applying_deltas();
        let (_, output) = frame(&ctx, table_rect(), &lines, Vec::new(), &mut moved);
        let boxes: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(shape) if shape.rect.size() == vec2(theme::CHECKBOX, theme::CHECKBOX) => Some(shape.rect),
                _ => None,
            })
            // Below the head's own box: the boxes whose centre is in the row's shown half.
            .filter(|found| {
                (found.left() - (row.left() + columns.check.left)).abs() < 1e-3
                    && found.center().y >= row.center().y
                    && found.center().y < row.bottom()
            })
            .collect();
        output.drop_without_applying_deltas();
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        assert_eq!(boxes[0].center().y, row.center().y, "the box left the row's centre line");
        // A click on the hidden half of its name does nothing; on the hidden half of its box it is the head's box.
        let hidden = row.top() + ROW_HEIGHT / 4.0;
        let name = pos2(row.left() + columns.name.left + columns.name.width / 2.0, hidden);
        assert!(click_scrolled(&lines, name, scroll).iter().all(|did| *did == Did::None));
        let tick = pos2(row.left() + columns.check.left + theme::CHECKBOX / 2.0, hidden);
        assert_eq!(said_once(&click_scrolled(&lines, tick, scroll)), Did::TickedAll);
        // The shown half of the box ticks it.
        let shown = pos2(tick.x, row.center().y + theme::CHECKBOX / 4.0);
        assert_eq!(said_once(&click_scrolled(&lines, shown, scroll)), Did::Ticked("dotfiles".to_owned()));
    }

    #[test]
    fn a_long_group_name_and_note_are_cut_inside_the_row() {
        let long = GroupLine {
            key: "group::long".to_owned(),
            name: "a-group-whose-name-goes-on-and-on-".repeat(8),
            note: "a note that says a great deal more than it needs to, ".repeat(8),
            collapsed: false,
            chosen: false,
        };
        let lines = vec![Line::Group(long)];
        let ctx = context();
        let mut scroll = 0.0;
        frame(&ctx, table_rect(), &lines, Vec::new(), &mut scroll).1.drop_without_applying_deltas();
        let (_, output) = frame(&ctx, table_rect(), &lines, Vec::new(), &mut scroll);
        let texts: Vec<(String, f32)> = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.pos.x + text.galley.size().x)),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        let columns = columns::columns(table_rect().width());
        let name = texts.iter().find(|(text, _)| text.starts_with("a-group")).unwrap();
        let note = texts.iter().find(|(text, _)| text.starts_with("a note")).unwrap();
        assert!(name.0.ends_with('…') && note.0.ends_with('…'), "{name:?} {note:?}");
        assert!(name.1 <= table_rect().left() + columns.name.right() + 0.5, "the name runs past its column: {name:?}");
        assert!(note.1 <= table_rect().right() - theme::ROW_PAD_RIGHT + 0.5, "the note runs past the row: {note:?}");
    }

    #[test]
    fn the_lines_sit_under_the_head_one_after_another() {
        let lines = demo();
        let heights: Vec<f32> = lines.iter().map(height).collect();
        let body = body_rect(table_rect());
        assert_eq!(body.top(), table_rect().top() + HEAD_HEIGHT);
        let rows = placed(body, &heights, 0.0);
        assert_eq!(rows.len(), 16, "the demo's sixteen lines fit");
        assert_eq!(rows[0].1.top(), body.top());
        assert_eq!(rows[0].1.height(), GROUP_HEIGHT);
        assert_eq!(rows[1].1.height(), ROW_HEIGHT);
        for pair in rows.windows(2) {
            assert_eq!(pair[0].1.bottom(), pair[1].1.top());
        }
        // The drawing's last row ends where its table does: 40 + 4 × 36 + 12 × 44 = 712.
        assert_eq!(rows[15].1.bottom() - table_rect().top(), 712.0);
    }
}
