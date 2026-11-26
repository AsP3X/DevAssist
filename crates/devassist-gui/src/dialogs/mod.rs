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
//! repositories; [`group`], which moves them into a group or renames one; [`add`], which finds repositories on
//! GitHub and clones the ticked ones into a folder; [`folder`], which asks for a folder whose clones are imported, or
//! into which the chosen checkouts move; [`push`], which pushes one repository to another Git service (the group, Add,
//! folder and Push dialogs open with the focus in their first field). A dialog with a main button opens with
//! the focus on it, the group dialog with the focus in its name field, the remove dialog (which has no main
//! button) with no focus at all.
//!
//! The row's menu ([`crate::menu`]) is one more overlay on this bridge ([`Which::Menu`]): it opens with no focus,
//! has a shield but no scrim, and stands under the row's button that `repos_screen` places ([`before`] takes it).
//! While the core's Migrate menu is open, its side menu is placed beside it before the bake, a second pane of the
//! same overlay. When one of its lines (or of the side menu's) opens a dialog, the core's mode changes and the bridge
//! hands the overlay over to that dialog in the next frame, opened as every dialog is.
//!
//! The window never asks the core for its folder question of a clone (`Mode::Path` for `Purpose::Clone`): Add
//! asks for the folder itself. Should the core stand there all the same, [`which`] says Add, and [`after`] closes
//! that question (`Repos::close_path`) before drawing, which brings Add back: no empty overlay is drawn.

mod add;
mod folder;
mod group;
mod progress;
mod push;
mod remove;

use std::sync::Arc;

use devassist_core::repos::state::{Mode as CoreMode, Purpose, Repos};
use egui::text::LayoutJob;
use egui::{Color32, Context, Event, FontFamily, Galley, Id, Key, LayerId, Painter, Rect, Ui, pos2};

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
    /// The repositories found on GitHub, to tick and clone into a folder (`Mode::Add`).
    Add,
    /// The folder whose clones are imported, or into which checkouts move (`Mode::Path` for `Purpose::Import` and
    /// `Purpose::Move`).
    Folder,
    /// The address of another Git service one repository is pushed to (`Mode::Service`).
    Push,
}

/// The dialog or the menu `repos`'s mode asks for, if any.
#[must_use]
pub fn which(repos: &Repos) -> Option<Which> {
    match repos.mode {
        CoreMode::Progress if repos.job.is_some() => Some(Which::Progress),
        CoreMode::ConfirmRemove { .. } => Some(Which::Remove),
        CoreMode::Group if repos.group_prompt.is_some() => Some(Which::Group),
        CoreMode::Menu if repos.menu.is_some() => Some(Which::Menu),
        CoreMode::Add => Some(Which::Add),
        CoreMode::Path if repos.prompt.as_ref().is_some_and(|prompt| folder::shown(&prompt.purpose)) => Some(Which::Folder),
        CoreMode::Service if repos.service_prompt.is_some() => Some(Which::Push),
        // The core's own folder question of a clone, which the window never asks for: Add stands for it until
        // `after` closes it, and Add is back.
        CoreMode::Path if repos.prompt.as_ref().is_some_and(|prompt| matches!(prompt.purpose, Purpose::Clone(_))) => Some(Which::Add),
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
    /// How far the remove dialog's paths, the group dialog's groups or the Move dialog's rows are moved up, in points.
    list_scroll: f32,
    /// The group the group dialog's list showed picked last, so the list follows Up and Down.
    list_shown: Option<usize>,
    /// What the Add dialog keeps of its own: the folder field's text, its suggestions, the reason a folder was
    /// refused, the row its list showed highlighted last. Made when Add opens.
    add: Option<add::State>,
    /// What the folder dialog keeps of its own: the field's text as it last stood, the folders that match it, and a
    /// move's rows. Made when it opens.
    folder: Option<folder::State>,
    /// What the Push dialog keeps of its own: the field's text as it last stood, and the core's facts for it. Made
    /// when it opens.
    push: Option<push::State>,
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
        Some(Which::Add) => {
            dialogs.add = repos.map(add::State::new);
            overlay::open(ctx, Some(add::search_id()));
        }
        Some(Which::Folder) => {
            dialogs.folder = repos.map(folder::State::new);
            overlay::open(ctx, Some(folder::field_id()));
        }
        Some(Which::Push) => {
            dialogs.push = repos.map(push::State::new);
            overlay::open(ctx, Some(push::field_id()));
        }
        None => {}
    }
}

