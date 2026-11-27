//! The progress dialog ([`super::Which::Progress`]): the run the core keeps (`Repos::job`), with its title, its
//! summary, its tasks with their bars, the picked task's output, and its buttons. Esc hides it while the run goes
//! on and closes it once it has ended; a click on the scrim does nothing while the run goes on and closes it once
//! it has ended. The core closes a run that went well by itself, a moment after its end.
//!
//! While the run is a fetch of forks whose task has said how many there are (`Repos::fork_progress`), the dialog is
//! [`FETCH_WIDTH`] wide and counts them above its output, as drawn: "24 of 38 forks, 8 at a time", one bar, and
//! under it, while the fetch pauses, a pause icon with the core's sentence. The line under its title says where they
//! go (`Repos::fork_job_into`), its folder cut from the left when it would take more than [`INTO_LINES`] lines. A
//! fetch of one repository's forks shows no task list (the count stands in its place); a run of several keeps it,
//! above the count of the one fetched now. A run that has ended keeps its last count while its dialog shows it.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use devassist_core::repos::forklines::ForkProgress;
use devassist_core::repos::state::Repos;
use devassist_core::task::job::{Item, ItemState, Job};
use egui::{Color32, CornerRadius, FontFamily, Galley, Id, Key, Modifiers, Painter, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::{Dialogs, Header, Parts, Plan, inset, main_id, rows};
use crate::assets::{self, Icon};
use crate::overlay::{self, Kind};
use crate::theme::{
    BUTTON_HEIGHT, COUNT_GAP, COUNT_PAD, DIALOG_GAP, DIALOG_PAD, FETCH_WIDTH, HAIRLINE, HEADER_GAP, ICON, INSET_RADIUS, INTO_LINES, LINE_OPACITY,
    OUTPUT_GAP, OUTPUT_LINE, OUTPUT_PAD, OUTPUT_ROWS, OUTPUT_TEXT, PAUSE_ICON_GAP, PROGRESS_HEIGHT, PROGRESS_LEAST, PROGRESS_TRACK_OPACITY,
    PROGRESS_WIDTH, Palette, TASK_BAR, TASK_HEIGHT, TASK_NAME, TASK_PAD, TASK_ROWS, TEXT,
};
use crate::widgets::{self, draw_icon, font};

/// The start of the line under a fetch's title, before the folder.
const INTO: &str = "Into ";
/// The rest of that line, after the folder.
const INTO_REST: &str = ". What the checkout has already is taken from disk, so only what the forks added is downloaded.";

/// The line under the title of a fetch of forks into `folder`.
fn into_words(folder: &str) -> String {
    format!("{INTO}{folder}{INTO_REST}")
}

/// The count of a fetch of forks: "24 of 38 forks, 8 at a time".
fn counted(progress: &ForkProgress) -> String {
    format!("{} of {} forks, {} at a time", progress.done, progress.total, progress.at_a_time)
}

/// What the progress dialog shows of a fetch of forks: how far it is, and the collection the forks go into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Fetch {
    /// How far it is, in the core's numbers and words.
    progress: ForkProgress,
    /// The collection, once the core knows it.
    into: Option<PathBuf>,
    /// The run it is of (`Repos::job_stamp`).
    stamp: u64,
}

/// The fetch of forks `repos`'s run is, as the core says it now.
fn fetch_now(repos: &Repos) -> Option<Fetch> {
    repos.fork_progress().map(|progress| Fetch { progress, into: repos.fork_job_into(), stamp: repos.job_stamp() })
}

/// The fetch of forks the dialog of `repos`'s run shows: the core's now, or else `kept`, the last one the dialog showed,
/// when it is of this same run: between two repositories of a run of several (until the next one says its total) and
/// once the run has ended (the core says no more of it). Another run never shows it. A kept count shows no pause: a
/// pause belongs to the moment it was said, so only one the core reports now is drawn.
fn fetch_shown(repos: &Repos, kept: Option<&Fetch>) -> Option<Fetch> {
    repos.job.as_ref()?;
    fetch_now(repos).or_else(|| {
        let mut kept = kept.filter(|kept| kept.stamp == repos.job_stamp()).cloned()?;
        kept.progress.pause = None;
        Some(kept)
    })
}

/// Keeps in `dialogs` the fetch of forks `repos`'s run is now, so the dialog shows its run's last count until the next
/// one, or, once the run has ended, until it closes.
fn keep_fetch(repos: &Repos, dialogs: &mut Dialogs) {
    if let Some(now) = fetch_now(repos) {
        dialogs.fetch = Some(now);
    }
}

