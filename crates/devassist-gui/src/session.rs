//! What the window has loaded: the repositories, live, kept current by taking in what the core's workers send.

use std::ops::{Deref, DerefMut};
use std::time::Duration;

use devassist_core::config::Settings;
use devassist_core::repos::state::Repos;

use crate::demo;
use crate::launch::Launch;
use crate::theme;

/// What the window has loaded.
pub enum Session {
    /// The settings file has a mistake: its message. Nothing is loaded, so nothing can be saved over it.
    Broken(String),
    /// The repositories, live.
    Live(Box<Repos>),
    /// The drawing's repositories (`--demo`): they are on no disk, so all their work is held in the core from the
    /// start ([`Held`]), and nothing a person asks of them is run, fetched, opened or written.
    Demo(Held),
}

/// Repositories whose work the core holds (`Repos::hold_work`): made only by [`Held::new`], which holds it, so a
/// [`Session::Demo`] can never be unheld, whoever builds it.
pub struct Held(Box<Repos>);

impl Held {
    /// `repos`, with all their work held from now on.
    #[must_use]
    pub fn new(mut repos: Repos) -> Held {
        repos.hold_work();
        Held(Box::new(repos))
    }
}

impl Deref for Held {
    type Target = Repos;

    fn deref(&self) -> &Repos {
        &self.0
    }
}

impl DerefMut for Held {
    fn deref_mut(&mut self) -> &mut Repos {
        &mut self.0
    }
}

impl Session {
    /// Loads the repositories of `settings`, or the drawing's twelve when `launch` asks for the demo (whatever
    /// `settings` is), with the overlay `--show` names open.
    ///
    /// The window does not set `DevAssist` up: the settings' first-run mark is cleared, so the core opens no
    /// settings dialog and looks up no account. A run that only takes a picture reads the repositories as they are
    /// on disk and changes none: no fetch, no pull of the watched ones, no write of the settings file.
    #[must_use]
    pub fn start(settings: Result<Settings, String>, launch: &Launch) -> Session {
        if launch.demo {
            let mut repos = demo::repos();
            if let Some(name) = &launch.show
                && let Err(message) = demo::show(&mut repos, name)
            {
                // `launch::parse` lets only known names through.
                log::warn!("{message}");
            }
            return Session::demo(repos);
        }
        match settings {
            Err(message) => Session::Broken(message),
            Ok(settings) if launch.screenshot.is_some() => Session::Live(Box::new(Repos::reading(settings))),
            Ok(mut settings) => {
                settings.first_run = false;
                Session::Live(Box::new(Repos::new(settings)))
            }
        }
    }

    /// A demo of `repos` (the drawing's, from [`demo::repos`]), its work held.
    #[must_use]
    pub fn demo(repos: Repos) -> Session {
        Session::Demo(Held::new(repos))
    }

    /// Takes in what the core's workers sent. Returns how long the window may sleep before the next call: a short
    /// while when the core waits for something, a longer one otherwise (the core's own timers still run then).
    pub fn tick(&mut self) -> Duration {
        match self.repos_mut() {
            None => theme::TICK_IDLE,
            Some(repos) => {
                repos.drain();
                if repos.waiting() { theme::TICK_BUSY } else { theme::TICK_IDLE }
            }
        }
    }

    /// Stops what runs, for closing the window.
    pub fn close(&mut self) {
        if let Some(repos) = self.repos_mut() {
            repos.quit();
        }
    }

    /// Whether the repositories are the demo's.
    #[must_use]
    pub fn is_demo(&self) -> bool {
        matches!(self, Session::Demo(_))
    }

    /// Asks the core to fetch and read everything again (what `r` does in the terminal app). Does nothing for a
    /// broken settings file; a demo's core holds it and says so.
    pub fn refresh(&mut self) {
        if let Some(repos) = self.repos_mut() {
            repos.refresh();
        }
    }

