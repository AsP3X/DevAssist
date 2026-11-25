//! The dialogs over the Repositories screen: one function each, drawn on the overlay ([`crate::overlay`]) while
//! the core's mode asks for them.
//!
//! The core decides when a dialog is open: its `Repos::mode`. [`which`] reads the mode, and [`sync`] is the one
//! bridge from it to the overlay: in the frame the mode first asks for a dialog, it opens the overlay
//! (`overlay::open`) with the focus on the dialog's main button; once the mode asks for none, the dialog is no
//! longer shown, and the overlay closes. A new dialog is one more [`Which`] and one arm in [`which`], [`sync`],
//! [`plan`] and [`show`].
//!
//! Each frame runs [`before`] (before the bake: the dialog's place, so its glass is baked there) and [`after`]
//! (after the screen: the dialog itself, and what a person did in it, through the core's methods). A dialog's
//! parts are shared: [`Header`] (the title and the quiet line under it) and [`inset`] (a box with the field's fill
//! that scrolls with the wheel).
//!
//! The progress dialog ([`Which::Progress`]) shows the run the core keeps (`Repos::job`): its title, its summary,
//! its tasks with their bars, the picked task's output, and its buttons. Esc hides it while the run goes on and
//! closes it once it has ended; a click on the scrim does nothing while the run goes on and closes it once it has
//! ended. The core closes a run that went well by itself, a moment after its end.

use std::collections::VecDeque;
use std::sync::Arc;

use devassist_core::repos::state::{Mode as CoreMode, Repos};
use devassist_core::task::job::{Item, ItemState, Job};
use egui::text::LayoutJob;
use egui::{Color32, Context, CornerRadius, FontFamily, Galley, Id, Key, LayerId, Modifiers, Painter, Rect, Sense, Stroke, Ui, pos2, vec2};

use crate::assets;
use crate::baker::{Drawn, Want};
use crate::overlay::{self, Kind};
use crate::theme::{
    self, BUTTON_HEIGHT, DIALOG_GAP, DIALOG_PAD, DIALOG_TITLE, HAIRLINE, HEADER_GAP, HEADER_LINE_HEIGHT, INSET_RADIUS, LINE_OPACITY, Mode,
    OUTPUT_GAP, OUTPUT_LINE, OUTPUT_PAD, OUTPUT_ROWS, OUTPUT_TEXT, PROGRESS_HEIGHT, PROGRESS_LEAST, PROGRESS_TRACK_OPACITY, PROGRESS_WIDTH, Palette,
    TASK_BAR, TASK_HEIGHT, TASK_NAME, TASK_PAD, TASK_ROWS, TEXT,
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

/// A dialog the core's mode asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The progress of the core's run (`Mode::Progress`).
    Progress,
}

/// The dialog `repos`'s mode asks for, if any.
#[must_use]
pub fn which(repos: &Repos) -> Option<Which> {
    match repos.mode {
        CoreMode::Progress if repos.job.is_some() => Some(Which::Progress),
        _ => None,
    }
}

/// What the dialogs keep from one frame to the next.
#[derive(Clone, Debug, Default)]
pub struct Dialogs {
    /// The dialog [`sync`] last saw the core's mode ask for.
    open: Option<Which>,
    /// How far the progress dialog's tasks are moved up, in points.
    tasks_scroll: f32,
    /// The task whose output the progress dialog showed last, so the list follows a run that moves on.
    shown: Option<usize>,
    /// The wheel turned over the output that does not come to a whole line yet, in points.
    wheel: f32,
}

/// The id of a dialog's main button, the one Enter presses: every dialog's, so that the focus [`sync`] gives it
/// stays when its words change ("Hide" becomes "Close").
fn main_id() -> Id {
    Id::unique("dialog-main")
}

/// The id of the progress dialog's button that stops the run.
fn cancel_id() -> Id {
    Id::unique("dialog-cancel")
}

/// The bridge from the core's mode to the overlay: in the frame `repos`'s mode first asks for a dialog, opens the
/// overlay with the focus on that dialog's main button, and starts its own state afresh. Once the mode asks for
/// none, there is nothing to open: the dialog is no longer shown, and the overlay closes. Called before the screen
/// (for what the core's workers changed) and after it (for what the screen asked: `l`, a click on the status bar).
pub fn sync(ctx: &Context, repos: Option<&Repos>, dialogs: &mut Dialogs) {
    let want = repos.and_then(which);
    if want == dialogs.open {
        return;
    }
    *dialogs = Dialogs { open: want, ..Dialogs::default() };
    match want {
        Some(Which::Progress) => overlay::open(ctx, Some(main_id())),
        None => {}
    }
}

