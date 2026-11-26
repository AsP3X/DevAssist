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
//!
//! The row's menu ([`crate::menu`]) is one more overlay on this bridge ([`Which::Menu`]): it opens with no focus,
//! has a shield but no scrim, and stands under the row's button that `repos_screen` places ([`before`] takes it).
//! When one of its lines opens a dialog, the core's mode changes and the bridge hands the overlay over to that
//! dialog in the next frame, opened as every dialog is.

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
use crate::menu;
use crate::overlay;
use crate::shell::Page;
use crate::theme::{self, DIALOG_TITLE, HEADER_GAP, HEADER_LINE_HEIGHT, HEADER_LINES, INSET_RADIUS, LIST_PAD, LIST_ROW, Mode, Palette, TEXT};
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
    /// The row's menu ([`crate::menu`], `Mode::Menu`): no dialog, but an overlay on the same bridge.
    Menu,
}

/// The dialog or the menu `repos`'s mode asks for, if any.
#[must_use]
pub fn which(repos: &Repos) -> Option<Which> {
    match repos.mode {
        CoreMode::Progress if repos.job.is_some() => Some(Which::Progress),
        CoreMode::ConfirmRemove { .. } => Some(Which::Remove),
        CoreMode::Group if repos.group_prompt.is_some() => Some(Which::Group),
        CoreMode::Menu if repos.menu.is_some() => Some(Which::Menu),
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
        // The remove dialog has no main button; the menu's lines take no focus, the arrows move its highlight.
        Some(Which::Remove | Which::Menu) => overlay::open(ctx, None),
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
    /// height, to [`HEADER_LINES`] lines at most, the last ending with an ellipsis when there is more.
    #[must_use]
    pub fn new(painter: &Painter, title: &str, line: &str, width: f32) -> Header {
        let title = painter.layout(title.to_owned(), font(DIALOG_TITLE, assets::medium_family()), Color32::PLACEHOLDER, width);
        let mut job = LayoutJob::simple(line.to_owned(), font(TEXT, FontFamily::Proportional), Color32::PLACEHOLDER, width);
        job.wrap.max_rows = HEADER_LINES;
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

/// Where a dialog's or the menu's parts are, decided before the bake: its pane is baked there.
#[derive(Clone, Debug)]
pub struct Plan(Planned);

/// A dialog, always with its header, or the menu, which has none.
#[derive(Clone, Debug)]
enum Planned {
    /// A dialog: its title and quiet line, and where its parts are.
    Dialog(Header, Parts),
    /// The menu, and where it stands.
    Menu(menu::Places),
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
    /// A dialog's plan: its header and its parts.
    fn dialog(header: Header, parts: Parts) -> Plan {
        Plan(Planned::Dialog(header, parts))
    }

    /// The dialog or the menu this is the plan of.
    fn which(&self) -> Which {
        match &self.0 {
            Planned::Dialog(_, parts) => parts.which(),
            Planned::Menu(_) => Which::Menu,
        }
    }

    /// The whole pane.
    fn pane(&self) -> Rect {
        match &self.0 {
            Planned::Dialog(_, parts) => parts.pane(),
            Planned::Menu(places) => places.pane,
        }
    }

    /// The glass pane of the dialog or the menu, to add to the panes baked this frame.
    #[must_use]
    pub fn want(&self) -> Want {
        match &self.0 {
            Planned::Dialog(_, parts) => overlay::want_dialog(parts.pane()),
            Planned::Menu(places) => overlay::want_menu(places.pane),
        }
    }
}

/// Where the dialog or the menu the core's mode asks for stands in `window`, a dialog's header laid out with
/// `painter`'s faces, the menu under the row's button at `anchor` (from `repos_screen::menu_anchor`); `None` when
/// the mode asks for none, or for the menu while the screen has not placed its button yet.
#[must_use]
pub fn plan(painter: &Painter, window: Rect, repos: Option<&Repos>, anchor: Option<Rect>) -> Option<Plan> {
    let repos = repos?;
    match which(repos)? {
        Which::Progress => progress::plan(painter, window, repos),
        Which::Remove => Some(remove::plan(painter, window, repos)),
        Which::Group => group::plan(painter, window, repos),
        Which::Menu => anchor.map(|anchor| Plan(Planned::Menu(menu::plan(window, anchor)))),
    }
}

/// Before the bake: brings the overlay in line with the core's mode ([`sync`]) and gives the place of the dialog
/// or the menu it asks for (the menu under `anchor`), whose glass is then baked with the other panes
/// ([`Plan::want`]).
pub fn before(ctx: &Context, repos: Option<&Repos>, anchor: Option<Rect>, dialogs: &mut Dialogs) -> Option<Plan> {
    sync(ctx, repos, dialogs);
    plan(&ctx.layer_painter(LayerId::background()), ctx.content_rect(), repos, anchor)
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
    if let Some(plan) = planned.filter(|plan| plan.which() == want) {
        show(ctx, &plan, repos, drawn, mode, dialogs);
    } else {
        // Opened after the bake: the shield alone keeps it open, with its focus, and paints nothing.
        let window = ctx.content_rect();
        overlay::show(ctx, |ui| overlay::shield(ui, window, shield_id()));
        ctx.request_repaint();
    }
}

/// Draws the dialog or the menu of `plan` on the overlay: the shield, for a dialog the scrim, its glass among
/// `drawn`, its pane, then its own parts; what a person does in it goes to `repos` through its methods.
pub fn show(ctx: &Context, plan: &Plan, repos: &mut Repos, drawn: &[Drawn], mode: Mode, dialogs: &mut Dialogs) {
    let palette = theme::palette(mode);
    let window = ctx.content_rect();
    overlay::show(ctx, |ui| {
        // For a dialog a click on its scrim; for the menu, which has none, a click outside it.
        let outside = overlay::shield(ui, window, shield_id());
        if matches!(plan.0, Planned::Dialog(..)) {
            overlay::scrim(ui, window, palette);
        }
        overlay::glass(ui, drawn);
        overlay::pane(ui, plan.pane(), Id::unique("dialog-pane"));
        ui.set_clip_rect(plan.pane());
        match &plan.0 {
            Planned::Dialog(header, Parts::Progress(places)) => progress::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Remove(places)) => remove::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Group(places)) => group::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Menu(places) => menu::show(ui, *places, repos, palette, outside),
        }
    });
    if which(repos) != Some(plan.which()) {
        // The dialog went: the screen gets its keys back in the next frame, at once.
        ctx.request_repaint();
    }
}

/// The id of the shield under every dialog.
fn shield_id() -> Id {
    Id::unique("dialog-shield")
}

#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::testing::test_settings;
    use egui::RawInput;

    use super::testing::{Window, run, window};
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

    /// On three live checkouts, each one behind: a run of api is started and hidden, one of notes is queued behind
    /// it, and `open` opens an overlay for web. Frames run until the first run has ended and the queued one has
    /// started and ended, `check` asked after every one of them; the core's mode keeps the overlay all along.
    /// Gives the window back, the overlay still open.
    fn runs_end_and_start_under(name: &str, open: fn(&mut Window), check: fn(&Window)) -> (Window, super::testing::Temp) {
        use super::testing::{behind, live, until};
        let (root, settings) = behind(name, &["api", "notes", "web"]);
        let mut window = live(settings, 3);
        let repos = window.session.repos_mut().unwrap();
        repos.pick("api");
        repos.update_chosen();
        until(&mut window, "open", |window| window.shows(Which::Progress));
        window.key(egui::Key::Escape);
        assert!(!window.open() && window.job().running(), "the run of api is hidden while it goes");
        let repos = window.session.repos_mut().unwrap();
        repos.pick("notes");
        repos.update_chosen();
        assert_eq!(window.repos().queue.len(), 1, "notes waits behind api");
        open(&mut window);
        check(&window);
        let started = std::cell::Cell::new(false);
        until(&mut window, "both run", |window| {
            check(window);
            started.set(started.get() || window.job().items.iter().any(|item| item.name == "notes"));
            started.get() && !window.repos().busy && window.repos().queue.is_empty() && !window.job().running()
        });
        for _ in 0..3 {
            window.session.tick();
            window.frame(Vec::new());
            check(&window);
        }
        // The status line says how the last run went, in the core's words.
        assert!(matches!(&window.job().result, Some(Ok(message)) if *message == window.repos().status), "{:?}", window.job().result);
        (window, root)
    }

    /// No folder of the remove dialog is called clean while its state is not known.
    fn truthful(window: &Window) {
        for line in window.repos().remove_lines() {
            let read = window.repos().installed.iter().find(|repo| repo.folder == line.name).is_some_and(|repo| repo.ready && repo.error.is_none());
            assert!(read || line.state != devassist_core::repos::state::RemoveState::Clean, "{} is called clean unread", line.name);
        }
    }

    #[test]
    fn a_run_that_ends_and_one_that_starts_leave_the_remove_dialog_as_it_was() {
        let _live = crate::session::tests::live_git();
        let open: fn(&mut Window) = |window| {
            let repos = window.session.repos_mut().unwrap();
            repos.marked = vec!["web".to_owned()];
            repos.begin_remove();
            window.frame(Vec::new());
        };
        let check: fn(&Window) = |window| {
            assert!(window.shows(Which::Remove), "the remove dialog went: {}", window.repos().status);
            assert_eq!(window.repos().remove_lines().iter().map(|line| line.name.as_str()).collect::<Vec<_>>(), ["web"], "what will be removed");
            truthful(window);
        };
        let (mut window, root) = runs_end_and_start_under("under-remove", open, check);
        assert!(root.0.join("web").exists(), "nothing was removed");
        window.press("Cancel");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        // `l` shows how the queued run ended.
        window.typed(egui::Key::L, "l");
        assert!(window.shows(Which::Progress));
        assert_eq!(window.job().items.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(), ["notes"]);
        window.session.close();
    }

    #[test]
    fn a_run_that_ends_and_one_that_starts_leave_the_group_dialog_as_it_was() {
        let _live = crate::session::tests::live_git();
        let open: fn(&mut Window) = |window| {
            let repos = window.session.repos_mut().unwrap();
            repos.marked = vec!["web".to_owned()];
            repos.open_group_prompt();
            window.frame(Vec::new());
        };
        let check: fn(&Window) = |window| {
            assert!(window.shows(Which::Group), "the group dialog went: {}", window.repos().status);
            let prompt = window.repos().group_prompt.as_ref().unwrap();
            assert_eq!(prompt.repos, ["web"], "what will be moved");
        };
        let (mut window, _root) = runs_end_and_start_under("under-group", open, check);
        window.press("Cancel");
        assert!(!window.open() && window.repos().mode == CoreMode::List);
        window.session.close();
    }

    #[test]
    fn a_run_that_ends_and_one_that_starts_leave_the_menu_as_it_was() {
        let _live = crate::session::tests::live_git();
        let open: fn(&mut Window) = |window| {
            let repos = window.session.repos_mut().unwrap();
            repos.clear_marks();
            repos.pick("web");
            window.frame(Vec::new());
            window.key(egui::Key::Enter);
            window.frame(Vec::new());
        };
        let check: fn(&Window) = |window| {
            assert!(window.shows(Which::Menu), "the menu went: {}", window.repos().status);
            assert_eq!(window.screen_menu(), Some("web"));
            assert_eq!(window.repos().chosen(), ["web"], "what the menu acts on");
        };
        let (mut window, _root) = runs_end_and_start_under("under-menu", open, check);
        window.key(egui::Key::Escape);
        assert!(window.repos().mode == CoreMode::List);
        window.session.close();
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
