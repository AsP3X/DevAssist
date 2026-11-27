//! What the dialogs' tests share: the window as the app runs it, a held run, and temporary checkouts for the tests
//! that run the real core.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use devassist_core::config::Settings;
use devassist_core::repos::state::Repos;
use devassist_core::task::job::{Job, JobEvent};
use egui::{
    Context, Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, Stroke, TextureId, TouchPhase, ViewportId,
    pos2, vec2,
};

use super::{Dialogs, Parts, Plan, Planned, Which, add, after, before, folder, group, progress, push, remove, which};
use crate::baker::Sheet;
use crate::overlay;
use crate::repos_screen::{self, Screen};
use crate::session::Session;
use crate::shell;
use crate::theme::{Mode, WINDOW};
use crate::widgets::{self, tests::context};

/// The design's window.
pub(crate) fn window() -> Rect {
    Rect::from_min_size(Pos2::ZERO, vec2(WINDOW[0], WINDOW[1]))
}

/// A held run of `names`, the first running at 3 of 4 with `lines` of output each, as the core keeps it.
pub(crate) fn run(names: &[&str], lines: usize) -> Job {
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
pub(crate) fn session_with(job: Job) -> Session {
    let mut session = Session::demo(crate::demo::repos());
    let repos = session.repos_mut().unwrap();
    repos.job = Some(job);
    repos.show_job();
    session
}

/// The demo's repositories with a held fetch of the forks of api that has said `lines` (each with whether it is a
/// progress line), its dialog open: as the core's `testing::fork_job` gives it.
pub(crate) fn fetch_session(lines: &[(String, bool)]) -> Session {
    fetch_session_of(&["api"], lines)
}

/// [`fetch_session`] for the forks of `names`, one task each, the first running.
pub(crate) fn fetch_session_of(names: &[&str], lines: &[(String, bool)]) -> Session {
    let mut session = Session::demo(crate::demo::repos());
    devassist_core::repos::state::testing::fork_job_of(session.repos_mut().unwrap(), names, &said(lines));
    session
}

/// `lines` as the core's held runs take them.
pub(crate) fn said(lines: &[(String, bool)]) -> Vec<(&str, bool)> {
    lines.iter().map(|(line, progress)| (line.as_str(), *progress)).collect()
}

fn sheet() -> Sheet {
    Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
}

/// What a test does to the session once, between a frame's bake and its draw.
pub(crate) type Between = Box<dyn FnOnce(&mut Session)>;

/// The window as the app runs it: the dialog placed before the bake, the screen, then the dialog.
pub(crate) struct Window {
    pub(crate) ctx: Context,
    pub(crate) session: Session,
    screen: Screen,
    pub(crate) dialogs: Dialogs,
    pub(crate) size: [f32; 2],
    /// The dialog's plan in the last frame, when it was drawn.
    plan: Option<Plan>,
    /// The last frame baked a dialog's glass (`before` placed one).
    pub(crate) baked: bool,
    /// Where the last frame baked the glass of the menu's side menu, when `before` placed it.
    pub(crate) baked_side: Option<Rect>,
    /// The last frame painted something on the overlay's layer.
    pub(crate) painted: bool,
    /// The last frame asked for the next one at once.
    pub(crate) repaint: bool,
    /// The filled rectangles the last frame painted, with their fills.
    pub(crate) fills: Vec<(Rect, egui::Color32)>,
    /// Where the menu stands under, in place of the row's button the screen placed: for a menu at another place.
    pub(crate) anchor: Option<Rect>,
    /// What the next frame does to the session between the bake (`before`) and the draw (`after`), once.
    pub(crate) between: Option<Between>,
    /// The texts the last frame painted, in their order: each one's words, where its galley lies, and whether it was
    /// cut short.
    pub(crate) texts: Vec<(String, Rect, bool)>,
}

impl Window {
    pub(crate) fn new(session: Session) -> Window {
        Window {
            ctx: context(),
            session,
            screen: Screen::default(),
            dialogs: Dialogs::default(),
            size: WINDOW,
            plan: None,
            baked: false,
            baked_side: None,
            painted: false,
            repaint: false,
            fills: Vec::new(),
            anchor: None,
            between: None,
            texts: Vec::new(),
        }
    }

    /// The window on `session` from now on, as a fresh one starts: no key down, the screen, the dialogs, the keyboard's focus and
    /// what the window's controls keep in egui (a folder field's rows closed by Esc, an overlay's opening) as they are
    /// in a new window, and two frames drawn, so an overlay of what was there before has closed.
    pub(crate) fn restart(&mut self, session: Session) {
        self.session = session;
        self.screen = Screen::default();
        self.dialogs = Dialogs::default();
        self.ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        self.ctx.data_mut(egui::util::IdTypeMap::clear);
        // A second later on egui's clock, with every key let go that a step left down: no click made before is part of
        // a double click with the next one, and no key is down as the new window's overlay opens.
        let now = self.ctx.input(|input| input.time);
        let down: Vec<Key> = self.ctx.input(|input| input.keys_down.iter().copied().collect());
        let released = down.into_iter().map(|key| Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE });
        self.frame_at(released.collect(), Some(now + 1.0));
        self.frame(Vec::new());
    }

    pub(crate) fn frame(&mut self, events: Vec<Event>) {
        self.frame_at(events, None);
    }

    pub(crate) fn frame_at(&mut self, events: Vec<Event>, time: Option<f64>) {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0], self.size[1]));
        let input = RawInput { screen_rect: Some(window), events, time, ..Default::default() };
        let (session, screen, dialogs) = (&mut self.session, &mut self.screen, &mut self.dialogs);
        let (anchor, mut between) = (self.anchor, self.between.take());
        let (mut plan, mut baked, mut baked_side, mut painted) = (None, false, None, false);
        let output = self.ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            let planned = before(&ctx, session.repos(), anchor.or_else(|| repos_screen::menu_anchor(screen)), dialogs);
            baked = planned.is_some();
            baked_side = planned.as_ref().and_then(|plan| plan.wants().into_iter().find(|want| want.id == overlay::SUBMENU)).map(|want| want.rect);
            let layout = shell::layout(window.size(), true, repos_screen::has_toolbar(session));
            let screen_plan = repos_screen::plan(ui.painter(), &layout, session);
            let keys = !overlay::is_open(&ctx);
            repos_screen::show(ui, &layout, screen_plan.as_ref(), session, Some(&sheet()), Mode::Light, screen, keys);
            if let Some(between) = between.take() {
                between(session);
            }
            after(&ctx, planned, session.repos_mut(), &[], Mode::Light, dialogs);
            plan = super::plan(ui.painter(), window, session.repos(), anchor.or_else(|| repos_screen::menu_anchor(screen)), dialogs);
            painted = ctx.graphics(|layers| layers.get(overlay::layer()).is_some_and(|list| !list.is_empty()));
        });
        self.repaint = output.viewport_output.get(&ViewportId::ROOT).is_some_and(|viewport| viewport.repaint_delay.is_zero());
        self.fills = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Rect(rect) => Some((rect.rect, rect.fill)),
                _ => None,
            })
            .collect();
        self.texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((text.galley.text().to_owned(), text.galley.rect.translate(text.pos.to_vec2()), text.galley.elided)),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        (self.plan, self.baked, self.baked_side, self.painted) = (plan, baked, baked_side, painted);
    }

    /// A click at `at`, over four frames.
    pub(crate) fn click(&mut self, at: Pos2) {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            self.frame(events);
        }
    }

    /// `key` pressed in one frame and let go in the next, as a tap.
    pub(crate) fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
    }

    /// `key` pressed with the `text` it types in one frame, and let go in the next.
    pub(crate) fn typed(&mut self, key: Key, text: &str) {
        self.frame(vec![
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text(text.to_owned()),
        ]);
        self.frame(vec![Event::Key { key, physical_key: None, pressed: false, repeat: false, modifiers: Modifiers::NONE }]);
    }

    pub(crate) fn repos(&self) -> &Repos {
        self.session.repos().unwrap()
    }

    pub(crate) fn job(&self) -> &Job {
        self.repos().job.as_ref().unwrap()
    }

    /// Whether a dialog is open: the core's mode asks for one and the overlay is shown.
    pub(crate) fn open(&self) -> bool {
        which(self.repos()).is_some() && overlay::is_open(&self.ctx)
    }

    /// Whether the dialog `dialog` is open.
    pub(crate) fn shows(&self, dialog: Which) -> bool {
        which(self.repos()) == Some(dialog) && overlay::is_open(&self.ctx)
    }

    /// The dialog's parts in the last frame.
    pub(super) fn parts(&self) -> Parts {
        match &self.plan.as_ref().unwrap().0 {
            Planned::Dialog(_, parts) => *parts,
            Planned::Menu(_) => panic!("the menu, not a dialog"),
        }
    }

    /// The places of the buttons in the last frame, with their words.
    pub(crate) fn buttons(&self) -> Vec<(Rect, String)> {
        let owned = |words: Vec<&str>| words.into_iter().map(str::to_owned).collect::<Vec<String>>();
        let (room, words) = match self.parts() {
            Parts::Progress(places) => (places.buttons, owned(progress::button_words(self.job()))),
            Parts::Remove(places) => (places.buttons, owned(remove::button_words())),
            Parts::Group(places) => (places.buttons, owned(group::said(self.repos()))),
            Parts::Add(places) => (places.buttons, add::said(self.repos())),
            Parts::Folder(places) => (places.buttons, folder::said(self.repos())),
            Parts::Push(places) => (places.buttons, owned(push::said(self.repos()))),
        };
        let widths: Vec<f32> = words.iter().map(|words| Window::button_width(words)).collect();
        overlay::buttons(room, &widths).into_iter().zip(words).collect()
    }

    /// The menu's pane in the last frame, when the menu was drawn.
    pub(crate) fn menu_pane(&self) -> Option<Rect> {
        match &self.plan.as_ref()?.0 {
            Planned::Menu(places) => Some(places.pane),
            Planned::Dialog(..) => None,
        }
    }

    /// The menu's side menu in the last frame, when the menu was placed with it.
    pub(crate) fn submenu_pane(&self) -> Option<Rect> {
        match &self.plan.as_ref()?.0 {
            Planned::Menu(places) => places.side,
            Planned::Dialog(..) => None,
        }
    }

    /// Where the overlay's controls were in the last frame: a dialog's pane, or the menu's with its side menu's.
    pub(crate) fn overlay_rect(&self) -> Option<Rect> {
        self.plan.as_ref().map(Plan::whole)
    }

    /// Whether the Add dialog's Retry button has the keyboard's focus.
    pub(crate) fn add_retry_focused(&self) -> bool {
        self.ctx.memory(|memory| memory.has_focus(add::retry_id()))
    }

    /// The Add dialog's Retry button in the last frame, while GitHub failed.
    pub(crate) fn add_retry(&self) -> Option<Rect> {
        match self.plan.as_ref()?.0 {
            Planned::Dialog(_, Parts::Add(places)) => places.retry,
            _ => None,
        }
    }

    /// The Add dialog's checkboxes in the last frame: one at the left of each found row in view.
    pub(crate) fn add_boxes(&self) -> Vec<Pos2> {
        let Some(Planned::Dialog(_, Parts::Add(places))) = self.plan.as_ref().map(|plan| &plan.0) else { return Vec::new() };
        let found = super::add_rows(places.list, self.repos(), &self.dialogs);
        let x = places.list.left() + crate::theme::ADD_ROW_PAD + crate::theme::CHECKBOX / 2.0;
        found.iter().map(|(row, _)| pos2(x, row.center().y)).filter(|at| places.list.contains(*at)).collect()
    }

    /// The Add dialog's folder field's text, as the dialog keeps it, while Add is open.
    pub(crate) fn add_folder_text(&self) -> Option<String> {
        self.dialogs.add.as_ref().map(|state| state.folder_text().to_owned())
    }

    /// Whether the Add dialog's folder field has the keyboard and shows its rows of folders in the last frame.
    pub(crate) fn add_folder_rows_open(&self) -> bool {
        let focused = self.ctx.memory(|memory| memory.has_focus(add::folder_id()));
        let tall = matches!(self.plan.as_ref().map(|plan| &plan.0), Some(Planned::Dialog(_, Parts::Add(places))) if places.folder.height() > crate::theme::DIALOG_FIELD_HEIGHT + 1.0);
        focused && tall
    }

    /// The window's layout, as its frames lay it out.
    pub(crate) fn layout(&self) -> shell::Layout {
        shell::layout(vec2(self.size[0], self.size[1]), true, repos_screen::has_toolbar(&self.session))
    }

    /// The row whose menu the screen keeps open.
    pub(crate) fn screen_menu(&self) -> Option<&str> {
        self.screen.menu_row()
    }

    fn button_width(words: &str) -> f32 {
        let mut width = 0.0;
        let input = RawInput { screen_rect: Some(window()), ..Default::default() };
        context().run_ui(input, |ui| width = widgets::button_width(ui, words)).drop_without_applying_deltas();
        width
    }

    pub(crate) fn press(&mut self, words: &str) {
        let (place, _) = self.buttons().into_iter().find(|(_, said)| said == words).unwrap();
        self.click(place.center());
    }

    /// The words of the dialog's buttons, from the left.
    pub(crate) fn said(&self) -> Vec<String> {
        self.buttons().into_iter().map(|(_, words)| words).collect()
    }
}

