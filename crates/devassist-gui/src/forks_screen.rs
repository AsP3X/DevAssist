//! The forks of one repository, a screen of the Repositories page: it takes the table's place while the core has
//! an overview of forks open (`Repos::forks`). A head row and a row a fork, in the core's order, each cell in the
//! core's words; the highlighted fork (`ForksView::chosen`) on the selection's fill. The status bar sums the forks
//! up and names their folder and when they were fetched. Back to the repositories is the shell's round button.
//!
//! [`plan`] decides every cell, pure, from the core's overview; [`show`] draws it and says what a press asked
//! ([`Command`]), which [`apply`] does through the core's methods. The window keeps its own scroll; the core's
//! `scroll` and `filtering` are the terminal app's.

use devassist_core::repos::forks::{DayForm, ForkState, day, fork_ahead};
use devassist_core::repos::gitops::ForkInfo;
use devassist_core::repos::state::{ForkSort, ForksView, Repos};
use egui::{Align2, Color32, FontFamily, Pos2, Rect, Role, Sense, Ui, WidgetInfo, pos2};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::columns::{self, ForkColumns, Span};
use crate::repos_screen;
use crate::repos_view::Status;
use crate::shell::Layout;
use crate::table::{self, Fill};
use crate::theme::{self, CELL_ICON_GAP, ICON_SMALL, Mode, Palette, RADIUS_PANEL, ROW_HEIGHT, RULE_LABEL_GAP, TEXT};
use crate::widgets::{self, draw_icon, font};
use crate::{frost, overlay};

/// The head over the forks' names.
const FORK: &str = "Fork";
/// The head over their branches.
const BRANCHES: &str = "Branches";
/// The head over their pull requests.
const PULL_REQUESTS: &str = "Pull requests";
/// The head over the date of their newest commit.
const NEWEST_COMMIT: &str = "Newest commit";
/// The head over their state.
const STATE: &str = "State";
/// The ahead column's head when the core cannot tell what it compares with.
const AHEAD_OF_THE_ORIGINAL: &str = "Ahead of the original";
/// A fork with no pull requests.
const NO_PULLS: &str = "None";
/// A fork with nothing ahead of the original.
const NOTHING_AHEAD: &str = "Nothing";

/// The ahead column's head: what the forks are compared with, as the core names it (`AsP3X/main`).
fn ahead_of(base: &str) -> String {
    format!("Ahead of {base}")
}

/// How far a fork is ahead.
fn ahead(commits: u32) -> String {
    format!("{commits} ahead")
}

/// The screen's title: the forks of the original's repository, by its name (`owner/name` without its owner).
fn forks_of(original: &str) -> String {
    let name = original.rsplit_once('/').map_or(original, |(_, name)| name);
    format!("Forks of {name}")
}

/// Whether the Repositories page shows the forks of a repository instead of its table: while the core has them open.
#[must_use]
pub fn shown(repos: &Repos) -> bool {
    repos.forks.is_some()
}

/// The top row's title while the forks show: "Forks of {name}"; `None` while they do not.
#[must_use]
pub fn title(repos: &Repos) -> Option<String> {
    repos.forks.as_ref().map(|view| forks_of(&view.original))
}

/// What the status bar says while the forks show: the core's sentence, kept whole, then the folder of the forks
/// (cut from its left) and, when the core knows it, the day they were fetched; these give way first in a narrow
/// window.
#[must_use]
pub fn status(view: &ForksView) -> Status {
    let mut notes = vec![(Icon::Folder, view.dest.display().to_string())];
    if let Some(fetched) = view.fetched() {
        notes.push((Icon::Clock, format!("Fetched {}", day(fetched, DayForm::Words))));
    }
    Status { text: view.summary(), error: false, notes, keep_text: true, path: Some(0) }
}

/// How a cell's words are drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    /// In the text colour.
    Plain,
    /// In the text colour, medium.
    Medium,
    /// In the quiet colour: nothing to look at.
    Quiet,
    /// In the warning colour, medium.
    Warning,
}

/// A cell's words and how they are drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Cell {
    /// The words.
    pub(crate) text: String,
    /// How.
    pub(crate) tone: Tone,
}