/// The face of the count's words and its pause's.
fn count_face() -> egui::FontId {
    font(TEXT, assets::medium_family())
}

/// The count's words laid out with `painter`'s faces in `width`.
fn count_galley(painter: &Painter, progress: &ForkProgress, width: f32) -> Arc<Galley> {
    painter.layout(counted(progress), count_face(), Color32::PLACEHOLDER, width)
}

/// The pause's words laid out with `painter`'s faces beside its icon, in a count `width` wide inside.
fn pause_galley(painter: &Painter, words: &str, width: f32) -> Arc<Galley> {
    painter.layout(words.to_owned(), count_face(), Color32::PLACEHOLDER, width - ICON - PAUSE_ICON_GAP)
}

/// How tall the count of `progress` is in a dialog whose inside is `width` wide, laid out with `painter`'s faces: its
/// words, its bar and, while the fetch pauses, the pause's line, with the room around them.
fn count_height(painter: &Painter, progress: &ForkProgress, width: f32) -> f32 {
    let inner = width - 2.0 * COUNT_PAD;
    let words = count_galley(painter, progress, inner).size().y;
    let pause = progress.pause.as_deref().map_or(0.0, |pause| COUNT_GAP + pause_galley(painter, pause, inner).size().y.max(ICON));
    2.0 * COUNT_PAD + words + COUNT_GAP + PROGRESS_HEIGHT + pause
}

/// The line under the title of a fetch into `folder`, laid out with `painter`'s faces in `width`: whole when it takes
/// [`INTO_LINES`] lines at most, else with as much of the folder's end as keeps it to them ([`widgets::path_tail_in`]).
fn into_line(painter: &Painter, folder: &str, width: f32) -> String {
    let face = font(TEXT, FontFamily::Proportional);
    let fits = |words: &str| painter.layout(words.to_owned(), face.clone(), Color32::PLACEHOLDER, width).rows.len() <= INTO_LINES;
    let whole = into_words(folder);
    if fits(&whole) {
        return whole;
    }
    // The most room the folder may take, found by halving: a folder given more room takes more lines.
    let cut = |room: f32| into_words(&widgets::path_tail_in(painter, folder, &face, room));
    let (mut low, mut high) = (0.0, widgets::width(painter, folder, &face));
    for _ in 0..INTO_STEPS {
        let mid = f32::midpoint(low, high);
        if fits(&cut(mid)) { low = mid } else { high = mid }
    }
    // With no room for any of the folder, the line names it in words; the hover still has the whole path.
    let tail = widgets::path_tail_in(painter, folder, &face, low);
    if tail.trim_start_matches('…').is_empty() { into_words(THE_FORKS_FOLDER) } else { into_words(&tail) }
}

/// The folder in the line under a fetch's title when there is no room for any of its path.
const THE_FORKS_FOLDER: &str = "the forks folder";

/// How many times [`into_line`] halves the room it looks in: to well under a character of a folder of a few
/// thousand points.
const INTO_STEPS: usize = 14;

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
    /// The count of a fetch of forks, while the run is one.
    pub(super) count: Option<Rect>,
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
#[cfg(test)]
fn progress_places(window: Rect, header: f32, tasks: usize) -> Places {
    places_with(window, header, Some(tasks), None)
}

