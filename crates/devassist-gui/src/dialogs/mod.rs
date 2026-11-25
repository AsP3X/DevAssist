//! The dialogs over the Repositories screen: one module each, drawn on the overlay ([`crate::overlay`]) while
//! the core's mode asks for them.
//!
//! The core decides when a dialog is open: its `Repos::mode`. [`which`] reads the mode, and [`sync`] is the one
//! bridge from it to the overlay: in the frame the mode first asks for a dialog, it opens the overlay
//! (`overlay::open`) with the focus on the dialog's main button; once the mode asks for none, the dialog is no
//! longer shown, and the overlay closes. A new dialog is one more [`Which`] and one arm in [`which`], [`sync`],
//! [`plan`] and [`show`].
//!
//! Each frame runs [`before`] (before the bake: the dialog's place, so its glass is baked there) and [`after`]
//! (after the screen: the dialog itself, and what a person did in it, through the core's methods). A dialog is
//! never drawn without its glass: one the screen opens in a frame (`l`, a click on the status bar) comes after the
//! bake, so in that frame only its shield stands (it keeps the overlay open, its focus, and every press from the
//! screen) and nothing is painted; the next frame, asked for at once, draws all of it. A dialog that goes asks for
//! a frame at once too, so the screen has its keys back without waiting.
//!
//! A dialog is on the Repositories screen: one that opens while another page is shown brings that page back
//! ([`page_for`]), so the status bar and `l`, which bring a hidden run back, are always at hand. A dialog's
//! parts are shared: [`Header`] (the title and the quiet line under it) and [`inset`] (a box with the field's fill
//! that scrolls with the wheel).
//!
//! The dialogs: [`progress`], the run the core keeps; [`remove`], which deletes or forgets the chosen
//! repositories; [`group`], which moves them into a group or renames one. A dialog with a main button opens with
//! the focus on it, the group dialog with the focus in its name field, the remove dialog (which has no main
//! button) with no focus at all.

mod group;
mod progress;
mod remove;

use std::sync::Arc;

use devassist_core::repos::state::{Mode as CoreMode, Repos};
use egui::text::LayoutJob;
use egui::{Color32, Context, FontFamily, Galley, Id, LayerId, Painter, Rect, Ui, pos2};

pub use self::remove::remove_title;
use crate::assets;
use crate::baker::{Drawn, Want};
use crate::overlay;
use crate::shell::Page;
use crate::theme::{self, DIALOG_TITLE, HEADER_GAP, HEADER_LINE_HEIGHT, INSET_RADIUS, LIST_PAD, LIST_ROW, Mode, Palette, TEXT};
use crate::widgets::font;

/// A dialog the core's mode asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Which {
    /// The progress of the core's run (`Mode::Progress`).
    Progress,
    /// The question before the chosen repositories are deleted or forgotten (`Mode::ConfirmRemove`).
    Remove,
    /// The name of the group the chosen repositories go into, or a group's new name (`Mode::Group`).
    Group,
}