impl Cell {
    fn new(text: impl Into<String>, tone: Tone) -> Cell {
        Cell { text: text.into(), tone }
    }
}

/// A fork's row, as the core says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Row {
    /// The fork's owner: how the core names it.
    pub(crate) owner: String,
    /// `owner/name`.
    pub(crate) fork: Cell,
    /// How many branches it has.
    pub(crate) branches: Cell,
    /// How many pull requests it has, or "None".
    pub(crate) pulls: Cell,
    /// The commits it is ahead, for the rule: 0 for nothing to show.
    pub(crate) ahead: u32,
    /// "{n} ahead", or "Nothing".
    pub(crate) ahead_words: Cell,
    /// The day of its newest commit; empty when nothing was fetched from it.
    pub(crate) newest: Cell,
    /// Its state in the core's words, with a capital.
    pub(crate) state: Cell,
    /// The icon before the state, when it has one.
    pub(crate) state_icon: Option<Icon>,
    /// It is the highlighted fork.
    pub(crate) chosen: bool,
}

/// Which column's head carries the sort's arrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Sorted {
    /// By name.
    Fork,
    /// Most changes first.
    Ahead,
    /// Newest commit first.
    Newest,
}

/// Everything the screen draws on its panel, decided before it is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The panel.
    panel: Rect,
    /// Where the columns are.
    pub(crate) columns: ForkColumns,
    /// The ahead column's head.
    pub(crate) ahead_head: String,
    /// The column sorted by.
    pub(crate) sorted: Sorted,
    /// The rows, in the core's order.
    pub(crate) rows: Vec<Row>,
    /// Why the panel shows no fork, trimmed, and whether it is an error: then no head row is drawn.
    pub(crate) empty: Option<(String, bool)>,
}

/// The row of `fork`, `chosen` when it is the highlighted one.
fn row(fork: &ForkInfo, chosen: bool) -> Row {
    let state = ForkState::of(fork);
    let quiet = matches!(state, ForkState::Unchanged | ForkState::NotFetched);
    let pulls = if fork.pulls == 0 { Cell::new(NO_PULLS, Tone::Quiet) } else { Cell::new(fork.pulls.to_string(), Tone::Plain) };
    let commits = fork_ahead(fork).unwrap_or(0);
    let ahead_words = if commits > 0 { Cell::new(ahead(commits), Tone::Medium) } else { Cell::new(NOTHING_AHEAD, Tone::Quiet) };
    let words = widgets::capitalised(state.words());
    let (state, state_icon) = match state {
        ForkState::Changes => (Cell::new(words, Tone::Medium), Some(Icon::Folder)),
        ForkState::Gone => (Cell::new(words, Tone::Warning), Some(Icon::CloudOff)),
        ForkState::Unchanged | ForkState::NotFetched => (Cell::new(words, Tone::Quiet), None),
    };
    Row {
        owner: fork.owner.clone(),
        fork: Cell::new(format!("{}/{}", fork.owner, fork.name), Tone::Medium),
        branches: Cell::new(fork.branches.to_string(), Tone::Plain),
        pulls,
        ahead: commits,
        ahead_words,
        newest: Cell::new(day(fork.date, DayForm::Words), if quiet { Tone::Quiet } else { Tone::Plain }),
        state,
        state_icon,
        chosen,
    }
}

/// What the screen draws on the panel at `panel` for the overview `view`.
#[must_use]
pub fn plan(view: &ForksView, panel: Rect) -> Plan {
    let chosen = view.chosen().map(|fork| fork.owner.clone());
    let rows = view.rows().into_iter().map(|fork| row(fork, chosen.as_ref() == Some(&fork.owner))).collect();
    let sorted = match view.sort {
        ForkSort::Changed => Sorted::Ahead,
        ForkSort::Newest => Sorted::Newest,
        ForkSort::Name => Sorted::Fork,
    };
    Plan {
        panel,
        columns: columns::fork_columns(panel.width()),
        ahead_head: view.base().map_or_else(|| AHEAD_OF_THE_ORIGINAL.to_owned(), ahead_of),
        sorted,
        rows,
        empty: view.empty_message().map(|message| (message.trim().to_owned(), view.error.is_some())),
    }
}

