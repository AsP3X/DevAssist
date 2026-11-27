//! The forks of one repository, a screen of the Repositories page: it takes the table's place while the core has
//! an overview of forks open (`Repos::forks`). In the top row "Read again" and "Open the folder" and the main button
//! "Fetch the forks"; in the toolbar the filter, the segments "All" and "With changes" and the sort; a head row and
//! a row a fork, in the core's order, each cell in the core's words, with its buttons that open its folder and its
//! page on GitHub; the highlighted fork (`ForksView::chosen`) on the selection's fill. The status bar says what the
//! core said since the person last did something here (`Repos::fork_notice`), else the forks' sentence, their folder
//! and when they were fetched. Back to the repositories is the shell's round button, Esc or Backspace.
//!
//! [`plan`] decides every cell and control, from the core's overview; [`show`] draws them and says what the keys and
//! presses asked ([`Command`]), which [`apply_all`] does through the core's methods. The window keeps its own scroll
//! and its own filter field; the core's `scroll` and `filtering` are the terminal app's.
//!
//! Where the window differs from the terminal app: `q` and Left do not go back (Esc and Backspace do), `/` keeps the
//! filter's text, and Esc in the filter clears its text first, then leaves it.

use devassist_core::repos::forks::{DayForm, ForkState, day, fork_ahead};
use devassist_core::repos::gitops::ForkInfo;
use devassist_core::repos::state::{ForkSort, ForksView, Mode as CoreMode, Repos};
use egui::{
    Align2, Color32, Context, Event, FocusDirection, FontFamily, Id, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Response, Role, Sense, Ui,
    Vec2, WidgetInfo, pos2,
};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::columns::{self, ForkColumns, Span};
use crate::repos_screen::{self, Fit, Top};
use crate::repos_view::Status;
use crate::shell::{self, Layout, Wants};
use crate::table::{self, Fill};
use crate::theme::{
    self, CELL_ICON_GAP, FIELD_MIN_WIDTH, FIELD_WIDTH, HOVER_OPACITY, ICON, ICON_SMALL, Mode, PAGE_ROWS, PANEL_TEXT_INSET, Palette, RADIUS_PANEL,
    ROW_BUTTON, ROW_BUTTON_RADIUS, ROW_HEIGHT, RULE_LABEL_GAP, STATE_GAP, TEXT, TOOLBAR_GAP,
};
use crate::widgets::{self, Item, Segment, draw_icon, font};
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
/// The top row's action: read the forks' folder again.
const READ_AGAIN: &str = "Read again";
/// The top row's action and a row's button: open the folder of the fork (the highlighted one, or the row's).
const OPEN_THE_FOLDER: &str = "Open the folder";
/// A row's button: open the fork's page on GitHub.
const OPEN_ON_GITHUB: &str = "Open on GitHub";
/// The top row's main button: fetch the forks of the repository.
const FETCH_THE_FORKS: &str = "Fetch the forks";
/// What the filter field says while it is empty.
const FILTER_HINT: &str = "Filter by owner or name";
/// The segment that shows every fork.
const ALL: &str = "All";
/// The segment that shows the forks with commits of their own.
const WITH_CHANGES: &str = "With changes";
/// The filter field's glass pane.
const FILTER_PANE: &str = "forks-filter";
/// The segments' glass pane.
const SEGMENTS_PANE: &str = "forks-segments";
/// The sort capsule's glass pane.
const SORT_PANE: &str = "forks-sort";
/// The glass pane of the top row's actions.
const ACTIONS_PANE: &str = "forks-actions";
/// The places of the top row's actions among its capsule's items ([`top_items`]): Read again.
const TOP_READ: usize = 0;
/// Open the folder.
const TOP_OPEN: usize = 1;

/// The ahead column's head: what the forks are compared with, as the core names it (`AsP3X/main`).
fn ahead_of(base: &str) -> String {
    format!("Ahead of {base}")
}

/// How far a fork is ahead.
fn ahead(commits: u32) -> String {
    format!("{commits} ahead")
}

/// The screen's title: the forks of the original's repository, by its name (`owner/name` without its owner); by
/// the repository's own name when the original names none.
fn forks_of(view: &ForksView) -> String {
    let name = view.original.rsplit_once('/').map_or(view.original.as_str(), |(_, name)| name);
    let name = if name.trim().is_empty() { view.repo.as_str() } else { name };
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
    repos.forks.as_ref().map(forks_of)
}

/// What the status bar says while the forks show; `None` while they do not. Its words: what the core said since the
/// person last did something to the forks (`Repos::fork_notice`), in the danger colour when it is an error; else,
/// while rows show, an error of the last read (until a read goes well); else the core's sentence about the forks.
/// They are kept whole; the folder of the forks (cut from its left) and, when the core knows it, the day they were
/// fetched give way first in a narrow window.
#[must_use]
pub fn status(repos: &Repos) -> Option<Status> {
    let view = repos.forks.as_ref()?;
    let dest = view.dest.display().to_string();
    // No folder known, no note with an icon and no words.
    let mut notes: Vec<(Icon, String)> = (!dest.is_empty()).then_some((Icon::Folder, dest)).into_iter().collect();
    let path = (!notes.is_empty()).then_some(0);
    if let Some(fetched) = view.fetched() {
        notes.push((Icon::Clock, format!("Fetched {}", day(fetched, DayForm::Words))));
    }
    let (text, error) = match (repos.fork_notice(), &view.error) {
        (Some((said, error)), _) => (said.to_owned(), error),
        (None, Some(error)) if !view.rows().is_empty() => (widgets::capitalised(error.trim()), true),
        (None, _) => (view.summary(), false),
    };
    Some(Status { text, error, notes, keep_text: true, path })
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
    /// It has a folder of its own: its row has the button that opens it.
    pub(crate) folder: bool,
    /// It is on GitHub: its row has the button that opens its page there.
    pub(crate) github: bool,
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

/// Why the panel shows no fork, in the core's words (trimmed), whether that is an error, and, for an error whose
/// words the core's sentence does not hold, those words as a second, quiet line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Empty {
    /// The core's sentence.
    pub(crate) text: String,
    /// It says the forks could not be read.
    pub(crate) error: bool,
    /// What went wrong, with a capital.
    pub(crate) detail: Option<String>,
}

/// The toolbar's controls: the filter field, the segments and the sort capsule at the right end.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Toolbar {
    /// The filter field.
    pub(crate) field: Rect,
    /// The segments "All" and "With changes".
    pub(crate) segments: Rect,
    /// The sort capsule.
    pub(crate) sort: Rect,
    /// The sort's words, with a capital.
    pub(crate) sort_words: String,
    /// The sort shows its icon alone, its words then its name and hover text.
    pub(crate) sort_compact: bool,
    /// How many forks there are, after "All".
    all: String,
    /// How many have commits of their own, after "With changes".
    changed: String,
}

/// Everything the screen draws, decided before it is drawn (and its glass baked).
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The repository whose forks these are, by row name.
    repo: String,
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
    /// Why the panel shows no fork: then no head row is drawn.
    pub(crate) empty: Option<Empty>,
    /// The top row's actions and its main button, when there is room for them.
    pub(crate) top: Option<Top>,
    /// The toolbar.
    pub(crate) toolbar: Toolbar,
    /// The filter's text, as the core keeps it.
    filter: String,
    /// Only the forks with commits of their own are shown.
    changed_only: bool,
}

impl Plan {
    /// Adds the glass of the screen's controls to the panes baked this frame. The main button is a fill, not glass.
    pub fn add_wants(&self, wants: &mut Wants) {
        widgets::add_glass(wants, FILTER_PANE, self.toolbar.field);
        widgets::add_glass(wants, SEGMENTS_PANE, self.toolbar.segments);
        widgets::add_glass(wants, SORT_PANE, self.toolbar.sort);
        if let Some(top) = &self.top {
            widgets::add_glass(wants, ACTIONS_PANE, top.actions);
        }
    }
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
    // A date before 1970 is a clock gone wrong, not a day: no date, as for a fork nothing was fetched from.
    let newest = if fork.date > 0 { day(fork.date, DayForm::Words) } else { String::new() };
    Row {
        owner: fork.owner.clone(),
        fork: Cell::new(format!("{}/{}", fork.owner, fork.name), Tone::Medium),
        branches: Cell::new(fork.branches.to_string(), Tone::Plain),
        pulls,
        ahead: commits,
        ahead_words,
        newest: Cell::new(newest, if quiet { Tone::Quiet } else { Tone::Plain }),
        state,
        state_icon,
        chosen,
        folder: fork.folder,
        github: !fork.gone,
    }
}

/// The top row's capsule: "Read again" and "Open the folder", with their icons only unless it is [`Fit::Full`].
/// Their places are `TOP_READ` and `TOP_OPEN`.
fn top_items(fit: Fit) -> [Item<'static>; 2] {
    let compact = fit != Fit::Full;
    [
        Item::Action { icon: Icon::RotateCw, words: READ_AGAIN, on: false, danger: false, compact },
        Item::Action { icon: Icon::FolderOpen, words: OPEN_THE_FOLDER, on: false, danger: false, compact },
    ]
}

/// The sort capsule's one item: the sort, whose words say which it is; its icon alone when `compact`.
fn sort_items(words: &str, compact: bool) -> [Item<'_>; 1] {
    [Item::Action { icon: Icon::ArrowUpDown, words, on: false, danger: false, compact }]
}

/// The segments: "All" with how many forks there are, "With changes" with how many have commits of their own; the
/// chosen one is the second when `changed_only`.
fn segment_items<'a>(all: &'a str, changed: &'a str, changed_only: bool) -> [Segment<'a>; 2] {
    [Segment { words: ALL, count: all, chosen: !changed_only }, Segment { words: WITH_CHANGES, count: changed, chosen: changed_only }]
}

