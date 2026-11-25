//! The Repositories screen in the shell: the toolbar above the panel (the filter, the sort, and what is ticked),
//! the table on the panel or the sentence that says why there is none, the status bar's words, and the keys of
//! the terminal app. Every change goes through a method of the core's `Repos`.

use devassist_core::repos::state::{GROUP_KEY, Repos, SortMode};
use egui::{Event, FontFamily, Id, Key, Painter, Pos2, Rect, Ui, pos2};

use crate::assets::{self, Icon};
use crate::baker::Sheet;
use crate::frost;
use crate::repos_view::{self, Line, Status};
use crate::session::Session;
use crate::shell::{Layout, Wants};
use crate::table::{self, Did};
use crate::theme::{
    self, FIELD_WIDTH, GROUP_HEIGHT, HEAD_HEIGHT, Mode, PAGE_ROWS, PANEL_TEXT_INSET, RADIUS_PANEL, ROW_HEIGHT, STATE_GAP, TEXT, TEXT_TITLE,
    TOOLBAR_GAP,
};
use crate::widgets::{self, Item, font};

/// The filter field's glass pane.
const FILTER: &str = "filter";
/// The view capsule's glass pane.
const VIEW: &str = "view";
/// The selection capsule's glass pane.
const SELECTION: &str = "selection";
/// What the filter field says while it is empty.
const FILTER_HINT: &str = "Filter repositories";
/// The sort by name.
const BY_NAME: &str = "By name";
/// The sort that puts what needs attention first.
const ATTENTION_FIRST: &str = "Needs attention first";
/// The title of a panel without repositories.
const NO_REPOSITORIES: &str = "No repositories yet";
/// What to do without repositories.
const ADD_IN_TERMINAL: &str = "Add them in the terminal app for now: adding from the window comes next.";
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
#[derive(Clone, Copy, Debug, Default)]
pub struct Screen {
    /// How far the table's lines are moved up.
    scroll: f32,
    /// The filter field had the focus at the end of the last frame.
    filter_focused: bool,
}

/// Where the toolbar's controls are this frame, and what they say: decided before the bake, so their glass is
/// baked where they are drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The filter field.
    field: Rect,
    /// The view capsule.
    view: Rect,
    /// The words of the sort.
    sort: &'static str,
    /// The sort that puts what needs attention first is on.
    attention_first: bool,
    /// The selection capsule and its count, when something is ticked.
    selection: Option<(Rect, String)>,
}

impl Plan {
    /// Adds the glass of the toolbar's controls to the panes baked this frame.
    pub fn add_wants(&self, wants: &mut Wants) {
        widgets::add_glass(wants, FILTER, self.field);
        widgets::add_glass(wants, VIEW, self.view);
        if let Some((rect, _)) = &self.selection {
            widgets::add_glass(wants, SELECTION, *rect);
        }
    }
}

/// Whether the screen has its toolbar: when there are repositories to filter, sort and tick.
#[must_use]
pub fn has_toolbar(session: &Session) -> bool {
    session.repos().is_some_and(|repos| !repos.installed.is_empty())
}

/// The toolbar's controls in `layout`, measured with `painter`'s faces; `None` when the screen has no toolbar.
#[must_use]
pub fn plan(painter: &Painter, layout: &Layout, session: &Session) -> Option<Plan> {
    let repos = session.repos().filter(|_| has_toolbar(session))?;
    let attention_first = repos.sort == SortMode::NeedsUpdate;
    let sort = if attention_first { ATTENTION_FIRST } else { BY_NAME };
    let view_width = widgets::capsule_width(painter, &view_items(sort, attention_first));
    let count = repos.marked.len();
    let words = selected(count);
    let selection_width = (count > 0).then(|| widgets::capsule_width(painter, &selection_items(&words)));
    let (field, view, selection) = places(layout.toolbar, view_width, selection_width);
    Some(Plan { field, view, sort, attention_first, selection: selection.map(|rect| (rect, words)) })
}

/// The view capsule's items: the sort.
fn view_items(sort: &str, attention_first: bool) -> [Item<'_>; 1] {
    [Item::Action { icon: Icon::ArrowUpDown, words: sort, on: attention_first }]
}

