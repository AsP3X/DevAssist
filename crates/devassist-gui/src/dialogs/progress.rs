//! The progress dialog ([`super::Which::Progress`]): the run the core keeps (`Repos::job`), with its title, its
//! summary, its tasks with their bars, the picked task's output, and its buttons. Esc hides it while the run goes
//! on and closes it once it has ended; a click on the scrim does nothing while the run goes on and closes it once
//! it has ended. The core closes a run that went well by itself, a moment after its end.

use std::collections::VecDeque;

use devassist_core::repos::state::Repos;
use devassist_core::task::job::{Item, ItemState, Job};
use egui::{CornerRadius, FontFamily, Id, Key, Modifiers, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, inset, main_id, rows};
use crate::assets;
use crate::overlay::{self, Kind};
use crate::theme::{
    BUTTON_HEIGHT, DIALOG_GAP, DIALOG_PAD, HAIRLINE, INSET_RADIUS, LINE_OPACITY, OUTPUT_GAP, OUTPUT_LINE, OUTPUT_PAD, OUTPUT_ROWS, OUTPUT_TEXT,
    PROGRESS_HEIGHT, PROGRESS_LEAST, PROGRESS_TRACK_OPACITY, PROGRESS_WIDTH, Palette, TASK_BAR, TASK_HEIGHT, TASK_NAME, TASK_PAD, TASK_ROWS, TEXT,
};
use crate::widgets::{self, font};

/// The progress dialog's button that stops the run. The drawing says "Cancel the update"; one dialog serves every
/// kind of run, so it says what the button does to any of them.
const CANCEL: &str = "Cancel";
/// The same button once it was pressed and the run is stopping.
const CANCELLING: &str = "Cancelling…";
/// The progress dialog's main button while the run goes on: the dialog goes, the run goes on.
const HIDE: &str = "Hide";
/// The progress dialog's main button once the run has ended.
const CLOSE: &str = "Close";

/// The id of the progress dialog's button that stops the run.
fn cancel_id() -> Id {
    Id::unique("dialog-cancel")
}

/// The parts of the progress dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Places {
    /// The whole pane.
    pub(super) pane: Rect,
    /// The title and the quiet line.
    pub(super) header: Rect,
    /// The box of the tasks.
    pub(super) tasks: Rect,
    /// The box of the output.
    pub(super) output: Rect,
    /// The row of the buttons.
    pub(super) buttons: Rect,
}

/// The height of an output box that shows `lines` lines.
fn output_height(lines: usize) -> f32 {
    2.0 * OUTPUT_PAD + rows(lines) * OUTPUT_LINE + rows(lines.saturating_sub(1)) * OUTPUT_GAP
}

/// How many whole lines an output box `height` points tall shows.
#[expect(clippy::cast_possible_truncation, clippy::cast_sign_loss, reason = "a whole, positive count of a few lines")]
fn output_rows(height: f32) -> usize {
    ((height - 2.0 * OUTPUT_PAD + OUTPUT_GAP) / (OUTPUT_LINE + OUTPUT_GAP)).floor().max(0.0) as usize
}

/// Where the progress dialog's parts are in `window`, with a header `header` points tall and `tasks` tasks: the
/// header at the top, the buttons at the bottom, between them the tasks (up to [`TASK_ROWS`] rows; more scroll)
/// and the output ([`OUTPUT_ROWS`] lines). In a window too short for all of it, the tasks keep one row and the
/// output gives up lines first, then the tasks; the buttons stay inside the pane.
fn progress_places(window: Rect, header: f32, tasks: usize) -> Places {
    let tasks_want = TASK_HEIGHT * rows(tasks.min(TASK_ROWS));
    let output_want = output_height(OUTPUT_ROWS);
    let height = 2.0 * DIALOG_PAD + header + 3.0 * DIALOG_GAP + tasks_want + output_want + BUTTON_HEIGHT;
    let pane = overlay::dialog_rect(window, vec2(PROGRESS_WIDTH, height));
    let inside = pane.shrink(DIALOG_PAD);
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    let header = Rect::from_min_size(inside.min, vec2(inside.width(), header.min(inside.height())));
    let top = header.bottom() + DIALOG_GAP;
    let room = (buttons.top() - DIALOG_GAP - top - DIALOG_GAP).max(0.0);
    let tasks_height = tasks_want.min((room - output_want).max(TASK_HEIGHT.min(tasks_want))).min(room);
    let output_height = (room - tasks_height).clamp(0.0, output_want);
    let tasks = Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), tasks_height));
    let output = Rect::from_min_size(pos2(inside.left(), tasks.bottom() + DIALOG_GAP), vec2(inside.width(), output_height));
    Places { pane, header, tasks, output, buttons }
}

