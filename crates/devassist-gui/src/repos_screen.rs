//! The Repositories screen in the shell: "Import", "Update all" and the main button "Add repository" in the top row
//! (also with no repositories), the toolbar above the panel (the filter, the sort, and the selection bar with what
//! can be done to the ticked rows), the table on the panel or the sentence that says why there is none, the status
//! bar's words, and the keys of the terminal app (`a` opens Add, `i` Import). Every change goes
//! through a method of the core's `Repos`; what it says comes back through the status bar.
//!
//! Where the terminal app and the window differ: a double click on a row does not update it, Esc does not cancel a
//! run that has a progress dialog, shown or hidden (its button does; Esc cancels only work without one, such as a
//! deletion from disk), and `d` on a group's header with nothing ticked does not dissolve the group (the core
//! would, without asking): the status line asks for repositories instead.

use devassist_core::repos::state::{Action, GROUP_KEY, Mode as CoreMode, Repos, SortMode};
use egui::{CursorIcon, Event, FocusDirection, FontFamily, Id, Key, Modifiers, Painter, PointerButton, Pos2, Rect, Sense, Ui, pos2};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::repos_view::{self, Line, Status};
use crate::session::Session;
use crate::shell::{self, Layout, Page, Wants};
use crate::table::{self, Did};
use crate::theme::{
    self, BAR_GAP, FIELD_MIN_WIDTH, FIELD_WIDTH, Mode, PAGE_ROWS, PANEL_TEXT_INSET, RADIUS_PANEL, STATE_GAP, TEXT, TEXT_TITLE, TOOLBAR_GAP,
};
use crate::widgets::{self, Item, font};
use crate::{frost, overlay};

/// The filter field's glass pane.
const FILTER: &str = "filter";
/// The view capsule's glass pane.
const VIEW: &str = "view";
/// The selection capsule's glass pane.
const SELECTION: &str = "selection";
/// The glass pane of the top row's actions.
const ACTIONS: &str = "actions";
/// The top row's action: add the clones in a folder.
const IMPORT: &str = "Import";
/// The top row's action: update every repository.
const UPDATE_ALL: &str = "Update all";
/// The top row's main button: find repositories on GitHub and clone them.
const ADD_REPOSITORY: &str = "Add repository";
/// The places of the top row's actions among its capsule's items ([`top_items`]): Import.
const TOP_IMPORT: usize = 0;
/// Update all.
const TOP_UPDATE_ALL: usize = 1;
/// The selection bar's actions on what is ticked, as the drawing words them.
const UPDATE: &str = "Update";
/// Move what is ticked into a group.
const MOVE_TO_GROUP: &str = "Move to group";
/// Open the folders of what is ticked.
const OPEN_FOLDER: &str = "Open folder";
/// Remove what is ticked.
const REMOVE: &str = "Remove";
/// The places of the selection bar's buttons among its items ([`selection_items`]).
const BAR_UPDATE: usize = 2;
/// Move to group.
const BAR_GROUP: usize = 3;
/// Open folder.
const BAR_OPEN: usize = 4;
/// Remove.
const BAR_REMOVE: usize = 5;
/// The button that clears the ticks.
const BAR_CLEAR: usize = 6;
/// What the filter field says while it is empty.
const FILTER_HINT: &str = "Filter repositories";
/// The sort by name.
const BY_NAME: &str = "By name";
/// The sort that puts what needs attention first.
const ATTENTION_FIRST: &str = "Needs attention first";
/// The title of a panel without repositories.
const NO_REPOSITORIES: &str = "No repositories yet";
/// What to do without repositories: the top row's two buttons.
const ADD_OR_IMPORT: &str = "Add repository finds them on GitHub. Import takes clones you already have.";
/// The title of a panel whose settings file could not be read.
const BROKEN: &str = "The settings file has a mistake";
/// What to do about a settings file with a mistake.
const CORRECT_THE_FILE: &str = "Correct the file and start DevAssist again. Nothing was loaded, so nothing is overwritten.";
/// What to do when the filter lets nothing through.
const ESC_CLEARS: &str = "Esc clears the filter.";

/// The title of a panel whose filter lets nothing through.
fn nothing_matches(filter: &str) -> String {
    format!("Nothing matches {filter}")
}

/// How many repositories are ticked, in the selection capsule.
fn selected(count: usize) -> String {
    format!("{count} selected")
}

/// What the screen keeps from one frame to the next.
#[derive(Clone, Debug, Default)]
pub struct Screen {
    /// How far the table's lines are moved up.
    scroll: f32,
    /// The filter field had the focus at the end of the last frame.
    filter_focused: bool,
    /// The filter field had the focus when an overlay took the keyboard, and gets it back when the overlay goes.
    refocus: bool,
    /// While the row's menu is open: the key of the row it was opened on, and where that row's button is.
    menu: Option<(String, Rect)>,
}

#[cfg(test)]
impl Screen {
    /// The key of the row the open menu belongs to.
    pub(crate) fn menu_row(&self) -> Option<&str> {
        self.menu.as_ref().map(|(key, _)| key.as_str())
    }
}

/// Where the button is that the open menu stands under, as the screen placed it in the last frame: the "…" of the
/// row it was opened on (`table::dots_rect`). `None` while no menu is open, and in the frame one opens, before the
/// screen has placed it.
#[must_use]
pub fn menu_anchor(screen: &Screen) -> Option<Rect> {
    screen.menu.as_ref().map(|(_, anchor)| *anchor)
}

/// Opens the menu for the row `key`, as its "…" asks: for that row alone when it is not ticked (the ticks are
/// cleared first), for every ticked row when it is. The row is highlighted either way.
pub fn open_menu(repos: &mut Repos, key: &str) {
    if !repos.is_marked_local(key) {
        repos.clear_marks();
    }
    repos.pick(key);
    repos.enter_row();
}

/// How a capsule of actions shows them, or the top row its actions and its main button: with their words, or with
/// their icons only when the room is too narrow.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Fit {
    /// Icons and words.
    Full,
    /// Icons only; each word is its button's name and hover text. In the top row, the capsule's actions; the main
    /// button keeps its words.
    Compact,
    /// Icons only, and the selection bar's count without its word ("9999" for "9999 selected", which is its name and
    /// hover text); in the top row, the main button with its icon alone too.
    Bare,
}

/// Where the screen's controls are this frame, and what they say: decided before the bake, so their glass is
/// baked where they are drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The toolbar, when there are repositories to filter, sort and tick.
    toolbar: Option<Toolbar>,
    /// The top row's actions and its main button, when there is room for them.
    top: Option<Top>,
}

/// The toolbar's controls.
#[derive(Clone, Debug, PartialEq)]
struct Toolbar {
    /// The filter field.
    field: Rect,
    /// The view capsule.
    view: Rect,
    /// The words of the sort.
    sort: &'static str,
    /// The selection capsule, how many are ticked and how it fits, when something is ticked.
    selection: Option<(Rect, usize, Fit)>,
}

/// The top row's controls: the capsule with "Import" and "Update all", the main button "Add repository" at its right,
/// and how they fit. The forks screen places its own the same way.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Top {
    /// The capsule.
    pub(crate) actions: Rect,
    /// The main button.
    pub(crate) main: Rect,
    /// How they fit.
    pub(crate) fit: Fit,
}

impl Plan {
    /// Adds the glass of the screen's controls to the panes baked this frame. The main button is a fill, not glass.
    pub fn add_wants(&self, wants: &mut Wants) {
        if let Some(toolbar) = &self.toolbar {
            widgets::add_glass(wants, FILTER, toolbar.field);
            widgets::add_glass(wants, VIEW, toolbar.view);
            if let Some((rect, ..)) = &toolbar.selection {
                widgets::add_glass(wants, SELECTION, *rect);
            }
        }
        if let Some(top) = &self.top {
            widgets::add_glass(wants, ACTIONS, top.actions);
        }
    }
}

/// Whether the screen has its toolbar: when there are repositories to filter, sort and tick, and the forks of one
/// do not stand in the table's place ([`crate::forks_screen::shown`]).
#[must_use]
pub fn has_toolbar(session: &Session) -> bool {
    session.repos().is_some_and(|repos| !repos.installed.is_empty() && !crate::forks_screen::shown(repos))
}

/// The screen's controls in `layout`, measured with `painter`'s faces: the top row's, and the toolbar when there are
/// repositories; `None` when nothing is loaded (the settings file has a mistake) or while the forks of a repository
/// take the table's place: the forks screen has the page then.
#[must_use]
pub fn plan(painter: &Painter, layout: &Layout, session: &Session) -> Option<Plan> {
    let repos = session.repos().filter(|repos| !crate::forks_screen::shown(repos))?;
    let toolbar = has_toolbar(session).then(|| {
        let sort = if repos.sort == SortMode::NeedsUpdate { ATTENTION_FIRST } else { BY_NAME };
        let view_width = widgets::capsule_width(painter, &view_items(sort));
        let count = repos.marked.len();
        let (number, words) = (count.to_string(), selected(count));
        let fits = [Fit::Full, Fit::Compact, Fit::Bare];
        let selection_widths = (count > 0).then(|| fits.map(|fit| widgets::capsule_width(painter, &selection_items(&number, &words, fit))));
        let (field, view, selection) = places(layout.toolbar, view_width, selection_widths);
        Toolbar { field, view, sort, selection: selection.map(|(rect, fit)| (rect, count, fit)) }
    });
    let title_width = widgets::width(painter, Page::Repositories.name(), &shell::title_font());
    let actions = [Fit::Full, Fit::Compact].map(|fit| widgets::capsule_width(painter, &top_items(fit)));
    let main = [false, true].map(|compact| widgets::main_action_width(painter, ADD_REPOSITORY, compact, layout.actions.height()));
    let top = top_place(shell::action_room(layout, title_width), actions, main);
    Some(Plan { toolbar, top })
}

/// The view capsule's items: the sort, whose words say which it is (no fill, as the design draws it).
fn view_items(sort: &str) -> [Item<'_>; 1] {
    [Item::action(Icon::ArrowUpDown, sort)]
}

/// The top row's capsule: "Import" and "Update all", with their icons only unless it is [`Fit::Full`]. Their places
/// are `TOP_IMPORT` and `TOP_UPDATE_ALL`.
fn top_items(fit: Fit) -> [Item<'static>; 2] {
    let compact = fit != Fit::Full;
    [
        Item::Action { icon: Icon::FolderInput, words: IMPORT, on: false, danger: false, compact },
        Item::Action { icon: Icon::ArrowDownToLine, words: UPDATE_ALL, on: false, danger: false, compact },
    ]
}

/// The selection capsule's items, in the drawing's order: how many are ticked (`words`, or `number` alone when it
/// is [`Fit::Bare`]), a divider, what can be done to them (Remove in the danger colour), and the button that clears
/// the ticks. Their places are `BAR_UPDATE` and on.
fn selection_items<'a>(number: &'a str, words: &'a str, fit: Fit) -> [Item<'a>; 7] {
    let compact = fit != Fit::Full;
    let action = |icon, words, danger| Item::Action { icon, words, on: false, danger, compact };
    [
        if fit == Fit::Bare { Item::Count { shown: number, name: words } } else { Item::Label(words) },
        Item::Divider,
        action(Icon::ArrowDownToLine, UPDATE, false),
        action(Icon::Group, MOVE_TO_GROUP, false),
        action(Icon::FolderOpen, OPEN_FOLDER, false),
        action(Icon::Trash2, REMOVE, true),
        Item::action(Icon::X, ""),
    ]
}