/// A dialog's title and the quiet line under it, laid out for the dialog's width.
#[derive(Clone, Debug)]
pub struct Header {
    /// The title, in the medium face.
    title: Arc<Galley>,
    /// The quiet line, wrapped to the width.
    line: Arc<Galley>,
}

impl Header {
    /// `title` and `line` laid out with `painter`'s faces in `width` points; the line wraps, at the design's line
    /// height.
    #[must_use]
    pub fn new(painter: &Painter, title: &str, line: &str, width: f32) -> Header {
        let title = painter.layout(title.to_owned(), font(DIALOG_TITLE, assets::medium_family()), Color32::PLACEHOLDER, width);
        let mut job = LayoutJob::simple(line.to_owned(), font(TEXT, FontFamily::Proportional), Color32::PLACEHOLDER, width);
        for section in &mut job.sections {
            section.format.line_height = Some(TEXT * HEADER_LINE_HEIGHT);
        }
        Header { title, line: painter.layout_job(job) }
    }

    /// How tall it is.
    #[must_use]
    pub fn height(&self) -> f32 {
        self.title.size().y + HEADER_GAP + self.line.size().y
    }

    /// Draws it at the top left of `rect`, cut to `rect`.
    pub fn draw(&self, ui: &Ui, rect: Rect, palette: Palette) {
        let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        painter.galley(rect.min, self.title.clone(), palette.on_glass);
        painter.galley(pos2(rect.left(), rect.top() + self.title.size().y + HEADER_GAP), self.line.clone(), palette.quiet);
    }
}

/// A box inset in a dialog, as [`inset`] drew it.
pub struct Inset {
    /// Paints inside the box, cut to it.
    pub painter: Painter,
    /// How far the wheel turned over the box this frame, in points; positive when it turned up (back).
    pub wheel: f32,
}

/// Draws a box inset in a dialog at `rect` (its tasks, its output, its list of paths): the field's fill with round
/// corners. Gives a painter cut to it and the wheel turned over it.
#[must_use]
pub fn inset(ui: &Ui, rect: Rect, palette: Palette) -> Inset {
    ui.painter().rect_filled(rect, INSET_RADIUS, palette.field);
    let wheel = if ui.rect_contains_pointer(rect) { ui.input(|input| input.smooth_scroll_delta().y) } else { 0.0 };
    Inset { painter: ui.painter().with_clip_rect(rect.intersect(ui.clip_rect())), wheel }
}

/// Where a dialog's parts are, decided before the bake: its pane is baked there.
#[derive(Clone, Debug)]
pub struct Plan {
    /// The dialog.
    which: Which,
    /// Its title and quiet line.
    header: Header,
    /// Where its parts are.
    places: Places,
}

impl Plan {
    /// The dialog's glass pane, to add to the panes baked this frame.
    #[must_use]
    pub fn want(&self) -> Want {
        overlay::want_dialog(self.places.pane)
    }
}

/// The parts of the progress dialog.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Places {
    /// The whole pane.
    pane: Rect,
    /// The title and the quiet line.
    header: Rect,
    /// The box of the tasks.
    tasks: Rect,
    /// The box of the output.
    output: Rect,
    /// The row of the buttons.
    buttons: Rect,
}