/// The places of the filter field, the segments (`segments` wide) and the sort capsule (`full` wide with its words,
/// `compact` with its icon alone) in the toolbar row at `toolbar`: the field and the segments from the left, the sort
/// at the right end. The sort keeps its words as long as it can: beside a field of [`FIELD_WIDTH`], else beside a
/// field narrowed for it down to [`FIELD_MIN_WIDTH`]; then it shows its icon alone, beside a field as wide as is left
/// (down to none, in windows narrower than the app allows). Gives the three and whether the sort is compact; nothing
/// leaves the row or lies over another.
pub(crate) fn toolbar_places(toolbar: Rect, segments: f32, [full, compact]: [f32; 2]) -> (Rect, Rect, Rect, bool) {
    let (top, bottom) = (toolbar.top(), toolbar.bottom());
    // What is left for the sort beside a field `field` wide.
    let room = |field: f32| toolbar.width() - field - TOOLBAR_GAP - segments - TOOLBAR_GAP;
    let narrowed = |sort: f32| FIELD_WIDTH - (sort - room(FIELD_WIDTH));
    let (field_width, sort_width, is_compact) = if full <= room(FIELD_WIDTH) {
        (FIELD_WIDTH, full, false)
    } else if full <= room(FIELD_MIN_WIDTH) {
        (narrowed(full), full, false)
    } else if compact <= room(FIELD_WIDTH) {
        (FIELD_WIDTH, compact, true)
    } else {
        (narrowed(compact).max(0.0), compact, true)
    };
    let field = Rect::from_min_max(pos2(toolbar.left(), top), pos2((toolbar.left() + field_width).min(toolbar.right()), bottom));
    let segments_left = (field.right() + TOOLBAR_GAP).min(toolbar.right());
    let segments = Rect::from_min_max(pos2(segments_left, top), pos2((segments_left + segments).min(toolbar.right()), bottom));
    let sort_left = (toolbar.right() - sort_width).max(segments.right() + TOOLBAR_GAP).min(toolbar.right());
    let sort = Rect::from_min_max(pos2(sort_left, top), pos2(toolbar.right(), bottom));
    (field, segments, sort, is_compact)
}

/// What the screen draws for the overview `view` in `layout`, measured with `painter`'s faces: the top row's
/// controls (through the Repositories screen's fitting, [`repos_screen::top_place`]), the toolbar's, and the panel's.
#[must_use]
pub fn plan(view: &ForksView, layout: &Layout, painter: &Painter) -> Plan {
    let panel = layout.panel;
    let chosen = view.chosen().map(|fork| fork.owner.clone());
    let rows = view.rows().into_iter().map(|fork| row(fork, chosen.as_ref() == Some(&fork.owner))).collect();
    let sorted = match view.sort {
        ForkSort::Changed => Sorted::Ahead,
        ForkSort::Newest => Sorted::Newest,
        ForkSort::Name => Sorted::Fork,
    };
    let empty = view.empty_message().map(|message| {
        let error = view.error.is_some() && !view.loading;
        // The core's sentence names no detail of the error; its own words go under it, unless the sentence has them.
        let detail = view
            .error
            .as_deref()
            .map(str::trim)
            .filter(|detail| error && !detail.is_empty() && !message.to_lowercase().contains(&detail.to_lowercase()))
            .map(widgets::capitalised);
        Empty { text: message.trim().to_owned(), error, detail }
    });
    let counts = view.counts();
    let (all, changed) = (counts.all.to_string(), counts.changed.to_string());
    let sort_words = widgets::capitalised(view.sort.label());
    let segments_width = widgets::segments_width(painter, &segment_items(&all, &changed, view.changed_only));
    let sort_widths = [false, true].map(|compact| widgets::capsule_width(painter, &sort_items(&sort_words, compact)));
    let (field, segments, sort, sort_compact) = toolbar_places(layout.toolbar, segments_width, sort_widths);
    let title_width = widgets::width(painter, &forks_of(view), &shell::title_font());
    let actions = [Fit::Full, Fit::Compact].map(|fit| widgets::capsule_width(painter, &top_items(fit)));
    let main = [false, true].map(|compact| widgets::main_action_width(painter, FETCH_THE_FORKS, compact, layout.actions.height()));
    Plan {
        repo: view.repo.clone(),
        panel,
        columns: columns::fork_columns(panel.width()),
        ahead_head: view.base().map_or_else(|| AHEAD_OF_THE_ORIGINAL.to_owned(), ahead_of),
        sorted,
        rows,
        empty,
        top: repos_screen::top_place(shell::action_room(layout, title_width), actions, main),
        toolbar: Toolbar { field, segments, sort, sort_words, sort_compact, all, changed },
        filter: view.filter.clone(),
        changed_only: view.changed_only,
    }
}

/// What a key or a press on the screen asks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Highlight the fork of this owner (a click on its row).
    Pick(String),
    /// Move the highlight this many rows.
    Move(isize),
    /// Open the highlighted fork's folder.
    Open,
    /// Highlight the fork of this owner and open its folder (its row's button, a double click on its row).
    OpenRow(String),
    /// Open the highlighted fork's page on GitHub.
    Github,
    /// Highlight the fork of this owner and open its page on GitHub (its row's button).
    GithubRow(String),
    /// Fetch the forks.
    Fetch,
    /// Read the forks' folder again.
    Read,
    /// Show the run's progress dialog again.
    ShowRun,
    /// Sort the next way.
    Sort,
    /// Show only the forks with commits of their own (`true`), or all of them.
    ShowChanged(bool),
    /// Switch between all the forks and the ones with commits of their own.
    ToggleChanged,
    /// The filter's text is now this.
    Filter(String),
    /// Clear a filter that has text, else go back to the repositories.
    Escape,
    /// Go back to the repositories.
    Back,
}

/// Does what `command` asks of `repos`, through the core's methods. With no fork shown, opening one (its folder, its
/// page) does nothing and says nothing: no fork is highlighted.
pub fn apply(command: Command, repos: &mut Repos) {
    let rows = repos.forks.as_ref().is_some_and(|view| !view.rows().is_empty());
    match command {
        Command::Pick(owner) => repos.pick_fork(&owner),
        Command::Move(rows) => repos.move_fork(rows),
        Command::Open if rows => repos.open_fork_folder(),
        Command::OpenRow(owner) => {
            repos.pick_fork(&owner);
            repos.open_fork_folder();
        }
        Command::Github if rows => repos.open_fork_on_github(),
        Command::GithubRow(owner) => {
            repos.pick_fork(&owner);
            repos.open_fork_on_github();
        }
        Command::Open | Command::Github => {}
        Command::Fetch => repos.update_shown_forks(),
        Command::Read => repos.load_forks(),
        Command::ShowRun => repos.show_job(),
        Command::Sort => repos.cycle_fork_sort(),
        Command::ShowChanged(on) => repos.set_fork_changed_only(on),
        Command::ToggleChanged => repos.toggle_fork_changed_only(),
        Command::Filter(text) => repos.set_fork_filter(&text),
        Command::Escape if repos.forks.as_ref().is_some_and(|view| !view.filter.is_empty()) => repos.clear_fork_filter(),
        Command::Escape | Command::Back => repos.close_forks(),
    }
}

/// Does what `commands` ask of `repos`, in order, until one closes the forks or opens a dialog: what comes after it
/// is no longer this screen's.
pub fn apply_all(commands: Vec<Command>, repos: &mut Repos) {
    for command in commands {
        if repos.forks.is_none() || repos.mode != CoreMode::List {
            break;
        }
        apply(command, repos);
    }
}

/// What a key on the screen asks, as the terminal app reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Move the highlight this many rows.
    Move(isize),
    /// Open the highlighted fork's folder (Enter, `o`).
    Open,
    /// Open its page on GitHub (`g`).
    Github,
    /// Fetch the forks (`u`).
    Fetch,
    /// Read them again (`r`, F5).
    Read,
    /// Show a hidden run (`l`).
    ShowRun,
    /// Sort the next way (`s`).
    Sort,
    /// Switch All and With changes (`c`).
    Changed,
    /// Put the keyboard in the filter (`/`).
    Filter,
    /// Type this into the filter: text that came after `/` in the same frame.
    Type(char),
    /// Clear the filter, else go back (Esc).
    Escape,
    /// Go back (Backspace).
    Back,
}

impl Press {
    /// Whether it moves the highlight, and so goes on while its key is held; every other key acts once a press.
    fn moves(self) -> bool {
        matches!(self, Press::Move(_))
    }
}

/// What the keys among `events` ask, in order. Letters are read from the typed text, so they follow the keyboard's
/// layout; nothing typed or pressed with Ctrl, Alt or Cmd held is a key here. A held key moves the highlight again
/// and again, but acts once. Once `/` has asked for the filter, the rest of the frame's text is typed into it and its
/// other keys are the field's.
fn presses(events: &[Event]) -> Vec<Press> {
    let mut presses = Vec::new();
    // The modifiers held with the last key, and whether it was a repeat: the text a key types comes after it.
    let (mut held, mut repeated) = (Modifiers::NONE, false);
    let commanding = |modifiers: Modifiers| modifiers.ctrl || modifiers.alt || modifiers.command || modifiers.mac_cmd;
    for event in events {
        if let Event::Key { modifiers, repeat, .. } = event {
            (held, repeated) = (*modifiers, *repeat);
        }
        match event {
            Event::Key { .. } if presses.contains(&Press::Filter) => {}
            Event::Key { key, pressed: true, modifiers, repeat, .. } if !commanding(*modifiers) => {
                let press = match key {
                    Key::ArrowUp => Press::Move(-1),
                    Key::ArrowDown => Press::Move(1),
                    Key::PageUp => Press::Move(-PAGE_ROWS),
                    Key::PageDown => Press::Move(PAGE_ROWS),
                    Key::Home => Press::Move(isize::MIN / 2),
                    Key::End => Press::Move(isize::MAX / 2),
                    Key::Enter => Press::Open,
                    Key::Escape => Press::Escape,
                    Key::Backspace => Press::Back,
                    Key::F5 => Press::Read,
                    _ => continue,
                };
                if !*repeat || press.moves() {
                    presses.push(press);
                }
            }
            Event::Text(text) => {
                for typed in text.chars() {
                    if presses.contains(&Press::Filter) {
                        presses.push(Press::Type(typed));
                        continue;
                    }
                    if commanding(held) {
                        continue;
                    }
                    let press = match typed {
                        'j' => Press::Move(1),
                        'k' => Press::Move(-1),
                        'o' | 'O' => Press::Open,
                        'g' | 'G' => Press::Github,
                        'u' | 'U' => Press::Fetch,
                        'r' => Press::Read,
                        'l' | 'L' => Press::ShowRun,
                        's' | 'S' => Press::Sort,
                        'c' | 'C' => Press::Changed,
                        '/' => Press::Filter,
                        _ => continue,
                    };
                    if !repeated || press.moves() {
                        presses.push(press);
                    }
                }
            }
            _ => {}
        }
    }
    presses
}

/// What the screen keeps from one frame to the next.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// How far the rows are moved up.
    scroll: f32,
    /// The owner of the fork highlighted in the last frame: when the highlight moves, it is followed into view.
    chosen: Option<String>,
    /// The repository whose forks were shown in the last frame: the forks of another start afresh.
    repo: Option<String>,
    /// The filter and the segment of the last frame: when either changes, the rows start at the top.
    shown: Option<(String, bool)>,
    /// The filter field had the focus at the end of the last frame.
    filter_focused: bool,
    /// The filter field had the focus when an overlay took the keyboard, and gets it back when the overlay goes.
    refocus: bool,
    /// The owner of the row the last click landed on, when it highlighted that row: a second click on the same row
    /// makes a double click that opens its folder; a click on anything else (a row's button) does not.
    clicked_row: Option<String>,
}

