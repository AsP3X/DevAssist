//! The repositories table: a head row and the lines in view on a frosted panel, each cell drawn from what
//! [`crate::repos_view`] decided, at the places [`crate::columns`] gives. It draws and says what was clicked; it
//! changes nothing itself.
//!
//! The lines are of two heights and are placed by hand ([`in_view`]); only those in view are drawn, so a table of
//! a thousand repositories costs a frame no more than one of twelve. The wheel over the table moves it.

use egui::epaint::CornerRadiusF32;
use egui::{Align2, Color32, FontFamily, Painter, Pos2, Rect, Role, Sense, Ui, WidgetInfo, pos2, vec2};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::columns::{self, Columns, Span};
use crate::frost;
use crate::repos_view::{Changes, GroupLine, Line, Origin, Pulls, RepoLine, Tone};
use crate::theme::{
    self, CELL_ICON_GAP, GROUP_FILL_OPACITY, GROUP_HEIGHT, GROUP_ROW_GAP, HAIRLINE, HEAD_FILL_OPACITY, HEAD_HEIGHT, HOVER_OPACITY, ICON, ICON_SMALL,
    LINE_OPACITY, Mode, NAME_ICON_GAP, PROGRESS_HEIGHT, PROGRESS_LEAST, PROGRESS_TRACK_OPACITY, PULL_GAP, PULL_ICON_GAP, Palette, RADIUS_PANEL,
    ROW_HEIGHT, RULE_BAR, RULE_BAR_TOP, RULE_BREAK_END, RULE_BREAK_GAP, RULE_FIFTH_OPACITY, RULE_HEIGHT, RULE_LABEL_GAP, RULE_LEGEND_GAP, RULE_TICK,
    RULE_TICK_FIFTH, RULE_TICK_OPACITY, RULE_WIDTH, TEXT, TEXT_SMALL,
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
    if widgets::checkbox(ui, tick_rect(head, &columns), head_tick(lines), palette, SELECT_ALL).clicked() {
        did = Did::TickedAll;
    }

    ui.set_clip_rect(body.intersect(outer_clip));
    for (index, row) in placed(body, &heights, *scroll) {
        let under_line = index + 1 < lines.len();
        let shown = row.intersect(body);
        match &lines[index] {
            Line::Group(group) => {
                let response = ui.interact(shown, ui.scope_id().with(("table-group", &group.key)), Sense::click());
                response.widget_info(|| WidgetInfo::labeled(Role::Button, true, &group.name));
                let fill = if group.chosen { theme::selected_fill(mode) } else { soft(palette, GROUP_FILL_OPACITY) };
                draw_fill(ui.painter(), row, fill, under_line, palette, ui.rect_contains_pointer(shown));
                draw_group(ui.painter(), row, &columns, group, palette);
                if response.clicked() {
                    did = Did::Folded(group.key.clone());
                }
            }
            Line::Repo(repo) => {
                let response = ui.interact(shown, ui.scope_id().with(("table-row", &repo.key)), Sense::click());
                response.widget_info(|| WidgetInfo::labeled(Role::Row, true, &repo.name));
                let fill = if repo.chosen { theme::selected_fill(mode) } else { Color32::TRANSPARENT };
                draw_fill(ui.painter(), row, fill, under_line, palette, ui.rect_contains_pointer(shown));
                draw_repo(ui, row, &columns, repo, palette);
                let tick = if repo.ticked { Tick::On } else { Tick::Off };
                if widgets::checkbox(ui, tick_rect(row, &columns).intersect(body), tick, palette, &select(&repo.name)).clicked() {
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

/// Paints a line's fill, the hover's over it when `hovered`, and the line under it when `under_line`.
fn draw_fill(painter: &Painter, row: Rect, fill: Color32, under_line: bool, palette: Palette, hovered: bool) {
    painter.rect_filled(row, 0.0, fill);
    if hovered {
        painter.rect_filled(row, 0.0, soft(palette, HOVER_OPACITY));
    }
    if under_line {
        draw_line_under(painter, row, palette);
    }
}

/// Paints the line along the bottom of `row`.
fn draw_line_under(painter: &Painter, row: Rect, palette: Palette) {
    painter.rect_filled(Rect::from_min_max(pos2(row.left(), row.bottom() - HAIRLINE), row.max), 0.0, soft(palette, LINE_OPACITY));
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
fn draw_group(painter: &Painter, row: Rect, columns: &Columns, group: &GroupLine, palette: Palette) {
    let y = row.center().y;
    let arrow = if group.collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
    draw_icon(painter, pos2(row.left() + columns.check.left + ICON / 2.0, y), arrow, ICON, palette.on_glass);
    let name = painter.text(
        pos2(row.left() + columns.check.right() + GROUP_ROW_GAP, y),
        Align2::LEFT_CENTER,
        &group.name,
        font(TEXT, assets::medium_family()),
        palette.on_glass,
    );
    painter.text(pos2(name.right() + GROUP_ROW_GAP, y), Align2::LEFT_CENTER, &group.note, font(TEXT_SMALL, FontFamily::Proportional), palette.quiet);
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
    let none = pulls.open + pulls.merged + pulls.closed + pulls.new + pulls.changed == 0;
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
        let ctx = context();
        let mut scroll = 0.0;
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
        let heights: Vec<f32> = lines.iter().map(height).collect();
        placed(body_rect(table_rect()), &heights, 0.0).into_iter().find(|(at, _)| *at == index).map(|(_, row)| row).unwrap()
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

    /// Draws a table of `lines` where the design has it, over the lake in `mode`, with the pointer at `pointer`, and
    /// writes the picture to `target/shots/{name}`.
    fn draw_picture(lines: &[Line], pointer: Option<Pos2>, mode: Mode, name: &str) {
        let scene = Scene { size: WINDOW, pixels_per_point: 1.0, mode, backdrop: "lake".to_owned(), blur: true };
        let (wall, mut bitmap) = picture::wallpaper_window(&scene);
        let panel = sheet_picture(&wall, &scene, Surface::Panel).unwrap();
        let sheet = Sheet { texture: TextureId::User(1), uv: panel.uv, edge: panel.edge };
        let ctx = context();
        let mut textures = HashMap::from([(sheet.texture, panel.bitmap.image())]);
        let mut scroll = 0.0;
        let mut shapes = Vec::new();
        // The first frame lays the faces out; the second is the one painted.
        for _ in 0..2 {
            let events = pointer.map(Event::PointerMoved).into_iter().collect();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), events, ..Default::default() };
            let mut output = ctx.run_ui(input, |ui| {
                show(ui, table_rect(), lines, &sheet, theme::palette(mode), mode, &mut scroll);
            });
            picture::take_textures(std::mem::take(&mut output.textures_delta), &mut textures);
            shapes = std::mem::take(&mut output.shapes);
        }
        picture::paint(&mut bitmap, &ctx.tessellate(shapes, 1.0), &textures);
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/shots").join(name);
        crate::screenshot::write_png(&path, &bitmap.image()).unwrap();
        eprintln!("wrote {}", path.display());
    }

    #[test]
    #[ignore = "writes pictures for a person to look at; run by hand"]
    fn draw_the_demo_table_to_pictures() {
        draw_picture(&demo(), None, Mode::Light, "table-light.png");
        draw_picture(&demo(), None, Mode::Dark, "table-dark.png");
        // The states the demo does not show: a chosen row, the pointer over another, a fetch running, bars past the
        // scale, GitHub knowing no pull requests, a folded group.
        let states: Vec<Line> = demo()
            .into_iter()
            .map(|line| match line {
                Line::Repo(repo) => Line::Repo(match repo.name.as_str() {
                    "web" => RepoLine { chosen: true, ..repo },
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