/// `count` as points' worth of rows.
fn rows(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
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

/// Where the dialog the core's mode asks for stands in `window`, its header laid out with `painter`'s faces;
/// `None` when the mode asks for none.
#[must_use]
pub fn plan(painter: &Painter, window: Rect, repos: Option<&Repos>) -> Option<Plan> {
    let repos = repos?;
    match which(repos)? {
        Which::Progress => {
            let job = repos.job.as_ref()?;
            let header = Header::new(painter, &job.title, &job.summary().to_string(), PROGRESS_WIDTH - 2.0 * DIALOG_PAD);
            let places = progress_places(window, header.height(), job.items.len());
            Some(Plan { which: Which::Progress, header, places })
        }
    }
}

/// Before the bake: brings the overlay in line with the core's mode ([`sync`]) and gives the place of the dialog
/// it asks for, whose glass is then baked with the other panes ([`Plan::want`]).
pub fn before(ctx: &Context, repos: Option<&Repos>, dialogs: &mut Dialogs) -> Option<Plan> {
    sync(ctx, repos, dialogs);
    plan(&ctx.layer_painter(LayerId::background()), ctx.content_rect(), repos)
}

/// After the screen: brings the overlay in line with what the screen asked of the core ([`sync`]), then draws the
/// dialog the mode asks for over everything, with `drawn` (the baker's pictures of this frame) for its glass, and
/// does what a person did in it. `planned` is what [`before`] gave; a dialog the screen opened in this frame has no
/// glass yet, and gets it in the next frame, which is asked for.
pub fn after(ctx: &Context, planned: Option<Plan>, repos: Option<&mut Repos>, drawn: &[Drawn], mode: Mode, dialogs: &mut Dialogs) {
    let Some(repos) = repos else { return };
    sync(ctx, Some(repos), dialogs);
    let Some(want) = which(repos) else { return };
    let plan = if let Some(plan) = planned.filter(|plan| plan.which == want) {
        plan
    } else {
        ctx.request_repaint();
        let Some(plan) = plan(&ctx.layer_painter(LayerId::background()), ctx.content_rect(), Some(repos)) else { return };
        plan
    };
    show(ctx, &plan, repos, drawn, mode, dialogs);
}

/// Draws the dialog of `plan` on the overlay: the shield, the scrim, its glass among `drawn`, its pane, then the
/// dialog's own parts; what a person does in it goes to `repos` through its methods.
pub fn show(ctx: &Context, plan: &Plan, repos: &mut Repos, drawn: &[Drawn], mode: Mode, dialogs: &mut Dialogs) {
    let palette = theme::palette(mode);
    let window = ctx.content_rect();
    overlay::show(ctx, |ui| {
        let scrim = overlay::shield(ui, window, Id::unique("dialog-shield"));
        overlay::scrim(ui, window, palette);
        overlay::glass(ui, drawn);
        overlay::pane(ui, plan.places.pane, Id::unique("dialog-pane"));
        ui.set_clip_rect(plan.places.pane);
        match plan.which {
            Which::Progress => progress(ui, plan, repos, palette, dialogs, scrim),
        }
    });
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

/// The progress dialog of `plan` for `repos`'s run, on the overlay in `ui`; `scrim` is whether the scrim was
/// clicked in this frame.
fn progress(ui: &mut Ui, plan: &Plan, repos: &mut Repos, palette: Palette, dialogs: &mut Dialogs, scrim: bool) {
    let Some(job) = repos.job.as_ref() else { return };
    let places = plan.places;
    let running = job.running();
    let buttons = progress_buttons(job);
    let ids: Vec<Id> = buttons.iter().filter(|button| button.press.is_some()).map(|button| button.id()).collect();
    overlay::walk(ui, &ids);
    plan.header.draw(ui, places.header, palette);
    let mut done = Done { picked: tasks(ui, places.tasks, job, palette, dialogs), ..Done::default() };
    (done.view, done.scrolled) = output(ui, places.output, job, palette, dialogs);
    let widths: Vec<f32> = buttons.iter().map(|button| widgets::button_width(ui, button.words)).collect();
    for (place, button) in overlay::buttons(places.buttons, &widths).into_iter().zip(&buttons) {
        match button.press {
            Some(press) => {
                if widgets::button(ui, place, button.words, button.kind, palette, button.id()).clicked() {
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
        if ui.interact(visible, Id::unique(("dialog-task", index)), Sense::CLICK).clicked() {
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
    painter.text(pos2(left, y), egui::Align2::LEFT_CENTER, widgets::ellipsis(&item.name, &medium, TASK_NAME, ui), medium, palette.on_glass);
    let bar = Rect::from_min_size(pos2(left + TASK_NAME + TASK_PAD, y - PROGRESS_HEIGHT / 2.0), vec2(TASK_BAR, PROGRESS_HEIGHT));
    if item.state != ItemState::Waiting {
        painter.rect_filled(bar, PROGRESS_HEIGHT / 2.0, palette.on_glass.gamma_multiply(PROGRESS_TRACK_OPACITY));
        if let Some(share) = item.bar_fill() {
            #[expect(clippy::cast_possible_truncation, reason = "a share between 0 and 1 of the bar's width")]
            let filled = (f64::from(TASK_BAR) * share.clamp(0.0, 1.0)).round() as f32;
            let filled = filled.clamp(PROGRESS_LEAST, TASK_BAR);
            painter.rect_filled(Rect::from_min_size(bar.min, vec2(filled, PROGRESS_HEIGHT)), PROGRESS_HEIGHT / 2.0, palette.accent);
        }
    }
    let colour = match item.state {
        ItemState::Waiting => palette.quiet,
        ItemState::Failed => palette.danger,
        ItemState::Running | ItemState::Done => palette.on_glass,
    };
    let state_left = bar.right() + TASK_PAD;
    let room = row.right() - TASK_PAD - state_left;
    painter.text(pos2(state_left, y), egui::Align2::LEFT_CENTER, widgets::ellipsis(&item.short_state(), &regular, room, ui), regular, colour);
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
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{Duration, Instant};

    use devassist_core::config::Settings;
    use devassist_core::repos::state::JOB_LINGER;
    use devassist_core::repos::state::testing::test_settings;
    use devassist_core::task::job::JobEvent;
    use egui::{Event, Memory, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, RawInput, TextureId, TouchPhase};

    use super::*;
    use crate::baker::Sheet;
    use crate::repos_screen::{self, Screen};
    use crate::repos_view;
    use crate::session::Session;
    use crate::shell;
    use crate::theme::{MIN_WINDOW, WINDOW};
    use crate::widgets::tests::context;

    fn window() -> Rect {
        Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))
    }

    /// A held run of `names`, the first running at 3 of 4 with `lines` of output each, as the core keeps it.
    fn run(names: &[&str], lines: usize) -> Job {
        let mut job = Job::new("Updating 2 repositories", names.iter().map(|name| (*name).to_owned()).collect(), 0);
        for (index, name) in names.iter().enumerate().rev() {
            job.on(JobEvent::Start(index));
            for line in 0..lines {
                job.on(JobEvent::Line(format!("{name} line {line}"), false));
            }
        }
        // The first is the one that runs; the others wait again, as far as the bars go.
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Steps(3, 4));
        job
    }

    /// The demo's repositories with `job` as their run, its dialog open.
    fn session_with(job: Job) -> Session {
        let mut session = Session::demo(crate::demo::repos());
        let repos = session.repos_mut().unwrap();
        repos.job = Some(job);
        repos.show_job();
        session
    }

    fn sheet() -> Sheet {
        Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
    }

    /// The window as the app runs it: the dialog placed before the bake, the screen, then the dialog.
    struct Window {
        ctx: Context,
        session: Session,
        screen: Screen,
        dialogs: Dialogs,
        size: [f32; 2],
        /// The dialog's places in the last frame, when it was drawn.
        places: Option<Places>,
    }

    impl Window {
        fn new(session: Session) -> Window {
            Window { ctx: context(), session, screen: Screen::default(), dialogs: Dialogs::default(), size: WINDOW, places: None }
        }

        fn frame(&mut self, events: Vec<Event>) {
            self.frame_at(events, None);
        }

        fn frame_at(&mut self, events: Vec<Event>, time: Option<f64>) {
            let window = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0], self.size[1]));
            let input = RawInput { screen_rect: Some(window), events, time, ..Default::default() };
            let (session, screen, dialogs) = (&mut self.session, &mut self.screen, &mut self.dialogs);
            let mut places = None;
            self.ctx
                .run_ui(input, |ui| {
                    let ctx = ui.ctx().clone();
                    let planned = before(&ctx, session.repos(), dialogs);
                    let layout = shell::layout(window.size(), true, repos_screen::has_toolbar(session));
                    let plan = repos_screen::plan(ui.painter(), &layout, session);
                    let keys = !overlay::is_open(&ctx);
                    repos_screen::show(ui, &layout, plan.as_ref(), session, Some(&sheet()), Mode::Light, screen, keys);
                    after(&ctx, planned, session.repos_mut(), &[], Mode::Light, dialogs);
                    places = session.repos().and_then(|repos| super::plan(ui.painter(), window, Some(repos))).map(|plan| plan.places);
                })
                .drop_without_applying_deltas();
            self.places = places;
        }

        /// A click at `at`, over four frames.
        fn click(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
            for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
                self.frame(events);
            }
        }

        fn key(&mut self, key: Key) {
            self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
            self.frame(Vec::new());
        }

        fn typed(&mut self, key: Key, text: &str) {
            self.frame(vec![
                Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
                Event::Text(text.to_owned()),
            ]);
            self.frame(Vec::new());
        }

        fn repos(&self) -> &Repos {
            self.session.repos().unwrap()
        }

        fn job(&self) -> &Job {
            self.repos().job.as_ref().unwrap()
        }

        /// Whether the dialog is open: the core's mode asks for it and the overlay is shown.
        fn open(&self) -> bool {
            which(self.repos()) == Some(Which::Progress) && overlay::is_open(&self.ctx)
        }

        fn places(&self) -> Places {
            self.places.unwrap()
        }

        /// The places of the buttons in the last frame, with their words.
        fn buttons(&self) -> Vec<(Rect, &'static str)> {
            let buttons = progress_buttons(self.job());
            let widths: Vec<f32> = buttons.iter().map(|button| Window::button_width(button.words)).collect();
            overlay::buttons(self.places().buttons, &widths).into_iter().zip(buttons.iter().map(|button| button.words)).collect()
        }

        fn button_width(words: &str) -> f32 {
            let mut width = 0.0;
            let input = RawInput { screen_rect: Some(window()), ..Default::default() };
            context().run_ui(input, |ui| width = widgets::button_width(ui, words)).drop_without_applying_deltas();
            width
        }

        fn press(&mut self, words: &str) {
            let (place, _) = self.buttons().into_iter().find(|(_, said)| *said == words).unwrap();
            self.click(place.center());
        }
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
        assert!(window.open());
        assert_eq!(window.ctx.memory(Memory::focused), Some(main_id()));
        window.session.repos_mut().unwrap().job.as_mut().unwrap().finish(Err("Updated 1, 1 failed: web".to_owned()), false);
        window.frame(Vec::new());
        assert_eq!(window.buttons().iter().map(|(_, words)| *words).collect::<Vec<_>>(), ["Close"]);
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
        let tasks = window.places().tasks;
        window.click(pos2(tasks.center().x, tasks.top() + TASK_HEIGHT * 1.5));
        assert_eq!(window.job().shown(), 1);
        assert_eq!(window.job().picked, Some(1));
        // The box shows the newest lines of the picked task, as many as fit: the drawing's seven.
        let view = output_rows(window.places().output.height());
        assert_eq!(view, OUTPUT_ROWS);
        let job = window.job();
        let shown = visible_lines(&job.items[1].output, job.scroll, view);
        assert_eq!(shown, (5..12).map(|line| format!("web line {line}")).collect::<Vec<_>>());
        assert_eq!(job.view, OUTPUT_ROWS, "the core knows how many lines are shown");

        // The wheel over the output scrolls it back, a line at a time, and no further than the oldest line.
        let output = window.places().output;
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

    fn wheel(points: f32) -> Event {
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, points),
            phase: TouchPhase::Move,
            source: MouseWheelSource::Wheel,
            modifiers: Modifiers::NONE,
        }
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
        let tasks = window.places().tasks;
        window.frame(vec![Event::PointerMoved(tasks.center())]);
        for _ in 0..60 {
            window.frame(vec![wheel(-200.0)]);
        }
        for _ in 0..30 {
            window.frame(Vec::new());
        }
        let most = 50.0 * TASK_HEIGHT - tasks.height();
        assert!((window.dialogs.tasks_scroll - most).abs() < 1e-3, "{} of {most}", window.dialogs.tasks_scroll);
        // The last row is there to click.
        window.click(pos2(tasks.center().x, tasks.bottom() - TASK_HEIGHT / 2.0));
        assert_eq!(window.job().picked, Some(49));
        assert!(window.buttons().iter().all(|(place, _)| window.places().pane.contains_rect(*place)));
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

    fn git(dir: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.invalid"])
            .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
    }

    /// Two checkouts under a fresh temporary folder `name`, each one commit behind its own origin. Gives the root and
    /// the settings that manage them.
    fn behind(name: &str) -> (PathBuf, Settings) {
        let root = std::env::temp_dir().join(format!("devassist-gui-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut checkouts = Vec::new();
        for repo in ["api", "web"] {
            let origin = root.join(format!("{repo}-origin"));
            std::fs::create_dir_all(&origin).unwrap();
            git(&origin, &["init", "-q", "-b", "main"]);
            std::fs::write(origin.join("one"), "one").unwrap();
            git(&origin, &["add", "one"]);
            git(&origin, &["commit", "-q", "-m", "one"]);
            git(&root, &["clone", "-q", origin.to_str().unwrap(), repo]);
            std::fs::write(origin.join("two"), "two").unwrap();
            git(&origin, &["add", "two"]);
            git(&origin, &["commit", "-q", "-m", "two"]);
            checkouts.push(root.join(repo));
        }
        let settings = Settings {
            owners_explicit: true,
            update_check: false,
            update_check_now: false,
            ..Settings::for_folder(checkouts, root.join("devassist.toml"))
        };
        (root, settings)
    }

    /// Runs frames of `window`, taking in what the core's workers sent, until `done` or 30 seconds.
    fn until(window: &mut Window, what: &str, done: impl Fn(&Window) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !done(window) {
            assert!(Instant::now() < deadline, "still not {what} after 30 seconds");
            window.session.tick();
            window.frame(Vec::new());
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn a_real_update_opens_the_dialog_shows_its_tasks_and_closes_after_the_linger() {
        let (root, settings) = behind("update");
        let mut window = Window::new(Session::Live(Box::new(Repos::new(settings))));
        until(&mut window, "read and fetched", |window| !window.repos().waiting() && window.repos().installed.len() == 2);
        assert!(!window.open());

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
        assert_eq!(window.buttons().iter().map(|(_, words)| *words).collect::<Vec<_>>(), ["Close"]);
        assert!(window.job().items.iter().all(|item| item.state == ItemState::Done && item.bar_fill() == Some(1.0)));
        until(&mut window, "closed", |window| !window.open());
        assert!(ended.elapsed() >= JOB_LINGER / 2, "it closed before the linger");
        assert!(window.repos().mode == CoreMode::List);
        assert!(root.join("api").join("two").exists() && root.join("web").join("two").exists(), "both were updated");
        window.session.close();
        drop(window);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_run_that_fails_while_hidden_says_so_in_the_status_bar() {
        let (root, settings) = behind("failure");
        let mut window = Window::new(Session::Live(Box::new(Repos::new(settings))));
        until(&mut window, "read and fetched", |window| !window.repos().waiting() && window.repos().installed.len() == 2);
        // The origins are gone: every fetch fails.
        std::fs::remove_dir_all(root.join("api-origin")).unwrap();
        std::fs::remove_dir_all(root.join("web-origin")).unwrap();

        window.session.repos_mut().unwrap().update_all();
        until(&mut window, "open", Window::open);
        window.key(Key::Escape);
        assert!(!window.open(), "Esc hides it");
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
        assert_eq!(window.buttons().iter().map(|(_, words)| *words).collect::<Vec<_>>(), ["Close"]);
        std::thread::sleep(JOB_LINGER);
        window.session.tick();
        window.frame(Vec::new());
        assert!(window.open(), "a failed run's dialog stays until it is closed");
        window.press("Close");
        assert!(!window.open());
        window.session.close();
        drop(window);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_mode_alone_opens_and_closes_the_overlay() {
        let ctx = context();
        let mut dialogs = Dialogs::default();
        let mut repos = Repos::offline(test_settings());
        repos.hold_work();
        let input = || RawInput { screen_rect: Some(window()), ..Default::default() };
        ctx.run_ui(input(), |ui| sync(ui.ctx(), Some(&repos), &mut dialogs)).drop_without_applying_deltas();
        assert_eq!((dialogs.open, overlay::is_open(&ctx)), (None, false));
        // No run: the mode asks for no dialog, whatever it says.
        repos.mode = CoreMode::Progress;
        assert_eq!(which(&repos), None);
        repos.job = Some(run(&["api"], 1));
        ctx.run_ui(input(), |ui| {
            sync(ui.ctx(), Some(&repos), &mut dialogs);
            assert!(overlay::is_open(ui.ctx()), "opened in the frame the mode asks for it");
        })
        .drop_without_applying_deltas();
        assert_eq!(dialogs.open, Some(Which::Progress));
        repos.hide_job();
        ctx.run_ui(input(), |ui| sync(ui.ctx(), Some(&repos), &mut dialogs)).drop_without_applying_deltas();
        assert_eq!(dialogs.open, None);
    }
}