/// [`progress_places`] with `tasks` rows of tasks (`None`: no task list) and, for a fetch of forks, a count `count`
/// points tall between the tasks and the output, the dialog [`FETCH_WIDTH`] wide. The count keeps its whole height
/// while there is room; the output gives up lines first, then the tasks; the buttons stay inside the pane. Without a
/// count it is the progress dialog as it always was.
fn places_with(window: Rect, header: f32, tasks: Option<usize>, count: Option<f32>) -> Places {
    let tasks_want = TASK_HEIGHT * rows(tasks.unwrap_or(0).min(TASK_ROWS));
    let output_want = output_height(OUTPUT_ROWS);
    let count_want = count.unwrap_or(0.0);
    // The boxes between the header and the buttons, each with the gap above it, and the gap above the buttons.
    let boxes = 1 + usize::from(tasks.is_some()) + usize::from(count.is_some());
    let width = if count.is_some() { FETCH_WIDTH } else { PROGRESS_WIDTH };
    let height = 2.0 * DIALOG_PAD + header + rows(boxes + 1) * DIALOG_GAP + tasks_want + count_want + output_want + BUTTON_HEIGHT;
    let pane = overlay::dialog_rect(window, vec2(width, height));
    let inside = pane.shrink(DIALOG_PAD);
    let buttons = Rect::from_min_max(pos2(inside.left(), (inside.bottom() - BUTTON_HEIGHT).max(inside.top())), inside.max);
    let header = Rect::from_min_size(inside.min, vec2(inside.width(), header.min(inside.height())));
    let top = header.bottom() + DIALOG_GAP;
    let room = (buttons.top() - DIALOG_GAP - top - rows(boxes - 1) * DIALOG_GAP).max(0.0);
    let count_height = count_want.min(room);
    let left = room - count_height;
    let tasks_height = if tasks.is_some() { tasks_want.min((left - output_want).max(TASK_HEIGHT.min(tasks_want))).min(left) } else { 0.0 };
    let output_height = (left - tasks_height).clamp(0.0, output_want);
    let tasks_rect = Rect::from_min_size(pos2(inside.left(), top), vec2(inside.width(), tasks_height));
    let mut next = if tasks.is_some() { tasks_rect.bottom() + DIALOG_GAP } else { top };
    let count = count.map(|_| {
        let rect = Rect::from_min_size(pos2(inside.left(), next), vec2(inside.width(), count_height));
        next = rect.bottom() + DIALOG_GAP;
        rect
    });
    let output = Rect::from_min_size(pos2(inside.left(), next), vec2(inside.width(), output_height));
    Places { pane, header, tasks: tasks_rect, count, output, buttons }
}

/// Where the progress dialog of `repos`'s run stands in `window`, its header laid out with `painter`'s faces; `kept`
/// is the fetch of forks the dialog showed last. `None` without a run.
pub(super) fn plan(painter: &Painter, window: Rect, repos: &Repos, kept: Option<&Fetch>) -> Option<Plan> {
    let job = repos.job.as_ref()?;
    let fetch = fetch_shown(repos, kept);
    let width = if fetch.is_some() { FETCH_WIDTH } else { PROGRESS_WIDTH } - 2.0 * DIALOG_PAD;
    let line = match fetch.as_ref().and_then(|fetch| fetch.into.as_ref()) {
        Some(folder) => into_line(painter, &folder.display().to_string(), width),
        None => job.summary().to_string(),
    };
    let header = Header::new(painter, &job.title, &line, width);
    // A fetch of one repository's forks shows its count in the task list's place.
    let tasks = (fetch.is_none() || job.items.len() > 1).then_some(job.items.len());
    let count = fetch.as_ref().map(|fetch| count_height(painter, &fetch.progress, width));
    let places = places_with(window, header.height(), tasks, count);
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
    keep_fetch(repos, dialogs);
    let fetch = fetch_shown(repos, dialogs.fetch.as_ref());
    let Some(job) = repos.job.as_ref() else { return };
    let running = job.running();
    let buttons = progress_buttons(job);
    let ids: Vec<Id> = buttons.iter().filter(|button| button.press.is_some()).map(|button| button.id()).collect();
    overlay::walk(ui, &ids);
    header.draw(ui, places.header, palette);
    let mut done = Done::default();
    if let Some(fetch) = &fetch {
        if let Some(folder) = &fetch.into {
            whole_folder_on_hover(ui, header, places.header, &folder.display().to_string());
        }
        if let Some(rect) = places.count {
            count(ui, rect, &fetch.progress, palette);
        }
    }
    // A fetch of one repository's forks has no task list: its count stands there.
    if places.count.is_none() || job.items.len() > 1 {
        done.picked = tasks(ui, places.tasks, job, palette, dialogs);
    }
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
        draw_bar(painter, bar.min, TASK_BAR, item.bar_fill(), palette);
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

/// Draws one of the dialog's bars from `at`, `width` long: its track and, when it is known, `share` of it filled, at
/// least [`PROGRESS_LEAST`].
fn draw_bar(painter: &Painter, at: Pos2, width: f32, share: Option<f64>, palette: Palette) {
    painter.rect_filled(
        Rect::from_min_size(at, vec2(width, PROGRESS_HEIGHT)),
        PROGRESS_HEIGHT / 2.0,
        palette.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY),
    );
    if let Some(share) = share {
        #[expect(clippy::cast_possible_truncation, reason = "a share between 0 and 1 of the bar's width")]
        let filled = (f64::from(width) * share.clamp(0.0, 1.0)).round() as f32;
        let filled = filled.clamp(PROGRESS_LEAST, width);
        painter.rect_filled(Rect::from_min_size(at, vec2(filled, PROGRESS_HEIGHT)), PROGRESS_HEIGHT / 2.0, palette.progress);
    }
}