/// What a press on the screen asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Highlight the fork of this owner (a click on its row).
    Pick(String),
}

/// Does what `command` asks of `repos`, through the core's methods.
pub fn apply(command: Command, repos: &mut Repos) {
    match command {
        Command::Pick(owner) => repos.pick_fork(&owner),
    }
}

/// What the screen keeps from one frame to the next.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// How far the rows are moved up.
    scroll: f32,
    /// The owner of the fork highlighted in the last frame: when the highlight moves, it is followed into view.
    chosen: Option<String>,
}

/// The rows' heights, from the top.
fn heights(plan: &Plan) -> Vec<f32> {
    vec![ROW_HEIGHT; plan.rows.len()]
}

/// The face and colour of a cell's words.
fn ink(tone: Tone, palette: Palette) -> (FontFamily, Color32) {
    match tone {
        Tone::Plain => (FontFamily::Proportional, palette.on_glass),
        Tone::Medium => (assets::medium_family(), palette.on_glass),
        Tone::Quiet => (FontFamily::Proportional, palette.quiet),
        Tone::Warning => (assets::medium_family(), palette.warning),
    }
}

/// Draws `cell` from `left`, centred on `y`, cut to `room`; gives back whether it was cut.
fn draw_cell(ui: &Ui, left: f32, y: f32, cell: &Cell, room: f32, palette: Palette) -> bool {
    let (family, colour) = ink(cell.tone, palette);
    let face = font(TEXT, family);
    let said = widgets::ellipsis(&cell.text, &face, room, ui);
    let cut = said != cell.text;
    if !said.is_empty() {
        ui.painter().text(pos2(left, y), Align2::LEFT_CENTER, said, face, colour);
    }
    cut
}

/// Draws the screen on the panel of `layout` from `plan` (from [`plan`] this frame), with `sheet` the panel's frosted
/// sheet; `screen` keeps the scroll between frames. With `presses` (no overlay over it, and the click not the rest of
/// one that closed an overlay) gives back what a press asked.
pub fn show(ui: &mut Ui, layout: &Layout, plan: &Plan, sheet: Option<&Sheet>, mode: Mode, screen: &mut Screen, presses: bool) -> Option<Command> {
    let palette = theme::palette(mode);
    let panel = layout.panel;
    let sheet = sheet?;
    let window = Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size());
    frost::paint(ui.painter(), window, panel, RADIUS_PANEL, sheet);
    if let Some((message, error)) = &plan.empty {
        let colour = if *error { palette.danger } else { palette.quiet };
        let face = font(TEXT, FontFamily::Proportional);
        let said = widgets::ellipsis(message, &face, panel.width() - 2.0 * theme::PANEL_TEXT_INSET, ui);
        ui.painter().with_clip_rect(panel).text(panel.center(), Align2::CENTER_CENTER, said, face, colour);
        screen.chosen = None;
        return None;
    }
    let presses = presses && !overlay::stale_click(ui.ctx());
    let heights = heights(plan);
    let body = table::body_rect(panel);
    let chosen = plan.rows.iter().position(|row| row.chosen);
    let owner = chosen.map(|index| plan.rows[index].owner.clone());
    if owner != screen.chosen {
        if let Some(index) = chosen {
            screen.scroll = repos_screen::follow(&heights, index, screen.scroll, body.height());
        }
        screen.chosen = owner;
    }
    if ui.rect_contains_pointer(panel) {
        screen.scroll -= ui.input(|input| input.smooth_scroll_delta().y);
    }
    screen.scroll = table::held(screen.scroll, heights.iter().sum(), body.height());

    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(panel.intersect(outer_clip));
    draw_head(ui, table::head_rect(panel), plan, palette);
    ui.set_clip_rect(body.intersect(outer_clip));
    let mut asked = None;
    for (index, place) in table::placed(body, &heights, screen.scroll) {
        let row = &plan.rows[index];
        let shown = place.intersect(body);
        let response = ui.interact(shown, ui.scope_id().with(("fork-row", &row.owner)), Sense::CLICK);
        response.widget_info(|| WidgetInfo::selected(Role::Row, true, row.chosen, &row.fork.text));
        let hovered = !row.chosen && ui.rect_contains_pointer(shown);
        let fill = if row.chosen { theme::selected_fill(mode) } else { Color32::TRANSPARENT };
        let under_line = index + 1 < plan.rows.len();
        table::draw_fill(ui.painter(), &Fill { row: place, shown, panel, under_line, hovered }, &[fill], palette);
        if draw_row(ui, place, &plan.columns, row, palette) {
            response.clone().on_hover_text(&row.fork.text);
        }
        if response.clicked() && presses {
            asked = Some(Command::Pick(row.owner.clone()));
        }
    }
    ui.set_clip_rect(outer_clip);
    asked
}