/// The places of the filter field, the view capsule and the selection capsule in the toolbar row at `toolbar`, the
/// selection `widths` wide in each [`Fit`] (full, compact, bare) when something is ticked: the field and the view
/// from the left, the selection at the right end. Beside a field of [`FIELD_WIDTH`], the selection takes the first
/// fit that fits: with its words, then icons only, then the count without its word. When not even that fits, the
/// field narrows for it, down to [`FIELD_MIN_WIDTH`]; past that the field gives up the rest rather than the bar
/// being cut (in no window the app allows, with up to 9999 ticked, does it come to that: tested). The selection is
/// never narrower than its contents and never lies over the view.
fn places(toolbar: Rect, view_width: f32, widths: Option<[f32; 3]>) -> (Rect, Rect, Option<(Rect, Fit)>) {
    let (top, bottom) = (toolbar.top(), toolbar.bottom());
    // What is left for the selection beside a field `field` wide.
    let room = |field: f32| toolbar.width() - field - TOOLBAR_GAP - view_width - TOOLBAR_GAP;
    let chosen = widths.map(|widths| {
        let fits = [Fit::Full, Fit::Compact, Fit::Bare];
        let index = widths.iter().position(|width| *width <= room(FIELD_WIDTH)).unwrap_or(fits.len() - 1);
        (widths[index], fits[index])
    });
    let field_width = match chosen {
        Some((width, _)) if width > room(FIELD_WIDTH) => {
            let narrowed = (FIELD_WIDTH - (width - room(FIELD_WIDTH))).max(FIELD_MIN_WIDTH);
            if width <= room(narrowed) { narrowed } else { (FIELD_WIDTH - (width - room(FIELD_WIDTH))).max(0.0) }
        }
        _ => FIELD_WIDTH,
    };
    let field = Rect::from_min_max(pos2(toolbar.left(), top), pos2((toolbar.left() + field_width).min(toolbar.right()), bottom));
    let view_left = (field.right() + TOOLBAR_GAP).min(toolbar.right());
    let view = Rect::from_min_max(pos2(view_left, top), pos2((view_left + view_width).min(toolbar.right()), bottom));
    let selection = chosen.map(|(width, fit)| {
        let left = (toolbar.right() - width).max(view.right() + TOOLBAR_GAP).min(toolbar.right());
        (Rect::from_min_max(pos2(left, top), pos2(toolbar.right(), bottom)), fit)
    });
    (field, view, selection)
}

/// The places of the top row's capsule, `actions` wide with its words and without them, and of its main button,
/// `main` wide with its words and without them, at the right end of `room` ([`shell::action_room`]), the main button
/// rightmost and [`BAR_GAP`] between them. They take the first fit that fits: all words ([`Fit::Full`]), then the
/// capsule's icons only ([`Fit::Compact`]: the main button keeps its words longest), then the main button's icon
/// too ([`Fit::Bare`]); `None` when not even that fits, so nothing lies over the title or leaves the row.
pub(crate) fn top_place(room: Rect, [full, compact]: [f32; 2], [main_full, main_compact]: [f32; 2]) -> Option<Top> {
    let fits = [(Fit::Full, full, main_full), (Fit::Compact, compact, main_full), (Fit::Bare, compact, main_compact)];
    let (fit, actions, main) = fits.into_iter().find(|(_, actions, main)| actions + BAR_GAP + main <= room.width())?;
    let main = Rect::from_min_max(pos2(room.right() - main, room.top()), room.max);
    let actions = Rect::from_min_max(pos2(main.left() - BAR_GAP - actions, room.top()), pos2(main.left() - BAR_GAP, room.bottom()));
    Some(Top { actions, main, fit })
}

/// What the status bar says for the screen; `None` when nothing is loaded.
#[must_use]
pub fn status(session: &Session) -> Option<Status> {
    session.repos().map(repos_view::status)
}

/// The title and the lines under it that the panel shows instead of the table, when it has no table to show: the
/// settings file has a mistake, there are no repositories, or the filter lets none through.
#[must_use]
pub fn state_text(session: &Session) -> Option<(String, Vec<String>)> {
    let repos = match session {
        Session::Broken(message) => return Some((BROKEN.to_owned(), vec![message.clone(), CORRECT_THE_FILE.to_owned()])),
        Session::Live(_) | Session::Demo(_) => session.repos()?,
    };
    if repos.installed.is_empty() {
        // While the folders are still being read, the status bar says so and the panel waits.
        return (!repos.waiting()).then(|| (NO_REPOSITORIES.to_owned(), vec![ADD_OR_IMPORT.to_owned()]));
    }
    if filter_hides_everything(repos) {
        return Some((nothing_matches(&repos.list_filter), vec![ESC_CLEARS.to_owned()]));
    }
    None
}

/// What a key asks of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    /// Move the highlight this many rows.
    Move(isize),
    /// Highlight the first row (`true`) or the last.
    End(bool),
    /// Tick or untick the highlighted row and move down.
    Tick,
    /// Tick every row shown.
    TickAll,
    /// Clear the filter when it lets nothing through, else stop work that has no dialog, else clear the ticks.
    Escape,
    /// Change the sort.
    Sort,
    /// Put the focus in the filter.
    Filter,
    /// Type this into the filter: text that came after the key that put the focus there, in the same frame.
    Type(char),
    /// Fetch and read again.
    Refresh,
    /// Fold the highlighted group (`true`) or unfold it.
    Fold(bool),
    /// Show the run's progress dialog again.
    ShowRun,
    /// Open the menu of the highlighted row as its "…" does, or fold the highlighted group.
    Enter,
    /// Update the ticked rows, or the highlighted one (`u`).
    UpdateChosen,
    /// Update every repository (`U`).
    UpdateAll,
    /// Start or stop the automatic pull of the ticked rows, or the highlighted one (`w`).
    Watch,
    /// Open the folders of the ticked rows, or the highlighted one's (`o`).
    Open,
    /// Ask which group the ticked rows, or the highlighted one, go into (`m`).
    Group,
    /// Ask before removing the ticked rows, or the highlighted one (`d`, Delete).
    Remove,
    /// Open Add, to find repositories on GitHub (`a`).
    Add,
    /// Open Import, to add the clones in a folder (`i`).
    Import,
}

impl Command {
    /// Whether it acts on the repositories (and so happens once for a held key), rather than moving or showing. No
    /// wildcard: a new command is placed here on purpose.
    fn acts(self) -> bool {
        match self {
            Command::UpdateChosen
            | Command::UpdateAll
            | Command::Watch
            | Command::Open
            | Command::Group
            | Command::Remove
            | Command::Add
            | Command::Import => true,
            Command::Move(_)
            | Command::End(_)
            | Command::Tick
            | Command::TickAll
            | Command::Escape
            | Command::Sort
            | Command::Filter
            | Command::Type(_)
            | Command::Refresh
            | Command::Fold(_)
            | Command::ShowRun
            | Command::Enter => false,
        }
    }
}

#[cfg(test)]
impl Command {
    /// How many kinds of command there are: [`Command::kind`] gives each one its number below this.
    pub(crate) const KINDS: usize = 20;

    /// The number of this command's kind, whatever it carries, for the tests that press every key the screen knows
    /// and must reach every kind. No wildcard: a new command must be numbered here, and [`Command::KINDS`] grown.
    pub(crate) fn kind(self) -> usize {
        match self {
            Command::Move(_) => 0,
            Command::End(_) => 1,
            Command::Tick => 2,
            Command::TickAll => 3,
            Command::Escape => 4,
            Command::Sort => 5,
            Command::Filter => 6,
            Command::Type(_) => 7,
            Command::Refresh => 8,
            Command::Fold(_) => 9,
            Command::ShowRun => 10,
            Command::Enter => 11,
            Command::UpdateChosen => 12,
            Command::UpdateAll => 13,
            Command::Watch => 14,
            Command::Open => 15,
            Command::Group => 16,
            Command::Remove => 17,
            Command::Add => 18,
            Command::Import => 19,
        }
    }
}

/// What the keys among `events` ask, in order, as the terminal app reads them. Letters are read from the typed
/// text, so they follow the keyboard's layout; text typed with Ctrl, Alt or Cmd held (as the last key said) is no
/// key here, as Ctrl-A is told from `a`. Once `/` has asked for the filter, the rest of the frame's text is typed
/// into it and its other keys are the field's.
pub(crate) fn commands(events: &[Event]) -> Vec<Command> {
    let mut commands = Vec::new();
    // The modifiers held with the last key, and whether it was a repeat of a held key: the text a key types comes
    // after it.
    let (mut held, mut repeated) = (Modifiers::NONE, false);
    for event in events {
        if let Event::Key { modifiers, repeat, .. } = event {
            (held, repeated) = (*modifiers, *repeat);
        }
        match event {
            // Once the filter has the focus, its keys are the field's.
            Event::Key { .. } if commands.contains(&Command::Filter) => {}
            Event::Key { key, pressed: true, modifiers, repeat, .. } => {
                let command = match key {
                    Key::ArrowUp => Command::Move(-1),
                    Key::ArrowDown => Command::Move(1),
                    Key::PageUp => Command::Move(-PAGE_ROWS),
                    Key::PageDown => Command::Move(PAGE_ROWS),
                    Key::Home => Command::End(true),
                    Key::End => Command::End(false),
                    Key::Space if !modifiers.any() => Command::Tick,
                    Key::A if modifiers.command_only() => Command::TickAll,
                    // Held, it does nothing more: the press that closed a dialog does not go on to stop work, clear
                    // the ticks or clear the filter.
                    Key::Escape if !repeat => Command::Escape,
                    Key::F5 => Command::Refresh,
                    Key::ArrowLeft => Command::Fold(true),
                    Key::ArrowRight => Command::Fold(false),
                    // Held, it folds a header once, not on and off at the key's repeat rate.
                    Key::Enter if !modifiers.any() && !repeat => Command::Enter,
                    Key::Delete if !modifiers.any() && !repeat => Command::Remove,
                    _ => continue,
                };
                commands.push(command);
            }
            Event::Text(text) => {
                for typed in text.chars() {
                    if commands.contains(&Command::Filter) {
                        commands.push(Command::Type(typed));
                        continue;
                    }
                    if held.ctrl || held.alt || held.command || held.mac_cmd {
                        continue;
                    }
                    let command = match typed {
                        'j' => Command::Move(1),
                        'k' => Command::Move(-1),
                        's' | 'S' => Command::Sort,
                        'r' => Command::Refresh,
                        '/' => Command::Filter,
                        'l' | 'L' => Command::ShowRun,
                        'u' => Command::UpdateChosen,
                        'U' => Command::UpdateAll,
                        'w' | 'W' => Command::Watch,
                        'o' | 'O' => Command::Open,
                        'm' | 'M' => Command::Group,
                        'd' => Command::Remove,
                        'a' => Command::Add,
                        'i' | 'I' => Command::Import,
                        _ => continue,
                    };
                    // A held key moves the highlight again and again, but acts once.
                    if repeated && command.acts() {
                        continue;
                    }
                    commands.push(command);
                }
            }
            _ => {}
        }
    }
    commands
}

/// Every control of the screen's toolbar and top row that `plan` (from [`plan`]) places, with its words (an icon
/// alone by its name), measured with `painter`'s faces: the filter field, the sort, each item of the selection bar,
/// "Import", "Update all" and "Add repository". For the tests that press every control; it reads the same items the
/// screen draws.
#[cfg(test)]
pub(crate) fn control_places(painter: &Painter, plan: &Plan) -> Vec<(String, Rect)> {
    let mut controls = Vec::new();
    let words = |item: &Item<'_>| match *item {
        Item::Divider => "divider".to_owned(),
        Item::Action { icon, words: "", .. } => icon.name().to_owned(),
        Item::Label(words) | Item::Count { name: words, .. } | Item::Action { words, .. } => words.to_owned(),
    };
    if let Some(toolbar) = &plan.toolbar {
        controls.extend([(FILTER_HINT.to_owned(), toolbar.field), (toolbar.sort.to_owned(), toolbar.view)]);
        if let Some((rect, count, fit)) = toolbar.selection {
            let (number, said) = (count.to_string(), selected(count));
            let items = selection_items(&number, &said, fit);
            controls.extend(items.iter().map(words).zip(widgets::item_rects(painter, rect, &items)));
        }
    }
    if let Some(top) = plan.top {
        let items = top_items(top.fit);
        controls.extend(items.iter().map(words).zip(widgets::item_rects(painter, top.actions, &items)));
        controls.push((ADD_REPOSITORY.to_owned(), top.main));
    }
    controls
}

/// The top row's capsule and main button that `plan` places, for the contrast test's places.
#[cfg(test)]
pub(crate) fn top_row(plan: &Plan) -> Option<(Rect, Rect)> {
    plan.top.map(|top| (top.actions, top.main))
}