/// The id of the filter field.
fn filter_id() -> Id {
    Id::unique("forks-filter")
}

/// The id of the top row's main button.
fn main_id() -> Id {
    Id::unique("forks-fetch")
}

/// Takes the keyboard from the screen's filter as the forks close: it does not stay behind for another screen.
pub fn leave(ctx: &Context) {
    ctx.memory_mut(|memory| memory.surrender_focus(filter_id()));
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

/// Gives the place `area` (cut to `shown`) all of `words` as its hover text, named `id`: for words drawn cut there.
fn hover_whole(ui: &Ui, area: Rect, shown: Rect, id: Id, words: &str) {
    let area = area.intersect(shown);
    if area.is_positive() {
        ui.interact(area, id, Sense::hover()).on_hover_text(words);
    }
}

/// Draws `cell` from `left`, centred on `y`, cut to `room`, in the row at `place` of which `shown` is in view; when it
/// is cut, all of its words show on hover there (`id`).
#[expect(clippy::too_many_arguments, reason = "where the cell is, what it says, and the name of its hover")]
fn draw_cell(ui: &Ui, place: Rect, shown: Rect, left: f32, cell: &Cell, room: f32, palette: Palette, id: Id) {
    let (family, colour) = ink(cell.tone, palette);
    let face = font(TEXT, family);
    let said = widgets::ellipsis(&cell.text, &face, room, ui);
    if said != cell.text {
        let area = Rect::from_min_max(pos2(left, place.top()), pos2(left + room.max(0.0), place.bottom()));
        hover_whole(ui, area, shown, id, &cell.text);
    }
    if !said.is_empty() {
        ui.painter().text(pos2(left, place.center().y), Align2::LEFT_CENTER, said, face, colour);
    }
}

/// The places of a row's two buttons in the row at `place`: the folder's, then GitHub's, each [`ROW_BUTTON`] wide
/// from the left of the actions column and the row's whole height, to take its press.
fn button_rects(place: Rect, columns: &ForkColumns) -> [Rect; 2] {
    let left = place.left() + columns.actions.left;
    [0.0, 1.0].map(|at| Rect::from_min_max(pos2(left + at * ROW_BUTTON, place.top()), pos2(left + (at + 1.0) * ROW_BUTTON, place.bottom())))
}

/// Draws a row's round button with `icon` at `button` (its press taken on the part in `shown`), on the hover's fill
/// under the pointer, `words` its name and hover text; `id` names it. Gives what the pointer did to it.
fn row_button(ui: &Ui, button: Rect, shown: Rect, icon: Icon, words: &str, palette: Palette, id: Id) -> Response {
    let response = ui.interact(button.intersect(shown), id, Sense::CLICK).on_hover_text(words);
    response.widget_info(|| WidgetInfo::labeled(Role::Button, true, words));
    if response.hovered() {
        let round = Rect::from_center_size(button.center(), Vec2::splat(ROW_BUTTON));
        ui.painter().rect_filled(round, ROW_BUTTON_RADIUS, palette.on_glass.gamma_multiply(HOVER_OPACITY));
    }
    draw_icon(ui.painter(), button.center(), icon, ICON, palette.quiet);
    response
}

/// Draws the screen in `layout` from `plan` (from [`plan`] this frame): the top row's and the toolbar's controls,
/// then the rows or the sentence that stands for them on the panel, with `sheet` the panel's frosted sheet; `screen`
/// keeps the scroll, the filter's focus and the last click between frames. Gives back what the keys and presses
/// asked, in order.
///
/// Without `keys` (an overlay is open over the screen: `overlay::is_open`) the screen hears no key and takes no
/// press, and its filter neither keeps nor takes the keyboard's focus (it gets it back when the overlay goes). While
/// the filter has the keyboard, no key of the screen acts: letters type; Esc clears its text, or with none leaves
/// it; Enter and Down leave it, the highlight where it was.
pub fn show(ui: &mut Ui, layout: &Layout, plan: &Plan, sheet: Option<&Sheet>, mode: Mode, screen: &mut Screen, keys: bool) -> Vec<Command> {
    let palette = theme::palette(mode);
    if screen.repo.as_deref() != Some(plan.repo.as_str()) {
        leave(ui.ctx());
        *screen = Screen { repo: Some(plan.repo.clone()), ..Screen::default() };
    }
    if !keys {
        // egui's own Tab would give the focus to the filter, the one control here that takes it.
        ui.memory_mut(|memory| {
            memory.surrender_focus(filter_id());
            memory.move_focus(FocusDirection::None);
        });
        screen.refocus |= screen.filter_focused;
        screen.filter_focused = false;
    } else if std::mem::take(&mut screen.refocus) {
        ui.memory_mut(|memory| memory.request_focus(filter_id()));
        screen.filter_focused = true;
    }
    // The frame's keys are the filter's when it held the focus at the end of the last frame.
    let typing = screen.filter_focused;
    // The rest of a double click whose first click closed an overlay is no press here.
    let presses_count = keys && !overlay::stale_click(ui.ctx());
    let mut asked = Vec::new();
    let events = if keys { ui.input(|input| input.events.clone()) } else { Vec::new() };
    if keys && typing && ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::ArrowDown)) {
        // Down gives the keyboard to the table, taken before the field sees it.
        ui.memory_mut(|memory| memory.surrender_focus(filter_id()));
    }
    let text = controls(ui, plan, palette, presses_count, &mut asked);
    if typing {
        let escaped = events.iter().any(|event| matches!(event, Event::Key { key: Key::Escape, pressed: true, repeat: false, .. }));
        if escaped && !text.is_empty() {
            asked.push(Command::Filter(String::new()));
        } else if escaped {
            ui.memory_mut(|memory| memory.surrender_focus(filter_id()));
        }
    } else {
        // The filter's text as the keys leave it: `/` keeps it, and what is typed after it in the frame goes on it.
        let mut typed: Option<String> = None;
        for press in presses(&events) {
            asked.push(match press {
                Press::Move(rows) => Command::Move(rows),
                Press::Open => Command::Open,
                Press::Github => Command::Github,
                Press::Fetch => Command::Fetch,
                Press::Read => Command::Read,
                Press::ShowRun => Command::ShowRun,
                Press::Sort => Command::Sort,
                Press::Changed => Command::ToggleChanged,
                Press::Escape => Command::Escape,
                Press::Back => Command::Back,
                Press::Filter => {
                    ui.memory_mut(|memory| memory.request_focus(filter_id()));
                    typed = Some(text.clone());
                    continue;
                }
                Press::Type(letter) => {
                    let Some(typed) = typed.as_mut() else { continue };
                    typed.push(letter);
                    Command::Filter(typed.clone())
                }
            });
        }
    }
    if !keys {
        // Nor a press on the field in the frame an overlay opens, before its shield is in place.
        ui.memory_mut(|memory| memory.surrender_focus(filter_id()));
    }
    screen.filter_focused = ui.memory(|memory| memory.has_focus(filter_id()));
    if !asked.is_empty() {
        ui.ctx().request_repaint();
    }
    let Some(sheet) = sheet else { return asked };
    let window = Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size());
    frost::paint(ui.painter(), window, layout.panel, RADIUS_PANEL, sheet);
    if let Some(empty) = &plan.empty {
        draw_empty(ui, layout.panel, empty, palette);
        screen.chosen = None;
        return asked;
    }
    table_rows(ui, plan, mode, screen, presses_count, &mut asked);
    asked
}

/// Draws the table of `plan` on its panel: the head row, each row in view with its buttons, the highlight followed
/// into view when it moved and the rows from the top when the filter or the segment changed; with `presses`, adds
/// what a click asked to `asked`.
fn table_rows(ui: &mut Ui, plan: &Plan, mode: Mode, screen: &mut Screen, presses: bool, asked: &mut Vec<Command>) {
    let palette = theme::palette(mode);
    let panel = plan.panel;
    let heights = heights(plan);
    let body = table::body_rect(panel);
    let shown_now = (plan.filter.clone(), plan.changed_only);
    if screen.shown.as_ref().is_some_and(|shown| *shown != shown_now) {
        screen.scroll = 0.0;
    }
    screen.shown = Some(shown_now);
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

    // The second (or third) click of a double click on a button presses nothing more: a button acts once.
    let second =
        ui.input(|input| input.pointer.button_double_clicked(PointerButton::Primary) || input.pointer.button_triple_clicked(PointerButton::Primary));
    let outer_clip = ui.clip_rect();
    ui.set_clip_rect(panel.intersect(outer_clip));
    draw_head(ui, table::head_rect(panel), plan, palette);
    ui.set_clip_rect(body.intersect(outer_clip));
    let mut on_row = false;
    for (index, place) in table::placed(body, &heights, screen.scroll) {
        let row = &plan.rows[index];
        let shown = place.intersect(body);
        let id = ui.scope_id().with(("fork-row", &row.owner));
        let response = ui.interact(shown, id, Sense::CLICK);
        response.widget_info(|| WidgetInfo::selected(Role::Row, true, row.chosen, &row.fork.text));
        let hovered = !row.chosen && ui.rect_contains_pointer(shown);
        let fill = if row.chosen { theme::selected_fill(mode) } else { Color32::TRANSPARENT };
        let under_line = index + 1 < plan.rows.len();
        table::draw_fill(ui.painter(), &Fill { row: place, shown, panel, under_line, hovered }, &[fill], palette);
        draw_row(ui, place, shown, &plan.columns, row, palette, id);
        let [folder, github] = button_rects(place, &plan.columns);
        let folder = row.folder.then(|| row_button(ui, folder, shown, Icon::FolderOpen, OPEN_THE_FOLDER, palette, id.with("folder")).clicked());
        let github = row.github.then(|| row_button(ui, github, shown, Icon::ExternalLink, OPEN_ON_GITHUB, palette, id.with("github")).clicked());
        if !presses {
            continue;
        }
        if folder == Some(true) || github == Some(true) {
            if !second {
                let owner = row.owner.clone();
                asked.push(if folder == Some(true) { Command::OpenRow(owner) } else { Command::GithubRow(owner) });
            }
        } else if response.double_clicked() && screen.clicked_row.as_deref() == Some(row.owner.as_str()) {
            asked.push(Command::OpenRow(row.owner.clone()));
            screen.clicked_row = None;
            on_row = true;
        } else if response.clicked() {
            asked.push(Command::Pick(row.owner.clone()));
            screen.clicked_row = Some(row.owner.clone());
            on_row = true;
        }
    }
    ui.set_clip_rect(outer_clip);
    // A click anywhere else (a row's button among them) starts no double click of a row.
    if !on_row && ui.input(|input| input.pointer.any_click()) {
        screen.clicked_row = None;
    }
}