/// Paints the head row at `head`: its fill and line, each column's head, the sort's arrow after the sorted one's.
fn draw_head(ui: &Ui, head: Rect, plan: &Plan, palette: Palette) {
    let painter = ui.painter();
    table::draw_head_fill(painter, head, palette);
    let medium = font(TEXT, assets::medium_family());
    let y = head.center().y;
    let c = &plan.columns;
    let heads = [
        (Some(c.fork), FORK, Some(Sorted::Fork)),
        (c.branches, BRANCHES, None),
        (c.pulls, PULL_REQUESTS, None),
        (Some(c.ahead), plan.ahead_head.as_str(), Some(Sorted::Ahead)),
        (c.newest, NEWEST_COMMIT, Some(Sorted::Newest)),
        (Some(c.state), STATE, None),
    ];
    for (span, words, sorts) in heads {
        let Some(span) = span else { continue };
        let arrow = sorts == Some(plan.sorted);
        let room = span.width - if arrow { CELL_ICON_GAP + ICON_SMALL } else { 0.0 };
        let said = widgets::ellipsis(words, &medium, room, ui);
        let drawn = painter.text(pos2(head.left() + span.left, y), Align2::LEFT_CENTER, said, medium.clone(), palette.on_glass);
        if arrow {
            draw_icon(painter, pos2(drawn.right() + CELL_ICON_GAP + ICON_SMALL / 2.0, y), Icon::ArrowDown, ICON_SMALL, palette.quiet);
        }
    }
}

/// Paints a fork's cells in the row at `place`; gives back whether its name was cut.
fn draw_row(ui: &Ui, place: Rect, c: &ForkColumns, row: &Row, palette: Palette) -> bool {
    let y = place.center().y;
    let at = |span: Span| place.left() + span.left;
    let cut = draw_cell(ui, at(c.fork), y, &row.fork, c.fork.width, palette);
    for (span, cell) in [(c.branches, &row.branches), (c.pulls, &row.pulls), (c.newest, &row.newest)] {
        if let Some(span) = span {
            draw_cell(ui, at(span), y, cell, span.width, palette);
        }
    }
    table::draw_forward(ui.painter(), at(c.ahead), y, columns::forward(row.ahead, c.room), c.room, palette);
    let words_left = at(c.ahead) + columns::rule_length(c.room) + RULE_LABEL_GAP;
    draw_cell(ui, words_left, y, &row.ahead_words, place.left() + c.ahead.right() - words_left, palette);
    let mut left = at(c.state);
    if let Some(icon) = row.state_icon {
        let colour = if icon == Icon::CloudOff { palette.warning } else { palette.on_glass };
        draw_icon(ui.painter(), pos2(left + ICON_SMALL / 2.0, y), icon, ICON_SMALL, colour);
        left += ICON_SMALL + CELL_ICON_GAP;
    }
    draw_cell(ui, left, y, &row.state, place.left() + c.state.right() - left, palette);
    cut
}