/// Where the progress dialog of `repos`'s run stands in `window`, its header laid out with `painter`'s faces;
/// `None` without a run.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos) -> Option<Plan> {
    let job = repos.job.as_ref()?;
    let header = Header::new(painter, &job.title, &job.summary().to_string(), PROGRESS_WIDTH - 2.0 * DIALOG_PAD);
    let places = progress_places(window, header.height(), job.items.len());
    Some(Plan::dialog(header, Parts::Progress(places)))
}

/// What a button of the progress dialog does when pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Press {
    /// Stops the run.
    Cancel,
    /// Hides the dialog; the run goes on.
    Hide,
    /// Closes the dialog of a run that has ended.
    Close,
}

/// A button of the progress dialog: its words, its kind, and what it does; `None` when it cannot be pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Button {
    /// Its words.
    words: &'static str,
    /// Its kind.
    kind: Kind,
    /// What a press does; `None` while it cannot be pressed.
    press: Option<Press>,
}

impl Button {
    /// Its id, for the keyboard's focus: the main button's for the main one.
    fn id(self) -> Id {
        if self.kind == Kind::Main { main_id() } else { cancel_id() }
    }
}

/// The progress dialog's buttons for `job`, from the left: while it runs, Cancel (it reads "Cancelling…" and cannot
/// be pressed once pressed) and Hide, the main one; once it has ended, Close, the main one.
fn progress_buttons(job: &Job) -> Vec<Button> {
    if !job.running() {
        return vec![Button { words: CLOSE, kind: Kind::Main, press: Some(Press::Close) }];
    }
    let cancel = if job.cancelling {
        Button { words: CANCELLING, kind: Kind::Plain, press: None }
    } else {
        Button { words: CANCEL, kind: Kind::Plain, press: Some(Press::Cancel) }
    };
    vec![cancel, Button { words: HIDE, kind: Kind::Main, press: Some(Press::Hide) }]
}

/// The words of the progress dialog's buttons for `job`, from the left.
#[cfg(test)]
pub(super) fn button_words(job: &Job) -> Vec<&'static str> {
    progress_buttons(job).into_iter().map(|button| button.words).collect()
}

/// The lines of `output` a box of `rows` lines shows, `scroll` lines back from the newest: the newest at the
/// bottom.
fn visible_lines(output: &VecDeque<String>, scroll: usize, rows: usize) -> Vec<&str> {
    let end = output.len().saturating_sub(scroll);
    let start = end.saturating_sub(rows);
    output.iter().skip(start).take(end - start).map(String::as_str).collect()
}

/// The scroll of a list of rows [`TASK_HEIGHT`] tall in `room` points that shows the whole row `index`, moving
/// `scroll` as little as it takes.
fn follow(index: usize, scroll: f32, room: f32) -> f32 {
    let top = rows(index) * TASK_HEIGHT;
    let bottom = top + TASK_HEIGHT;
    if top < scroll {
        top
    } else if bottom > scroll + room {
        (bottom - room).max(0.0)
    } else {
        scroll
    }
}

/// What a person did in the progress dialog in one frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Done {
    /// The task clicked.
    picked: Option<usize>,
    /// Lines the output was scrolled back (negative: forward).
    scrolled: isize,
    /// The button pressed.
    pressed: Option<Press>,
    /// Lines of output the box shows.
    view: usize,
}