/// The selection capsule's items: how many are ticked, and the button that clears the ticks.
fn selection_items(count: &str) -> [Item<'_>; 2] {
    [Item::Label(count), Item::Action { icon: Icon::X, words: "", on: false }]
}

/// The places of the filter field, the view capsule and the selection capsule in the toolbar row at `toolbar`: the
/// field and the view from the left, the selection at the right end when it has a width, never over the view.
fn places(toolbar: Rect, view_width: f32, selection_width: Option<f32>) -> (Rect, Rect, Option<Rect>) {
    let (top, bottom) = (toolbar.top(), toolbar.bottom());
    let field = Rect::from_min_max(pos2(toolbar.left(), top), pos2((toolbar.left() + FIELD_WIDTH).min(toolbar.right()), bottom));
    let view_left = (field.right() + TOOLBAR_GAP).min(toolbar.right());
    let view = Rect::from_min_max(pos2(view_left, top), pos2((view_left + view_width).min(toolbar.right()), bottom));
    let selection = selection_width.map(|width| {
        let left = (toolbar.right() - width).max(view.right() + TOOLBAR_GAP).min(toolbar.right());
        Rect::from_min_max(pos2(left, top), pos2(toolbar.right(), bottom))
    });
    (field, view, selection)
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
        Session::Live(repos) | Session::Demo(repos) => repos,
    };
    if repos.installed.is_empty() {
        // While the folders are still being read, the status bar says so and the panel waits.
        return (!repos.waiting()).then(|| (NO_REPOSITORIES.to_owned(), vec![ADD_IN_TERMINAL.to_owned()]));
    }
    if !repos.list_filter.is_empty() && repos.rows().is_empty() {
        return Some((nothing_matches(&repos.list_filter), vec![ESC_CLEARS.to_owned()]));
    }
    None
}

/// What a key asks of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    /// Move the highlight this many rows.
    Move(isize),
    /// Highlight the first row (`true`) or the last.
    End(bool),
    /// Tick or untick the highlighted row and move down.
    Tick,
    /// Tick every row shown.
    TickAll,
    /// Clear the ticks, or the filter when it had the focus.
    Escape,
    /// Change the sort.
    Sort,
    /// Put the focus in the filter.
    Filter,
    /// Fetch and read again.
    Refresh,
    /// Fold the highlighted group (`true`) or unfold it.
    Fold(bool),
}

/// What the keys among `events` ask, in order, as the terminal app reads them. Letters are read from the typed
/// text, so they follow the keyboard's layout.
fn commands(events: &[Event]) -> Vec<Command> {
    let mut commands = Vec::new();
    for event in events {
        match event {
            Event::Key { key, pressed: true, modifiers, .. } => {
                let command = match key {
                    Key::ArrowUp => Command::Move(-1),
                    Key::ArrowDown => Command::Move(1),
                    Key::PageUp => Command::Move(-PAGE_ROWS),
                    Key::PageDown => Command::Move(PAGE_ROWS),
                    Key::Home => Command::End(true),
                    Key::End => Command::End(false),
                    Key::Space if !modifiers.any() => Command::Tick,
                    Key::A if modifiers.command_only() => Command::TickAll,
                    Key::Escape => Command::Escape,
                    Key::F5 => Command::Refresh,
                    Key::ArrowLeft => Command::Fold(true),
                    Key::ArrowRight => Command::Fold(false),
                    _ => continue,
                };
                commands.push(command);
            }
            Event::Text(text) => commands.extend(text.chars().filter_map(|typed| match typed {
                'j' => Some(Command::Move(1)),
                'k' => Some(Command::Move(-1)),
                's' | 'S' => Some(Command::Sort),
                'r' => Some(Command::Refresh),
                '/' => Some(Command::Filter),
                _ => None,
            })),
            _ => {}
        }
    }
    commands
}