/// A turn of the wheel by `points`.
pub(crate) fn wheel(points: f32) -> Event {
    Event::MouseWheel {
        unit: MouseWheelUnit::Point,
        delta: vec2(0.0, points),
        phase: TouchPhase::Move,
        source: MouseWheelSource::Wheel,
        modifiers: Modifiers::NONE,
    }
}

pub(crate) fn git(dir: &Path, args: &[&str]) {
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

/// A temporary folder, removed when dropped: also when a test fails.
pub(crate) struct Temp(pub(crate) PathBuf);

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Checkouts `repos` under a fresh temporary folder `name`, each one commit behind its own origin. Gives the
/// folder (removed when dropped) and the settings that manage them.
pub(crate) fn behind(name: &str, repos: &[&str]) -> (Temp, Settings) {
    let root = Temp(std::env::temp_dir().join(format!("devassist-gui-{name}-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&root.0);
    std::fs::create_dir_all(&root.0).unwrap();
    let mut checkouts = Vec::new();
    for repo in repos {
        let origin = root.0.join(format!("{repo}-origin"));
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        std::fs::write(origin.join("one"), "one").unwrap();
        git(&origin, &["add", "one"]);
        git(&origin, &["commit", "-q", "-m", "one"]);
        git(&root.0, &["clone", "-q", origin.to_str().unwrap(), repo]);
        std::fs::write(origin.join("two"), "two").unwrap();
        git(&origin, &["add", "two"]);
        git(&origin, &["commit", "-q", "-m", "two"]);
        checkouts.push(root.0.join(repo));
    }
    let settings = Settings {
        owners_explicit: true,
        update_check: false,
        update_check_now: false,
        ..Settings::for_folder(checkouts, root.0.join("devassist.toml"))
    };
    (root, settings)
}

/// How long [`until`] waits before it calls a wait a hang. The work waited for is real git on temporary folders, a
/// second or two alone; but the crate's tests run in parallel, and the contrast tests keep every core busy for
/// minutes, during which starting git (and `gh` for a clone's token) can take many times as long. The budget only
/// tells a hang from slow work: what a test proves is what it checks once the wait is over.
const UNTIL_BUDGET: Duration = Duration::from_secs(180);

/// Runs frames of `window`, taking in what the core's workers sent, until `done` or [`UNTIL_BUDGET`].
pub(crate) fn until(window: &mut Window, what: &str, done: impl Fn(&Window) -> bool) {
    let deadline = Instant::now() + UNTIL_BUDGET;
    while !done(window) {
        assert!(Instant::now() < deadline, "still not {what} after {UNTIL_BUDGET:?}");
        window.session.tick();
        window.frame(Vec::new());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A window on the live repositories of `settings`, `count` of them, read and fetched. GitHub is held
/// (`testing::hold_github`): no test that uses it asks `gh auth token` or GitHub anything; the checkouts' origins are
/// folders on this disk. A test that needs GitHub asked makes its window itself.
pub(crate) fn live(settings: Settings, count: usize) -> Window {
    let mut repos = Repos::new(settings);
    devassist_core::repos::state::testing::hold_github(&mut repos);
    let mut window = Window::new(Session::Live(Box::new(repos)));
    until(&mut window, "read and fetched", |window| !window.repos().waiting() && window.repos().installed.len() == count);
    assert!(!window.open());
    window
}