/// The rectangle of the row of the fork of `owner` in the table on `panel`, its rows moved up by `scroll`, whole
/// (it may reach past the panel); `None` when no row has that owner.
#[cfg(test)]
pub(crate) fn row_rect(plan: &Plan, scroll: f32, owner: &str) -> Option<Rect> {
    let index = plan.rows.iter().position(|row| row.owner == owner)?;
    let body = table::body_rect(plan.panel);
    #[expect(clippy::cast_precision_loss, reason = "a few thousand rows")]
    let top = body.top() + index as f32 * ROW_HEIGHT - scroll;
    Some(Rect::from_min_max(pos2(body.left(), top), pos2(body.right(), top + ROW_HEIGHT)))
}

/// How far the screen's rows are moved up, for the tests.
#[cfg(test)]
impl Screen {
    pub(crate) fn scroll(&self) -> f32 {
        self.scroll
    }
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::gitops::ForkList;
    use egui::{Context, Event, Modifiers, PointerButton, RawInput, Shape, Stroke, TextureId, vec2};

    use super::*;
    use crate::shell::{self, Page, Shown};
    use crate::widgets::tests::context;

    fn sheet() -> Sheet {
        Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
    }

    /// The demo with the forks of api open, as `--show forks` opens them.
    fn forks_open() -> Repos {
        let mut repos = crate::demo::repos();
        crate::demo::show(&mut repos, "forks").unwrap();
        repos
    }

    fn view(repos: &Repos) -> &ForksView {
        repos.forks.as_ref().unwrap()
    }

    /// The design's panel without a toolbar.
    fn panel() -> Rect {
        shell::layout(vec2(1440.0, 900.0), true, false).panel
    }