/// The scroll that shows the whole line at `index` of lines of `heights` in `room` points, moving `scroll` as
/// little as it takes.
pub(crate) fn follow(heights: &[f32], index: usize, scroll: f32, room: f32) -> f32 {
    let Some(height) = heights.get(index) else { return scroll };
    let top: f32 = heights[..index].iter().sum();
    let bottom = top + height;
    if top < scroll {
        top
    } else if bottom > scroll + room {
        (bottom - room).max(0.0)
    } else {
        scroll
    }
}

/// The id of the filter field.
fn filter_id() -> Id {
    Id::unique("repositories-filter")
}

/// Takes the keyboard from the screen's filter while another screen has the page (the forks of a repository): it
/// neither keeps the focus there nor gets it back when the screen returns.
pub(crate) fn leave(ctx: &egui::Context, screen: &mut Screen) {
    ctx.memory_mut(|memory| memory.surrender_focus(filter_id()));
    screen.filter_focused = false;
    screen.refocus = false;
}

/// The id of the top row's main button.
fn main_id() -> Id {
    Id::unique("repositories-add")
}

/// Draws the screen in `layout`: the toolbar of `plan` (from [`plan`] this frame), then the table or the sentence
/// that stands for it on the panel, with `sheet` its frosted sheet. The keys and presses change `session`'s
/// repositories through their methods; `screen` keeps the scroll and the focus between frames.
///
/// Without `keys` (an overlay is open over the screen: `overlay::is_open`) the screen handles no key and acts on no
/// press, and its filter neither keeps nor takes the keyboard's focus; the overlay's layer keeps the pointer's
/// hover and the wheel from it.
#[expect(clippy::too_many_arguments, reason = "the screen's one entry: what it draws, on what, and whether it has the keyboard")]
pub fn show(
    ui: &mut Ui,
    layout: &Layout,
    plan: Option<&Plan>,
    session: &mut Session,
    sheet: Option<&Sheet>,
    mode: Mode,
    screen: &mut Screen,
    keys: bool,
) {
    let palette = theme::palette(mode);
    if !keys {
        // egui's own Tab would give the focus to the filter, the one control here that takes it, as the field is
        // drawn: in the frame an overlay opens, before its layer is modal, nothing else stops that.
        ui.memory_mut(|memory| {
            memory.surrender_focus(filter_id());
            memory.move_focus(FocusDirection::None);
        });
        // The filter had the focus when the overlay came: it gets it back when the overlay goes.
        screen.refocus |= screen.filter_focused;
        screen.filter_focused = false;
    } else if std::mem::take(&mut screen.refocus) {
        // Back before the field is drawn: what is typed in this frame (the rest of a word) is the filter's, not
        // the screen's keys.
        ui.memory_mut(|memory| memory.request_focus(filter_id()));
        screen.filter_focused = true;
    }
    // The frame's keys are the filter's when it held the focus at the end of the last frame; egui takes the focus
    // away on Esc before the frame sees the key, so the focus now would not say it.
    let typing = screen.filter_focused;
    // The rest of a double click whose first click closed an overlay is no press here (`overlay::settle` took the
    // repeats of a key held since from the input already).
    let presses = keys && !overlay::stale_click(ui.ctx());
    let mut moved = false;
    if let (Some(plan), Some(repos)) = (plan, session.repos_mut()) {
        if keys && ui.memory(|memory| memory.has_focus(filter_id())) {
            // While the filter has the focus, the up and down keys move the highlight, as in the terminal app's
            // filter; taken before the field sees them.
            for (key, rows) in [(Key::ArrowUp, -1), (Key::ArrowDown, 1), (Key::PageUp, -PAGE_ROWS), (Key::PageDown, PAGE_ROWS)] {
                let presses = ui.input_mut(|input| input.count_and_consume_key(Modifiers::NONE, key));
                for _ in 0..presses {
                    repos.move_by(rows);
                    moved = true;
                }
            }
        }
        controls(ui, plan, repos, palette, presses);
    }

    let events = if keys { ui.input(|input| input.events.clone()) } else { Vec::new() };
    if typing {
        // The text was the field's; an Esc that took its focus away clears it.
        let escaped = events.iter().any(|event| matches!(event, Event::Key { key: Key::Escape, pressed: true, repeat: false, .. }));
        if escaped && let Some(repos) = session.repos_mut() {
            repos.set_list_filter("");
        }
    } else {
        for command in commands(&events) {
            moved |= apply(command, session, ui);
            // A key that opened a dialog or the menu is the last the screen hears in this frame: what comes after it
            // is the overlay's, from the next frame on.
            if session.repos().is_some_and(|repos| repos.mode != CoreMode::List) {
                break;
            }
        }
    }
    // A hidden run says how it goes in the status bar; a click there shows its dialog again, as `l` does.
    if presses && let Some(repos) = session.repos_mut().filter(|repos| repos.job.is_some()) {
        let response = ui.interact(layout.status, Id::unique("status-bar"), Sense::CLICK).on_hover_cursor(CursorIcon::PointingHand);
        if response.clicked() {
            repos.show_job();
        }
    }
    if !keys {
        // Nor a press on the field in the frame an overlay opens, before its shield is in place.
        ui.memory_mut(|memory| memory.surrender_focus(filter_id()));
    }
    screen.filter_focused = ui.memory(|memory| memory.has_focus(filter_id()));
    if moved {
        ui.ctx().request_repaint();
    }

    if let Some((title, lines)) = state_text(session) {
        if let Some(sheet) = sheet {
            frost::paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size()), layout.panel, RADIUS_PANEL, sheet);
        }
        draw_state(ui, layout.panel, &title, &lines, palette);
        // No table, no row to stand under.
        if let Some(repos) = session.repos_mut().filter(|repos| repos.mode == CoreMode::Menu) {
            repos.close_menu();
        }
        screen.menu = None;
        return;
    }
    let Some(repos) = session.repos_mut() else { return };
    let Some(sheet) = sheet else {
        // No table is drawn without its sheet, so a menu (Enter's) would have no row to stand under.
        if repos.mode == CoreMode::Menu {
            repos.close_menu();
        }
        screen.menu = None;
        return;
    };
    let lines = repos_view::lines(repos);
    if moved {
        let heights: Vec<f32> = lines.iter().map(table::height).collect();
        if let Some(index) = lines.iter().position(is_chosen) {
            screen.scroll = follow(&heights, index, screen.scroll, table::body_rect(layout.panel).height());
        }
    }
    let open = screen.menu.as_ref().filter(|_| repos.mode == CoreMode::Menu).map(|(key, _)| key.as_str());
    let did = table::show(ui, layout.panel, &lines, sheet, palette, mode, &mut screen.scroll, open);
    match if presses { did } else { Did::None } {
        Did::Menu { key, anchor } => {
            open_menu(repos, &key);
            screen.menu = Some((key, anchor));
        }
        Did::Picked(key) => repos.pick(&key),
        Did::Ticked(key) => repos.toggle_mark_of(&key),
        Did::Folded(key) => {
            // As in the terminal app: the header is highlighted, then folded, so the highlight never stays on a row
            // the fold hides.
            if let Some(group) = key.strip_prefix(GROUP_KEY) {
                repos.pick(&key);
                repos.toggle_group(group);
            }
        }
        Did::TickedAll => repos.mark_all_visible(),
        Did::None => {}
    }
    keep_menu(repos, layout.panel, screen);
}

/// Draws the controls of `plan` (the top row's "Import", "Update all" and "Add repository", and, with repositories,
/// the filter, the sort and the selection bar) and, with `keys`, does what a press on them asks of `repos`. A double
/// click is one press: its second click does nothing (as a held key acts once), so "Open folder" opens each folder
/// once and "Update all" queues one run.
fn controls(ui: &mut Ui, plan: &Plan, repos: &mut Repos, palette: theme::Palette, keys: bool) {
    let second =
        ui.input(|input| input.pointer.button_double_clicked(PointerButton::Primary) || input.pointer.button_triple_clicked(PointerButton::Primary));
    let keys = keys && !second;
    if let Some(top) = plan.top {
        match widgets::capsule(ui, top.actions, &top_items(top.fit), palette) {
            Some(TOP_IMPORT) if keys => repos.run(Action::Import),
            Some(TOP_UPDATE_ALL) if keys => repos.update_all(),
            _ => {}
        }
        if widgets::main_action(ui, top.main, Icon::Plus, ADD_REPOSITORY, top.fit == Fit::Bare, palette, main_id()) && keys {
            repos.run(Action::Add);
        }
    }
    let Some(toolbar) = &plan.toolbar else { return };
    let mut text = repos.list_filter.clone();
    widgets::field(ui, toolbar.field, &mut text, FILTER_HINT, Icon::Search, palette, filter_id());
    repos.set_list_filter(&text);
    if widgets::capsule(ui, toolbar.view, &view_items(toolbar.sort), palette).is_some() && keys {
        repos.toggle_sort();
    }
    let ticked = toolbar.selection.map(|(rect, count, fit)| (rect, count.to_string(), selected(count), fit));
    if let Some((rect, number, words, fit)) = &ticked
        && let Some(pressed) = widgets::capsule(ui, *rect, &selection_items(number, words, *fit), palette)
        && keys
    {
        match pressed {
            BAR_UPDATE => repos.update_chosen(),
            BAR_GROUP => repos.open_group_prompt(),
            BAR_OPEN => repos.open_selected(),
            BAR_REMOVE => remove(repos),
            BAR_CLEAR => repos.clear_marks(),
            _ => {}
        }
    }
}

/// Keeps the open menu under its row's button: the row it was opened on (the highlighted one when the screen did
/// not open it: Enter, `--show menu`), placed again each frame. When that row is no longer shown (filtered out,
/// removed, the list read again without it) the menu closes. No menu, no row.
fn keep_menu(repos: &mut Repos, panel: Rect, screen: &mut Screen) {
    if repos.mode != CoreMode::Menu {
        screen.menu = None;
        return;
    }
    let key = screen.menu.take().map(|(key, _)| key).or_else(|| repos.selected.clone());
    let lines = repos_view::lines(repos);
    screen.menu = key.and_then(|key| table::dots_rect(panel, &lines, screen.scroll, &key).map(|anchor| (key, anchor)));
    if screen.menu.is_none() {
        repos.close_menu();
    }
}

/// Does what `command` asks of `session`; says whether it changed what is shown.
fn apply(command: Command, session: &mut Session, ui: &Ui) -> bool {
    if command == Command::Refresh {
        session.refresh();
        return false;
    }
    let Some(repos) = session.repos_mut() else { return false };
    match command {
        Command::Move(rows) => repos.move_by(rows),
        Command::End(start) => repos.move_to_end(start),
        Command::Tick => repos.toggle_list_mark(),
        Command::TickAll => repos.mark_all_visible(),
        // The panel then says that Esc clears the filter, and the ticks are out of sight.
        Command::Escape if filter_hides_everything(repos) => repos.set_list_filter(""),
        // Work without a dialog (a deletion from disk) has no Cancel button: Esc is how it stops.
        Command::Escape if esc_cancels(repos) => repos.cancel_work(),
        Command::Escape => repos.clear_marks(),
        Command::Sort => repos.toggle_sort(),
        Command::Filter => {
            if has_repositories(repos) {
                ui.memory_mut(|memory| memory.request_focus(filter_id()));
            }
            return false;
        }
        Command::Type(typed) => {
            let text = format!("{}{typed}", repos.list_filter);
            repos.set_list_filter(&text);
        }
        Command::Fold(fold) => repos.fold_selected(fold),
        Command::ShowRun => {
            repos.show_job();
            return false;
        }
        // On a repository's row, what its "…" does; on a group's header, what the core does (fold, or the menu for
        // what is ticked).
        Command::Enter => match repos.selected.clone().filter(|key| !key.starts_with(GROUP_KEY)) {
            Some(key) => open_menu(repos, &key),
            None => repos.enter_row(),
        },
        Command::UpdateChosen => repos.update_chosen(),
        Command::UpdateAll => repos.update_all(),
        Command::Watch => repos.toggle_watch(),
        Command::Open => repos.open_selected(),
        Command::Group => repos.open_group_prompt(),
        Command::Remove => remove(repos),
        Command::Add => repos.run(Action::Add),
        Command::Import => repos.run(Action::Import),
        Command::Refresh => {}
    }
    true
}