/// Draws the controls of `plan` (the top row's "Read again", "Open the folder" and "Fetch the forks", the filter, the
/// segments and the sort) and, with `presses`, adds what a press on them asked to `asked`. A double click is one
/// press: its second click does nothing. Gives the filter's text after this frame's typing.
fn controls(ui: &mut Ui, plan: &Plan, palette: Palette, presses: bool, asked: &mut Vec<Command>) -> String {
    let second =
        ui.input(|input| input.pointer.button_double_clicked(PointerButton::Primary) || input.pointer.button_triple_clicked(PointerButton::Primary));
    let presses = presses && !second;
    if let Some(top) = plan.top {
        match widgets::capsule(ui, top.actions, &top_items(top.fit), palette) {
            Some(TOP_READ) if presses => asked.push(Command::Read),
            Some(TOP_OPEN) if presses => asked.push(Command::Open),
            _ => {}
        }
        if widgets::main_action(ui, top.main, Icon::ArrowDownToLine, FETCH_THE_FORKS, top.fit == Fit::Bare, palette, main_id()) && presses {
            asked.push(Command::Fetch);
        }
    }
    let toolbar = &plan.toolbar;
    let mut text = plan.filter.clone();
    // Esc stays with the field: the screen reads it, to clear the text before the field gives up the keyboard.
    widgets::field_keeping(ui, toolbar.field, &mut text, FILTER_HINT, Icon::Search, palette, filter_id(), true);
    if text != plan.filter {
        asked.push(Command::Filter(text.clone()));
    }
    if let Some(index) = widgets::segments(ui, toolbar.segments, &segment_items(&toolbar.all, &toolbar.changed, plan.changed_only), palette)
        && presses
    {
        asked.push(Command::ShowChanged(index == 1));
    }
    if widgets::capsule(ui, toolbar.sort, &sort_items(&toolbar.sort_words, toolbar.sort_compact), palette).is_some() && presses {
        asked.push(Command::Sort);
    }
    text
}