    /// What one frame of the shell and the screen in a window of `size` drew and asked, with `events`: the shapes,
    /// whether back was pressed, and the screen's command.
    fn frame(ctx: &Context, size: [f32; 2], repos: &mut Repos, screen: &mut Screen, events: Vec<Event>) -> (Vec<Shape>, bool, Option<Command>) {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))), events, ..Default::default() };
        let (mut back, mut asked) = (false, None);
        let output = ctx.run_ui(input, |ui| {
            let layout = shell::layout(vec2(size[0], size[1]), true, false);
            let title = title(repos);
            let status = repos.forks.as_ref().map(status);
            let shown = Shown { mode: Mode::Light, waiting: 0, status: status.as_ref(), back: title.as_deref() };
            let (mut sidebar, mut page) = (true, Page::Repositories);
            back = shell::draw(ui, &layout, &mut sidebar, &mut page, shown);
            if let Some(view) = repos.forks.as_ref() {
                let plan = plan(view, layout.panel);
                asked = show(ui, &layout, &plan, Some(&sheet()), Mode::Light, screen, true);
            }
        });
        let shapes = output.shapes.iter().map(|clipped| clipped.shape.clone()).collect();
        output.drop_without_applying_deltas();
        (shapes, back, asked)
    }

    /// The words of every text shape among `shapes`.
    fn texts(shapes: &[Shape]) -> Vec<String> {
        shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Text(text) => Some(text.galley.text().to_owned()),
                _ => None,
            })
            .collect()
    }

    /// A click at `at` over frames: the pointer comes, presses and lets go.
    fn click(at: Pos2) -> Vec<Vec<Event>> {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        vec![vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()]
    }

    #[test]
    fn the_forks_take_the_tables_place_and_back_returns() {
        let mut repos = forks_open();
        assert!(shown(&repos));
        assert_eq!(title(&repos).as_deref(), Some("Forks of api"));
        let ctx = context();
        let mut screen = Screen::default();
        let toggle = shell::layout(vec2(1440.0, 900.0), true, false).toggle;
        let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let said = texts(&shapes);
        assert!(said.contains(&"Forks of api".to_owned()), "{said:?}");
        assert!(
            !said.contains(&"Repositories".to_owned()) || said.iter().filter(|text| *text == "Repositories").count() == 1,
            "the page's name is the sidebar's only"
        );
        let back = assets::icon(Icon::ArrowLeft).to_string();
        let toggle_icon = assets::icon(Icon::PanelLeft).to_string();
        assert!(said.contains(&back) && !said.contains(&toggle_icon), "the back button stands in the sidebar button's place");
        let mut pressed = false;
        for events in click(toggle.center()) {
            pressed |= frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, events).1;
        }
        assert!(pressed, "back was not pressed");
        repos.close_forks();
        assert!(!shown(&repos));
        let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let said = texts(&shapes);
        assert!(said.contains(&toggle_icon) && !said.contains(&back), "the sidebar button is back");
        assert!(!said.contains(&"Forks of api".to_owned()));
    }

    #[test]
    fn each_fork_row_says_what_the_core_says() {
        let repos = forks_open();
        let view = view(&repos);
        let plan = plan(view, panel());
        let rows = view.rows();
        assert_eq!(plan.rows.len(), 13);
        for (row, fork) in plan.rows.iter().zip(&rows) {
            assert_eq!(row.owner, fork.owner);
            assert_eq!(row.fork, Cell::new(format!("{}/{}", fork.owner, fork.name), Tone::Medium));
            assert_eq!(row.branches.text, fork.branches.to_string());
            assert_eq!(row.newest.text, day(fork.date, DayForm::Words));
            let state = ForkState::of(fork);
            assert_eq!(row.state.text, widgets::capitalised(state.words()));
            assert_eq!(row.ahead, fork_ahead(fork).unwrap_or(0));
            assert_eq!(row.chosen, view.chosen().is_some_and(|chosen| chosen.owner == fork.owner));
        }
        let by = |owner: &str| plan.rows.iter().find(|row| row.owner == owner).unwrap();
        let mkoenig = by("mkoenig");
        assert_eq!((mkoenig.pulls.text.as_str(), mkoenig.ahead_words.text.as_str(), mkoenig.newest.text.as_str()), ("2", "47 ahead", "23 Nov 2026"));
        assert_eq!((mkoenig.state.text.as_str(), mkoenig.state.tone, mkoenig.state_icon), ("Changes, folder", Tone::Medium, Some(Icon::Folder)));
        let petrov = by("a-petrov");
        assert_eq!((petrov.pulls.clone(), petrov.ahead_words.clone()), (Cell::new("None", Tone::Quiet), Cell::new("Nothing", Tone::Quiet)));
        assert_eq!((petrov.newest.tone, petrov.state.clone(), petrov.state_icon), (Tone::Quiet, Cell::new("No changes", Tone::Quiet), None));
        let gone = by("old-contrib");
        assert_eq!((gone.state.clone(), gone.state_icon), (Cell::new("Gone from GitHub", Tone::Warning), Some(Icon::CloudOff)));
        assert_eq!(gone.newest, Cell::new("11 Mar 2025", Tone::Plain), "a gone fork's date is plain");
        assert!(by("bob").chosen, "the drawing's highlight");
        assert_eq!(by("lin-wei").fork.text, "lin-wei/api-next");
        // A fork nothing was fetched from: no date, and the core's fourth state drawn as "No changes" is.
        let fetched_nothing = ForkInfo { branches: 0, ahead: None, date: 0, ..fork("x") };
        let r = row(&fetched_nothing, false);
        assert_eq!((r.newest.text.as_str(), r.state.clone()), ("", Cell::new("Not fetched", Tone::Quiet)));
        assert_eq!(plan.ahead_head, "Ahead of acme/main");
        assert_eq!(plan.empty, None);
    }

    #[test]
    fn a_fork_past_twenty_gets_a_broken_bar() {
        let mut repos = forks_open();
        let ctx = context();
        let mut screen = Screen::default();
        frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let plan = plan(view(&repos), panel());
        let bars = |owner: &str| -> Vec<(f32, f32)> {
            let place = row_rect(&plan, 0.0, owner).unwrap();
            let left = place.left() + plan.columns.ahead.left;
            shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Rect(rect)
                        if (rect.rect.height() - theme::RULE_BAR).abs() < 1e-3
                            && place.contains_rect(rect.rect)
                            && rect.fill == theme::palette(Mode::Light).on_glass =>
                    {
                        Some((rect.rect.left() - left, rect.rect.width()))
                    }
                    _ => None,
                })
                .collect()
        };
        // 47 ahead: a long piece, a gap, a short piece, as drawn; 9 ahead: one bar of nine commits.
        assert_eq!(bars("mkoenig"), [(1.0, 140.0), (147.0, 14.0)]);
        assert_eq!(bars("bob"), [(1.0, 72.0)]);
        assert_eq!(bars("a-petrov"), Vec::<(f32, f32)>::new());
        let mkoenig = plan.rows.iter().find(|row| row.owner == "mkoenig").unwrap();
        assert!(columns::forward(mkoenig.ahead, plan.columns.room).over);
    }

    #[test]
    fn the_sorted_column_carries_the_arrow() {
        let mut repos = forks_open();
        assert_eq!(plan(view(&repos), panel()).sorted, Sorted::Ahead);
        repos.set_fork_sort(ForkSort::Newest);
        assert_eq!(plan(view(&repos), panel()).sorted, Sorted::Newest);
        repos.set_fork_sort(ForkSort::Name);
        assert_eq!(plan(view(&repos), panel()).sorted, Sorted::Fork);
        // Drawn after the sorted head's words: one arrow, in the Fork column's head.
        let ctx = context();
        let mut screen = Screen::default();
        frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let arrow = assets::icon(Icon::ArrowDown).to_string();
        let head = table::head_rect(panel());
        let arrows: Vec<Rect> = shapes
            .iter()
            .filter_map(|shape| match shape {
                Shape::Text(text) if text.galley.text() == arrow => Some(text.galley.rect.translate(text.pos.to_vec2())),
                _ => None,
            })
            .collect();
        assert_eq!(arrows.len(), 1, "{arrows:?}");
        let fork = plan(view(&repos), panel()).columns.fork;
        assert!(head.contains(arrows[0].center()) && arrows[0].center().x < head.left() + fork.right(), "{arrows:?}");
    }

    /// A fetched fork of api by `owner` with one branch and nothing of its own.
    fn fork(owner: &str) -> ForkInfo {
        ForkInfo {
            owner: owner.to_owned(),
            name: "api".to_owned(),
            branches: 1,
            pulls: 0,
            ahead: Some(0),
            date: 1_700_000_000,
            folder: false,
            gone: false,
        }
    }

    /// A held core with `count` made-up forks of api open, the first highlighted.
    fn many(count: usize) -> Repos {
        let mut repos = crate::demo::repos();
        let forks = (0..count)
            .map(|index| ForkInfo { date: 1_700_000_000 - i64::try_from(index).unwrap() * 86_400, ..fork(&format!("owner-{index:03}")) })
            .collect();
        repos.hold_forks("api", ForkList { original: "acme/api".to_owned(), forks, ..ForkList::default() });
        repos.clear_marks();
        repos.pick("api");
        repos.open_forks_plain();
        repos
    }

    #[test]
    fn two_hundred_forks_scroll_and_the_highlight_stays_in_view() {
        let size = [960.0, 600.0];
        let mut repos = many(200);
        let ctx = context();
        let mut screen = Screen::default();
        let panel = shell::layout(vec2(size[0], size[1]), true, false).panel;
        let body = table::body_rect(panel);
        let in_view = |repos: &Repos, screen: &Screen| {
            let plan = plan(view(repos), panel);
            let owner = view(repos).chosen().unwrap().owner.clone();
            body.contains_rect(row_rect(&plan, screen.scroll(), &owner).unwrap())
        };
        frame(&ctx, size, &mut repos, &mut screen, Vec::new());
        assert!(in_view(&repos, &screen));
        for (moved, owner) in [(150, "owner-150"), (49, "owner-199"), (-120, "owner-079"), (-79, "owner-000")] {
            repos.move_fork(moved);
            frame(&ctx, size, &mut repos, &mut screen, Vec::new());
            assert_eq!(view(&repos).chosen().unwrap().owner, owner);
            assert!(in_view(&repos, &screen), "{owner} out of view at {}", screen.scroll());
        }
        // The wheel moves the rows, held to the end.
        let wheel = Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: vec2(0.0, -1.0e6),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        };
        frame(&ctx, size, &mut repos, &mut screen, vec![Event::PointerMoved(body.center())]);
        for _ in 0..30 {
            frame(&ctx, size, &mut repos, &mut screen, vec![wheel.clone()]);
        }
        for _ in 0..30 {
            frame(&ctx, size, &mut repos, &mut screen, Vec::new());
        }
        assert!((screen.scroll() - (200.0 * ROW_HEIGHT - body.height())).abs() < 1e-3, "{}", screen.scroll());
        // A click on a row highlights it, and nothing else.
        let plan = plan(view(&repos), panel);
        let last = row_rect(&plan, screen.scroll(), "owner-199").unwrap();
        let mut asked = Vec::new();
        for events in click(last.center()) {
            asked.extend(frame(&ctx, size, &mut repos, &mut screen, events).2);
        }
        assert_eq!(asked, [Command::Pick("owner-199".to_owned())]);
        for command in asked {
            apply(command, &mut repos);
        }
        assert_eq!(view(&repos).chosen().unwrap().owner, "owner-199");
        assert!(in_view(&repos, &screen));
    }

    /// What a case does to the overview, and whether the panel then says an error.
    type Case = (fn(&mut ForksView), bool);

    #[test]
    fn an_error_or_an_empty_list_is_said_in_the_cores_words() {
        let ctx = context();
        let cases: [Case; 4] = [
            (|view| view.forks.clear(), false),
            (|view| view.loading = true, false),
            (|view| view.error = Some("no folder".to_owned()), true),
            (|view| view.filter = "nobody".to_owned(), false),
        ];
        for (set, error) in cases {
            let mut repos = forks_open();
            let view = repos.forks.as_mut().unwrap();
            set(view);
            if error || view.loading {
                view.forks.clear();
            }
            let said = view.empty_message().unwrap();
            let plan = plan(view, panel());
            assert_eq!(plan.empty, Some((said.trim().to_owned(), error)));
            let mut screen = Screen::default();
            frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
            let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
            let palette = theme::palette(Mode::Light);
            let drawn: Vec<(String, Color32)> = shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Text(text) if panel().contains(text.pos) => Some((text.galley.text().to_owned(), text.fallback_color)),
                    _ => None,
                })
                .collect();
            assert_eq!(drawn, [(said.trim().to_owned(), if error { palette.danger } else { palette.quiet })], "no head row, the core's words");
        }
    }

    #[test]
    fn the_status_bar_counts_the_forks() {
        let repos = forks_open();
        let status = status(view(&repos));
        assert_eq!(status.text, "13 forks. 6 have changes and a folder. 1 is gone from GitHub. 27 branches and 9 pull requests are saved.");
        assert_eq!(status.text, view(&repos).summary());
        let dest = view(&repos).dest.display().to_string();
        assert!(dest.ends_with("api-fork"), "{dest}");
        assert_eq!(status.notes, [(Icon::Folder, dest), (Icon::Clock, "Fetched 25 Nov 2026".to_owned())]);
        assert!(!status.error && status.keep_text);
        assert_eq!(status.path, Some(0));
        // Without a fetch date, the folder alone.
        let mut unfetched = many(2);
        assert_eq!(status_of(&mut unfetched).notes.len(), 1);
        // A narrow bar: the fetched day gives way first, then the folder; never the sentence.
        let words = [300.0, 110.0];
        let note = theme::STATUS_GAP + theme::STATUS_ICON + theme::STATUS_ICON_GAP;
        assert_eq!(shell::notes_fit(1200.0, 600.0, &words, Some(0)), [Some(300.0), Some(110.0)]);
        assert_eq!(shell::notes_fit(600.0 + 2.0 * note + 400.0, 600.0, &words, Some(0)), [Some(300.0), None]);
        assert_eq!(shell::notes_fit(600.0 + note + 200.0, 600.0, &words, Some(0)), [Some(200.0), None], "the folder is cut from its left");
        assert_eq!(shell::notes_fit(600.0 + note + 100.0, 600.0, &words, Some(0)), [None, None]);
        assert_eq!(shell::notes_fit(100.0, 600.0, &words, Some(0)), [None, None]);
    }

    fn status_of(repos: &mut Repos) -> Status {
        status(view(repos))
    }
}