/// Draws the count of a fetch of forks in the box at `rect`: its words, its bar filled as far as the forks are, and,
/// while the fetch pauses, the pause icon and the core's sentence, wrapped when it does not fit.
fn count(ui: &Ui, rect: Rect, progress: &ForkProgress, palette: Palette) {
    let inset = inset(ui, rect, palette);
    let inner = rect.shrink(COUNT_PAD);
    let words = count_galley(&inset.painter, progress, inner.width());
    let bar = pos2(inner.left(), inner.top() + words.size().y + COUNT_GAP);
    inset.painter.galley(inner.min, words, palette.on_glass);
    let share = if progress.total == 0 { 0.0 } else { f64::from(progress.done) / f64::from(progress.total) };
    draw_bar(&inset.painter, bar, inner.width(), Some(share), palette);
    if let Some(pause) = &progress.pause {
        let top = bar.y + PROGRESS_HEIGHT + COUNT_GAP;
        let said = pause_galley(&inset.painter, pause, inner.width());
        let line = said.size().y.max(ICON);
        draw_icon(&inset.painter, pos2(inner.left() + ICON / 2.0, top + line / 2.0), Icon::Pause, ICON, palette.on_glass);
        let at = pos2(inner.left() + ICON + PAUSE_ICON_GAP, top + (line - said.size().y) / 2.0);
        inset.painter.galley(at, said, palette.on_glass);
    }
}