/// Asks before removing the ticked rows, or the highlighted one (`Repos::begin_remove`). On a group's header with
/// nothing ticked the core would dissolve the group without asking; the window asks for repositories instead.
fn remove(repos: &mut Repos) {
    if repos.marked.is_empty() && repos.selected_group().is_some() {
        repos.ask_for_repositories_to_remove();
    } else {
        repos.begin_remove();
    }
}

/// Whether Esc on the list stops what the core is busy with: only work that has no dialog at all (a deletion from
/// disk, the watch's pull). A run with a progress dialog, shown or hidden, is stopped by its Cancel button; `l`
/// brings a hidden one back.
fn esc_cancels(repos: &Repos) -> bool {
    repos.busy && repos.job.as_ref().is_none_or(|job| !job.running())
}

/// Whether a filter is set and lets no row through.
fn filter_hides_everything(repos: &Repos) -> bool {
    !repos.list_filter.is_empty() && repos.rows().is_empty()
}

/// Whether there are repositories, and so a filter to type into.
fn has_repositories(repos: &Repos) -> bool {
    !repos.installed.is_empty()
}

/// Whether a line is the highlighted one.
fn is_chosen(line: &Line) -> bool {
    match line {
        Line::Group(group) => group.chosen,
        Line::Repo(repo) => repo.chosen,
    }
}