#[cfg(test)]
thread_local! {
    /// How often this thread's dialogs asked the disk for folders.
    static DISK_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// How often this thread's dialogs asked the disk for folders so far.
#[cfg(test)]
pub(crate) fn disk_reads() -> usize {
    DISK_READS.with(std::cell::Cell::get)
}

/// The folders that match `text`, for a dialog's folder field (Add's and the folder dialog's): on disk
/// (`prompt::suggest`, what the core's `Repos::path_suggestions` gives for the same text) for a live session; the
/// demo's made-up ones ([`crate::demo::folders_matching`]) while the core holds all work, so a demo or a picture
/// never asks the disk. It reads the disk on the caller's thread: ask again only when the text changed.
fn folder_suggestions(repos: &Repos, text: &str) -> Vec<String> {
    if repos.work_held() {
        return crate::demo::folders_matching(text);
    }
    #[cfg(test)]
    DISK_READS.with(|reads| reads.set(reads.get() + 1));
    devassist_core::prompt::suggest(text)
}

/// Takes every press of `key` without modifiers from this frame's input, also its repeats while it is held; says
/// whether one was a fresh press (a held key acts once). With `text`, the text it types goes too.
fn take_press(ui: &Ui, key: Key, text: Option<&str>) -> bool {
    ui.input_mut(|input| {
        let mut fresh = false;
        input.events.retain(|event| match event {
            Event::Key { key: pressed_key, pressed: true, repeat, modifiers, .. } if *pressed_key == key && modifiers.is_none() => {
                fresh |= !*repeat;
                false
            }
            Event::Text(typed) => text != Some(typed.as_str()),
            _ => true,
        });
        fresh
    })
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
    /// The Add dialog's.
    Add(add::Places),
    /// The folder dialog's.
    Folder(folder::Places),
    /// The Push dialog's.
    Push(push::Places),
}

impl Parts {
    /// The dialog these are the parts of.
    fn which(self) -> Which {
        match self {
            Parts::Progress(_) => Which::Progress,
            Parts::Remove(_) => Which::Remove,
            Parts::Group(_) => Which::Group,
            Parts::Add(_) => Which::Add,
            Parts::Folder(_) => Which::Folder,
            Parts::Push(_) => Which::Push,
        }
    }

    /// The whole pane.
    fn pane(self) -> Rect {
        match self {
            Parts::Progress(places) => places.pane,
            Parts::Remove(places) => places.pane,
            Parts::Group(places) => places.pane,
            Parts::Add(places) => places.pane,
            Parts::Folder(places) => places.pane,
            Parts::Push(places) => places.pane,
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

    /// Where its controls are: the dialog's pane, or the menu's with its side menu's.
    fn whole(&self) -> Rect {
        match &self.0 {
            Planned::Dialog(_, parts) => parts.pane(),
            Planned::Menu(places) => places.whole(),
        }
    }

    /// The glass panes of the dialog, or of the menu and its side menu, to add to the panes baked this frame.
    #[must_use]
    pub fn wants(&self) -> Vec<Want> {
        match &self.0 {
            Planned::Dialog(_, parts) => vec![overlay::want_dialog(parts.pane())],
            Planned::Menu(places) => std::iter::once(overlay::want_menu(places.pane)).chain(places.side.map(overlay::want_submenu)).collect(),
        }
    }
}

/// Where the dialog or the menu the core's mode asks for stands in `window`, a dialog's header laid out with
/// `painter`'s faces, the menu under the row's button at `anchor` (from `repos_screen::menu_anchor`) and its side
/// menu beside it while the core's Migrate menu is open, with what
/// `dialogs` keep (the Add and folder dialogs' folder fields and their rows); `None` when the mode asks for none, or for the menu
/// while the screen has not placed its button yet.
#[must_use]
pub fn plan(painter: &Painter, window: Rect, repos: Option<&Repos>, anchor: Option<Rect>, dialogs: &Dialogs) -> Option<Plan> {
    let repos = repos?;
    match which(repos)? {
        Which::Progress => progress::plan(painter, window, repos),
        Which::Remove => Some(remove::plan(painter, window, repos)),
        Which::Group => group::plan(painter, window, repos),
        Which::Add => dialogs.add.as_ref().map(|state| add::plan(painter, window, repos, state)),
        Which::Folder => dialogs.folder.as_ref().and_then(|state| folder::plan(painter, window, repos, state)),
        Which::Push => dialogs.push.as_ref().and_then(|state| push::plan(painter, window, repos, state)),
        Which::Menu => anchor.map(|anchor| Plan(Planned::Menu(menu::plan(window, anchor, repos.migrate_highlight().is_some())))),
    }
}

/// Before the bake: brings the overlay in line with the core's mode ([`sync`]) and gives the place of the dialog
/// or the menu it asks for (the menu under `anchor`), whose glass is then baked with the other panes
/// ([`Plan::wants`]).
pub fn before(ctx: &Context, repos: Option<&Repos>, anchor: Option<Rect>, dialogs: &mut Dialogs) -> Option<Plan> {
    sync(ctx, repos, dialogs);
    plan(&ctx.layer_painter(LayerId::background()), ctx.content_rect(), repos, anchor, dialogs)
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
    if repos.mode == CoreMode::Path && matches!(repos.prompt.as_ref().map(|prompt| &prompt.purpose), Some(Purpose::Clone(_))) {
        // The core's own folder question of a clone, which the window never asks for: Add asks for the folder.
        repos.close_path();
    }
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
/// `drawn`, its pane, then its own parts (the menu paints its side menu's glass itself, in a frame it is shown); what
/// a person does in it goes to `repos` through its methods.
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
        ui.set_clip_rect(plan.whole());
        match &plan.0 {
            Planned::Dialog(header, Parts::Progress(places)) => progress::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Remove(places)) => remove::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Group(places)) => group::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Add(places)) => add::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Folder(places)) => folder::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Dialog(header, Parts::Push(places)) => push::show(ui, header, *places, repos, palette, dialogs, outside),
            Planned::Menu(places) => menu::show(ui, *places, repos, palette, outside, drawn),
        }
    });
    if which(repos) != Some(plan.which()) {
        // The dialog went: the screen gets its keys back in the next frame, at once.
        ctx.request_repaint();
    }
}