/// The scroll that shows the whole line at `index` of lines of `heights` in `room` points, moving `scroll` as
/// little as it takes.
fn follow(heights: &[f32], index: usize, scroll: f32, room: f32) -> f32 {
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

/// Draws the screen in `layout`: the toolbar of `plan` (from [`plan`] this frame), then the table or the sentence
/// that stands for it on the panel, with `sheet` its frosted sheet. The keys and presses change `session`'s
/// repositories through their methods; `screen` keeps the scroll and the focus between frames.
pub fn show(ui: &mut Ui, layout: &Layout, plan: Option<&Plan>, session: &mut Session, sheet: Option<&Sheet>, mode: Mode, screen: &mut Screen) {
    let palette = theme::palette(mode);
    let mut moved = false;
    if let (Some(plan), Some(repos)) = (plan, session.repos_mut()) {
        let mut text = repos.list_filter.clone();
        widgets::field(ui, plan.field, &mut text, FILTER_HINT, Icon::Search, palette, filter_id());
        repos.set_list_filter(&text);
        if widgets::capsule(ui, plan.view, &view_items(plan.sort, plan.attention_first), palette).is_some() {
            repos.toggle_sort();
        }
        if let Some((rect, count)) = &plan.selection
            && widgets::capsule(ui, *rect, &selection_items(count), palette) == Some(1)
        {
            repos.clear_marks();
        }
    }

    // The keys act when no text field has the focus; Esc in the filter clears it (egui has taken the focus away
    // by the time the frame sees the Esc).
    let typing = ui.memory(|memory| memory.has_focus(filter_id()));
    if !typing {
        let events = ui.input(|input| input.events.clone());
        for command in commands(&events) {
            moved |= apply(command, session, screen.filter_focused, ui);
        }
    }
    screen.filter_focused = ui.memory(|memory| memory.has_focus(filter_id()));

    if let Some((title, lines)) = state_text(session) {
        if let Some(sheet) = sheet {
            frost::paint(ui.painter(), Rect::from_min_size(Pos2::ZERO, ui.ctx().content_rect().size()), layout.panel, RADIUS_PANEL, sheet);
        }
        draw_state(ui, layout.panel, &title, &lines, palette);
        return;
    }
    let (Some(repos), Some(sheet)) = (session.repos_mut(), sheet) else { return };
    let lines = repos_view::lines(repos);
    if moved {
        let heights: Vec<f32> = lines.iter().map(line_height).collect();
        if let Some(index) = lines.iter().position(is_chosen) {
            screen.scroll = follow(&heights, index, screen.scroll, layout.panel.height() - HEAD_HEIGHT);
        }
    }
    match table::show(ui, layout.panel, &lines, sheet, palette, mode, &mut screen.scroll) {
        Did::Picked(key) => repos.pick(&key),
        Did::Ticked(key) => repos.toggle_mark_of(&key),
        Did::Folded(key) => {
            if let Some(group) = key.strip_prefix(GROUP_KEY) {
                repos.toggle_group(group);
            }
        }
        Did::TickedAll => repos.mark_all_visible(),
        Did::None => {}
    }
}

/// Does what `command` asks of `session`, the filter having had the focus at the end of the last frame when
/// `filter_focused`; says whether the highlight may have moved.
fn apply(command: Command, session: &mut Session, filter_focused: bool, ui: &Ui) -> bool {
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
        Command::Escape if filter_focused => repos.set_list_filter(""),
        Command::Escape => repos.clear_marks(),
        Command::Sort => repos.toggle_sort(),
        Command::Filter => {
            if has_repositories(repos) {
                ui.memory_mut(|memory| memory.request_focus(filter_id()));
            }
            return false;
        }
        Command::Fold(fold) => repos.fold_selected(fold),
        Command::Refresh => {}
    }
    ui.ctx().request_repaint();
    true
}

/// Whether there are repositories, and so a filter to type into.
fn has_repositories(repos: &Repos) -> bool {
    !repos.installed.is_empty()
}