/// Draws `title` in medium and `lines` in the quiet colour under it, centred on the panel at `panel`, each cut to
/// the panel's width.
fn draw_state(ui: &Ui, panel: Rect, title: &str, lines: &[String], palette: theme::Palette) {
    let painter = ui.painter().with_clip_rect(panel);
    let room = panel.width() - 2.0 * PANEL_TEXT_INSET;
    let title_font = font(TEXT_TITLE, assets::medium_family());
    let line_font = font(TEXT, FontFamily::Proportional);
    let title = painter.layout_no_wrap(widgets::ellipsis(title, &title_font, room, ui), title_font, palette.on_glass);
    let lines: Vec<_> =
        lines.iter().map(|line| painter.layout_no_wrap(widgets::ellipsis(line, &line_font, room, ui), line_font.clone(), palette.quiet)).collect();
    let height = title.size().y + STATE_GAP + lines.iter().map(|line| line.size().y).sum::<f32>();
    let mut top = panel.center().y - height / 2.0;
    let title_height = title.size().y;
    painter.galley(pos2(panel.center().x - title.size().x / 2.0, top), title, palette.on_glass);
    top += title_height + STATE_GAP;
    for line in lines {
        let line_height = line.size().y;
        painter.galley(pos2(panel.center().x - line.size().x / 2.0, top), line, palette.quiet);
        top += line_height;
    }
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::testing::test_settings;
    use egui::{Context, RawInput, Stroke, TextureId, vec2};

    use super::*;
    use crate::theme::GROUP_HEIGHT;
    use crate::widgets::tests::context;

    /// The design's window.
    const WINDOW: [f32; 2] = [1440.0, 900.0];

    fn sheet() -> Sheet {
        Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
    }

    fn key(key: Key, modifiers: Modifiers) -> Event {
        Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    /// A key pressed without modifiers.
    fn press(code: Key) -> Event {
        key(code, Modifiers::NONE)
    }

    /// A key that types `text`, as the window gets it: the key, then the text.
    fn typed(code: Key, text: &str) -> Vec<Event> {
        vec![press(code), Event::Text(text.to_owned())]
    }

    /// Runs one frame of the screen at the design's size with `events`.
    fn frame(ctx: &Context, session: &mut Session, screen: &mut Screen, events: Vec<Event>) {
        frame_with(ctx, session, screen, events, true);
    }

    /// Runs one frame of the screen at the design's size with `events`, with the keyboard when `keys`.
    fn frame_with(ctx: &Context, session: &mut Session, screen: &mut Screen, events: Vec<Event>, keys: bool) {
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), events, ..Default::default() };
        let output = ctx.run_ui(input, |ui| {
            let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, has_toolbar(session));
            let plan = plan(ui.painter(), &layout, session);
            show(ui, &layout, plan.as_ref(), session, Some(&sheet()), Mode::Light, screen, keys);
        });
        output.drop_without_applying_deltas();
    }

    #[test]
    fn without_the_keyboard_the_screen_hears_no_key_and_its_filter_takes_no_focus() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        let rows = keys(&session);
        session.repos_mut().unwrap().pick(&rows[0]);
        let before = (repos(&session).selected.clone(), ticked(&session), repos(&session).list_filter.clone());
        let all = [
            vec![press(Key::ArrowDown), press(Key::End), key(Key::A, Modifiers::COMMAND), press(Key::Escape)],
            typed(Key::Space, " "),
            typed(Key::S, "s"),
            typed(Key::Slash, "/"),
            typed(Key::P, "p"),
            vec![press(Key::Tab)],
            vec![press(Key::ArrowLeft)],
        ];
        for events in all {
            frame_with(&ctx, &mut session, &mut screen, events, false);
            frame_with(&ctx, &mut session, &mut screen, Vec::new(), false);
            assert_eq!(focused(&ctx), None, "the filter took the focus");
        }
        assert_eq!((repos(&session).selected.clone(), ticked(&session), repos(&session).list_filter.clone()), before);
        assert!(repos(&session).sort == SortMode::Name);
        assert_eq!(repos(&session).collapsed, Vec::<String>::new());

        // A filter that had the focus loses it when the keyboard goes, and gets no text.
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        assert_eq!(focused(&ctx), Some(filter_id()));
        frame_with(&ctx, &mut session, &mut screen, vec![Event::Text("po".to_owned())], false);
        assert_eq!(focused(&ctx), None);
        assert_eq!(repos(&session).list_filter, "");
    }

    fn repos(session: &Session) -> &Repos {
        session.repos().unwrap()
    }

    /// The keys of the rows the table shows, in its order.
    fn keys(session: &Session) -> Vec<String> {
        repos_view::lines(repos(session))
            .into_iter()
            .map(|line| match line {
                Line::Group(group) => group.key,
                Line::Repo(repo) => repo.key,
            })
            .collect()
    }

    /// The repositories the table shows ticked.
    fn ticked(session: &Session) -> Vec<String> {
        repos_view::lines(repos(session))
            .into_iter()
            .filter_map(|line| match line {
                Line::Repo(repo) if repo.ticked => Some(repo.key),
                _ => None,
            })
            .collect()
    }

    fn shown_repos(session: &Session) -> usize {
        repos_view::lines(repos(session)).iter().filter(|line| matches!(line, Line::Repo(_))).count()
    }

    #[test]
    fn keys_do_what_the_terminal_app_does() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        let rows = keys(&session);
        session.repos_mut().unwrap().pick(&rows[0]);

        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowDown), press(Key::ArrowDown)]);
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[2].as_str()), "two rows down from the first");
        let highlighted = rows[2].clone();
        assert!(!ticked(&session).contains(&highlighted));

        frame(&ctx, &mut session, &mut screen, typed(Key::Space, " "));
        assert!(ticked(&session).contains(&highlighted), "Space ticks the highlighted row");
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[3].as_str()), "and moves the highlight one lower");

        frame(&ctx, &mut session, &mut screen, vec![key(Key::A, Modifiers::COMMAND)]);
        assert_eq!(ticked(&session).len(), shown_repos(&session), "Ctrl-A ticks every row shown");
        assert_eq!(shown_repos(&session), 12);

        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        assert_eq!(ticked(&session), Vec::<String>::new(), "Esc clears the ticks");

        assert!(repos(&session).sort == SortMode::Name);
        frame(&ctx, &mut session, &mut screen, typed(Key::S, "s"));
        assert!(repos(&session).sort == SortMode::NeedsUpdate, "s changes the sort");

        frame(&ctx, &mut session, &mut screen, vec![press(Key::End)]);
        assert_eq!(repos(&session).selected.as_deref(), keys(&session).last().map(String::as_str), "End goes to the last row");
        frame(&ctx, &mut session, &mut screen, typed(Key::K, "k"));
        let rows = keys(&session);
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[rows.len() - 2].as_str()), "k goes one row up");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Home)]);
        assert_eq!(repos(&session).selected.as_deref(), keys(&session).first().map(String::as_str), "Home goes to the first row");

        session.repos_mut().unwrap().toggle_mark_of("api");
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        assert_eq!(repos(&session).list_filter, "", "the slash itself is not typed into the filter");
        frame(&ctx, &mut session, &mut screen, vec![Event::Text("po".to_owned())]);
        assert_eq!(repos(&session).list_filter, "po");
        // While the filter has the focus, letters are words, not keys: "s" does not change the sort back.
        frame(&ctx, &mut session, &mut screen, typed(Key::S, "s"));
        assert_eq!(repos(&session).list_filter, "pos");
        assert!(repos(&session).sort == SortMode::NeedsUpdate);
        // Esc in the filter clears it and leaves the ticks alone. (The Esc pressed before is let go first: a second
        // press of a key egui holds down is a repeat, which does nothing.)
        let up = Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        frame(&ctx, &mut session, &mut screen, vec![up, press(Key::Escape)]);
        assert_eq!(repos(&session).list_filter, "");
        assert_eq!(ticked(&session), ["api"]);
    }

    #[test]
    fn the_arrows_fold_and_unfold_the_highlighted_group() {
        let ctx = context();
        // Folding saves through the core, which writes nothing for the held demo.
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        session.repos_mut().unwrap().pick("group::home");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowLeft)]);
        assert_eq!(repos(&session).collapsed, ["home"]);
        assert!(!keys(&session).contains(&"dotfiles".to_owned()));
        // The save kept the watch on homelab.
        assert_eq!(repos(&session).watched_count(), 1);
        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowRight)]);
        assert_eq!(repos(&session).collapsed, Vec::<String>::new());
        assert_eq!(repos(&session).watched_count(), 1);
        assert!(!repos(&session).config_path.exists());
    }

    #[test]
    fn a_click_on_a_groups_header_highlights_it_then_folds_it() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        session.repos_mut().unwrap().pick("dotfiles");
        // The demo starts with the header of home, right under the table's head.
        let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true);
        let body = table::body_rect(layout.panel);
        let at = pos2(body.center().x, body.top() + GROUP_HEIGHT / 2.0);
        let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for events in [Vec::new(), vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)]] {
            frame(&ctx, &mut session, &mut screen, events);
        }
        assert_eq!(repos(&session).collapsed, ["home"]);
        assert_eq!(repos(&session).selected.as_deref(), Some("group::home"), "the highlight is on the header, not on a hidden row");
    }

    #[test]
    fn the_keys_are_read_as_the_terminal_app_reads_them() {
        let read = |events: Vec<Event>| commands(&events);
        assert_eq!(
            read(vec![press(Key::ArrowUp), press(Key::PageDown), press(Key::PageUp)]),
            [Command::Move(-1), Command::Move(10), Command::Move(-10)]
        );
        assert_eq!(read(typed(Key::J, "j")), [Command::Move(1)]);
        assert_eq!(read(typed(Key::R, "r")), [Command::Refresh]);
        assert_eq!(read(vec![press(Key::F5)]), [Command::Refresh]);
        assert_eq!(read(typed(Key::Slash, "/")), [Command::Filter]);
        assert_eq!(read(typed(Key::S, "S")), [Command::Sort]);
        assert_eq!(read(typed(Key::L, "l")), [Command::ShowRun]);
        assert_eq!(read(typed(Key::L, "L")), [Command::ShowRun]);
        // Ctrl with A ticks all; a alone opens Add, as in the terminal app (a capital A is no key), i and I Import.
        assert_eq!(read(vec![key(Key::A, Modifiers::COMMAND)]), [Command::TickAll]);
        assert_eq!(read(typed(Key::A, "a")), [Command::Add]);
        assert_eq!(read(typed(Key::A, "A")), []);
        assert_eq!(read(typed(Key::I, "i")), [Command::Import]);
        assert_eq!(read(typed(Key::I, "I")), [Command::Import]);
        // The actions, with the terminal app's letters: u and U are two commands, d and Delete one.
        assert_eq!(read(typed(Key::U, "u")), [Command::UpdateChosen]);
        assert_eq!(read(typed(Key::U, "U")), [Command::UpdateAll]);
        for letter in ["w", "W"] {
            assert_eq!(read(typed(Key::W, letter)), [Command::Watch]);
        }
        for letter in ["o", "O"] {
            assert_eq!(read(typed(Key::O, letter)), [Command::Open]);
        }
        for letter in ["m", "M"] {
            assert_eq!(read(typed(Key::M, letter)), [Command::Group]);
        }
        assert_eq!(read(typed(Key::D, "d")), [Command::Remove]);
        assert_eq!(read(vec![press(Key::Delete)]), [Command::Remove]);
        assert_eq!(read(held(Key::Escape, None, 3)), [Command::Escape], "a held Esc acts once");
        // As in the terminal app, a capital D is no key.
        assert_eq!(read(typed(Key::D, "D")), []);
        // With Ctrl, Alt or Cmd held, a letter is no key here, even where the system sends its text.
        for held in [Modifiers::CTRL, Modifiers::ALT, Modifiers::COMMAND, Modifiers::MAC_CMD] {
            for (code, letter) in [(Key::U, "u"), (Key::D, "d"), (Key::W, "w"), (Key::O, "o"), (Key::M, "m"), (Key::U, "U"), (Key::I, "i")] {
                assert_eq!(read(vec![key(code, held), Event::Text(letter.to_owned())]), [], "{held:?} {letter}");
            }
            // With A, Ctrl (Cmd) ticks all; the text it may send opens nothing.
            let tick_all: &[Command] = if held.command_only() { &[Command::TickAll] } else { &[] };
            assert_eq!(read(vec![key(Key::A, held), Event::Text("a".to_owned())]), tick_all, "{held:?} a");
            assert_eq!(read(vec![key(Key::Delete, held)]), [], "{held:?} Delete");
        }
        // Shift is how a capital is typed: it does not stop the letter.
        assert_eq!(read(vec![key(Key::U, Modifiers::SHIFT), Event::Text("U".to_owned())]), [Command::UpdateAll]);
        // A released key is nothing.
        let released = Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        assert_eq!(read(vec![released]), []);
    }

    /// A held demo, laid out, after `events` in one frame and an empty one.
    fn demo_after(events: Vec<Event>) -> Session {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        frame(&ctx, &mut session, &mut screen, events);
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session
    }

    /// A live session without repositories (it runs nothing: there is nothing to run on), after `events`.
    fn empty_after(events: Vec<Event>) -> Session {
        let ctx = context();
        let mut session = Session::Live(Box::new(Repos::offline(test_settings())));
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, events);
        session
    }

    fn removing(session: &Session) -> Option<Vec<String>> {
        match &repos(session).mode {
            CoreMode::ConfirmRemove { folders } => Some(folders.clone()),
            _ => None,
        }
    }

    #[test]
    fn each_key_calls_what_the_terminal_app_calls() {
        use devassist_core::repos::state::HELD;
        // The demo ticks api and web and holds its work: what would run says so, and nothing changes.
        for letter in ["u", "U", "w", "o"] {
            let session = demo_after(typed(Key::U, letter));
            assert_eq!(repos(&session).status, HELD, "{letter}");
            assert_eq!(ticked(&session), ["api", "web"], "{letter}: the ticks stay");
            assert!(repos(&session).mode == CoreMode::List, "{letter}");
            assert_eq!(repos(&session).watched_count(), 1, "{letter}: the watch is as it was");
        }
        // Without repositories, u and U each say what the core says for them.
        assert_eq!(repos(&empty_after(typed(Key::U, "u"))).status, "Select a repository first.");
        assert_eq!(repos(&empty_after(typed(Key::U, "U"))).status, "There are no repositories to update.");
        // m asks for the group of the ticked ones; d and Delete ask before removing them; Enter opens the menu.
        assert!(repos(&demo_after(typed(Key::M, "m"))).mode == CoreMode::Group);
        let ticks = Some(vec!["api".to_owned(), "web".to_owned()]);
        assert_eq!(removing(&demo_after(typed(Key::D, "d"))), ticks);
        assert_eq!(removing(&demo_after(vec![press(Key::Delete)])), ticks);
        // Enter on a row (the demo starts on a group's header, which has no "…" for the menu to stand under).
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().pick("api");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Enter)]);
        assert!(repos(&session).mode == CoreMode::Menu);
        // Not with Ctrl, Alt or Cmd held.
        for held in [Modifiers::CTRL, Modifiers::ALT, Modifiers::COMMAND] {
            for (code, letter) in [(Key::D, "d"), (Key::M, "m"), (Key::U, "u"), (Key::W, "w"), (Key::O, "o")] {
                let session = demo_after(vec![key(code, held), Event::Text(letter.to_owned())]);
                assert!(repos(&session).mode == CoreMode::List, "{held:?} {letter}");
                assert_eq!(repos(&session).status, crate::demo::repos().status, "{held:?} {letter}");
            }
            assert_eq!(removing(&demo_after(vec![key(Key::Delete, held)])), None);
        }
        // Not while the filter has the keyboard: there they are words.
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        for (code, letter) in [(Key::D, "d"), (Key::U, "u"), (Key::M, "m"), (Key::W, "w"), (Key::O, "o")] {
            frame(&ctx, &mut session, &mut screen, typed(code, letter));
        }
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Delete)]);
        assert_eq!(repos(&session).list_filter, "dumwo", "and Delete at the end of the field deletes nothing");
        assert!(repos(&session).mode == CoreMode::List);
        assert_eq!(repos(&session).status, crate::demo::repos().status);
    }

    #[test]
    fn a_and_i_open_their_dialogs_once() {
        use crate::dialogs::Which;
        use crate::dialogs::testing::Window;
        // a opens Add, i Import, through the core; a held key opens it once, and types nothing into it.
        for (code, letter, which, field) in [(Key::A, "a", Which::Add, "search"), (Key::I, "i", Which::Folder, "folder")] {
            let mut window = Window::new(Session::demo(crate::demo::repos()));
            window.frame(Vec::new());
            window.frame(held(code, Some(letter), 0));
            for _ in 0..4 {
                window.frame(vec![
                    Event::Key { key: code, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE },
                    Event::Text(letter.to_owned()),
                ]);
            }
            window.frame(vec![Event::Key { key: code, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
            window.frame(Vec::new());
            assert!(window.shows(which), "{letter} did not open its dialog");
            let typed = match which {
                Which::Add => window.repos().add_filter.clone(),
                _ => window.repos().prompt.as_ref().unwrap().input.clone(),
            };
            assert!(!typed.contains(letter), "{letter} was typed into the {field} field: {typed:?}");
            // Its Esc closes it; the next press opens it again.
            window.key(Key::Escape);
            assert!(!window.open());
            window.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
            window.typed(code, letter);
            assert!(window.shows(which), "{letter} did not open its dialog again");
        }
        // Not with a modifier, not while the filter has the keys, not on a page other than this screen's.
        for held in [Modifiers::CTRL, Modifiers::ALT, Modifiers::COMMAND] {
            for (code, letter) in [(Key::A, "a"), (Key::I, "i")] {
                let session = demo_after(vec![key(code, held), Event::Text(letter.to_owned())]);
                assert!(repos(&session).mode == CoreMode::List, "{held:?} {letter}");
            }
        }
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        frame(&ctx, &mut session, &mut screen, typed(Key::A, "a"));
        frame(&ctx, &mut session, &mut screen, typed(Key::I, "i"));
        assert_eq!(repos(&session).list_filter, "ai");
        assert!(repos(&session).mode == CoreMode::List);
        // Without the keyboard (an overlay is open over the screen) they do nothing.
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame_with(&ctx, &mut session, &mut screen, typed(Key::A, "a"), false);
        frame_with(&ctx, &mut session, &mut screen, typed(Key::I, "i"), false);
        assert!(repos(&session).mode == CoreMode::List);
    }

    /// `code` held: pressed, then repeated `repeats` times, each with the text it types.
    fn held(code: Key, text: Option<&str>, repeats: usize) -> Vec<Event> {
        (0..=repeats)
            .flat_map(|index| {
                let pressed = Event::Key { key: code, physical_key: None, pressed: true, repeat: index > 0, modifiers: Modifiers::NONE };
                std::iter::once(pressed).chain(text.map(|text| Event::Text(text.to_owned())))
            })
            .collect()
    }

    #[test]
    fn a_held_action_key_acts_once_and_a_held_arrow_keeps_moving() {
        let read = |events: Vec<Event>| commands(&events);
        let actions = [
            (Key::U, "u", Command::UpdateChosen),
            (Key::U, "U", Command::UpdateAll),
            (Key::W, "w", Command::Watch),
            (Key::O, "o", Command::Open),
            (Key::M, "m", Command::Group),
            (Key::D, "d", Command::Remove),
            (Key::A, "a", Command::Add),
            (Key::I, "i", Command::Import),
        ];
        for (code, letter, command) in actions {
            assert_eq!(read(held(code, Some(letter), 3)), [command], "{letter}");
        }
        assert_eq!(read(held(Key::Delete, None, 3)), [Command::Remove]);
        assert_eq!(read(held(Key::J, Some("j"), 2)), [Command::Move(1); 3]);
        assert_eq!(read(held(Key::ArrowDown, None, 2)), [Command::Move(1); 3]);

        // While a fetch runs, a held u queues one update, not one a repeat.
        let mut busy = devassist_core::repos::state::testing::grouped_app();
        busy.busy = true;
        let ctx = context();
        let mut session = Session::Live(Box::new(busy));
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().pick("api");
        frame(&ctx, &mut session, &mut screen, held(Key::U, Some("u"), 4));
        assert_eq!(repos(&session).queue.len(), 1);
    }

    #[test]
    fn a_held_enter_folds_a_header_once() {
        assert_eq!(commands(&held(Key::Enter, None, 3)), [Command::Enter]);
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().clear_marks();
        session.repos_mut().unwrap().pick("group::home");
        frame(&ctx, &mut session, &mut screen, held(Key::Enter, None, 4));
        assert_eq!(repos(&session).collapsed, ["home"], "folded once, not folded and unfolded at the key's repeat rate");
    }

    #[test]
    fn enter_on_a_row_does_what_its_dots_do() {
        // The highlight on notes, which is not ticked: the ticks of api and web go, the menu is for notes alone.
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().pick("notes");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Enter)]);
        assert!(repos(&session).mode == CoreMode::Menu);
        assert_eq!(ticked(&session), Vec::<String>::new(), "the other rows' ticks are cleared");
        assert_eq!(repos(&session).chosen(), ["notes"], "the menu acts on the row it hangs on");
        // The highlight on web, which is ticked: the menu is for both ticked rows, and the ticks stay. (A fresh
        // context: egui reads a second press of a key it holds down as a repeat.)
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().pick("web");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Enter)]);
        assert!(repos(&session).mode == CoreMode::Menu);
        assert_eq!(
            (ticked(&session), repos(&session).chosen()),
            (vec!["api".to_owned(), "web".to_owned()], vec!["api".to_owned(), "web".to_owned()])
        );
    }

    #[test]
    fn esc_cancels_only_work_that_has_no_dialog() {
        use devassist_core::repos::state::testing::grouped_app;
        let mut repos = grouped_app();
        assert!(!esc_cancels(&repos), "nothing runs");
        repos.busy = true;
        assert!(esc_cancels(&repos), "a deletion from disk: busy, and no run with a dialog");
        repos.job = Some(crate::dialogs::testing::run(&["api"], 1));
        for hidden in [false, true] {
            if hidden {
                repos.hide_job();
            } else {
                repos.show_job();
            }
            assert!(!esc_cancels(&repos), "a run with its dialog, hidden {hidden}: its Cancel button stops it");
        }
        repos.job.as_mut().unwrap().finish(Ok("Updated api.".to_owned()), false);
        assert!(esc_cancels(&repos), "a run that ended is no run: the busy work after it has no dialog");
        repos.busy = false;
        repos.fetching = true;
        assert!(!esc_cancels(&repos), "a fetch is not the work Esc stops");
    }

    #[test]
    fn esc_while_a_deletion_runs_cancels_it_and_says_so() {
        // The cancel flag is the whole process's: the tests that run git take turns with this one.
        let _live = crate::session::tests::live_git();
        let mut busy = devassist_core::repos::state::testing::grouped_app();
        busy.busy = true;
        busy.marked = vec!["api".to_owned()];
        let ctx = context();
        let mut session = Session::Live(Box::new(busy));
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        let cancelled = devassist_core::sys::process::was_cancelled();
        devassist_core::sys::process::clear_cancel();
        assert!(cancelled, "the core was asked to cancel");
        assert_eq!(repos(&session).status, "Cancelling…", "the core's words");
        assert_eq!(repos(&session).marked, ["api"], "Esc cancelled, it did not also clear the ticks");
        // A filter that lets nothing through is cleared first, as before.
        session.repos_mut().unwrap().set_list_filter("zzz");
        let up = Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        frame(&ctx, &mut session, &mut screen, vec![up, press(Key::Escape)]);
        let cancelled = devassist_core::sys::process::was_cancelled();
        devassist_core::sys::process::clear_cancel();
        assert_eq!((repos(&session).list_filter.as_str(), cancelled), ("", false));
    }

    #[test]
    fn the_filter_gets_its_focus_back_after_an_overlay_and_the_rest_of_the_word_is_typed() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        frame(&ctx, &mut session, &mut screen, typed(Key::P, "p"));
        assert_eq!(focused(&ctx), Some(filter_id()));
        // A dialog opens by itself and goes: the screen has no keyboard for a while.
        for _ in 0..3 {
            frame_with(&ctx, &mut session, &mut screen, Vec::new(), false);
        }
        assert_eq!(focused(&ctx), None);
        // The rest of the word comes in the first frame the keyboard is back: it is typed, not run.
        frame(&ctx, &mut session, &mut screen, [typed(Key::O, "o"), typed(Key::W, "w")].concat());
        assert_eq!(repos(&session).list_filter, "pow");
        assert_eq!(repos(&session).status, crate::demo::repos().status, "o and w were run as keys");
        assert_eq!(focused(&ctx), Some(filter_id()));
        // A filter that did not have the focus does not get it.
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        assert_eq!(focused(&ctx), None);
        frame_with(&ctx, &mut session, &mut screen, Vec::new(), false);
        frame(&ctx, &mut session, &mut screen, Vec::new());
        assert_eq!(focused(&ctx), None);
    }

    #[test]
    fn without_a_frosted_sheet_no_menu_is_left_open_unseen() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        session.repos_mut().unwrap().pick("api");
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))),
            events: vec![press(Key::Enter)],
            ..Default::default()
        };
        ctx.run_ui(input, |ui| {
            let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, has_toolbar(&session));
            let plan = plan(ui.painter(), &layout, &session);
            show(ui, &layout, plan.as_ref(), &mut session, None, Mode::Light, &mut screen, true);
        })
        .drop_without_applying_deltas();
        assert!(repos(&session).mode == CoreMode::List, "a menu with no table to stand on stayed open");
    }

    #[test]
    fn no_key_acts_after_one_that_opened_a_dialog_in_the_same_frame() {
        let ticks = Some(vec!["api".to_owned(), "web".to_owned()]);
        assert_eq!(removing(&demo_after([typed(Key::D, "d"), vec![press(Key::Enter)]].concat())), ticks);
        assert_eq!(removing(&demo_after([typed(Key::D, "d"), typed(Key::M, "m")].concat())), ticks);
        assert!(repos(&demo_after([typed(Key::M, "m"), typed(Key::D, "d")].concat())).mode == CoreMode::Group);
    }

    #[test]
    fn d_on_a_header_asks_for_repositories_and_dissolves_nothing() {
        for remove in [typed(Key::D, "d"), vec![press(Key::Delete)]] {
            let ctx = context();
            let mut session = Session::demo(crate::demo::repos());
            let mut screen = Screen::default();
            frame(&ctx, &mut session, &mut screen, Vec::new());
            let repos_mut = session.repos_mut().unwrap();
            repos_mut.clear_marks();
            repos_mut.pick("group::home");
            let (groups, rows) = (repos(&session).groups(), keys(&session));
            frame(&ctx, &mut session, &mut screen, remove.clone());
            assert_eq!(repos(&session).status, "Pick repositories to remove.");
            assert!(repos(&session).status_error);
            assert!(repos(&session).mode == CoreMode::List);
            assert_eq!((repos(&session).groups(), keys(&session)), (groups, rows), "the group is still there");

            // With rows ticked, d on a header asks about the ticked rows, as the core does. (The key is let go
            // first: egui reads a second press of a key still held as a repeat, which acts no more.)
            let up = |key| Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
            frame(&ctx, &mut session, &mut screen, vec![up(Key::D), up(Key::Delete)]);
            session.repos_mut().unwrap().toggle_mark_of("api");
            frame(&ctx, &mut session, &mut screen, remove);
            assert_eq!(removing(&session), Some(vec!["api".to_owned()]));
        }
    }

    #[test]
    fn an_error_status_is_drawn_in_the_danger_colour() {
        // Nothing to remove: the core says so as an error.
        let session = empty_after(typed(Key::D, "d"));
        assert_eq!(status(&session).map(|status| (status.text, status.error)), Some(("Select a repository to remove.".to_owned(), true)));
        let demo = Session::demo(crate::demo::repos());
        for mode in [Mode::Light, Mode::Dark] {
            let palette = theme::palette(mode);
            assert_eq!(status_colour(&session, mode), Some(palette.danger), "{mode:?}");
            assert_eq!(status_colour(&demo, mode), Some(palette.quiet), "{mode:?}: a plain status is quiet");
        }
    }

    /// The colour the shell draws `session`'s status in, in `mode`.
    fn status_colour(session: &Session, mode: Mode) -> Option<egui::Color32> {
        let ctx = context();
        let shown_status = status(session)?;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true);
        let output = ctx.run_ui(input, |ui| {
            let (mut sidebar, mut page) = (true, shell::Page::Repositories);
            shell::draw(ui, &layout, &mut sidebar, &mut page, shell::Shown { mode, waiting: 0, status: Some(&shown_status), back: None });
        });
        let colour = output.shapes.iter().find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == shown_status.text => Some(text.fallback_color),
            _ => None,
        });
        output.drop_without_applying_deltas();
        colour
    }

    /// The toolbar's plan for `session` in a window of `size`, with the sidebar when `sidebar`, on `ctx`.
    fn planned(ctx: &Context, session: &Session, size: egui::Vec2, sidebar: bool) -> (shell::Layout, Plan) {
        let layout = shell::layout(size, sidebar, has_toolbar(session));
        let mut out = None;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)), ..Default::default() };
        ctx.run_ui(input, |ui| out = plan(ui.painter(), &layout, session)).drop_without_applying_deltas();
        (layout, out.unwrap())
    }

    #[test]
    fn the_selection_bar_drops_its_words_before_it_overlaps() {
        let ctx = context();
        let two = Session::demo(crate::demo::repos());
        let mut compact_seen = false;
        for count in [1, 2, 12, 99, 999, 9999] {
            for sort in [SortMode::Name, SortMode::NeedsUpdate] {
                let mut session = Session::demo(crate::demo::repos());
                // The count is what the bar measures; folders beyond the demo's stand for a long list.
                let marked = &mut session.repos_mut().unwrap().marked;
                marked.clear();
                marked.extend((0..count).map(|index| format!("repo-{index}")));
                if repos(&session).sort != sort {
                    session.repos_mut().unwrap().toggle_sort();
                }
                for width in (960..=2000).step_by(8) {
                    for sidebar in [true, false] {
                        #[expect(clippy::cast_precision_loss, reason = "window widths of a few thousand points")]
                        let size = vec2(width as f32, 900.0);
                        let (layout, planned) = planned(&ctx, &session, size, sidebar);
                        let planned = planned.toolbar.unwrap();
                        let (bar, _, fit) = planned.selection.unwrap();
                        let at = format!("{width} sidebar {sidebar} {:?} {fit:?}", repos(&session).marked.len());
                        assert!(layout.toolbar.contains_rect(bar) && layout.toolbar.contains_rect(planned.field), "{at}");
                        assert!(planned.field.right() + TOOLBAR_GAP <= planned.view.left() + 1e-3, "{at}");
                        assert!(planned.view.right() + TOOLBAR_GAP <= bar.left() + 1e-3, "{at}: {bar:?} over {:?}", planned.view);
                        assert!(planned.field.width() >= FIELD_MIN_WIDTH, "{at}");
                        // Never cut: the bar's contents fit its rect.
                        let content = widths(&ctx, &session)[fit as usize];
                        assert!(content <= bar.width() + 1e-3, "{at}: {content} in {bar:?}");
                        // The words go only when they would not fit beside the field at its full width.
                        let [full, compact, _] = widths(&ctx, &session);
                        let room = layout.toolbar.width() - FIELD_WIDTH - 2.0 * TOOLBAR_GAP - planned.view.width();
                        assert_eq!(fit == Fit::Full, full <= room, "{at}");
                        // And the count its word only when the icons alone would not fit there either.
                        assert_eq!(fit == Fit::Bare, compact > room, "{at}");
                        compact_seen |= fit == Fit::Compact;
                    }
                }
            }
        }
        assert!(compact_seen, "the narrow windows drop the words");
        // The design's window shows every word, the drawing's order.
        let (_, planned) = planned(&ctx, &two, vec2(WINDOW[0], WINDOW[1]), true);
        assert_eq!(planned.toolbar.unwrap().selection.map(|(_, count, fit)| (count, fit)), Some((2, Fit::Full)));
        let names: Vec<&str> = selection_items("2", "2 selected", Fit::Full)
            .iter()
            .filter_map(|item| match item {
                Item::Action { words, .. } => Some(*words),
                _ => None,
            })
            .collect();
        assert_eq!(names, ["Update", "Move to group", "Open folder", "Remove", ""]);
        assert!(matches!(selection_items("2", "2 selected", Fit::Full)[BAR_REMOVE], Item::Action { danger: true, .. }));
    }

    /// The selection bar's width in each fit (full, compact, bare), for `session` on `ctx`.
    fn widths(ctx: &Context, session: &Session) -> [f32; 3] {
        let mut out = [0.0; 3];
        let count = repos(session).marked.len();
        let (number, words) = (count.to_string(), selected(count));
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| {
            out = [Fit::Full, Fit::Compact, Fit::Bare].map(|fit| widgets::capsule_width(ui.painter(), &selection_items(&number, &words, fit)));
        })
        .drop_without_applying_deltas();
        out
    }

    #[test]
    fn the_bar_is_never_narrower_than_what_it_holds() {
        // A bar that does not fit beside the field at its least width, even without its words.
        let narrow = Rect::from_min_size(pos2(280.0, 60.0), vec2(668.0, 36.0));
        // The count drops its word, and the field narrows for what is still missing.
        let (field, view, bar) = places(narrow, 241.0, Some([548.0, 260.0, 200.0]));
        assert_eq!(bar.map(|(bar, fit)| (bar.width(), fit)), Some((200.0, Fit::Bare)));
        assert_eq!(field.width(), 668.0 - 2.0 * TOOLBAR_GAP - 241.0 - 200.0);
        assert!(field.width() >= FIELD_MIN_WIDTH && field.right() + TOOLBAR_GAP <= view.left() + 1e-3);
        // Words that do not fit even beside the field at its least width: the field gives up the rest, the bar
        // is not cut and lies over nothing.
        let (field, view, bar) = places(narrow, 241.0, Some([548.0, 260.0, 230.0]));
        let (bar, fit) = bar.unwrap();
        assert_eq!((bar.width(), fit), (230.0, Fit::Bare), "the bar is cut: {bar:?}, field {field:?}, view {view:?}");
        assert!(field.width() < FIELD_MIN_WIDTH && view.right() + TOOLBAR_GAP <= bar.left() + 1e-3);
    }

    #[test]
    fn the_places_keep_the_bar_off_the_field_and_the_view() {
        let toolbar = Rect::from_min_size(pos2(280.0, 60.0), vec2(1148.0, 36.0));
        // Room for the words: the field keeps the design's width.
        let (field, view, bar) = places(toolbar, 252.0, Some([548.0, 260.0, 220.0]));
        assert_eq!(field.width(), FIELD_WIDTH);
        assert_eq!(bar, Some((Rect::from_min_size(pos2(880.0, 60.0), vec2(548.0, 36.0)), Fit::Full)));
        assert!(view.right() + TOOLBAR_GAP <= 880.0);
        // Room for the icons only.
        let (_, _, bar) = places(toolbar, 252.0, Some([610.0, 260.0, 220.0]));
        assert_eq!(bar, Some((Rect::from_min_size(pos2(1168.0, 60.0), vec2(260.0, 36.0)), Fit::Compact)));
        // Not for those: the count drops its word first, and the field keeps its width while that fits.
        let narrow = Rect::from_min_size(pos2(280.0, 60.0), vec2(668.0, 36.0));
        let (field, _, bar) = places(narrow, 188.0, Some([548.0, 260.0, 180.0]));
        assert_eq!((field.width(), bar.map(|(_, fit)| fit)), (FIELD_WIDTH, Some(Fit::Bare)));
        // Then the field gives up what it must, no less than its least width.
        let (field, view, bar) = places(narrow, 188.0, Some([548.0, 260.0, 230.0]));
        let (bar, fit) = bar.unwrap();
        assert_eq!(fit, Fit::Bare);
        assert_eq!(field.width(), 668.0 - 2.0 * TOOLBAR_GAP - 188.0 - 230.0);
        assert!(view.right() + TOOLBAR_GAP <= bar.left() + 1e-3 && (bar.right() - narrow.right()).abs() < 1e-3);
        // Nothing ticked: no bar, and the field keeps its width.
        let (field, _, bar) = places(narrow, 188.0, None);
        assert_eq!((field.width(), bar), (FIELD_WIDTH, None));
    }

    /// The width of the page's title in the top row, on `ctx`.
    fn title_width(ctx: &Context) -> f32 {
        let mut width = 0.0;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| width = widgets::width(ui.painter(), shell::Page::Repositories.name(), &shell::title_font()))
            .drop_without_applying_deltas();
        width
    }

    #[test]
    fn the_top_row_gives_way_before_it_meets_the_title() {
        let ctx = context();
        let title = title_width(&ctx);
        let demo = Session::demo(crate::demo::repos());
        let none = Session::Live(Box::new(Repos::offline(test_settings())));
        // Every width from far narrower than the window allows (where the row gives way) up to 2000, with and without
        // the sidebar, with repositories and without.
        let mut seen = Vec::new();
        for session in [&demo, &none] {
            for width in (400..=2000).step_by(4) {
                for sidebar in [true, false] {
                    #[expect(clippy::cast_precision_loss, reason = "window widths of a few thousand points")]
                    let size = vec2(width as f32, 900.0);
                    let (layout, planned) = planned(&ctx, session, size, sidebar);
                    let at = format!("{width} sidebar {sidebar}");
                    let Some(top) = planned.top else {
                        assert!(width < 960, "{at}: no top row in a window the app allows");
                        continue;
                    };
                    assert!(top.actions.left() >= layout.title.left() + title + theme::BAR_GAP - 1e-3, "{at}: over the title");
                    assert!(top.main.right() <= layout.captions.left() - theme::BAR_GAP + 1e-3, "{at}: over the window's buttons");
                    assert!((top.actions.right() + theme::BAR_GAP - top.main.left()).abs() < 1e-3, "{at}: the capsule and the button");
                    for rect in [top.actions, top.main] {
                        assert_eq!((rect.top(), rect.bottom()), (layout.toggle.top(), layout.toggle.bottom()), "{at}: out of the row");
                    }
                    // Never cut: each holds what it draws.
                    let [actions, main] = top_widths(&ctx, top.fit);
                    assert!((top.actions.width() - actions).abs() < 1e-3 && (top.main.width() - main).abs() < 1e-3, "{at}: {top:?}");
                    // In every window the app allows, all the words.
                    if width >= 960 {
                        assert_eq!(top.fit, Fit::Full, "{at}");
                    }
                    if !seen.contains(&top.fit) {
                        seen.push(top.fit);
                    }
                }
            }
        }
        seen.sort_by_key(|fit| *fit as usize);
        assert_eq!(seen, [Fit::Full, Fit::Compact, Fit::Bare], "a narrow window shows every way the row gives way");
        // The order of giving way, on a room of a known width: the capsule's words go first, the main button's last.
        let (full, compact, main_full, main_compact) = (200.0, 66.0, 150.0, 36.0);
        let fit = |width: f32| top_place(Rect::from_min_size(pos2(400.0, 12.0), vec2(width, 36.0)), [full, compact], [main_full, main_compact]);
        assert_eq!(fit(362.0).map(|top| top.fit), Some(Fit::Full));
        assert_eq!(fit(361.0).map(|top| top.fit), Some(Fit::Compact));
        assert_eq!(fit(228.0).map(|top| top.fit), Some(Fit::Compact));
        assert_eq!(fit(227.0).map(|top| top.fit), Some(Fit::Bare));
        let bare = fit(114.0).unwrap();
        assert_eq!(
            (bare.actions, bare.main),
            (Rect::from_min_size(pos2(400.0, 12.0), vec2(66.0, 36.0)), Rect::from_min_size(pos2(478.0, 12.0), vec2(36.0, 36.0)))
        );
        assert_eq!(fit(113.0), None, "not even the icons fit: nothing");
    }

    /// The widths of the top row's capsule and main button in `fit`, on `ctx`.
    fn top_widths(ctx: &Context, fit: Fit) -> [f32; 2] {
        let mut out = [0.0; 2];
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| {
            out = [
                widgets::capsule_width(ui.painter(), &top_items(fit)),
                widgets::main_action_width(ui.painter(), ADD_REPOSITORY, fit == Fit::Bare, theme::BAR_HEIGHT),
            ];
        })
        .drop_without_applying_deltas();
        out
    }

    #[test]
    fn an_empty_list_still_offers_add_and_import() {
        let ctx = context();
        let mut session = Session::Live(Box::new(Repos::offline(test_settings())));
        let (layout, planned) = planned(&ctx, &session, vec2(WINDOW[0], WINDOW[1]), true);
        assert_eq!(planned.toolbar, None, "no toolbar: nothing to filter, sort or tick");
        assert_eq!(planned.top.map(|top| top.fit), Some(Fit::Full));
        let mut wants = Wants::default();
        planned.add_wants(&mut wants);
        assert_eq!(wants.as_slice().iter().map(|want| want.id).collect::<Vec<_>>(), ["actions"]);
        assert_eq!(layout.toolbar.width(), 0.0);
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        // Every control of the row is there.
        let controls: Vec<String> = {
            let mut out = Vec::new();
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
            ctx.run_ui(input, |ui| out = control_places(ui.painter(), &planned).into_iter().map(|(words, _)| words).collect())
                .drop_without_applying_deltas();
            out
        };
        assert_eq!(controls, ["Import", "Update all", "Add repository"]);
        // Update all with nothing to update does what the core says, and nothing else.
        let at = planned.top.unwrap();
        let place = {
            let mut place = Rect::NOTHING;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
            ctx.run_ui(input, |ui| place = widgets::item_rects(ui.painter(), at.actions, &top_items(at.fit))[TOP_UPDATE_ALL])
                .drop_without_applying_deltas();
            place
        };
        click(&ctx, &mut session, &mut screen, place.center());
        assert_eq!(repos(&session).status, "There are no repositories to update.");
        assert!(repos(&session).mode == CoreMode::List && repos(&session).queue.is_empty() && !repos(&session).busy);
        // U too.
        assert_eq!(repos(&empty_after(typed(Key::U, "U"))).status, "There are no repositories to update.");
        // Import and Add repository open their dialogs.
        click(&ctx, &mut session, &mut screen, {
            let mut place = Rect::NOTHING;
            let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
            ctx.run_ui(input, |ui| place = widgets::item_rects(ui.painter(), at.actions, &top_items(at.fit))[TOP_IMPORT])
                .drop_without_applying_deltas();
            place.center()
        });
        assert!(repos(&session).mode == CoreMode::Path, "Import did not open");
        session.repos_mut().unwrap().close_path();
        click(&ctx, &mut session, &mut screen, at.main.center());
        assert!(repos(&session).mode == CoreMode::Add, "Add did not open");
        // The keys too.
        assert!(repos(&empty_after(typed(Key::A, "a"))).mode == CoreMode::Add);
        assert!(repos(&empty_after(typed(Key::I, "i"))).mode == CoreMode::Path);
        // The panel says what the two buttons do.
        assert_eq!(state_text(&empty_after(Vec::new())).map(|(_, lines)| lines), Some(vec![ADD_OR_IMPORT.to_owned()]));
    }

    /// A click at `at` on the screen of `session`, over four frames.
    fn click(ctx: &Context, session: &mut Session, screen: &mut Screen, at: Pos2) {
        let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            frame(ctx, session, screen, events);
        }
    }

    /// Where the selection bar's item `index` is, at the design's size, for `session`.
    fn bar_item(session: &Session, index: usize) -> Rect {
        let ctx = context();
        let (_, planned) = planned(&ctx, session, vec2(WINDOW[0], WINDOW[1]), true);
        let (rect, count, fit) = planned.toolbar.unwrap().selection.unwrap();
        let (number, count) = (count.to_string(), selected(count));
        let mut place = Rect::NOTHING;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| place = widgets::item_rects(ui.painter(), rect, &selection_items(&number, &count, fit))[index])
            .drop_without_applying_deltas();
        place
    }

    #[test]
    fn the_selection_bar_and_the_top_row_call_the_cores_methods() {
        use devassist_core::repos::state::HELD;
        let pressed = |index: usize| {
            let ctx = context();
            let mut session = Session::demo(crate::demo::repos());
            let mut screen = Screen::default();
            frame(&ctx, &mut session, &mut screen, Vec::new());
            let at = bar_item(&session, index).center();
            click(&ctx, &mut session, &mut screen, at);
            session
        };
        let update = pressed(BAR_UPDATE);
        assert_eq!((repos(&update).status.as_str(), ticked(&update)), (HELD, vec!["api".to_owned(), "web".to_owned()]));
        assert!(repos(&pressed(BAR_GROUP)).mode == CoreMode::Group);
        assert_eq!(repos(&pressed(BAR_OPEN)).status, HELD);
        assert_eq!(removing(&pressed(BAR_REMOVE)), Some(vec!["api".to_owned(), "web".to_owned()]));
        assert_eq!(ticked(&pressed(BAR_CLEAR)), Vec::<String>::new());
        // The count takes no press. (The divider is a point wide: a press on it is egui's to give to the nearest
        // button, as for any press beside a button.)
        let session = pressed(0);
        assert_eq!((repos(&session).status.as_str(), ticked(&session).len()), (crate::demo::repos().status.as_str(), 2));

        // Update all, in the top row.
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        let at = top_item(&session, TOP_UPDATE_ALL).center();
        session.repos_mut().unwrap().clear_marks();
        click(&ctx, &mut session, &mut screen, at);
        assert_eq!(repos(&session).status, HELD);
        // Import and Add repository open their dialogs through the core.
        let opened = |at: fn(&Session) -> Rect| {
            let ctx = context();
            let mut session = Session::demo(crate::demo::repos());
            let mut screen = Screen::default();
            frame(&ctx, &mut session, &mut screen, Vec::new());
            let at = at(&session).center();
            click(&ctx, &mut session, &mut screen, at);
            session
        };
        let import = opened(|session| top_item(session, TOP_IMPORT));
        assert!(repos(&import).mode == CoreMode::Path && repos(&import).prompt.is_some(), "Import did not open");
        let add = opened(|session| planned(&context(), session, vec2(WINDOW[0], WINDOW[1]), true).1.top.unwrap().main);
        assert!(repos(&add).mode == CoreMode::Add, "Add did not open");
    }

    /// Where the top row's capsule item `index` is, at the design's size, for `session`.
    fn top_item(session: &Session, index: usize) -> Rect {
        let ctx = context();
        let (_, planned) = planned(&ctx, session, vec2(WINDOW[0], WINDOW[1]), true);
        let top = planned.top.unwrap();
        let mut place = Rect::NOTHING;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| place = widgets::item_rects(ui.painter(), top.actions, &top_items(top.fit))[index]).drop_without_applying_deltas();
        place
    }

    #[test]
    fn a_double_click_on_a_button_acts_once() {
        // While a fetch runs, "Update all" and the bar's Update queue their run: a double click queues one.
        let mut busy = devassist_core::repos::state::testing::grouped_app();
        busy.busy = true;
        busy.marked = vec!["api".to_owned()];
        let ctx = context();
        let mut session = Session::Live(Box::new(busy));
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        for (at, time) in [(top_item(&session, TOP_UPDATE_ALL).center(), 1.0), (bar_item(&session, BAR_UPDATE).center(), 5.0)] {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            let frames = [
                (time, vec![Event::PointerMoved(at)]),
                (time + 0.02, vec![button(true)]),
                (time + 0.04, vec![button(false)]),
                (time + 0.1, vec![button(true)]),
                (time + 0.12, vec![button(false)]),
                (time + 0.2, Vec::new()),
            ];
            for (time, events) in frames {
                let input = RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))),
                    time: Some(time),
                    events,
                    ..Default::default()
                };
                ctx.run_ui(input, |ui| {
                    let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, has_toolbar(&session));
                    let plan = plan(ui.painter(), &layout, &session);
                    show(ui, &layout, plan.as_ref(), &mut session, Some(&sheet()), Mode::Light, &mut screen, true);
                })
                .drop_without_applying_deltas();
            }
        }
        assert_eq!(repos(&session).queue.len(), 2, "each button queued one run, not one a click");
    }

    #[test]
    fn without_the_keyboard_the_bar_and_the_top_row_take_no_press() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        let before = repos(&session).status.clone();
        let (_, planned) = planned(&context(), &session, vec2(WINDOW[0], WINDOW[1]), true);
        let top = planned.top.unwrap();
        let presses = [
            bar_item(&session, BAR_REMOVE).center(),
            bar_item(&session, BAR_CLEAR).center(),
            top_item(&session, TOP_IMPORT).center(),
            top_item(&session, TOP_UPDATE_ALL).center(),
            top.main.center(),
        ];
        for at in presses {
            let button = |pressed| Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                frame_with(&ctx, &mut session, &mut screen, events, false);
            }
        }
        assert!(repos(&session).mode == CoreMode::List);
        assert_eq!((repos(&session).status.clone(), ticked(&session)), (before, vec!["api".to_owned(), "web".to_owned()]));
    }

    #[test]
    fn the_selection_bars_update_updates_both_ticked_checkouts_for_real() {
        use crate::dialogs::Which;
        use crate::dialogs::testing::{Window, behind, live, until};
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("bar-update", &["api", "web"]);
        let mut window = live(settings, 2);
        for folder in ["api", "web"] {
            window.session.repos_mut().unwrap().toggle_mark_of(folder);
        }
        window.frame(Vec::new());
        let at = bar_item(&window.session, BAR_UPDATE).center();
        window.click(at);
        until(&mut window, "open", |window| window.shows(Which::Progress));
        let names: Vec<String> = window.job().items.iter().map(|item| item.name.clone()).collect();
        assert_eq!(names, ["api", "web"], "the progress dialog shows the two tasks");
        until(&mut window, "ended", |window: &Window| !window.job().running());
        assert!(matches!(window.job().result, Some(Ok(_))), "{:?}", window.job().result);
        assert!(root.0.join("api").join("two").exists() && root.0.join("web").join("two").exists(), "both were updated");
        window.session.close();
    }

    #[test]
    fn the_empty_and_the_broken_states_say_what_to_do() {
        let broken = Session::Broken("line 1: theme is light, dark or system".to_owned());
        assert_eq!(
            state_text(&broken),
            Some((
                "The settings file has a mistake".to_owned(),
                vec![
                    "line 1: theme is light, dark or system".to_owned(),
                    "Correct the file and start DevAssist again. Nothing was loaded, so nothing is overwritten.".to_owned()
                ]
            ))
        );
        let none = Session::Live(Box::new(Repos::offline(test_settings())));
        assert_eq!(
            state_text(&none),
            Some(("No repositories yet".to_owned(), vec!["Add repository finds them on GitHub. Import takes clones you already have.".to_owned()]))
        );
        let mut demo = Session::demo(crate::demo::repos());
        assert_eq!(state_text(&demo), None);
        demo.repos_mut().unwrap().set_list_filter("zzz");
        assert_eq!(state_text(&demo), Some(("Nothing matches zzz".to_owned(), vec!["Esc clears the filter.".to_owned()])));
        // A filter that lets something through is a table.
        demo.repos_mut().unwrap().set_list_filter("api");
        assert_eq!(state_text(&demo), None);
        // No toolbar where there is nothing to filter.
        assert!(!has_toolbar(&broken) && !has_toolbar(&none) && has_toolbar(&demo));
    }

    #[test]
    fn the_toolbar_is_the_designs() {
        let toolbar = Rect::from_min_size(pos2(280.0, 60.0), vec2(1148.0, 36.0));
        let (field, view, selection) = places(toolbar, 252.0, Some([548.0, 260.0, 220.0]));
        assert_eq!(field, Rect::from_min_size(pos2(280.0, 60.0), vec2(280.0, 36.0)));
        assert_eq!(view, Rect::from_min_size(pos2(568.0, 60.0), vec2(252.0, 36.0)));
        assert_eq!(selection, Some((Rect::from_min_size(pos2(880.0, 60.0), vec2(548.0, 36.0)), Fit::Full)));
        assert_eq!(places(toolbar, 252.0, None).2, None);
        // In a narrow toolbar the selection keeps off the view, and nothing leaves the row.
        let narrow = Rect::from_min_size(pos2(12.0, 60.0), vec2(600.0, 36.0));
        let (field, view, selection) = places(narrow, 252.0, Some([300.0, 200.0, 180.0]));
        let (selection, _) = selection.unwrap();
        assert!(selection.left() >= view.right() + TOOLBAR_GAP || selection.width() == 0.0, "{selection:?} {view:?}");
        for rect in [field, view, selection] {
            assert!(narrow.contains_rect(rect), "{rect:?}");
        }
    }

    #[test]
    fn the_plan_bakes_the_toolbars_glass_and_counts_the_ticks() {
        let ctx = context();
        let session = Session::demo(crate::demo::repos());
        let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true);
        let mut planned = None;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| planned = plan(ui.painter(), &layout, &session)).drop_without_applying_deltas();
        let planned = planned.unwrap();
        let toolbar = planned.toolbar.clone().unwrap();
        assert_eq!(toolbar.sort, "By name");
        let (rect, count, _) = toolbar.selection.unwrap();
        assert_eq!(count, 2);
        assert_eq!(rect.right(), layout.toolbar.right());
        let top = planned.top.unwrap();
        assert_eq!(top.main.right(), layout.actions.right());
        assert_eq!(top.actions.right() + theme::BAR_GAP, top.main.left());
        let mut wants = Wants::default();
        planned.add_wants(&mut wants);
        let ids: Vec<&str> = wants.as_slice().iter().map(|want| want.id).collect();
        assert_eq!(ids, ["filter", "view", "selection", "actions"]);
        assert!(wants.as_slice().iter().all(|want| want.surface == theme::Surface::Capsule && (want.radius - 18.0).abs() < f32::EPSILON));
        assert_eq!(plan(&ctx.layer_painter(egui::LayerId::background()), &layout, &Session::Broken(String::new())), None);
    }

    #[test]
    fn the_highlight_is_kept_in_view() {
        let heights = [36.0, 44.0, 44.0, 44.0, 44.0];
        assert_eq!(follow(&heights, 1, 0.0, 100.0), 0.0, "already in view");
        assert_eq!(follow(&heights, 3, 0.0, 100.0), 68.0, "below: its bottom comes to the room's bottom");
        assert_eq!(follow(&heights, 0, 50.0, 100.0), 0.0, "above: its top comes to the room's top");
        assert_eq!(follow(&heights, 9, 50.0, 100.0), 50.0, "no such line: the scroll stays");
    }

    #[test]
    fn the_status_is_the_cores() {
        let demo = Session::demo(crate::demo::repos());
        assert_eq!(status(&demo).map(|status| status.text), Some("12 installed. 2 are behind. 1 has new commits on other branches.".to_owned()));
        assert_eq!(status(&Session::Broken(String::new())), None);
    }

    /// Which widget has egui's keyboard focus.
    fn focused(ctx: &Context) -> Option<Id> {
        ctx.memory(egui::Memory::focused)
    }

    #[test]
    fn the_arrows_in_the_filter_move_the_highlight_and_the_focus_stays_there() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        let rows = keys(&session);
        session.repos_mut().unwrap().pick(&rows[0]);
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        assert_eq!(focused(&ctx), Some(filter_id()));

        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowDown)]);
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[1].as_str()), "ArrowDown in the filter moves the highlight");
        assert_eq!(focused(&ctx), Some(filter_id()), "and the focus stays in the filter");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowDown), press(Key::ArrowUp), press(Key::ArrowDown)]);
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[2].as_str()));
        assert_eq!(focused(&ctx), Some(filter_id()));

        // Tab keeps the focus in the filter: nothing else on the screen can hold it.
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Tab)]);
        frame(&ctx, &mut session, &mut screen, Vec::new());
        assert_eq!(focused(&ctx), Some(filter_id()), "Tab in the filter keeps the focus");

        // Out of the filter, Space ticks the highlighted row only, and nothing else is pressed.
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        assert_eq!(focused(&ctx), None);
        let before = ticked(&session);
        frame(&ctx, &mut session, &mut screen, typed(Key::Space, " "));
        let after = ticked(&session);
        assert_eq!(after.len(), before.len() + 1, "{before:?} then {after:?}");
        assert!(after.contains(&rows[2]));
        assert_eq!(repos(&session).selected.as_deref(), Some(rows[3].as_str()), "the highlight moved down once, no row was clicked");
        assert_eq!(focused(&ctx), None);

        // With nothing focused, Tab gives the focus to the filter, the one control that takes it.
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Tab)]);
        frame(&ctx, &mut session, &mut screen, Vec::new());
        assert_eq!(focused(&ctx), Some(filter_id()));
    }

    #[test]
    fn esc_clears_a_filter_that_lets_nothing_through_even_without_the_focus() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        frame(&ctx, &mut session, &mut screen, typed(Key::Slash, "/"));
        frame(&ctx, &mut session, &mut screen, vec![Event::Text("zzz".to_owned())]);
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Enter)]);
        assert_eq!(focused(&ctx), None, "Enter leaves the filter");
        assert_eq!(repos(&session).list_filter, "zzz");
        assert_eq!(state_text(&session).map(|(_, lines)| lines), Some(vec!["Esc clears the filter.".to_owned()]));
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        assert_eq!(repos(&session).list_filter, "", "Esc does what the panel says");
        assert_eq!(ticked(&session), ["api", "web"], "and the ticks, hidden by the filter, stay");
    }

    #[test]
    fn text_in_the_frame_the_focus_moves_goes_where_the_focus_is() {
        let ctx = context();
        let mut session = Session::demo(crate::demo::repos());
        let mut screen = Screen::default();
        frame(&ctx, &mut session, &mut screen, Vec::new());
        // "/po" typed so fast that it comes in one frame: the "po" is the filter's.
        let fast = [typed(Key::Slash, "/"), typed(Key::P, "p"), typed(Key::O, "o")].concat();
        frame(&ctx, &mut session, &mut screen, fast);
        assert_eq!(repos(&session).list_filter, "po");
        assert_eq!(focused(&ctx), Some(filter_id()));
        // Esc and an "s" in one frame: the Esc leaves and clears the filter, the "s" was typed for it, not a key.
        frame(&ctx, &mut session, &mut screen, [vec![press(Key::Escape)], typed(Key::S, "s")].concat());
        assert_eq!(repos(&session).list_filter, "");
        assert!(repos(&session).sort == SortMode::Name, "the s did not change the sort");
    }
}