/// The progress dialog at `places`, with `header`, for `repos`'s run, on the overlay in `ui`; `scrim` is whether
/// the scrim was clicked in this frame.
pub(super) fn show(ui: &mut Ui, header: &Header, places: Places, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(job) = repos.job.as_ref() else { return };
    let running = job.running();
    let buttons = progress_buttons(job);
    let ids: Vec<Id> = buttons.iter().filter(|button| button.press.is_some()).map(|button| button.id()).collect();
    overlay::walk(ui, &ids);
    header.draw(ui, places.header, palette);
    let mut done = Done { picked: tasks(ui, places.tasks, job, palette, dialogs), ..Done::default() };
    (done.view, done.scrolled) = output(ui, places.output, job, palette, dialogs);
    let widths: Vec<f32> = buttons.iter().map(|button| widgets::button_width(ui, button.words)).collect();
    for (place, button) in overlay::buttons(places.buttons, &widths).into_iter().zip(&buttons) {
        match button.press {
            Some(press) => {
                if widgets::button(ui, place, button.words, button.kind, palette, button.id()) {
                    done.pressed = Some(press);
                }
            }
            None => widgets::button_off(ui, place, button.words, palette),
        }
    }
    let escape = ui.input_mut(|input| input.consume_key(Modifiers::NONE, Key::Escape));
    // Esc hides a run that goes on and closes one that has ended; a click on the scrim only closes one that ended.
    if done.pressed.is_none() && (escape || (scrim && !running)) {
        done.pressed = Some(if running { Press::Hide } else { Press::Close });
    }
    act(repos, done);
}

/// Does in `repos` what a person did in the progress dialog.
fn act(repos: &mut Repos, done: Done) {
    repos.set_job_view(done.view);
    if let Some(index) = done.picked {
        repos.pick_job_at(index);
    }
    if done.scrolled != 0 {
        repos.scroll_job(done.scrolled);
    }
    match done.pressed {
        Some(Press::Cancel) => repos.cancel_work(),
        // The terminal app closes a run's dialog, ended or not, the same way.
        Some(Press::Hide | Press::Close) => repos.hide_job(),
        None => {}
    }
}

/// Draws the tasks of `job` in the box at `rect`: one row each, the shown one on the selection's fill, scrolled by
/// the wheel. Says which one was clicked.
fn tasks(ui: &mut Ui, rect: Rect, job: &Job, palette: Palette, dialogs: &mut Dialogs) -> Option<usize> {
    let inset = inset(ui, rect, palette);
    let most = (TASK_HEIGHT * rows(job.items.len()) - rect.height()).max(0.0);
    let shown = job.shown();
    if dialogs.shown != Some(shown) {
        dialogs.shown = Some(shown);
        dialogs.tasks_scroll = follow(shown, dialogs.tasks_scroll, rect.height());
    }
    dialogs.tasks_scroll = (dialogs.tasks_scroll - inset.wheel).clamp(0.0, most);
    let last = job.items.len().saturating_sub(1);
    let mut picked = None;
    for (index, item) in job.items.iter().enumerate() {
        let row =
            Rect::from_min_size(pos2(rect.left(), rect.top() + rows(index) * TASK_HEIGHT - dialogs.tasks_scroll), vec2(rect.width(), TASK_HEIGHT));
        let visible = row.intersect(rect);
        if !visible.is_positive() {
            continue;
        }
        if overlay::fresh_click(ui, &ui.interact(visible, Id::unique(("dialog-task", index)), Sense::CLICK)) {
            picked = Some(index);
        }
        if index == shown {
            inset.painter.rect_filled(visible, row_corners(visible, rect), palette.selection);
        }
        if index != last {
            let y = row.bottom() - HAIRLINE / 2.0;
            inset.painter.hline(row.x_range(), y, Stroke::new(HAIRLINE, palette.on_glass.gamma_multiply(LINE_OPACITY)));
        }
        task(ui, &inset.painter, row, item, palette);
    }
    picked
}