/// The height of a line in the table.
fn line_height(line: &Line) -> f32 {
    match line {
        Line::Group(_) => GROUP_HEIGHT,
        Line::Repo(_) => ROW_HEIGHT,
    }
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
    use egui::{Context, Modifiers, RawInput, Stroke, TextureId, vec2};

    use super::*;
    use crate::shell;
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
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), events, ..Default::default() };
        let output = ctx.run_ui(input, |ui| {
            let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, has_toolbar(session));
            let plan = plan(ui.painter(), &layout, session);
            show(ui, &layout, plan.as_ref(), session, Some(&sheet()), Mode::Light, screen);
        });
        output.drop_without_applying_deltas();
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
        let mut session = Session::Demo(Box::new(crate::demo::repos()));
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
        // Esc in the filter clears it and leaves the ticks alone.
        frame(&ctx, &mut session, &mut screen, vec![press(Key::Escape)]);
        assert_eq!(repos(&session).list_filter, "");
        assert_eq!(ticked(&session), ["api"]);
    }

    #[test]
    fn the_arrows_fold_and_unfold_the_highlighted_group() {
        let ctx = context();
        let mut demo = crate::demo::repos();
        // Folding saves through the core; this one writes to a file of its own.
        let folder = std::env::temp_dir().join(format!("devassist-gui-fold-{}", std::process::id()));
        demo.config_path = folder.join("devassist.toml");
        let mut session = Session::Demo(Box::new(demo));
        let mut screen = Screen::default();
        session.repos_mut().unwrap().pick("group::home");
        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowLeft)]);
        assert_eq!(repos(&session).collapsed, ["home"]);
        assert!(!keys(&session).contains(&"dotfiles".to_owned()));
        frame(&ctx, &mut session, &mut screen, vec![press(Key::ArrowRight)]);
        assert_eq!(repos(&session).collapsed, Vec::<String>::new());
        let _ = std::fs::remove_dir_all(folder);
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
        // Ctrl with A ticks all; A alone types an "a", which is no key here.
        assert_eq!(read(vec![key(Key::A, Modifiers::COMMAND)]), [Command::TickAll]);
        assert_eq!(read(typed(Key::A, "a")), []);
        // A released key is nothing.
        let released = Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE };
        assert_eq!(read(vec![released]), []);
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
            Some(("No repositories yet".to_owned(), vec!["Add them in the terminal app for now: adding from the window comes next.".to_owned()]))
        );
        let mut demo = Session::Demo(Box::new(crate::demo::repos()));
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
        let (field, view, selection) = places(toolbar, 252.0, Some(548.0));
        assert_eq!(field, Rect::from_min_size(pos2(280.0, 60.0), vec2(280.0, 36.0)));
        assert_eq!(view, Rect::from_min_size(pos2(568.0, 60.0), vec2(252.0, 36.0)));
        assert_eq!(selection, Some(Rect::from_min_size(pos2(880.0, 60.0), vec2(548.0, 36.0))));
        assert_eq!(places(toolbar, 252.0, None).2, None);
        // In a narrow toolbar the selection keeps off the view, and nothing leaves the row.
        let narrow = Rect::from_min_size(pos2(12.0, 60.0), vec2(600.0, 36.0));
        let (field, view, selection) = places(narrow, 252.0, Some(200.0));
        let selection = selection.unwrap();
        assert!(selection.left() >= view.right() + TOOLBAR_GAP || selection.width() == 0.0, "{selection:?} {view:?}");
        for rect in [field, view, selection] {
            assert!(narrow.contains_rect(rect), "{rect:?}");
        }
    }

    #[test]
    fn the_plan_bakes_the_toolbars_glass_and_counts_the_ticks() {
        let ctx = context();
        let session = Session::Demo(Box::new(crate::demo::repos()));
        let layout = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true);
        let mut planned = None;
        let input = RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))), ..Default::default() };
        ctx.run_ui(input, |ui| planned = plan(ui.painter(), &layout, &session)).drop_without_applying_deltas();
        let planned = planned.unwrap();
        assert_eq!(planned.sort, "By name");
        let (rect, count) = planned.selection.clone().unwrap();
        assert_eq!(count, "2 selected");
        assert_eq!(rect.right(), layout.toolbar.right());
        let mut wants = Wants::default();
        planned.add_wants(&mut wants);
        let ids: Vec<&str> = wants.as_slice().iter().map(|want| want.id).collect();
        assert_eq!(ids, ["filter", "view", "selection"]);
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
        let demo = Session::Demo(Box::new(crate::demo::repos()));
        assert_eq!(status(&demo).map(|status| status.text), Some("12 installed. 2 are behind. 1 has new commits on other branches.".to_owned()));
        assert_eq!(status(&Session::Broken(String::new())), None);
    }
}