/// Gives the line under the title of `header` (at `rect`) the whole of `folder` as its hover text, when the line
/// shows it cut.
fn whole_folder_on_hover(ui: &Ui, header: &Header, rect: Rect, folder: &str) {
    if header.line.text().contains(folder) {
        return;
    }
    let line = Rect::from_min_size(pos2(rect.left(), rect.top() + header.title.size().y + HEADER_GAP), header.line.size()).intersect(rect);
    if line.is_positive() {
        ui.interact(line, Id::unique("progress-into"), Sense::hover()).on_hover_text(folder);
    }
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
    use std::path::PathBuf;
    use std::time::Instant;

    use devassist_core::repos::forklines;
    use devassist_core::repos::state::{JOB_LINGER, Mode as CoreMode, Msg};
    use devassist_core::task::job::JobEvent;
    use egui::{Event, Memory, Pos2, RawInput};

    use super::super::Which;
    use super::super::testing::{Window, behind, fetch_session, live, run, session_with, until, wheel, window};
    use super::*;
    use crate::assets::Icon;
    use crate::repos_view;
    use crate::shell;
    use crate::theme::{COUNT_PAD, FETCH_WIDTH, MIN_WINDOW, WINDOW};
    use crate::widgets::tests::context;

    /// How tall the progress dialog's header of `title` and `line` is, laid out as the dialog lays it out.
    fn header_height(_: &Window, title: &str, line: &str) -> f32 {
        let mut height = 0.0;
        let input = RawInput { screen_rect: Some(window()), ..Default::default() };
        context()
            .run_ui(input, |ui| height = Header::new(ui.painter(), title, line, PROGRESS_WIDTH - 2.0 * DIALOG_PAD).height())
            .drop_without_applying_deltas();
        height
    }

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
    fn the_tables_keys_do_nothing_while_the_progress_dialog_is_open() {
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        window.frame(Vec::new());
        let before = (window.repos().marked.clone(), window.repos().selected.clone(), window.repos().list_filter.clone());
        // (Space is not among them: it presses Hide, which has the focus.)
        window.typed(Key::D, "d");
        window.typed(Key::M, "m");
        window.typed(Key::U, "u");
        window.typed(Key::Slash, "/");
        window.key(Key::ArrowDown);
        window.key(Key::Delete);
        assert!(window.shows(Which::Progress), "{}", window.repos().status);
        assert_eq!((window.repos().marked.clone(), window.repos().selected.clone(), window.repos().list_filter.clone()), before);
        assert!(window.repos().queue.is_empty() && window.job().running());
    }

    #[test]
    fn a_long_summary_leaves_the_tasks_and_the_output_their_room_in_the_smallest_window() {
        // An update of a hundred that all failed: the core's summary names every one of them.
        let names: Vec<String> = (0..100).map(|index| format!("repository-{index:03}")).collect();
        let mut job = Job::new("Updating 100 repositories", names.clone(), 0);
        for index in 0..names.len() {
            job.on(devassist_core::task::job::JobEvent::Start(index));
            job.on(devassist_core::task::job::JobEvent::Failed(index, "could not reach the origin".to_owned()));
        }
        job.finish(Err(format!("Updated 0, 100 failed: {}", names.join(", "))), false);
        let mut window = Window::new(session_with(job));
        window.size = MIN_WINDOW;
        window.frame(Vec::new());
        let places = parts(&window);
        assert!(places.tasks.height() >= TASK_HEIGHT, "the tasks are squeezed to {}", places.tasks.height());
        assert!(output_rows(places.output.height()) >= 3, "the output is squeezed to {}", places.output.height());
        assert!(places.header.height() < places.pane.height() / 3.0, "the summary takes {places:?}");
    }

    /// The collection the tests' fetch of forks goes into: the demo's api-fork.
    fn into() -> PathBuf {
        PathBuf::from(crate::demo::CODE_DIR).join("work").join("api-fork")
    }

    /// The lines of a fetch of forks of 38 that has fetched 24, in the core's words; with `paused`, the drawing's pause
    /// after a timeout, 12 seconds left.
    fn fetched(paused: bool) -> Vec<(String, bool)> {
        let mut lines = vec![(forklines::total(38, &into()), false), (forklines::fetching(24, 38), true)];
        if paused {
            lines.extend([(forklines::paused("The fetch timed out", 30), false), (forklines::countdown(12), true)]);
        }
        lines
    }

    /// The drawing's sentence under the title for the fetch into `folder`.
    fn into_line(folder: &str) -> String {
        format!("Into {folder}. What the checkout has already is taken from disk, so only what the forks added is downloaded.")
    }

    /// The texts the dialog drew inside `rect` in the last frame: those from its title on, not the table's under it.
    fn texts_in(window: &Window, rect: Rect) -> Vec<String> {
        let title = window.texts.iter().position(|(words, ..)| *words == window.job().title).expect("no title");
        window.texts[title..].iter().filter(|(_, at, _)| rect.contains(at.center())).map(|(words, ..)| words.clone()).collect()
    }

    /// The bars filled with the progress colour inside `area` in the last frame: the dialog's, not the table's under it.
    fn filled_bars(window: &Window, area: Rect) -> Vec<Rect> {
        let progress = crate::theme::palette(crate::theme::Mode::Light).progress;
        window.fills.iter().filter(|(rect, fill)| *fill == progress && area.contains_rect(*rect)).map(|(rect, _)| *rect).collect()
    }

    #[test]
    fn a_fetch_of_forks_counts_them_in_one_bar() {
        let mut window = Window::new(fetch_session(&fetched(false)));
        window.frame(Vec::new());
        assert!(window.shows(Which::Progress));
        let places = parts(&window);
        assert!((places.pane.width() - FETCH_WIDTH).abs() < 1e-3, "{places:?}");
        let count = places.count.expect("no count");
        assert!(places.header.bottom() <= count.top() && count.bottom() <= places.output.top(), "{places:?}");
        // One repository: the count stands in the task list's place, as drawn.
        assert!(places.tasks.height().abs() < 1e-3, "{places:?}");
        assert_eq!(texts_in(&window, count), ["24 of 38 forks, 8 at a time"]);
        // One bar, the progress dialog's, filled 24 of 38 of its 568 points.
        let bars = filled_bars(&window, count);
        assert_eq!(bars.len(), 1, "{bars:?}");
        let track = count.width() - 2.0 * COUNT_PAD;
        assert!((track - 568.0).abs() < 1e-3);
        assert!((bars[0].width() - (track * 24.0 / 38.0).round()).abs() < 1e-3, "{bars:?}");
        assert!(count.contains_rect(bars[0]));
        // The line under the title says where they go.
        let line = into_line(&into().display().to_string());
        assert!(window.texts.iter().any(|(words, ..)| *words == line), "{:?}", window.texts);
        assert_eq!(window.said(), ["Cancel", "Hide"]);

        // A run that ends keeps its last numbers while its dialog shows it.
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Ok("Updated the forks of api.".to_owned()), false);
        assert_eq!(window.repos().fork_progress(), None, "the core says nothing more of it");
        window.frame(Vec::new());
        let ended = parts(&window);
        assert_eq!(ended.count, Some(count));
        assert_eq!(texts_in(&window, count), ["24 of 38 forks, 8 at a time"]);
        assert_eq!(window.said(), ["Close"]);
        // Shown again later, it is the plain dialog of how the run ended.
        window.press("Close");
        window.session.repos_mut().unwrap().show_job();
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(window.open() && parts(&window).count.is_none());
    }

    #[test]
    fn a_pause_is_said_under_the_bar_and_goes_when_the_fetch_goes_on() {
        let mut window = Window::new(fetch_session(&fetched(true)));
        window.frame(Vec::new());
        let places = parts(&window);
        let count = places.count.expect("no count");
        let pause = "The fetch timed out. Paused, going on in 0:12.";
        let icon = assets::icon(Icon::Pause).to_string();
        assert_eq!(texts_in(&window, count), ["24 of 38 forks, 8 at a time".to_owned(), icon.clone(), pause.to_owned()]);
        let bar = filled_bars(&window, count)[0];
        let said = window.texts.iter().find(|(words, ..)| words == pause).unwrap().1;
        assert!(said.top() >= bar.bottom(), "the pause is under the bar");
        // The pause goes on: its line goes, and the count is as tall as it was without it.
        window.session.repos_mut().unwrap().on_msg(Msg::Job(JobEvent::Line(forklines::going_on(), false)));
        window.frame(Vec::new());
        let without = parts(&window).count.expect("no count");
        assert_eq!(texts_in(&window, without), ["24 of 38 forks, 8 at a time"]);
        assert!(without.height() < count.height());
        let plain = Window::new(fetch_session(&fetched(false)));
        let mut plain = plain;
        plain.frame(Vec::new());
        assert!((parts(&plain).count.unwrap().height() - without.height()).abs() < 1e-3);
        assert!(window.buttons().iter().all(|(place, _)| parts(&window).pane.contains_rect(*place)));
    }

    #[test]
    fn another_run_keeps_the_plain_progress_dialog() {
        let mut window = Window::new(session_with(run(&["api", "web"], 3)));
        window.frame(Vec::new());
        let places = parts(&window);
        assert_eq!(places.count, None);
        let job = window.job();
        let header = header_height(&window, &job.title, &job.summary().to_string());
        assert_eq!(places, progress_places(super::super::testing::window(), header, 2), "the dialog as it was");
        assert!(window.texts.iter().any(|(words, ..)| *words == job.summary().to_string()));
        assert_eq!(filled_bars(&window, places.tasks).len(), 1, "the running task's bar alone");
    }

    #[test]
    fn the_count_and_the_buttons_stay_in_view_at_the_smallest_window() {
        let mut lines = fetched(false);
        lines.extend((0..40).map(|fork| (format!("Fetching owner-{fork}/api."), false)));
        lines.extend(fetched(true).into_iter().skip(2));
        for size in [MIN_WINDOW, [MIN_WINDOW[0], 400.0]] {
            let mut window = Window::new(fetch_session(&lines));
            window.size = size;
            window.frame(Vec::new());
            let places = parts(&window);
            let whole = Rect::from_min_size(Pos2::ZERO, vec2(size[0], size[1]));
            assert!(whole.contains_rect(places.pane), "{size:?}: {places:?}");
            let count = places.count.expect("no count");
            for part in [places.header, count, places.output, places.buttons] {
                assert!(places.pane.contains_rect(part), "{size:?}: {part:?} leaves {:?}", places.pane);
            }
            assert!((places.buttons.height() - BUTTON_HEIGHT).abs() < 1e-3, "{size:?}: {places:?}");
            assert!(count.bottom() <= places.output.top() && places.output.bottom() <= places.buttons.top(), "{size:?}: {places:?}");
            // The whole count, its pause too; the output gives up its lines first.
            assert_eq!(texts_in(&window, count).len(), 3, "{size:?}: {:?}", texts_in(&window, count));
            assert!(output_rows(places.output.height()) >= 1, "{size:?}: {places:?}");
            assert!(window.buttons().iter().all(|(place, _)| places.pane.contains_rect(*place)), "{size:?}");
            window.press("Hide");
            assert!(!window.open(), "{size:?}: Hide was out of reach");
        }
    }

    #[test]
    fn a_long_folder_is_cut_from_its_left_and_shown_whole_on_hover() {
        let deep: PathBuf = (0..12).fold(PathBuf::from(crate::demo::CODE_DIR), |path, at| path.join(format!("a-rather-long-folder-{at}")));
        let into = deep.join("api-fork");
        let mut window = Window::new(fetch_session(&[(forklines::total(38, &into), false)]));
        window.frame(Vec::new());
        let full = into_line(&into.display().to_string());
        let header = parts(&window).header;
        let (line, at, _) = window.texts.iter().find(|(words, ..)| words.starts_with("Into ")).cloned().expect("no line under the title");
        assert_ne!(line, full, "the folder is not cut");
        assert!(
            line.starts_with("Into …")
                && line.ends_with("api-fork. What the checkout has already is taken from disk, so only what the forks added is downloaded."),
            "{line}"
        );
        assert!(at.height() < 3.0 * TEXT * crate::theme::HEADER_LINE_HEIGHT - 1.0, "more than two lines: {at:?}");
        assert!(header.contains_rect(at));
        // On hover, the whole folder.
        let folder = into.display().to_string();
        window.frame_at(vec![Event::PointerMoved(at.center())], Some(10.0));
        for step in 1..=10 {
            window.frame_at(Vec::new(), Some(10.0 + 0.2 * f64::from(step)));
        }
        assert!(window.texts.iter().any(|(words, ..)| *words == folder), "no hover text with the whole folder");
    }

    #[test]
    fn a_count_belongs_to_its_own_run() {
        use devassist_core::repos::state::testing;
        let mut window = Window::new(fetch_session(&fetched(false)));
        window.frame(Vec::new());
        assert!(parts(&window).count.is_some());
        // The fetch ends; a queued update starts in the same open dialog.
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Ok("Updated the forks of api.".to_owned()), false);
        window.frame(Vec::new());
        assert!(parts(&window).count.is_some(), "the ended fetch keeps its count");
        testing::start_run(window.session.repos_mut().unwrap(), run(&["web"], 3));
        window.frame(Vec::new());
        assert!(window.shows(Which::Progress));
        let plain = |window: &Window| {
            let places = parts(window);
            let summary = window.job().summary().to_string();
            assert_eq!(places.count, None, "the update shows the fetch's count");
            assert!((places.pane.width() - PROGRESS_WIDTH).abs() < 1e-3);
            assert!(window.texts.iter().any(|(words, ..)| *words == summary), "not the update's own line");
            assert!(!window.texts.iter().any(|(words, ..)| words.starts_with("Into ")), "the fetch's folder line stayed");
        };
        plain(&window);
        // And once it has ended, failed.
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Err("Updated 0, 1 failed: web".to_owned()), false);
        window.frame(Vec::new());
        plain(&window);
    }

    #[test]
    fn a_run_of_several_keeps_its_count_between_repositories() {
        use devassist_core::repos::state::testing;
        let mut window = Window::new(super::super::testing::fetch_session_of(&["api", "web"], &fetched(false)));
        window.frame(Vec::new());
        let count = parts(&window).count.expect("no count");
        assert!(parts(&window).tasks.height() > 0.0, "a run of several keeps its task list");
        assert_eq!(texts_in(&window, count), ["24 of 38 forks, 8 at a time"]);
        let api = into_line(&into().display().to_string());
        // web starts: until it says its total, api's count and folder stay.
        testing::fork_job_next(window.session.repos_mut().unwrap(), &[]);
        assert_eq!(window.repos().fork_progress(), None);
        window.frame(Vec::new());
        let still = parts(&window).count.expect("the count dropped out between repositories");
        assert_eq!(still, count, "the dialog moved");
        assert_eq!(texts_in(&window, still), ["24 of 38 forks, 8 at a time"]);
        assert!(window.texts.iter().any(|(words, ..)| *words == api));
        // web's total arrives: its count and folder.
        let web = into().with_file_name("web-fork");
        let total = forklines::total(3, &web);
        testing::fork_job_next(window.session.repos_mut().unwrap(), &[(total.as_str(), false)]);
        window.frame(Vec::new());
        let count = parts(&window).count.expect("no count");
        assert_eq!(texts_in(&window, count), ["0 of 3 forks, 3 at a time"]);
        assert!(window.texts.iter().any(|(words, ..)| *words == into_line(&web.display().to_string())));
    }

    #[test]
    fn a_kept_count_shows_no_pause_the_fetch_no_longer_reports() {
        use devassist_core::repos::state::testing;
        let height_without = {
            let mut plain = Window::new(super::super::testing::fetch_session_of(&["api", "web"], &fetched(false)));
            plain.frame(Vec::new());
            parts(&plain).count.unwrap().height()
        };
        // A run of two: api paused when last drawn, then api ends and web has said nothing yet.
        let mut window = Window::new(super::super::testing::fetch_session_of(&["api", "web"], &fetched(true)));
        window.frame(Vec::new());
        let count = parts(&window).count.unwrap();
        assert_eq!(texts_in(&window, count).len(), 3, "the pause is not drawn");
        testing::fork_job_next(window.session.repos_mut().unwrap(), &[]);
        window.frame(Vec::new());
        let between = parts(&window).count.expect("the count dropped out");
        assert_eq!(texts_in(&window, between), ["24 of 38 forks, 8 at a time"], "an old pause is shown");
        assert!((between.height() - height_without).abs() < 1e-3, "the pane keeps the old pause's room");
        // A single fetch that ends right after a pause: its last count, no pause.
        let mut window = Window::new(fetch_session(&fetched(true)));
        window.frame(Vec::new());
        testing::fork_job_end(window.session.repos_mut().unwrap());
        window.frame(Vec::new());
        assert!(!window.job().running());
        let ended = parts(&window).count.expect("the finished dialog lost its count");
        assert_eq!(texts_in(&window, ended), ["24 of 38 forks, 8 at a time"]);
        assert_eq!(window.said(), ["Close"]);
    }

    #[test]
    fn the_last_task_of_a_run_of_several_keeps_its_count_when_the_run_ends() {
        use devassist_core::repos::state::testing;
        let mut window = Window::new(super::super::testing::fetch_session_of(&["api", "web"], &fetched(false)));
        window.frame(Vec::new());
        let web = forklines::total(3, &into().with_file_name("web-fork"));
        let done = forklines::fetching(2, 3);
        testing::fork_job_next(window.session.repos_mut().unwrap(), &[(web.as_str(), false), (done.as_str(), true)]);
        window.frame(Vec::new());
        testing::fork_job_end(window.session.repos_mut().unwrap());
        window.frame(Vec::new());
        assert!(!window.job().running());
        let count = parts(&window).count.expect("the finished dialog lost its count");
        assert_eq!(texts_in(&window, count), ["2 of 3 forks, 3 at a time"]);
    }

    #[test]
    fn a_folder_with_no_room_is_named_the_forks_folder() {
        let folder = (0..12).fold(PathBuf::from(crate::demo::CODE_DIR), |path, at| path.join(format!("folder-{at}"))).join("api-fork");
        let mut said = String::new();
        let input = RawInput { screen_rect: Some(window()), ..Default::default() };
        // So narrow that the sentence alone takes its two lines.
        context().run_ui(input, |ui| said = super::into_line(ui.painter(), &folder.display().to_string(), 200.0)).drop_without_applying_deltas();
        assert_eq!(said, into_line("the forks folder"));
    }

    #[test]
    fn four_repositories_a_count_a_pause_and_forty_lines_fit_the_smallest_window() {
        let mut lines = fetched(false);
        lines.extend((0..40).map(|fork| (format!("Fetching owner-{fork}/api."), false)));
        lines.extend(fetched(true).into_iter().skip(2));
        let mut window = Window::new(super::super::testing::fetch_session_of(&["api", "web", "docs", "notes"], &lines));
        window.size = MIN_WINDOW;
        window.frame(Vec::new());
        let places = parts(&window);
        let whole = Rect::from_min_size(Pos2::ZERO, vec2(MIN_WINDOW[0], MIN_WINDOW[1]));
        assert!(whole.contains_rect(places.pane), "{places:?}");
        let count = places.count.expect("no count");
        let parts_in_order = [places.header, places.tasks, count, places.output, places.buttons];
        for part in parts_in_order {
            assert!(places.pane.contains_rect(part), "{part:?} leaves {:?}", places.pane);
        }
        for pair in parts_in_order.windows(2) {
            assert!(pair[0].bottom() <= pair[1].top() + 1e-3, "{:?} overlaps {:?}", pair[0], pair[1]);
        }
        assert!(places.tasks.height() >= TASK_HEIGHT && (places.buttons.height() - BUTTON_HEIGHT).abs() < 1e-3, "{places:?}");
        assert_eq!(texts_in(&window, count).len(), 3, "{:?}", texts_in(&window, count));
        assert!(output_rows(places.output.height()) >= 1, "{places:?}");
        // The output still scrolls back.
        window.frame(vec![Event::PointerMoved(places.output.center())]);
        for _ in 0..10 {
            window.frame(vec![wheel(60.0)]);
        }
        assert!(window.job().scroll > 0, "the output did not scroll");
        assert!(window.buttons().iter().all(|(place, _)| parts(&window).pane.contains_rect(*place)));
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