/// The box of the found repositories of the Add dialog `plan` places, if it is the Add dialog's.
#[cfg(test)]
pub(crate) fn add_found_box(plan: &Plan) -> Option<Rect> {
    match &plan.0 {
        Planned::Dialog(_, Parts::Add(places)) => Some(places.list),
        _ => None,
    }
}

/// The folder field, with the room its rows take, of the Add or the folder dialog that `plan` places.
#[cfg(test)]
pub(crate) fn folder_field_box(plan: &Plan) -> Option<Rect> {
    match &plan.0 {
        Planned::Dialog(_, Parts::Add(places)) => Some(places.folder),
        Planned::Dialog(_, Parts::Folder(places)) => Some(places.field),
        _ => None,
    }
}

/// The menu's pane and its side menu's, when `plan` places the menu.
#[cfg(test)]
pub(crate) fn menu_panes(plan: &Plan) -> Option<(Rect, Option<Rect>)> {
    match &plan.0 {
        Planned::Menu(places) => Some((places.pane, places.side)),
        Planned::Dialog(..) => None,
    }
}

/// The line under the field of the folder or the Push dialog that `plan` places, while it says why the folder or the
/// address cannot be used (the core's refusal, or a move's reason while typing) for `repos` with what `dialogs` keep.
#[cfg(test)]
pub(crate) fn reason_box(plan: &Plan, repos: &Repos, dialogs: &Dialogs) -> Option<Rect> {
    match &plan.0 {
        Planned::Dialog(_, Parts::Folder(places)) => folder::reason_box(*places, repos, dialogs.folder.as_ref()?),
        Planned::Dialog(_, Parts::Push(places)) => push::reason_box(*places, repos),
        _ => None,
    }
}

/// Gives the keyboard's focus to the Add dialog's folder field, when the Add dialog is open on `repos`; says
/// whether it did. For a picture of its suggestions (`--show add-folder`).
pub fn focus_add_folder(ctx: &Context, repos: Option<&Repos>) -> bool {
    let open = repos.and_then(which) == Some(Which::Add) && overlay::is_open(ctx);
    if open {
        ctx.memory_mut(|memory| memory.request_focus(add::folder_id()));
    }
    open
}

/// The rows of the Add dialog's found repositories in the box `found` (from [`add_found_box`]), as the dialog lays
/// them out for `repos` with the scroll `dialogs` keep: each row's whole rectangle (it may lie partly outside the
/// box) and whether its repository is installed.
#[cfg(test)]
pub(crate) fn add_rows(found: Rect, repos: &Repos, dialogs: &Dialogs) -> Vec<(Rect, bool)> {
    repos
        .add_list()
        .iter()
        .enumerate()
        .map(|(index, remote)| {
            let top = found.top() + rows(index) * crate::theme::ADD_ROW - dialogs.list_scroll;
            (Rect::from_min_size(pos2(found.left(), top), egui::vec2(found.width(), crate::theme::ADD_ROW)), repos.is_installed(remote))
        })
        .collect()
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