/// The dialog `repos`'s mode asks for, if any.
#[must_use]
pub fn which(repos: &Repos) -> Option<Which> {
    match repos.mode {
        CoreMode::Progress if repos.job.is_some() => Some(Which::Progress),
        CoreMode::ConfirmRemove { .. } => Some(Which::Remove),
        CoreMode::Group if repos.group_prompt.is_some() => Some(Which::Group),
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
    /// How far the remove dialog's paths or the group dialog's groups are moved up, in points.
    list_scroll: f32,
    /// The group the group dialog's list showed picked last, so the list follows Up and Down.
    list_shown: Option<usize>,
}

/// The id of a dialog's main button, the one Enter presses: every dialog's, so that the focus [`sync`] gives it
/// stays when its words change ("Hide" becomes "Close").
fn main_id() -> Id {
    Id::unique("dialog-main")
}

/// The bridge from the core's mode to the overlay: in the frame `repos`'s mode first asks for a dialog, opens the
/// overlay with the focus on that dialog's main button (the group dialog's name field; nothing in the remove
/// dialog, which has no main button), and starts its own state afresh. Once the mode asks for none, there is
/// nothing to open: the dialog is no longer shown, and the overlay closes. Called before the screen (for what the
/// core's workers changed) and after it (for what the screen asked: `l`, a click on the status bar).
pub fn sync(ctx: &Context, repos: Option<&Repos>, dialogs: &mut Dialogs) {
    let want = repos.and_then(which);
    if want == dialogs.open {
        return;
    }
    *dialogs = Dialogs { open: want, ..Dialogs::default() };
    match want {
        Some(Which::Progress) => overlay::open(ctx, Some(main_id())),
        Some(Which::Remove) => overlay::open(ctx, None),
        Some(Which::Group) => overlay::open(ctx, Some(group::field_id())),
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

/// `count` as points' worth of rows.
fn rows(count: usize) -> f32 {
    f32::from(u16::try_from(count).unwrap_or(u16::MAX))
}

/// How tall a dialog's list of `count` rows is in its box: the rows and the room above and below them.
fn list_height(count: usize) -> f32 {
    2.0 * LIST_PAD[0] + rows(count) * LIST_ROW
}

/// How far a dialog's list of `count` rows can be moved up in a box `height` points tall.
fn most_scroll(count: usize, height: f32) -> f32 {
    (list_height(count) - height).max(0.0)
}

/// Where a dialog's parts are, decided before the bake: its pane is baked there.
#[derive(Clone, Debug)]
pub struct Plan {
    /// Its title and quiet line.
    header: Header,
    /// The dialog, and where its parts are.
    parts: Parts,
}

/// A dialog and where its parts are.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Parts {
    /// The progress dialog's.
    Progress(progress::Places),
    /// The remove dialog's.
    Remove(remove::Places),
    /// The group dialog's.
    Group(group::Places),
}

impl Parts {
    /// The dialog these are the parts of.
    fn which(self) -> Which {
        match self {
            Parts::Progress(_) => Which::Progress,
            Parts::Remove(_) => Which::Remove,
            Parts::Group(_) => Which::Group,
        }
    }

    /// The whole pane.
    fn pane(self) -> Rect {
        match self {
            Parts::Progress(places) => places.pane,
            Parts::Remove(places) => places.pane,
            Parts::Group(places) => places.pane,
        }
    }
}

impl Plan {
    /// The dialog's glass pane, to add to the panes baked this frame.
    #[must_use]
    pub fn want(&self) -> Want {
        overlay::want_dialog(self.parts.pane())
    }
}

/// Where the dialog the core's mode asks for stands in `window`, its header laid out with `painter`'s faces;
/// `None` when the mode asks for none.
#[must_use]
pub fn plan(painter: &Painter, window: Rect, repos: Option<&Repos>) -> Option<Plan> {
    let repos = repos?;
    match which(repos)? {
        Which::Progress => progress::plan(painter, window, repos),
        Which::Remove => Some(remove::plan(painter, window, repos)),
        Which::Group => group::plan(painter, window, repos),
    }
}

/// Before the bake: brings the overlay in line with the core's mode ([`sync`]) and gives the place of the dialog
/// it asks for, whose glass is then baked with the other panes ([`Plan::want`]).
pub fn before(ctx: &Context, repos: Option<&Repos>, dialogs: &mut Dialogs) -> Option<Plan> {
    sync(ctx, repos, dialogs);
    plan(&ctx.layer_painter(LayerId::background()), ctx.content_rect(), repos)
}

/// The page to show: the Repositories screen while a dialog is open over it, else `page`.
#[must_use]
pub fn page_for(repos: Option<&Repos>, page: Page) -> Page {
    if repos.and_then(which).is_some() { Page::Repositories } else { page }
}

/// After the screen: brings the overlay in line with what the screen asked of the core ([`sync`]), then draws the
/// dialog the mode asks for over everything, with `drawn` (the baker's pictures of this frame) for its glass, and
/// does what a person did in it. `planned` is what [`before`] gave. A dialog the screen opened in this frame has
/// no glass baked yet: this frame has its shield only, painting nothing, and the next frame, asked for now, draws
/// it whole.
pub fn after(ctx: &Context, planned: Option<Plan>, repos: Option<&mut Repos>, drawn: &[Drawn], mode: Mode, dialogs: &mut Dialogs) {
    let Some(repos) = repos else { return };
    sync(ctx, Some(repos), dialogs);
    let Some(want) = which(repos) else { return };
    if let Some(plan) = planned.filter(|plan| plan.parts.which() == want) {
        show(ctx, &plan, repos, drawn, mode, dialogs);
    } else {
        // Opened after the bake: the shield alone keeps it open, with its focus, and paints nothing.
        let window = ctx.content_rect();
        overlay::show(ctx, |ui| overlay::shield(ui, window, shield_id()));
        ctx.request_repaint();
    }
}

/// Draws the dialog of `plan` on the overlay: the shield, the scrim, its glass among `drawn`, its pane, then the
/// dialog's own parts; what a person does in it goes to `repos` through its methods.
pub fn show(ctx: &Context, plan: &Plan, repos: &mut Repos, drawn: &[Drawn], mode: Mode, dialogs: &mut Dialogs) {
    let palette = theme::palette(mode);
    let window = ctx.content_rect();
    overlay::show(ctx, |ui| {
        let scrim = overlay::shield(ui, window, shield_id());
        overlay::scrim(ui, window, palette);
        overlay::glass(ui, drawn);
        overlay::pane(ui, plan.parts.pane(), Id::unique("dialog-pane"));
        ui.set_clip_rect(plan.parts.pane());
        match plan.parts {
            Parts::Progress(places) => progress::show(ui, &plan.header, places, repos, palette, dialogs, scrim),
            Parts::Remove(places) => remove::show(ui, &plan.header, places, repos, palette, dialogs, scrim),
            Parts::Group(places) => group::show(ui, &plan.header, places, repos, palette, dialogs, scrim),
        }
    });
    if which(repos) != Some(plan.parts.which()) {
        // The dialog went: the screen gets its keys back in the next frame, at once.
        ctx.request_repaint();
    }
}

/// The id of the shield under every dialog.
fn shield_id() -> Id {
    Id::unique("dialog-shield")
}

#[cfg(test)]
mod testing;

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::testing::test_settings;
    use egui::RawInput;

    use super::testing::{run, window};
    use super::*;
    use crate::session::Session;
    use crate::widgets::tests::context;

    #[test]
    fn a_dialog_brings_the_repositories_page_with_it() {
        let mut session = Session::demo(crate::demo::repos());
        assert_eq!(page_for(session.repos(), Page::Docker), Page::Docker);
        session.repos_mut().unwrap().job = Some(run(&["api"], 1));
        session.repos_mut().unwrap().show_job();
        assert_eq!(page_for(session.repos(), Page::Docker), Page::Repositories);
        assert_eq!(page_for(session.repos(), Page::Repositories), Page::Repositories);
        session.repos_mut().unwrap().hide_job();
        assert_eq!(page_for(session.repos(), Page::Docker), Page::Docker);
        assert_eq!(page_for(None, Page::Docker), Page::Docker);
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