/// The corners of a row's fill `visible` in the box at `rect`: round where it meets the box's round corners.
fn row_corners(visible: Rect, rect: Rect) -> CornerRadius {
    let round = CornerRadius::from(INSET_RADIUS);
    let top = visible.top() <= rect.top() + HAIRLINE / 2.0;
    let bottom = visible.bottom() >= rect.bottom() - HAIRLINE / 2.0;
    CornerRadius {
        nw: if top { round.nw } else { 0 },
        ne: if top { round.ne } else { 0 },
        sw: if bottom { round.sw } else { 0 },
        se: if bottom { round.se } else { 0 },
    }
}

/// Draws one task's row at `row`: its name, its bar (none while it waits) and its state in the core's words.
fn task(ui: &Ui, painter: &Painter, row: Rect, item: &Item, palette: Palette) {
    let y = row.center().y;
    let medium = font(TEXT, assets::medium_family());
    let regular = font(TEXT, FontFamily::Proportional);
    let left = row.left() + TASK_PAD;
    painter.text(pos2(left, y), egui::Align2::LEFT_CENTER, widgets::ellipsis(&item.name, &medium, TASK_NAME, ui), medium.clone(), palette.on_glass);
    let bar = Rect::from_min_size(pos2(left + TASK_NAME + TASK_PAD, y - PROGRESS_HEIGHT / 2.0), vec2(TASK_BAR, PROGRESS_HEIGHT));
    if item.state != ItemState::Waiting {
        painter.rect_filled(bar, PROGRESS_HEIGHT / 2.0, palette.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY));
        if let Some(share) = item.bar_fill() {
            #[expect(clippy::cast_possible_truncation, reason = "a share between 0 and 1 of the bar's width")]
            let filled = (f64::from(TASK_BAR) * share.clamp(0.0, 1.0)).round() as f32;
            let filled = filled.clamp(PROGRESS_LEAST, TASK_BAR);
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(filled, PROGRESS_HEIGHT)), PROGRESS_HEIGHT / 2.0, palette.progress);
        }
    }
    let colour = match item.state {
        ItemState::Waiting => palette.quiet,
        ItemState::Failed => palette.danger,
        ItemState::Running | ItemState::Done => palette.on_glass,
    };
    // The running task's state is in the medium face, as the design sets it; the others in the regular one.
    let words = if item.state == ItemState::Running { medium } else { regular };
    let state_left = bar.right() + TASK_PAD;
    let room = row.right() - TASK_PAD - state_left;
    painter.text(pos2(state_left, y), egui::Align2::LEFT_CENTER, widgets::ellipsis(&item.short_state(), &words, room, ui), words, colour);
}

