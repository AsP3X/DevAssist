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

use super::{Dialogs, Parts, Plan, Planned, Which, add, after, before, group, progress, remove, which};
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

fn sheet() -> Sheet {
    Sheet { texture: TextureId::User(0), uv: Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), edge: Stroke::NONE }
}

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
    /// The last frame painted something on the overlay's layer.
    pub(crate) painted: bool,
    /// The last frame asked for the next one at once.
    pub(crate) repaint: bool,
    /// The filled rectangles the last frame painted, with their fills.
    pub(crate) fills: Vec<(Rect, egui::Color32)>,
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
            painted: false,
            repaint: false,
            fills: Vec::new(),
        }
    }

    /// The window on `session` from now on, as a fresh one starts: the screen, the dialogs and the keyboard's focus
    /// as they are in a new window, and two frames drawn, so an overlay of what was there before has closed.
    pub(crate) fn restart(&mut self, session: Session) {
        self.session = session;
        self.screen = Screen::default();
        self.dialogs = Dialogs::default();
        self.ctx.memory_mut(|memory| {
            if let Some(focused) = memory.focused() {
                memory.surrender_focus(focused);
            }
        });
        self.frame(Vec::new());
        self.frame(Vec::new());
    }

    pub(crate) fn frame(&mut self, events: Vec<Event>) {
        self.frame_at(events, None);
    }

    pub(crate) fn frame_at(&mut self, events: Vec<Event>, time: Option<f64>) {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(self.size[0], self.size[1]));
        let input = RawInput { screen_rect: Some(window), events, time, ..Default::default() };
        let (session, screen, dialogs) = (&mut self.session, &mut self.screen, &mut self.dialogs);
        let (mut plan, mut baked, mut painted) = (None, false, false);
        let output = self.ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            let planned = before(&ctx, session.repos(), repos_screen::menu_anchor(screen), dialogs);
            baked = planned.is_some();
            let layout = shell::layout(window.size(), true, repos_screen::has_toolbar(session));
            let screen_plan = repos_screen::plan(ui.painter(), &layout, session);
            let keys = !overlay::is_open(&ctx);
            repos_screen::show(ui, &layout, screen_plan.as_ref(), session, Some(&sheet()), Mode::Light, screen, keys);
            after(&ctx, planned, session.repos_mut(), &[], Mode::Light, dialogs);
            plan = super::plan(ui.painter(), window, session.repos(), repos_screen::menu_anchor(screen), dialogs);
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
        output.drop_without_applying_deltas();
        (self.plan, self.baked, self.painted) = (plan, baked, painted);
    }

    /// A click at `at`, over four frames.
    pub(crate) fn click(&mut self, at: Pos2) {
        let button = |pressed| Event::PointerButton { pos: at, button: PointerButton::Primary, pressed, modifiers: Modifiers::NONE };
        for events in [vec![Event::PointerMoved(at)], vec![button(true)], vec![button(false)], Vec::new()] {
            self.frame(events);
        }
    }

    pub(crate) fn key(&mut self, key: Key) {
        self.frame(vec![Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE }]);
        self.frame(Vec::new());
    }

    pub(crate) fn typed(&mut self, key: Key, text: &str) {
        self.frame(vec![
            Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE },
            Event::Text(text.to_owned()),
        ]);
        self.frame(Vec::new());
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
    pub(crate) fn buttons(&self) -> Vec<(Rect, &'static str)> {
        let (room, words) = match self.parts() {
            Parts::Progress(places) => (places.buttons, progress::button_words(self.job())),
            Parts::Remove(places) => (places.buttons, remove::button_words()),
            Parts::Group(places) => (places.buttons, group::said(self.repos())),
            // The main button's words are made each frame; a test's copy of them lives as long as the test.
            Parts::Add(places) => (places.buttons, add::said(self.repos()).into_iter().map(|words| &*words.leak()).collect()),
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
        let (place, _) = self.buttons().into_iter().find(|(_, said)| *said == words).unwrap();
        self.click(place.center());
    }

    /// The words of the dialog's buttons, from the left.
    pub(crate) fn said(&self) -> Vec<&'static str> {
        self.buttons().iter().map(|(_, words)| *words).collect()
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

/// Runs frames of `window`, taking in what the core's workers sent, until `done` or 30 seconds.
pub(crate) fn until(window: &mut Window, what: &str, done: impl Fn(&Window) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(30);
    while !done(window) {
        assert!(Instant::now() < deadline, "still not {what} after 30 seconds");
        window.session.tick();
        window.frame(Vec::new());
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// A window on the live repositories of `settings`, `count` of them, read and fetched.
pub(crate) fn live(settings: Settings, count: usize) -> Window {
    let mut window = Window::new(Session::Live(Box::new(Repos::new(settings))));
    until(&mut window, "read and fetched", |window| !window.repos().waiting() && window.repos().installed.len() == count);
    assert!(!window.open());
    window
}