    /// The repositories, when loaded.
    #[must_use]
    pub fn repos(&self) -> Option<&Repos> {
        match self {
            Session::Live(repos) => Some(repos),
            Session::Demo(repos) => Some(repos),
            Session::Broken(_) => None,
        }
    }

    /// The repositories, when loaded, for changing.
    #[must_use]
    pub fn repos_mut(&mut self) -> Option<&mut Repos> {
        match self {
            Session::Live(repos) => Some(repos),
            Session::Demo(repos) => Some(repos),
            Session::Broken(_) => None,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::Path;
    use std::process::Command;
    use std::time::Instant;

    use super::*;

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

    /// Held by every test of this crate that runs the core on real checkouts. Cancelling a run stops every git
    /// the process runs and raises one flag for all of it (`process::request_cancel`), so such tests take turns.
    pub(crate) fn live_git() -> std::sync::MutexGuard<'static, ()> {
        static LIVE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LIVE.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn demo() -> Launch {
        Launch { demo: true, ..Launch::default() }
    }

    #[test]
    fn a_demo_session_is_held() {
        let mut session = Session::start(Err("ignored in a demo".into()), &demo());
        assert!(session.is_demo());
        assert!(session.repos().unwrap().work_held(), "the demo's core holds all work from the start");
        // A fold works and is written nowhere.
        let path = session.repos().unwrap().config_path.clone();
        session.repos_mut().unwrap().toggle_group("home");
        assert_eq!(session.repos().unwrap().collapsed, ["home"]);
        assert!(!path.exists(), "the held demo wrote its settings");
        // `r` and F5 reach the core, which runs nothing and says so.
        session.refresh();
        session.tick();
        let repos = session.repos().unwrap();
        assert_eq!(repos.status, devassist_core::repos::state::HELD);
        assert!(!repos.waiting() && !repos.fetching && !repos.busy && repos.queue.is_empty());
        session.close();

        // The session holds its demo itself, whatever repositories it is built from.
        let unheld = Repos::offline(devassist_core::repos::state::testing::test_settings());
        assert!(!unheld.work_held());
        let session = Session::demo(unheld);
        assert!(session.is_demo() && session.repos().unwrap().work_held());
        assert!(Held::new(Repos::offline(devassist_core::repos::state::testing::test_settings())).work_held());
    }

    #[test]
    fn a_picture_run_reads_the_repositories_and_fetches_and_pulls_nothing() {
        let _live = live_git();
        let root = std::env::temp_dir().join(format!("devassist-gui-picture-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let origin = root.join("origin");
        std::fs::create_dir_all(&origin).unwrap();
        git(&origin, &["init", "-q", "-b", "main"]);
        std::fs::write(origin.join("one"), "one").unwrap();
        git(&origin, &["add", "one"]);
        git(&origin, &["commit", "-q", "-m", "one"]);
        git(&root, &["clone", "-q", origin.to_str().unwrap(), "tool"]);
        let clone = root.join("tool");
        // The origin moves on: a fetch or the watch's pull would bring "two".
        std::fs::write(origin.join("two"), "two").unwrap();
        git(&origin, &["add", "two"]);
        git(&origin, &["commit", "-q", "-m", "two"]);
        let settings = Settings {
            owners_explicit: true,
            update_check: false,
            update_check_now: false,
            watched: vec![clone.clone()],
            ..Settings::for_folder(vec![clone.clone()], root.join("devassist.toml"))
        };
        let picture = Launch { screenshot: Some(root.join("shot.png")), ..Launch::default() };

        let mut session = Session::start(Ok(settings), &picture);
        let deadline = Instant::now() + Duration::from_secs(20);
        while session.repos().is_some_and(Repos::waiting) {
            assert!(!session.repos().unwrap().fetching, "a picture run fetches nothing");
            assert!(Instant::now() < deadline, "the core was still waiting after 20 seconds");
            session.tick();
            std::thread::sleep(Duration::from_millis(20));
        }
        for _ in 0..10 {
            session.tick();
            let repos = session.repos().unwrap();
            assert!(!repos.fetching && !repos.busy, "nothing starts after the read");
        }
        assert_eq!(session.repos().unwrap().installed.len(), 1);
        assert!(!clone.join("two").exists(), "the watched repository was not pulled");
        let fetched = Command::new("git").arg("-C").arg(&clone).args(["rev-list", "--count", "origin/main"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&fetched.stdout).trim(), "1", "origin/main was not fetched");
        session.close();
        drop(session);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_settings_file_loads_nothing() {
        let mut session = Session::start(Err("line 1: theme is light, dark or system".into()), &Launch::default());
        assert!(matches!(&session, Session::Broken(text) if text.contains("line 1")));
        assert!(session.repos().is_none());
        assert!(session.repos_mut().is_none());
        assert_eq!(session.tick(), theme::TICK_IDLE);
        assert!(!session.is_demo());
        session.refresh();
        session.close();
    }

    #[test]
    fn a_demo_shown_with_its_progress_dialog_ticks_idle() {
        let launch = Launch { show: Some("progress".to_owned()), ..demo() };
        let mut session = Session::start(Err("ignored in a demo".into()), &launch);
        assert!(session.repos().unwrap().mode == devassist_core::repos::state::Mode::Progress);
        for _ in 0..3 {
            assert_eq!(session.tick(), theme::TICK_IDLE, "the demo's run makes the window repaint");
        }
        let repos = session.repos().unwrap();
        assert!(repos.work_held() && !repos.busy && repos.job.as_ref().is_some_and(devassist_core::task::job::Job::running));
        assert!(repos.mode == devassist_core::repos::state::Mode::Progress, "nothing closes the demo's dialog by itself");
    }

    #[test]
    fn the_demo_is_live_and_waits_for_nothing() {
        let mut session = Session::start(Err("ignored in a demo".into()), &demo());
        let repos = session.repos().unwrap();
        assert_eq!(repos.installed.len(), 12);
        assert_eq!(repos.groups(), ["home", "Tools", "Work"]); // as the core orders them; three more are in no group
        assert!(!repos.waiting());
        assert_eq!(session.tick(), theme::TICK_IDLE);
        // The demo's watched repository is not pulled: nothing started, nothing changed.
        let repos = session.repos().unwrap();
        assert!(!repos.waiting());
        assert_eq!(repos.status, "12 installed. 2 are behind. 1 has new commits on other branches.");
    }

    #[test]
    fn a_live_session_ticks_fast_while_the_core_waits() {
        let _live = live_git();
        let root = std::env::temp_dir().join(format!("devassist-gui-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main", "demo"]);
        let checkout = root.join("demo");
        std::fs::write(checkout.join("one"), "one").unwrap();
        git(&checkout, &["add", "one"]);
        git(&checkout, &["commit", "-q", "-m", "one"]);
        // No account and no remote: the core asks the network nothing.
        let settings = Settings {
            owners_explicit: true,
            update_check: false,
            update_check_now: false,
            ..Settings::for_folder(vec![checkout], root.join("devassist.toml"))
        };

        let mut session = Session::start(Ok(settings), &Launch::default());
        assert!(session.repos().is_some_and(Repos::waiting), "the read of the checkout is pending at the start");
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut ticks = Vec::new();
        loop {
            let slept = session.tick();
            ticks.push(slept);
            let waiting = session.repos().is_some_and(Repos::waiting);
            assert_eq!(slept == theme::TICK_BUSY, waiting, "a busy tick is exactly one taken while the core waits");
            if !waiting {
                break;
            }
            assert!(Instant::now() < deadline, "the core was still waiting after 20 seconds");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(ticks.first(), Some(&theme::TICK_BUSY));
        assert_eq!(session.tick(), theme::TICK_IDLE);
        assert_eq!(session.repos().unwrap().installed.len(), 1);
        assert_eq!(session.repos().unwrap().installed[0].branch, "main");

        session.close();
        drop(session);
        let _ = std::fs::remove_dir_all(&root);
    }
}