/// Draws the shown task's output in the box at `rect`, the newest line at the bottom, in the 12-point monospaced
/// face. Gives how many lines the box shows, and how many lines the wheel scrolled it back.
fn output(ui: &Ui, rect: Rect, job: &Job, palette: Palette, dialogs: &mut Dialogs) -> (usize, isize) {
    let inset = inset(ui, rect, palette);
    let view = output_rows(rect.height());
    let mono = font(OUTPUT_TEXT, FontFamily::Monospace);
    let room = rect.width() - 2.0 * OUTPUT_PAD;
    if let Some(item) = job.items.get(job.shown()) {
        for (at, line) in visible_lines(&item.output, job.scroll, view).into_iter().enumerate() {
            let top = rect.top() + OUTPUT_PAD + rows(at) * (OUTPUT_LINE + OUTPUT_GAP);
            let words = widgets::ellipsis(line, &mono, room, ui);
            inset.painter.text(pos2(rect.left() + OUTPUT_PAD, top), egui::Align2::LEFT_TOP, words, mono.clone(), palette.on_glass);
        }
    }
    dialogs.wheel += inset.wheel;
    let step = OUTPUT_LINE + OUTPUT_GAP;
    let lines = (dialogs.wheel / step).trunc();
    dialogs.wheel -= lines * step;
    #[expect(clippy::cast_possible_truncation, reason = "a frame's wheel turn is a few lines")]
    let lines = lines as isize;
    (view, lines)
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use devassist_core::repos::state::{JOB_LINGER, Mode as CoreMode};
    use egui::{Event, Memory, Pos2};

    use super::super::Which;
    use super::super::testing::{Window, behind, live, run, session_with, until, wheel, window};
    use super::*;
    use crate::repos_view;
    use crate::shell;
    use crate::theme::{MIN_WINDOW, WINDOW};

    /// The progress dialog's places in the window's last frame.
    fn parts(window: &Window) -> Places {
        let Parts::Progress(places) = window.parts() else { panic!("not the progress dialog") };
        places
    }

    fn words(job: &Job) -> Vec<(&'static str, Kind, bool)> {
        progress_buttons(job).into_iter().map(|button| (button.words, button.kind, button.press.is_some())).collect()
    }

    #[test]
    fn a_running_dialog_offers_cancel_and_hide_and_a_finished_one_offers_close() {
        let mut job = run(&["api", "web"], 3);
        assert_eq!(words(&job), [("Cancel", Kind::Plain, true), ("Hide", Kind::Main, true)]);
        job.cancelling = true;
        assert_eq!(words(&job), [("Cancelling…", Kind::Plain, false), ("Hide", Kind::Main, true)], "pressed once, it is off");
        job.finish(Ok("Updated 2.".to_owned()), false);
        assert_eq!(words(&job), [("Close", Kind::Main, true)]);

        // In the window: it opens with the focus on Hide, the main button; Close keeps it.
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.shows(Which::Progress));
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Err("Updated 1, 1 failed: web".to_owned()), false);
        window.frame(Vec::new());
        assert_eq!(window.said(), ["Close"]);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        // Once it has ended, a click on the scrim closes it, and the run stays the core's.
        window.click(pos2(40.0, 40.0));
        assert!(!window.open());
        assert!(window.repos().mode == CoreMode::List && window.repos().job.is_some());

        // Close and Enter close it; Cancel is the core's.
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        window.frame(Vec::new());
        // A click on the scrim does nothing while it runs.
        window.click(pos2(40.0, 40.0));
        assert!(window.open(), "a click on the scrim hid a running dialog");
        window.press("Cancel");
        assert!(window.open(), "Cancel does not close the dialog");
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Ok("Updated 2.".to_owned()), false);
        window.frame(Vec::new());
        window.press("Close");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        window.session.repos_mut().unwrap().show_job();
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.open());
        window.key(Key::Enter);
        assert!(!window.open(), "Enter presses Close, which has the focus");
    }

    #[test]
    fn a_click_on_a_task_shows_its_output() {
        let mut window = Window::new(session_with(run(&["api", "web"], 12)));
        window.frame(Vec::new());
        assert_eq!(window.job().shown(), 0, "it follows the running task");
        let tasks = parts(&window).tasks;
        window.click(pos2(tasks.center().x, tasks.top() + TASK_HEIGHT * 1.5));
        assert_eq!(window.job().shown(), 1);
        assert_eq!(window.job().picked, Some(1));
        // The box shows the newest lines of the picked task, as many as fit: the drawing's seven.
        let view = output_rows(parts(&window).output.height());
        assert_eq!(view, OUTPUT_ROWS);
        let job = window.job();
        let shown = visible_lines(&job.items[1].output, job.scroll, view);
        assert_eq!(shown, (5..12).map(|line| format!("web line {line}")).collect::<Vec<_>>());
        assert_eq!(job.view, OUTPUT_ROWS, "the core knows how many lines are shown");

        // The wheel over the output scrolls it back, a line at a time, and no further than the oldest line.
        let output = parts(&window).output;
        window.frame(vec![Event::PointerMoved(output.center())]);
        for _ in 0..20 {
            window.frame(vec![wheel(60.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        assert_eq!(window.job().scroll, 12 - OUTPUT_ROWS, "scrolled back to the oldest line");
        let job = window.job();
        assert_eq!(visible_lines(&job.items[1].output, job.scroll, view)[0], "web line 0");
    }

    #[test]
    fn hiding_keeps_the_run_and_l_shows_it_again() {
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        window.frame(Vec::new());
        assert!(window.open());
        window.key(Key::Escape);
        assert!(!window.open(), "Esc hides a running dialog");
        assert!(window.job().running(), "the run goes on");
        window.frame(Vec::new());
        // The screen has the keyboard again: `l` brings the dialog back, in that frame.
        window.typed(Key::L, "l");
        assert!(window.open(), "l shows the run again");
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        window.press("Hide");
        assert!(!window.open());
        window.frame(Vec::new());
        // A click on the status bar brings it back too.
        let status = shell::layout(vec2(WINDOW[0], WINDOW[1]), true, true).status;
        window.click(status.center());
        assert!(window.open(), "a click on the status bar shows the run again");
    }

    #[test]
    fn fifty_tasks_scroll_and_the_buttons_stay_in_view() {
        for size in [WINDOW, MIN_WINDOW, [MIN_WINDOW[0], 400.0]] {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            for count in [1, 2, 6, 7, 50] {
                let places = progress_places(window, 40.0, count);
                assert!(window.contains_rect(places.pane), "{size:?} {count}: {places:?}");
                for part in [places.header, places.tasks, places.output, places.buttons] {
                    assert!(places.pane.contains_rect(part), "{size:?} {count}: {part:?} leaves {:?}", places.pane);
                }
                assert!((places.buttons.height() - BUTTON_HEIGHT).abs() < 1e-3 || size[1] < 300.0, "{size:?}: {places:?}");
                assert!(places.tasks.height() <= TASK_HEIGHT * rows(count.min(TASK_ROWS)) + 1e-3);
                assert!(places.tasks.bottom() <= places.output.top() && places.output.bottom() <= places.buttons.top());
            }
        }
        // In the design's window: six rows, then the list scrolls; the output keeps its seven lines.
        let places = progress_places(window(), 40.0, 50);
        assert!((places.tasks.height() - 6.0 * TASK_HEIGHT).abs() < 1e-3);
        assert_eq!(output_rows(places.output.height()), OUTPUT_ROWS);
        // The drawing's dialog: 640 wide, two rows of tasks, seven lines of output.
        let places = progress_places(window(), 63.0, 2);
        assert!((places.pane.width() - 640.0).abs() < 1e-3 && (places.tasks.height() - 88.0).abs() < 1e-3);
        assert!((places.output.height() - 170.0).abs() < 1e-3, "{places:?}");

        // Fifty tasks in the window: the wheel moves the list down to the last row and no further; the buttons stay.
        let names: Vec<String> = (0..50).map(|index| format!("repository-{index}")).collect();
        let job = Job::new("Updating 50 repositories", names, 0);
        let mut window = Window::new(session_with(job));
        window.frame(Vec::new());
        let tasks = parts(&window).tasks;
        window.frame(vec![Event::PointerMoved(tasks.center())]);
        for _ in 0..60 {
            window.frame(vec![wheel(-200.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        let most = 50.0 * TASK_HEIGHT - tasks.height();
        assert!((window.dialogs.tasks_scroll - most).abs() < 1e-3, "{} of {most}", window.dialogs.tasks_scroll);
        assert_eq!(window.job().scroll, 0, "the wheel over the tasks left the output alone");
        // The last row is there to click.
        window.click(pos2(tasks.center().x, tasks.bottom() - TASK_HEIGHT / 2.0));
        assert_eq!(window.job().picked, Some(49));
        assert!(window.buttons().iter().all(|(place, _)| parts(&window).pane.contains_rect(*place)));
    }

    #[test]
    fn the_list_follows_the_run_and_the_output_shows_the_newest_lines() {
        assert!((follow(8, 0.0, 264.0) - (9.0 * TASK_HEIGHT - 264.0)).abs() < 1e-3);
        assert!((follow(1, 100.0, 264.0) - TASK_HEIGHT).abs() < 1e-3);
        assert!((follow(3, 0.0, 264.0)).abs() < 1e-3);
        let output: VecDeque<String> = (0..10).map(|line| line.to_string()).collect();
        assert_eq!(visible_lines(&output, 0, 3), ["7", "8", "9"]);
        assert_eq!(visible_lines(&output, 2, 3), ["5", "6", "7"]);
        assert_eq!(visible_lines(&output, 20, 3), Vec::<&str>::new());
        assert_eq!(visible_lines(&VecDeque::new(), 0, 3), Vec::<&str>::new());
        assert_eq!(output_rows(output_height(7)), 7);
        assert_eq!(output_rows(output_height(1)), 1);
        assert_eq!(output_rows(0.0), 0);
        let inset = Rect::from_min_size(Pos2::ZERO, vec2(100.0, 88.0));
        assert_eq!(row_corners(Rect::from_min_size(Pos2::ZERO, vec2(100.0, 44.0)), inset), CornerRadius { nw: 16, ne: 16, sw: 0, se: 0 });
        assert_eq!(row_corners(Rect::from_min_size(pos2(0.0, 44.0), vec2(100.0, 44.0)), inset), CornerRadius { nw: 0, ne: 0, sw: 16, se: 16 });
    }

    #[test]
    fn a_real_update_opens_the_dialog_shows_its_tasks_and_closes_after_the_linger() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("update", &["api", "web"]);
        let mut window = live(settings, 2);

        window.session.repos_mut().unwrap().update_all();
        until(&mut window, "open", Window::open);
        let job = window.job();
        assert_eq!(job.items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["api", "web"]);
        assert_eq!(job.title, "Updating 2 repositories");
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));

        // It ends well: Close, then the core closes it by itself after the linger.
        until(&mut window, "ended", |window| !window.job().running());
        let ended = Instant::now();
        assert!(matches!(window.job().result, Some(Ok(_))), "{:?}", window.job().result);
        // The frame that took in the end has just been drawn, well inside the linger.
        assert!(window.open(), "the dialog stays a moment after a run that went well");
        assert_eq!(window.said(), ["Close"]);
        assert!(window.job().items.iter().all(|item| item.state == ItemState::Done && item.bar_fill() == Some(1.0)));
        until(&mut window, "closed", |window| !window.open());
        assert!(ended.elapsed() >= JOB_LINGER / 2, "it closed before the linger");
        assert!(window.repos().mode == CoreMode::List);
        assert!(root.0.join("api").join("two").exists() && root.0.join("web").join("two").exists(), "both were updated");
        window.session.close();
    }

    #[test]
    fn cancel_stops_a_real_update_and_reads_cancelling_until_it_has() {
        let _live = crate::session::tests::live_git();
        // Enough checkouts that the run is still going when Cancel is pressed.
        let names = ["r0", "r1", "r2", "r3", "r4", "r5", "r6", "r7"];
        let (_root, settings) = behind("cancel", &names);
        let mut window = live(settings, names.len());

        window.session.repos_mut().unwrap().update_all();
        until(&mut window, "open", Window::open);
        assert_eq!(window.said(), ["Cancel", "Hide"]);
        // The press and the frames after it take in nothing from the core's workers: the run is still going.
        window.press("Cancel");
        assert!(window.job().cancelling, "Cancel asked the core to stop the run");
        assert_eq!(window.said(), ["Cancelling…", "Hide"]);
        assert_eq!(progress_buttons(window.job())[0].press, None, "it cannot be pressed again");
        assert!(window.open(), "Cancel does not close the dialog");

        until(&mut window, "ended", |window| !window.job().running());
        let job = window.job();
        assert!(job.cancelled && matches!(job.result, Some(Err(_))), "{:?}", job.result);
        assert_eq!(job.summary().words, "■ Cancelled.");
        assert_eq!(window.said(), ["Close"]);
        assert!(window.open(), "a cancelled run's dialog stays until it is closed");
        window.session.close();
    }

    #[test]
    fn a_run_that_fails_while_hidden_says_so_in_the_status_bar() {
        let _live = crate::session::tests::live_git();
        let (root, settings) = behind("failure", &["api", "web"]);
        let mut window = live(settings, 2);
        // The origins are gone: every fetch fails.
        std::fs::remove_dir_all(root.0.join("api-origin")).unwrap();
        std::fs::remove_dir_all(root.0.join("web-origin")).unwrap();

        window.session.repos_mut().unwrap().update_all();
        until(&mut window, "open", Window::open);
        window.key(Key::Escape);
        assert!(!window.open(), "Esc hides it");
        assert!(window.job().running(), "it was hidden while it ran");
        until(&mut window, "ended", |window| !window.job().running());
        for _ in 0..5 {
            window.frame(Vec::new());
        }
        assert!(!window.open(), "a run that fails while hidden stays hidden");
        let status = repos_view::status(window.repos());
        assert!(status.error, "{status:?}");
        assert_eq!(status.text, window.repos().status);
        assert_ne!(status.text, "");

        // `l` shows how it ended; Close closes it, and a failed run's dialog never closes by itself.
        window.typed(Key::L, "l");
        assert!(window.open());
        assert!(window.job().summary().words.starts_with('✗'), "{}", window.job().summary());
        assert_eq!(window.said(), ["Close"]);
        std::thread::sleep(JOB_LINGER);
        window.session.tick();
        window.frame(Vec::new());
        assert!(window.open(), "a failed run's dialog stays until it is closed");
        window.press("Close");
        assert!(!window.open());
        window.session.close();
    }

    #[test]
    fn a_run_that_ends_well_while_hidden_says_so_and_stays_hidden() {
        let _live = crate::session::tests::live_git();
        let (_root, settings) = behind("hidden", &["api", "web"]);
        let mut window = live(settings, 2);

        window.session.repos_mut().unwrap().update_all();
        until(&mut window, "open", Window::open);
        window.key(Key::Escape);
        assert!(!window.open() && window.job().running(), "it was hidden while it ran");
        until(&mut window, "ended", |window| !window.job().running());
        // No linger for a dialog that is not shown: nothing waits for it, nothing opens.
        assert!(window.repos().job_close_at.is_none());
        for _ in 0..5 {
            window.frame(Vec::new());
        }
        assert!(!window.open());
        let status = repos_view::status(window.repos());
        assert!(!status.error, "{status:?}");
        assert!(matches!(&window.job().result, Some(Ok(message)) if *message == status.text), "{status:?}");

        // `l` shows how it ended, with Close; it stays until it is closed.
        window.typed(Key::L, "l");
        assert!(window.open());
        assert!(window.job().summary().words.starts_with('✓'), "{}", window.job().summary());
        assert_eq!(window.said(), ["Close"]);
        std::thread::sleep(JOB_LINGER);
        window.session.tick();
        window.frame(Vec::new());
        assert!(window.open(), "a dialog brought back is not closed by the linger");
        window.session.close();
    }

    #[test]
    fn a_dialog_the_screen_opens_paints_nothing_until_its_glass_is_baked() {
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        // Opened by the core's mode, before the bake: whole in its first frame.
        window.frame(Vec::new());
        assert!(window.open() && window.baked && window.painted);
        window.key(Key::Escape);
        window.frame(Vec::new());
        assert!(!window.open() && !window.painted);

        // `l` opens it after the bake: that frame keeps the overlay open, with its focus, and paints nothing.
        window.frame(vec![
            Event::Key { key: Key::L, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text("l".to_owned()),
        ]);
        assert!(overlay::is_open(&window.ctx), "the overlay is open");
        assert!(!window.baked && !window.painted, "a frame without the dialog's glass painted the dialog");
        assert!(window.repaint, "the next frame is asked for at once");
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        // The next frame bakes its glass and draws it whole; the focus is still on Hide.
        window.frame(Vec::new());
        assert!(window.open() && window.baked && window.painted);
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
    }

    #[test]
    fn a_dialog_that_goes_gives_the_screen_its_keys_in_the_next_frame_at_once() {
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        let mut asked = Vec::new();
        for _ in 0..10 {
            window.frame(Vec::new());
            asked.push(window.repaint);
        }
        assert!(window.open() && !window.repaint, "an open dialog over a held run asks for no frames: {asked:?}");
        window.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        assert!(window.repos().mode == CoreMode::List);
        assert!(window.repaint, "the frame that hid it asks for the next at once");
    }
}