/// Draws why the panel at `panel` shows no fork, centred on it: the core's sentence (in the danger colour for an
/// error) and under it, quiet, the error's own words; each cut to the panel's width, and then all of it on hover.
fn draw_empty(ui: &Ui, panel: Rect, empty: &Empty, palette: Palette) {
    let painter = ui.painter().with_clip_rect(panel);
    let face = font(TEXT, FontFamily::Proportional);
    let room = panel.width() - 2.0 * PANEL_TEXT_INSET;
    let colour = if empty.error { palette.danger } else { palette.quiet };
    let lines: Vec<(&str, Color32)> =
        std::iter::once((empty.text.as_str(), colour)).chain(empty.detail.as_deref().map(|detail| (detail, palette.quiet))).collect();
    let galleys: Vec<_> =
        lines.iter().map(|(words, colour)| painter.layout_no_wrap(widgets::ellipsis(words, &face, room, ui), face.clone(), *colour)).collect();
    let gaps = STATE_GAP * f32::from(u8::try_from(galleys.len().saturating_sub(1)).unwrap_or(u8::MAX));
    let mut top = panel.center().y - f32::midpoint(galleys.iter().map(|galley| galley.size().y).sum::<f32>(), gaps);
    for (index, (galley, (words, colour))) in galleys.into_iter().zip(lines).enumerate() {
        let at = pos2(panel.center().x - galley.size().x / 2.0, top);
        let drawn = Rect::from_min_size(at, galley.size());
        if galley.text() != words {
            hover_whole(ui, drawn, panel, ui.scope_id().with(("forks-empty", index)), words);
        }
        top += galley.size().y + STATE_GAP;
        painter.galley(at, galley, colour);
    }
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

/// Paints a fork's cells in the row at `place` (of which `shown` is in view); a cut cell shows all of its words on
/// hover, named from `id`.
fn draw_row(ui: &Ui, place: Rect, shown: Rect, c: &ForkColumns, row: &Row, palette: Palette, id: Id) {
    let at = |span: Span| place.left() + span.left;
    draw_cell(ui, place, shown, at(c.fork), &row.fork, c.fork.width, palette, id.with("fork"));
    for (span, cell, name) in [(c.branches, &row.branches, "branches"), (c.pulls, &row.pulls, "pulls"), (c.newest, &row.newest, "newest")] {
        if let Some(span) = span {
            draw_cell(ui, place, shown, at(span), cell, span.width, palette, id.with(name));
        }
    }
    let y = place.center().y;
    table::draw_forward(ui.painter(), at(c.ahead), y, columns::forward(row.ahead, c.room), c.room, palette);
    let words_left = at(c.ahead) + columns::rule_length(c.room) + RULE_LABEL_GAP;
    draw_cell(ui, place, shown, words_left, &row.ahead_words, place.left() + c.ahead.right() - words_left, palette, id.with("ahead"));
    let mut left = at(c.state);
    if let Some(icon) = row.state_icon {
        let colour = if icon == Icon::CloudOff { palette.warning } else { palette.on_glass };
        draw_icon(ui.painter(), pos2(left + ICON_SMALL / 2.0, y), icon, ICON_SMALL, colour);
        left += ICON_SMALL + CELL_ICON_GAP;
    }
    draw_cell(ui, place, shown, left, &row.state, place.left() + c.state.right() - left, palette, id.with("state"));
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

/// The buttons of the row of the fork of `owner`, its rows moved up by `scroll`: the one that opens its folder and
/// the one that opens its page on GitHub, each where it takes its press, when the row has it.
#[cfg(test)]
pub(crate) fn row_buttons(plan: &Plan, scroll: f32, owner: &str) -> [Option<Rect>; 2] {
    let Some((place, row)) = row_rect(plan, scroll, owner).zip(plan.rows.iter().find(|row| row.owner == owner)) else { return [None, None] };
    let [folder, github] = button_rects(place, &plan.columns);
    [row.folder.then_some(folder), row.github.then_some(github)]
}

/// Every control of the screen's top row and toolbar that `plan` places, with its words (the sort by its words),
/// measured with `painter`'s faces: for the tests that press them; it reads the same items the screen draws.
#[cfg(test)]
pub(crate) fn control_places(painter: &Painter, plan: &Plan) -> Vec<(String, Rect)> {
    let toolbar = &plan.toolbar;
    let mut controls = vec![(FILTER_HINT.to_owned(), toolbar.field)];
    let segments = segment_items(&toolbar.all, &toolbar.changed, plan.changed_only);
    controls.extend(segments.iter().map(|segment| segment.words.to_owned()).zip(widgets::segment_rects(painter, toolbar.segments, &segments)));
    controls.push((toolbar.sort_words.clone(), toolbar.sort));
    if let Some(top) = plan.top {
        let items = top_items(top.fit);
        controls.extend([READ_AGAIN, OPEN_THE_FOLDER].map(str::to_owned).into_iter().zip(widgets::item_rects(painter, top.actions, &items)));
        controls.push((FETCH_THE_FORKS.to_owned(), top.main));
    }
    controls
}

/// The words of the segment `plan` shows chosen: "All", or "With changes".
#[cfg(test)]
pub(crate) fn chosen_segment(plan: &Plan) -> &'static str {
    if plan.changed_only { WITH_CHANGES } else { ALL }
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
    use devassist_core::repos::state::{HELD, Mode as CoreMode, Msg, testing};
    use egui::{Context, Event, Key, Modifiers, PointerButton, RawInput, Shape, Stroke, TextureId, vec2};

    use super::*;
    use crate::app::Pages;
    use crate::dialogs::{self, Dialogs};
    use crate::session::Session;
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

    /// The design's layout while the forks show: with the toolbar.
    fn layout_at(size: [f32; 2]) -> Layout {
        shell::layout(vec2(size[0], size[1]), true, true)
    }

    /// The design's panel.
    fn panel() -> Rect {
        layout_at([1440.0, 900.0]).panel
    }

    /// The plan of the forks of `repos` in a window of `size`, measured on `ctx`.
    fn plan_at(ctx: &Context, repos: &Repos, size: [f32; 2]) -> Plan {
        let mut out = None;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| out = Some(plan(view(repos), &layout_at(size), ui.painter()))).drop_without_applying_deltas();
        out.unwrap()
    }

    /// What one frame of the shell and the screen in a window of `size` drew and asked, with `events`: the shapes,
    /// whether back was pressed, and the screen's commands.
    fn frame(ctx: &Context, size: [f32; 2], repos: &mut Repos, screen: &mut Screen, events: Vec<Event>) -> (Vec<Shape>, bool, Vec<Command>) {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]))), events, ..Default::default() };
        let (mut back, mut asked) = (false, Vec::new());
        let output = ctx.run_ui(input, |ui| {
            let layout = layout_at(size);
            let title = title(repos);
            let status = status(repos);
            let shown = Shown { mode: Mode::Light, waiting: 0, status: status.as_ref(), back: title.as_deref() };
            let (mut sidebar, mut page) = (true, Page::Repositories);
            back = shell::draw(ui, &layout, &mut sidebar, &mut page, shown);
            if let Some(view) = repos.forks.as_ref() {
                let plan = plan(view, &layout, ui.painter());
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

    fn key_event(key: Key, pressed: bool, repeat: bool, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat, modifiers }
    }

    /// The window as the app runs it, on the Repositories page: the dialogs placed before the bake, the pages (the
    /// shell, the forks screen or the table) and the dialogs after them. Each frame is a tenth of a second on egui's
    /// clock unless [`Win::step`] says otherwise: a click's press and release make a click, and no two clicks make a
    /// double click by chance.
    struct Win {
        ctx: Context,
        session: Session,
        pages: Pages,
        dialogs: Dialogs,
        size: [f32; 2],
        time: f64,
        step: f64,
        /// The forks' plan in the last frame, when they showed.
        plan: Option<Plan>,
        /// The forks screen's controls in the last frame, with their words.
        controls: Vec<(String, Rect)>,
        /// The texts the last frame painted: their words, where, and their colour.
        texts: Vec<(String, Rect, Color32)>,
        /// The filled rectangles the last frame painted.
        fills: Vec<(Rect, Color32)>,
    }

    impl Win {
        fn new(session: Session) -> Win {
            let mut win = Win {
                ctx: context(),
                session,
                pages: Pages::default(),
                dialogs: Dialogs::default(),
                size: [1440.0, 900.0],
                time: 0.0,
                step: 0.1,
                plan: None,
                controls: Vec::new(),
                texts: Vec::new(),
                fills: Vec::new(),
            };
            win.frame(Vec::new());
            win
        }

        fn demo() -> Win {
            Win::new(Session::demo(forks_open()))
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.time += self.step;
            let window = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0], self.size[1]));
            let input = RawInput { screen_rect: Some(window), events, time: Some(self.time), ..Default::default() };
            let (session, pages, dialogs) = (&mut self.session, &mut self.pages, &mut self.dialogs);
            let (mut plan, mut controls) = (None, Vec::new());
            let output = self.ctx.run_ui(input, |ui| {
                let ctx = ui.ctx().clone();
                pages.page = dialogs::page_for(session.repos(), pages.page);
                let planned = pages.plan(ui.painter(), window.size(), session);
                let dialog = dialogs::before(&ctx, session.repos(), pages.menu_anchor(), dialogs);
                pages.draw(ui, &planned, session, Some(&sheet()), Mode::Light, None);
                dialogs::after(&ctx, dialog, session.repos_mut(), &[], Mode::Light, dialogs);
                plan = planned.forks().cloned();
                controls = plan.as_ref().map_or_default(|plan| control_places(ui.painter(), plan));
            });
            self.texts = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    Shape::Text(text) => Some((text.galley.text().to_owned(), text.galley.rect.translate(text.pos.to_vec2()), text.fallback_color)),
                    _ => None,
                })
                .collect();
            self.fills = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    Shape::Rect(rect) if rect.fill.a() > 0 => Some((rect.rect, rect.fill)),
                    _ => None,
                })
                .collect();
            output.drop_without_applying_deltas();
            (self.plan, self.controls) = (plan, controls);
        }

        /// A key pressed in one frame and let go in the next, typing `text` when it is some.
        fn key(&mut self, key: Key, text: Option<&str>) {
            let mut events = vec![key_event(key, true, false, Modifiers::NONE)];
            events.extend(text.map(|text| Event::Text(text.to_owned())));
            self.frame(events);
            self.frame(vec![key_event(key, false, false, Modifiers::NONE)]);
        }

        /// A letter typed: its key and its text, as the window gets them.
        fn letter(&mut self, typed: char) {
            let key = Key::from_name(&typed.to_ascii_uppercase().to_string()).unwrap_or(Key::Slash);
            self.key(key, Some(&typed.to_string()));
        }

        fn click(&mut self, at: Pos2) {
            for events in click(at) {
                self.frame(events);
            }
        }

        /// The place of the control with `words` in the last frame.
        fn control(&self, words: &str) -> Rect {
            self.controls.iter().find(|(said, _)| said == words).map_or_else(|| panic!("no {words}: {:?}", self.controls), |(_, rect)| *rect)
        }

        fn press(&mut self, words: &str) {
            let at = self.control(words).center();
            self.click(at);
        }

        fn repos(&self) -> &Repos {
            self.session.repos().unwrap()
        }

        fn repos_mut(&mut self) -> &mut Repos {
            self.session.repos_mut().unwrap()
        }

        fn view(&self) -> &ForksView {
            view(self.repos())
        }

        fn chosen(&self) -> Option<String> {
            self.view().chosen().map(|fork| fork.owner.clone())
        }

        fn plan(&self) -> &Plan {
            self.plan.as_ref().expect("the forks are not shown")
        }

        /// Whether the filter field has the keyboard.
        fn typing(&self) -> bool {
            self.ctx.memory(|memory| memory.has_focus(filter_id()))
        }

        /// What the status bar says, and its colour.
        fn status(&self) -> (String, Color32) {
            let bar = shell::layout(vec2(self.size[0], self.size[1]), true, true).status;
            let left = bar.left() + theme::STATUS_PADDING;
            self.texts
                .iter()
                .find(|(_, rect, _)| bar.contains(rect.center()) && (rect.left() - left).abs() < 1.0)
                .map(|(words, _, colour)| (words.clone(), *colour))
                .expect("the status bar says nothing")
        }

        /// Whether a text with `words` was drawn.
        fn drew(&self, words: &str) -> bool {
            self.texts.iter().any(|(said, ..)| said == words)
        }

        /// The row buttons of the fork of `owner` in the last frame: open the folder and open on GitHub.
        fn buttons(&self, owner: &str) -> [Option<Rect>; 2] {
            row_buttons(self.plan(), self.pages.forks_screen().scroll(), owner)
        }

        fn row(&self, owner: &str) -> Rect {
            row_rect(self.plan(), self.pages.forks_screen().scroll(), owner).unwrap()
        }

        /// The pointer rests at `at` until a tooltip has had time to show.
        fn rest(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)]);
            for _ in 0..10 {
                self.frame(Vec::new());
            }
        }
    }

    #[test]
    fn the_forks_take_the_tables_place_and_back_returns() {
        let mut repos = forks_open();
        assert!(shown(&repos));
        assert_eq!(title(&repos).as_deref(), Some("Forks of api"));
        let ctx = context();
        let mut screen = Screen::default();
        let toggle = layout_at([1440.0, 900.0]).toggle;
        let (shapes, ..) = frame(&ctx, [1440.0, 900.0], &mut repos, &mut screen, Vec::new());
        let said = texts(&shapes);
        // The title is the screen's, in the title's room; "Repositories" is the sidebar's entry alone, and lit.
        let layout = layout_at([1440.0, 900.0]);
        let placed = |words: &str| -> Vec<Pos2> {
            shapes
                .iter()
                .filter_map(|shape| match shape {
                    Shape::Text(text) if text.galley.text() == words => Some(text.galley.rect.translate(text.pos.to_vec2()).center()),
                    _ => None,
                })
                .collect()
        };
        let title_at = placed("Forks of api");
        assert!(title_at.len() == 1 && layout.title.contains(title_at[0]), "{title_at:?}");
        let entry = shell::page_rows(layout.sidebar).into_iter().find(|(page, _)| *page == Page::Repositories).map(|(_, row)| row).unwrap();
        let named = placed("Repositories");
        assert!(named.len() == 1 && entry.contains(named[0]), "{named:?}");
        let lit = theme::palette(Mode::Light).selection;
        assert!(shapes.iter().any(|shape| matches!(shape, Shape::Rect(rect) if rect.fill == lit && rect.rect == entry)), "Repositories is not lit");
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
        let ctx = context();
        let repos = forks_open();
        let view = view(&repos);
        let plan = plan_at(&ctx, &repos, [1440.0, 900.0]);
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
            assert_eq!((row.folder, row.github), (fork.folder, !fork.gone));
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
        // A fork nothing was fetched from: no date, and the core's fourth state drawn as "No changes" is; a date
        // before 1970 (a clock gone wrong) is no date either.
        let fetched_nothing = ForkInfo { branches: 0, ahead: None, date: 0, ..fork("x") };
        let r = row(&fetched_nothing, false);
        assert_eq!((r.newest.text.as_str(), r.state.clone()), ("", Cell::new("Not fetched", Tone::Quiet)));
        assert_eq!(row(&ForkInfo { date: -86_400, ..fork("x") }, false).newest.text, "");
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
        let plan = plan_at(&ctx, &repos, [1440.0, 900.0]);
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
        let ctx = context();
        let mut repos = forks_open();
        assert_eq!(plan_at(&ctx, &repos, [1440.0, 900.0]).sorted, Sorted::Ahead);
        repos.set_fork_sort(ForkSort::Newest);
        assert_eq!(plan_at(&ctx, &repos, [1440.0, 900.0]).sorted, Sorted::Newest);
        repos.set_fork_sort(ForkSort::Name);
        assert_eq!(plan_at(&ctx, &repos, [1440.0, 900.0]).sorted, Sorted::Fork);
        // Drawn after the sorted head's words: one arrow, in the Fork column's head.
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
        let fork = plan_at(&ctx, &repos, [1440.0, 900.0]).columns.fork;
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
        let panel = layout_at(size).panel;
        let body = table::body_rect(panel);
        let in_view = |repos: &Repos, screen: &Screen| {
            let plan = plan_at(&ctx, repos, size);
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
        frame(&ctx, size, &mut repos, &mut screen, vec![Event::PointerMoved(body.center())]);
        for _ in 0..30 {
            frame(&ctx, size, &mut repos, &mut screen, vec![crate::dialogs::testing::wheel(-1.0e6)]);
        }
        for _ in 0..30 {
            frame(&ctx, size, &mut repos, &mut screen, Vec::new());
        }
        assert!((screen.scroll() - (200.0 * ROW_HEIGHT - body.height())).abs() < 1e-3, "{}", screen.scroll());
        // A click on a row highlights it, and nothing else.
        let plan = plan_at(&ctx, &repos, size);
        let last = row_rect(&plan, screen.scroll(), "owner-199").unwrap();
        let mut asked = Vec::new();
        for events in click(last.center()) {
            asked.extend(frame(&ctx, size, &mut repos, &mut screen, events).2);
        }
        assert_eq!(asked, [Command::Pick("owner-199".to_owned())]);
        apply_all(asked, &mut repos);
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
            let plan = plan_at(&ctx, &repos, [1440.0, 900.0]);
            let detail = error.then(|| "No folder".to_owned());
            assert_eq!(plan.empty, Some(Empty { text: said.trim().to_owned(), error, detail: detail.clone() }));
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
            let mut wanted = vec![(said.trim().to_owned(), if error { palette.danger } else { palette.quiet })];
            // The error's own words under the core's sentence, quiet: the sentence names no detail.
            wanted.extend(detail.map(|detail| (detail, palette.quiet)));
            assert_eq!(drawn, wanted, "no head row, the core's words");
        }
    }

    #[test]
    fn the_status_bar_counts_the_forks() {
        let repos = forks_open();
        let said = status(&repos).unwrap();
        assert_eq!(said.text, "13 forks. 6 have changes and a folder. 1 is gone from GitHub. 27 branches and 9 pull requests are saved.");
        assert_eq!(said.text, view(&repos).summary());
        let dest = view(&repos).dest.display().to_string();
        assert!(dest.ends_with("api-fork"), "{dest}");
        assert_eq!(said.notes, [(Icon::Folder, dest), (Icon::Clock, "Fetched 25 Nov 2026".to_owned())]);
        assert!(!said.error && said.keep_text);
        assert_eq!(said.path, Some(0));
        // Without a fetch date or a folder, no note.
        assert_eq!(status(&many(2)).unwrap().notes.len(), 0);
        // A narrow bar: the fetched day gives way first, then the folder; never the sentence.
        let words = [300.0, 110.0];
        let note = theme::STATUS_GAP + theme::STATUS_ICON + theme::STATUS_ICON_GAP;
        assert_eq!(shell::notes_fit(1200.0, 600.0, &words, Some(0)), [Some(300.0), Some(110.0)]);
        assert_eq!(shell::notes_fit(600.0 + 2.0 * note + 400.0, 600.0, &words, Some(0)), [Some(300.0), None]);
        assert_eq!(shell::notes_fit(600.0 + note + 200.0, 600.0, &words, Some(0)), [Some(200.0), None], "the folder is cut from its left");
        assert_eq!(shell::notes_fit(600.0 + note + 100.0, 600.0, &words, Some(0)), [None, None]);
        assert_eq!(shell::notes_fit(100.0, 600.0, &words, Some(0)), [None, None]);
        assert_eq!(status(&crate::demo::repos()), None, "no forks, no words of theirs");
    }

    #[test]
    fn the_status_bar_says_what_the_core_said_then_the_forks_again() {
        let mut repos = forks_open();
        repos.update_shown_forks();
        let said = status(&repos).unwrap();
        assert_eq!((said.text.as_str(), said.error), (HELD, false), "the demo's refusal is heard");
        assert_eq!(said.notes.len(), 2, "the notes stay");
        repos.move_fork(1);
        assert_eq!(status(&repos).unwrap().text, view(&repos).summary(), "the highlight moved: the forks' sentence again");
        // An error while rows show is on the status bar, in the danger colour, until a read goes well.
        let generation = repos.forks_generation;
        repos.on_msg(Msg::Forks { generation, result: Err("the folder of the forks could not be read".to_owned()) });
        let said = status(&repos).unwrap();
        assert_eq!((said.text.as_str(), said.error), ("The folder of the forks could not be read", true));
        assert_ne!(view(&repos).rows().len(), 0);
        repos.load_forks();
        assert_eq!(status(&repos).unwrap().text, view(&repos).summary(), "read again, and well");
        assert!(!status(&repos).unwrap().error);
    }

    #[test]
    fn a_title_or_a_folder_the_core_cannot_name_leaves_no_gap() {
        let mut repos = forks_open();
        for original in ["", "acme/", " "] {
            original.clone_into(&mut repos.forks.as_mut().unwrap().original);
            assert_eq!(title(&repos).as_deref(), Some("Forks of api"), "{original:?}");
        }
        repos.forks.as_mut().unwrap().dest = std::path::PathBuf::new();
        let said = status(&repos).unwrap();
        assert_eq!(said.notes, [(Icon::Clock, "Fetched 25 Nov 2026".to_owned())], "no folder icon without its words");
        assert_eq!(said.path, None);
    }

    #[test]
    fn the_forks_of_another_repository_start_afresh() {
        let size = [960.0, 600.0];
        let mut repos = many(200);
        // The same owners for web.
        let forks = view(&repos).forks.clone();
        repos.hold_forks("web", ForkList { original: "acme/web".to_owned(), forks, ..ForkList::default() });
        let ctx = context();
        let mut screen = Screen::default();
        let body = table::body_rect(layout_at(size).panel);
        repos.pick_fork("owner-150");
        frame(&ctx, size, &mut repos, &mut screen, Vec::new());
        assert!(screen.scroll() > 0.0, "owner-150 was followed");
        // Wheeled back to the top: owner-150 is out of view now, and stays highlighted.
        frame(&ctx, size, &mut repos, &mut screen, vec![Event::PointerMoved(body.center())]);
        for events in std::iter::repeat_n(vec![crate::dialogs::testing::wheel(1.0e6)], 30).chain(std::iter::repeat_n(Vec::new(), 30)) {
            frame(&ctx, size, &mut repos, &mut screen, events);
        }
        assert_eq!(screen.scroll(), 0.0);
        // The pointer leaves the panel, so what is left of the wheel's smoothing moves nothing after.
        frame(&ctx, size, &mut repos, &mut screen, vec![Event::PointerMoved(Pos2::ZERO)]);
        // Back, then the forks of web, the same owner highlighted: they start at the top and follow it into view.
        repos.close_forks();
        repos.pick("web");
        repos.open_forks_plain();
        repos.pick_fork("owner-150");
        assert_eq!(view(&repos).repo, "web");
        frame(&ctx, size, &mut repos, &mut screen, Vec::new());
        let plan = plan_at(&ctx, &repos, size);
        assert!(body.contains_rect(row_rect(&plan, screen.scroll(), "owner-150").unwrap()), "web's highlight is not in view at {}", screen.scroll());
    }

    #[test]
    fn the_screen_starts_at_the_top_when_the_same_forks_open_again() {
        let mut win = Win::new(Session::demo(many(200)));
        win.size = [960.0, 600.0];
        win.frame(Vec::new());
        win.key(Key::End, None);
        assert_eq!(win.chosen().as_deref(), Some("owner-199"));
        assert!(win.pages.forks_screen().scroll() > 0.0, "the last fork was followed");
        win.click(win.pages_layout().toggle.center());
        assert!(win.repos().forks.is_none(), "back closed nothing");
        assert_eq!(win.pages.forks_screen().scroll(), 0.0, "what the screen kept is gone with the forks");
        win.repos_mut().pick("api");
        win.repos_mut().open_forks_plain();
        win.repos_mut().pick_fork("owner-000");
        win.frame(Vec::new());
        assert_eq!(win.pages.forks_screen().scroll(), 0.0, "the same forks again, at the top");
    }

    impl Win {
        fn pages_layout(&self) -> Layout {
            shell::layout(vec2(self.size[0], self.size[1]), true, true)
        }
    }

    #[test]
    fn the_filter_narrows_the_forks_and_esc_clears_it_before_going_back() {
        let mut win = Win::demo();
        assert_eq!(win.control(FILTER_HINT).width(), theme::FIELD_WIDTH, "the drawing's field");
        assert!(win.drew(FILTER_HINT), "the hint shows while the filter is empty");
        win.letter('/');
        assert!(win.typing(), "/ puts the keyboard in the filter");
        for typed in "lin".chars() {
            win.frame(vec![Event::Text(typed.to_string())]);
        }
        win.frame(Vec::new());
        assert_eq!(win.view().filter, "lin");
        assert_eq!(win.plan().rows.iter().map(|row| row.owner.as_str()).collect::<Vec<_>>(), ["lin-wei"]);
        // Esc in the field with text: the text goes, the keyboard stays.
        win.key(Key::Escape, None);
        assert_eq!(win.view().filter, "");
        assert!(win.typing() && win.repos().forks.is_some());
        win.frame(vec![Event::Text("zz".to_owned())]);
        win.frame(Vec::new());
        assert_eq!(win.plan().rows.len(), 0);
        assert!(win.drew(win.view().empty_message().unwrap().trim()), "the core's words for no match");
        win.key(Key::Escape, None);
        assert_eq!((win.view().filter.as_str(), win.typing()), ("", true));
        // Esc with no text leaves the field; the next goes back.
        win.key(Key::Escape, None);
        assert!(!win.typing() && win.repos().forks.is_some(), "the first Esc on an empty field only leaves it");
        // Enter leaves the field and keeps the filter; Esc then clears it without the keyboard, the next goes back.
        win.letter('/');
        win.frame(vec![Event::Text("bob".to_owned())]);
        win.key(Key::Enter, None);
        assert!(!win.typing());
        assert_eq!(win.view().filter, "bob");
        assert_eq!(win.repos().fork_notice(), None, "Enter in the field opened nothing");
        win.key(Key::Escape, None);
        assert_eq!(win.view().filter, "", "Esc cleared the filter first");
        assert!(win.repos().forks.is_some());
        // Down in the field gives the keyboard to the table; the highlight stays.
        win.letter('/');
        let chosen = win.chosen();
        win.key(Key::ArrowDown, None);
        assert!(!win.typing());
        assert_eq!(win.chosen(), chosen, "the Down that left the field moved nothing");
        win.key(Key::Escape, None);
        assert!(win.repos().forks.is_none(), "Esc with no filter goes back");
        win.frame(Vec::new());
        assert!(!win.typing(), "the filter's keyboard did not stay behind");
        // Backspace goes back too.
        win.repos_mut().pick("api");
        win.repos_mut().open_forks_plain();
        win.frame(Vec::new());
        win.key(Key::Backspace, None);
        assert!(win.repos().forks.is_none());
    }

    #[test]
    fn letters_typed_into_the_fork_filter_only_type() {
        let mut win = Win::demo();
        let reads = devassist_core::sys::disk_reads_here();
        win.letter('/');
        let stamp = win.repos().status_stamp();
        let before = (win.view().sort, win.view().changed_only, win.view().selected.clone());
        let letters = "urscgolfnaq jkUL/";
        for typed in letters.chars() {
            win.letter(typed);
        }
        assert_eq!(win.view().filter, letters, "every letter typed");
        assert_eq!((win.view().sort, win.view().changed_only, win.view().selected.clone()), before);
        assert_eq!(win.repos().status_stamp(), stamp, "nothing said: nothing asked");
        assert!(win.repos().forks.is_some() && win.repos().mode == CoreMode::List && win.repos().job.is_none());
        assert_eq!(devassist_core::sys::disk_reads_here(), reads);
    }

    #[test]
    fn with_changes_shows_only_forks_with_a_folder_and_says_how_many() {
        let mut win = Win::demo();
        win.size = [960.0, 600.0];
        win.frame(Vec::new());
        for words in ["All", "13", "With changes", "6"] {
            assert!(win.drew(words), "{words} is not drawn");
        }
        let lit = theme::palette(Mode::Light).selection;
        let all = win.control(ALL);
        let changes = win.control(WITH_CHANGES);
        let filled = |win: &Win, place: Rect| win.fills.iter().any(|(rect, fill)| *fill == lit && place.expand(0.5).contains_rect(*rect));
        assert!(filled(&win, all) && !filled(&win, changes), "All is the chosen one");
        // Scrolled down to the last fork, then the segment: only the forks with a folder, from the top.
        win.key(Key::End, None);
        assert!(win.pages.forks_screen().scroll() > 0.0);
        win.press(WITH_CHANGES);
        assert!(win.view().changed_only);
        assert!(win.plan().rows.iter().all(|row| row.folder) && win.plan().rows.len() == 6);
        assert_eq!(win.pages.forks_screen().scroll(), 0.0, "the list starts at the top");
        assert!(filled(&win, win.control(WITH_CHANGES)) && !filled(&win, win.control(ALL)));
        // c switches back, and again.
        win.letter('c');
        assert!(!win.view().changed_only);
        win.letter('C');
        assert!(win.view().changed_only);
        win.press(ALL);
        assert!(!win.view().changed_only);
    }

    #[test]
    fn the_sort_capsule_names_the_sort_and_cycles_it() {
        let mut win = Win::demo();
        assert!(win.drew("Most changes first"));
        win.press("Most changes first");
        assert_eq!(win.view().sort, ForkSort::Newest);
        assert!(win.drew("Newest commit first") && !win.drew("Most changes first"));
        win.letter('s');
        assert_eq!(win.view().sort, ForkSort::Name);
        assert!(win.drew("By name"));
        win.letter('S');
        assert_eq!(win.view().sort, ForkSort::Changed);
        // In the smallest window the app allows, every control keeps its words and none lies over another.
        win.size = [960.0, 600.0];
        for sort in [ForkSort::Changed, ForkSort::Newest, ForkSort::Name] {
            win.repos_mut().set_fork_sort(sort);
            win.frame(Vec::new());
            let toolbar = win.pages_layout().toolbar;
            let [field, segments, sort_place] = [FILTER_HINT, ALL, &widgets::capitalised(sort.label())].map(|words| win.control(words));
            assert!(field.right() + theme::TOOLBAR_GAP <= segments.left() + 1e-3, "{sort:?}");
            assert!(sort_place.left() >= win.control(WITH_CHANGES).right(), "{sort:?}");
            assert!(toolbar.contains_rect(sort_place) && (sort_place.right() - toolbar.right()).abs() < 1e-3, "{sort:?}");
            assert!(field.width() >= theme::FIELD_MIN_WIDTH, "{sort:?}");
        }
    }

    #[test]
    fn the_toolbar_gives_way_without_overlapping() {
        // The field narrows first, then the sort shows its icon alone; nothing leaves the toolbar or overlaps.
        let toolbar = Rect::from_min_size(pos2(280.0, 60.0), vec2(700.0, 36.0));
        let at = |width: f32| toolbar_places(Rect::from_min_size(toolbar.min, vec2(width, 36.0)), 200.0, [180.0, 36.0]);
        let (field, segments, sort, compact) = at(700.0);
        assert_eq!((field.width(), segments.width(), sort.width(), compact), (theme::FIELD_WIDTH, 200.0, 180.0, false));
        assert_eq!(sort.right(), toolbar.left() + 700.0);
        let (field, _, sort, compact) = at(620.0);
        assert!(!compact && field.width() < theme::FIELD_WIDTH && field.width() >= theme::FIELD_MIN_WIDTH && (sort.width() - 180.0).abs() < 1e-3);
        let (field, segments, sort, compact) = at(500.0);
        assert!(compact && (sort.width() - 36.0).abs() < 1e-3 && field.right() + theme::TOOLBAR_GAP <= segments.left() + 1e-3);
        for width in (200_u16..800).step_by(5) {
            let width = f32::from(width);
            let (field, segments, sort, _) = at(width);
            let room = Rect::from_min_size(toolbar.min, vec2(width, 36.0));
            for rect in [field, segments, sort] {
                assert!(room.expand(1e-3).contains_rect(rect), "{width}: {rect:?}");
            }
            assert!(field.right() <= segments.left() && segments.right() <= sort.left() + 1e-3, "{width}");
        }
    }

    #[test]
    fn keys_move_open_fetch_and_read_again_once_per_press() {
        let mut win = Win::demo();
        let reads = devassist_core::sys::disk_reads_here();
        let order: Vec<String> = win.view().rows().iter().map(|fork| fork.owner.clone()).collect();
        assert_eq!(win.chosen().as_deref(), Some("bob"));
        // Movement repeats while held.
        win.frame(vec![key_event(Key::ArrowDown, true, false, Modifiers::NONE)]);
        win.frame(vec![key_event(Key::ArrowDown, true, true, Modifiers::NONE)]);
        win.frame(vec![key_event(Key::ArrowDown, false, false, Modifiers::NONE)]);
        assert_eq!(win.chosen(), Some(order[4].clone()), "two down from bob");
        win.letter('k');
        win.letter('j');
        win.letter('j');
        assert_eq!(win.chosen(), Some(order[5].clone()));
        win.key(Key::Home, None);
        assert_eq!(win.chosen(), Some(order[0].clone()));
        win.key(Key::PageDown, None);
        assert_eq!(win.chosen(), Some(order[10].clone()));
        win.key(Key::End, None);
        assert_eq!(win.chosen(), order.last().cloned());
        win.key(Key::PageUp, None);
        assert_eq!(win.chosen(), Some(order[2].clone()));
        // Each acting key acts once, held or not: the held demo says its words once per press.
        let presses: [(Key, Option<&str>); 6] =
            [(Key::Enter, None), (Key::O, Some("o")), (Key::G, Some("g")), (Key::U, Some("u")), (Key::O, Some("O")), (Key::U, Some("U"))];
        for (key, text) in presses {
            let stamp = win.repos().status_stamp();
            let mut down = vec![key_event(key, true, false, Modifiers::NONE)];
            down.extend(text.map(|text| Event::Text(text.to_owned())));
            let mut held = vec![key_event(key, true, true, Modifiers::NONE)];
            held.extend(text.map(|text| Event::Text(text.to_owned())));
            win.frame(down);
            win.frame(held.clone());
            win.frame(held);
            win.frame(vec![key_event(key, false, false, Modifiers::NONE)]);
            assert_eq!(win.repos().status_stamp(), stamp + 1, "{key:?} {text:?} did not act once");
            assert_eq!(win.status(), (HELD.to_owned(), theme::palette(Mode::Light).quiet), "{key:?}: the demo's words on the status bar");
        }
        assert!(win.repos().job.is_none() && win.repos().queue.is_empty(), "the demo ran nothing");
        // Read again, by r and F5: the forks' sentence again, and the highlight stays.
        let chosen = win.chosen();
        win.letter('r');
        assert_eq!(win.status().0, win.view().summary());
        win.repos_mut().update_shown_forks();
        win.key(Key::F5, None);
        assert_eq!(win.status().0, win.view().summary());
        assert_eq!(win.chosen(), chosen);
        // Sort, segment: once per press.
        win.frame(vec![key_event(Key::S, true, false, Modifiers::NONE), Event::Text("s".to_owned())]);
        win.frame(vec![key_event(Key::S, true, true, Modifiers::NONE), Event::Text("s".to_owned())]);
        win.frame(vec![key_event(Key::S, false, false, Modifiers::NONE)]);
        assert_eq!(win.view().sort, ForkSort::Newest, "s acted once");
        // With Ctrl, Alt or Cmd held, a letter is no key here.
        let stamp = win.repos().status_stamp();
        for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::COMMAND] {
            win.frame(vec![key_event(Key::U, true, false, modifiers), Event::Text("u".to_owned())]);
            win.frame(vec![key_event(Key::U, false, false, modifiers)]);
            win.frame(vec![key_event(Key::Enter, true, false, modifiers)]);
            win.frame(vec![key_event(Key::Enter, false, false, modifiers)]);
        }
        assert_eq!(win.repos().status_stamp(), stamp);
        // The table's keys do nothing here, and q and Left do not go back.
        let ticks = win.repos().marked.clone();
        for typed in ['f', 'a', 'i', 'd', 'w', 'm', 'n', 'q', 'p', 't'] {
            win.letter(typed);
        }
        win.key(Key::Space, Some(" "));
        win.key(Key::ArrowLeft, None);
        win.key(Key::ArrowRight, None);
        win.key(Key::Delete, None);
        win.frame(vec![key_event(Key::A, true, false, Modifiers::COMMAND)]);
        win.frame(vec![key_event(Key::A, false, false, Modifiers::COMMAND)]);
        assert!(win.repos().forks.is_some() && win.repos().mode == CoreMode::List, "a table's key went on");
        assert!(!overlay::is_open(&win.ctx), "a table's key opened something");
        assert_eq!((win.repos().marked.clone(), win.repos().status_stamp()), (ticks, stamp));
        assert_eq!(devassist_core::sys::disk_reads_here(), reads, "the demo read the disk");
    }

    #[test]
    fn no_key_or_press_acts_under_an_overlay() {
        // A run's progress dialog over the forks: its keys are its own, and the Esc that hides it goes no further.
        let mut win = Win::demo();
        win.repos_mut().job = Some(crate::dialogs::testing::run(&["api"], 1));
        win.repos_mut().show_job();
        win.frame(Vec::new());
        win.frame(Vec::new());
        assert!(overlay::is_open(&win.ctx), "the dialog is not open");
        let (sort, chosen) = (win.view().sort, win.chosen());
        win.letter('s');
        win.letter('j');
        win.key(Key::End, None);
        let at = win.control(ALL).center();
        win.click(win.control(WITH_CHANGES).center());
        assert_eq!((win.view().sort, win.chosen(), win.view().changed_only), (sort, chosen.clone(), false));
        win.key(Key::Escape, None);
        win.frame(Vec::new());
        assert!(!overlay::is_open(&win.ctx), "Esc did not hide the dialog");
        assert!(win.repos().forks.is_some(), "the Esc that hid the dialog went back too");
        assert_eq!(win.chosen(), chosen);
        // Then the screen has the keyboard again.
        win.click(at);
        win.letter('s');
        assert_ne!(win.view().sort, sort);
    }

    #[test]
    fn a_fork_without_a_folder_has_no_folder_button_and_enter_says_why() {
        // A core that is not held: what it is asked of a fork without a folder or a gone one is a refusal in its
        // words, and nothing is opened. (Nothing here asks it to open a fork that could be opened.)
        let mut repos = Repos::empty(testing::test_settings());
        let forks = vec![ForkInfo { folder: true, ahead: Some(3), ..fork("bob") }, fork("cy"), ForkInfo { gone: true, ahead: None, ..fork("dee") }];
        testing::open_forks_of(&mut repos, "api", ForkList { original: "acme/api".to_owned(), forks, ..ForkList::default() });
        let mut win = Win::new(Session::Live(Box::new(repos)));
        let programs = devassist_core::sys::reached(devassist_core::sys::Reach::Program);
        let [folder, github] = win.buttons("bob");
        assert!(folder.is_some() && github.is_some(), "a fork with a folder on GitHub has both");
        assert_eq!(win.buttons("cy").map(|button| button.is_some()), [false, true], "no folder, no folder button");
        assert_eq!(win.buttons("dee").map(|button| button.is_some()), [false, false], "gone, and no folder");
        win.repos_mut().pick_fork("cy");
        win.frame(Vec::new());
        win.key(Key::Enter, None);
        let danger = theme::palette(Mode::Light).danger;
        assert_eq!(win.status(), ("cy has no commits of its own, so it has no folder. g opens it on GitHub.".to_owned(), danger));
        // The same from the top row's button.
        win.letter('j');
        assert_eq!(win.status().0, win.view().summary(), "the highlight moved: the forks' sentence again");
        win.repos_mut().pick_fork("cy");
        win.press(OPEN_THE_FOLDER);
        assert_eq!(win.status().0, "cy has no commits of its own, so it has no folder. g opens it on GitHub.");
        // A gone fork's GitHub page is not asked for: its row has no button, and g says why.
        win.repos_mut().pick_fork("dee");
        win.letter('g');
        assert_eq!(win.status(), ("dee/api is no longer on GitHub.".to_owned(), danger));
        assert!(win.repos().job.is_none());
        assert_eq!(devassist_core::sys::reached(devassist_core::sys::Reach::Program) - programs, 0, "a program was made ready");
    }

    #[test]
    fn a_gone_fork_has_no_github_button() {
        let mut win = Win::demo();
        let reads = devassist_core::sys::disk_reads_here();
        assert_eq!(win.buttons("old-contrib").map(|button| button.is_some()), [false, false]);
        assert_eq!(win.buttons("a-petrov").map(|button| button.is_some()), [false, true]);
        assert_eq!(win.buttons("mkoenig").map(|button| button.is_some()), [true, true]);
        // In the 72 points kept for them, the folder's first.
        let [folder, github] = win.buttons("mkoenig").map(Option::unwrap);
        let row = win.row("mkoenig");
        let actions = win.plan().columns.actions;
        assert_eq!(folder.left(), row.left() + actions.left);
        assert_eq!(github.left(), folder.right());
        assert!(github.right() <= row.left() + actions.right());
        // Their names on hover.
        win.rest(folder.center());
        assert!(win.drew(OPEN_THE_FOLDER), "no hover text on the folder button");
        win.rest(github.center());
        assert!(win.drew(OPEN_ON_GITHUB), "no hover text on the GitHub button");
        // A row's button acts on its row: the fork is highlighted, then the demo's words.
        let stamp = win.repos().status_stamp();
        let github = win.buttons("a-petrov")[1].unwrap();
        win.click(github.center());
        assert_eq!(win.chosen().as_deref(), Some("a-petrov"));
        assert_eq!(win.repos().status_stamp(), stamp + 1);
        assert_eq!(win.status().0, HELD);
        let folder = win.buttons("tessera-labs")[0].unwrap();
        win.click(folder.center());
        assert_eq!(win.chosen().as_deref(), Some("tessera-labs"));
        assert_eq!(win.repos().status_stamp(), stamp + 2);
        assert_eq!(devassist_core::sys::disk_reads_here(), reads);
    }

    #[test]
    fn a_double_click_opens_the_folder_once() {
        let mut win = Win::demo();
        win.step = 0.02;
        let reads = devassist_core::sys::disk_reads_here();
        let stamp = win.repos().status_stamp();
        let row = win.row("mkoenig");
        // Away from its buttons.
        let at = pos2(row.left() + 40.0, row.center().y);
        win.click(at);
        assert_eq!(win.chosen().as_deref(), Some("mkoenig"));
        assert_eq!(win.repos().status_stamp(), stamp, "one click only highlights");
        win.click(at);
        assert_eq!(win.repos().status_stamp(), stamp + 1, "the second click opened it, once");
        assert_eq!(win.status().0, HELD);
        // A third click is no second double click.
        win.click(at);
        assert_eq!(win.repos().status_stamp(), stamp + 1);
        // A click on a row's button, then one on the row beside it: no double click of the row.
        win.step = 1.0;
        win.frame(Vec::new());
        win.step = 0.02;
        let stamp = win.repos().status_stamp();
        let folder = win.buttons("tessera-labs")[0].unwrap();
        win.click(pos2(folder.left() + 2.0, folder.center().y));
        win.click(pos2(folder.left() - 2.0, folder.center().y));
        assert_eq!(win.chosen().as_deref(), Some("tessera-labs"));
        assert_eq!(win.repos().status_stamp(), stamp + 1, "only the button's press acted");
        // A double click on a row's button presses it once.
        win.step = 1.0;
        win.frame(Vec::new());
        win.step = 0.02;
        let stamp = win.repos().status_stamp();
        let folder = win.buttons("bob")[0].unwrap();
        win.click(folder.center());
        win.click(folder.center());
        assert_eq!(win.repos().status_stamp(), stamp + 1, "a double click on a button acts once");
        assert_eq!(devassist_core::sys::disk_reads_here(), reads);
    }

    #[test]
    fn the_top_row_gives_way_before_it_meets_the_title() {
        let ctx = context();
        let repos = forks_open();
        let title = {
            let mut width = 0.0;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1440.0, 900.0))), ..Default::default() };
            ctx.run_ui(input, |ui| width = widgets::width(ui.painter(), "Forks of api", &shell::title_font())).drop_without_applying_deltas();
            width
        };
        let mut seen = Vec::new();
        for width in (400..=2000).step_by(4) {
            for sidebar in [true, false] {
                #[expect(clippy::cast_precision_loss, reason = "window widths of a few thousand points")]
                let size = vec2(width as f32, 900.0);
                let layout = shell::layout(size, sidebar, true);
                let mut planned = None;
                let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
                ctx.run_ui(input, |ui| planned = Some(plan(view(&repos), &layout, ui.painter()))).drop_without_applying_deltas();
                let planned = planned.unwrap();
                let at = format!("{width} sidebar {sidebar}");
                let Some(top) = planned.top else {
                    assert!(width < 960, "{at}: no top row in a window the app allows");
                    continue;
                };
                assert!(top.actions.left() >= layout.title.left() + title + theme::BAR_GAP - 1e-3, "{at}: over the title");
                assert!(top.main.right() <= layout.captions.left() - theme::BAR_GAP + 1e-3, "{at}: over the window's buttons");
                assert!((top.actions.right() + theme::BAR_GAP - top.main.left()).abs() < 1e-3, "{at}");
                // In the design's window, all the words; in every window the app allows, the main button keeps its own.
                if width >= 1440 {
                    assert_eq!(top.fit, Fit::Full, "{at}");
                }
                if width >= 960 {
                    assert_ne!(top.fit, Fit::Bare, "{at}");
                }
                if !seen.contains(&top.fit) {
                    seen.push(top.fit);
                }
            }
        }
        seen.sort_by_key(|fit| *fit as usize);
        assert_eq!(seen, [Fit::Full, Fit::Compact, Fit::Bare], "a narrow window shows every way the row gives way");
        // Its presses: the demo reads again from its story, and says its words for the rest; icons only keep their
        // words as hover text.
        let mut win = Win::demo();
        let reads = devassist_core::sys::disk_reads_here();
        win.press(OPEN_THE_FOLDER);
        assert_eq!(win.status().0, HELD);
        win.press(READ_AGAIN);
        assert_eq!(win.status().0, win.view().summary());
        win.press(FETCH_THE_FORKS);
        assert_eq!(win.status().0, HELD);
        assert!(win.repos().job.is_none());
        win.size = [700.0, 900.0];
        win.frame(Vec::new());
        assert_eq!(win.plan().top.map(|top| top.fit), Some(Fit::Bare));
        win.rest(win.control(FETCH_THE_FORKS).center());
        assert!(win.drew(FETCH_THE_FORKS), "an icon alone has its words on hover");
        assert_eq!(devassist_core::sys::disk_reads_here(), reads);
    }

    #[test]
    fn nothing_shown_means_nothing_to_act_on() {
        let mut win = Win::demo();
        win.letter('/');
        win.frame(vec![Event::Text("nobody".to_owned())]);
        win.key(Key::Enter, None);
        assert!(win.plan().rows.is_empty() && win.chosen().is_none());
        let stamp = win.repos().status_stamp();
        win.key(Key::Enter, None);
        win.letter('o');
        win.letter('g');
        win.press(OPEN_THE_FOLDER);
        assert_eq!(win.repos().status_stamp(), stamp, "nothing to open, nothing said");
        // With changes and no fork with a folder: the same.
        win.key(Key::Escape, None);
        let mut repos = many(3);
        repos.set_fork_changed_only(true);
        let mut win = Win::new(Session::demo(repos));
        assert_eq!(win.plan().rows.len(), 0);
        let stamp = win.repos().status_stamp();
        win.key(Key::Enter, None);
        win.letter('g');
        assert_eq!(win.repos().status_stamp(), stamp);
        assert!(win.drew(win.view().empty_message().unwrap().trim()));
    }

    #[test]
    fn a_late_read_changes_nothing_after_going_back() {
        let mut win = Win::demo();
        let late = ForkList { original: "acme/api".to_owned(), forks: vec![fork("late")], ..ForkList::default() };
        // A read on its way when the person reads again: its answer is too late, the rows and the highlight stay.
        let first = win.repos().forks_generation;
        win.letter('r');
        win.repos_mut().on_msg(Msg::Forks { generation: first, result: Ok(late.clone()) });
        win.frame(Vec::new());
        assert_eq!(win.plan().rows.len(), 13);
        assert_eq!(win.chosen().as_deref(), Some("bob"));
        // A read on its way when the person goes back: nothing comes back.
        let open = win.repos().forks_generation;
        win.key(Key::Escape, None);
        assert!(win.repos().forks.is_none());
        win.repos_mut().on_msg(Msg::Forks { generation: open, result: Ok(late) });
        win.frame(Vec::new());
        assert!(win.repos().forks.is_none() && win.plan.is_none());
        assert!(win.drew("Repository"), "the table is back");
    }

    #[test]
    fn cut_words_show_all_of_them_on_hover() {
        let mut repos = forks_open();
        let view = repos.forks.as_mut().unwrap();
        let mkoenig = view.forks.iter_mut().find(|fork| fork.owner == "mkoenig").unwrap();
        mkoenig.ahead = Some(4_000_000_000);
        let mut win = Win::new(Session::demo(repos));
        win.size = [960.0, 900.0];
        win.frame(Vec::new());
        let row = win.row("mkoenig");
        let c = win.plan().columns;
        let ahead = "4000000000 ahead".to_owned();
        let words_left = row.left() + c.ahead.left + columns::rule_length(c.room) + RULE_LABEL_GAP;
        assert!(!win.drew(&ahead), "the ahead words are cut at 960");
        win.rest(pos2(words_left + 4.0, row.center().y));
        assert!(win.drew(&ahead), "no hover text with the whole ahead words");
        // The empty panel's sentence and the error's words, cut in a narrow panel, on hover.
        let mut repos = forks_open();
        let view = repos.forks.as_mut().unwrap();
        view.forks.clear();
        let detail = "the folder could not be read because something long and unlikely happened to it on the way ".repeat(4);
        view.error = Some(detail.trim().to_owned());
        let mut win = Win::new(Session::demo(repos));
        let panel = win.pages_layout().panel;
        let shown = win.texts.iter().find(|(words, rect, _)| panel.contains(rect.center()) && words.starts_with("The folder could")).cloned();
        let (said, rect, _) = shown.expect("the error's words are drawn");
        assert!(said.ends_with('…'), "{said}");
        win.rest(rect.center());
        assert!(win.drew(&widgets::capitalised(detail.trim())), "no hover text with the whole error");
    }

    #[test]
    fn the_filter_does_not_take_the_tables_filter_or_stay_behind() {
        let mut win = Win::demo();
        win.letter('/');
        win.frame(vec![Event::Text("bo".to_owned())]);
        assert!(win.typing());
        // Back by the round button while typing: the keyboard leaves with the forks, and the table's filter is empty.
        win.click(win.pages_layout().toggle.center());
        assert!(win.repos().forks.is_none());
        win.frame(Vec::new());
        assert!(!win.typing() && win.ctx.memory(|memory| memory.focused().is_none()));
        assert_eq!(win.repos().list_filter, "");
        win.letter('s');
        assert_eq!(win.repos().list_filter, "", "s on the table sorts, and types nowhere");
    }
}
