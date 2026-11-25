//! What the window has loaded: the repositories, live, kept current by taking in what the core's workers send.

use std::time::Duration;

use devassist_core::config::Settings;
use devassist_core::repos::state::Repos;

use crate::demo;
use crate::theme;

/// What the window has loaded.
pub enum Session {
    /// The settings file has a mistake: its message. Nothing is loaded, so nothing can be saved over it.
    Broken(String),
    /// The repositories, live.
    Live(Box<Repos>),
    /// The drawing's repositories (`--demo`): they are on no disk, so nothing that reads a disk or the network is
    /// ever asked of them.
    Demo(Box<Repos>),
}

impl Session {
    /// Loads the repositories of `settings`, or the drawing's twelve when `demo` (whatever `settings` is).
    ///
    /// The window does not set `DevAssist` up: the settings' first-run mark is cleared, so the core opens no
    /// settings dialog and looks up no account.
    #[must_use]
    pub fn start(settings: Result<Settings, String>, demo: bool) -> Session {
        if demo {
            return Session::Demo(Box::new(demo::repos()));
        }
        match settings {
            Err(message) => Session::Broken(message),
            Ok(mut settings) => {
                settings.first_run = false;
                Session::Live(Box::new(Repos::new(settings)))
            }
        }
    }

    /// Takes in what the core's workers sent. Returns how long the window may sleep before the next call: a short
    /// while when the core waits for something, a longer one otherwise (the core's own timers still run then).
    pub fn tick(&mut self) -> Duration {
        match self {
            Session::Broken(_) => theme::TICK_IDLE,
            Session::Live(repos) | Session::Demo(repos) => {
                repos.drain();
                if repos.waiting() { theme::TICK_BUSY } else { theme::TICK_IDLE }
            }
        }
    }

    /// Stops what runs, for closing the window.
    pub fn close(&mut self) {
        if let Session::Live(repos) | Session::Demo(repos) = self {
            repos.quit();
        }
    }

    /// Whether the repositories are the demo's.
    #[must_use]
    pub fn is_demo(&self) -> bool {
        matches!(self, Session::Demo(_))
    }

    /// Asks the core to fetch and read everything again (what `r` does in the terminal app). Does nothing for a
    /// demo, whose folders are on no disk, or for a broken settings file.
    pub fn refresh(&mut self) {
        if let Session::Live(repos) = self {
            repos.refresh();
        }
    }

    /// The repositories, when loaded.
    #[must_use]
    pub fn repos(&self) -> Option<&Repos> {
        match self {
            Session::Live(repos) | Session::Demo(repos) => Some(repos),
            Session::Broken(_) => None,
        }
    }

    /// The repositories, when loaded, for changing.
    #[must_use]
    pub fn repos_mut(&mut self) -> Option<&mut Repos> {
        match self {
            Session::Live(repos) | Session::Demo(repos) => Some(repos),
            Session::Broken(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
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

    #[test]
    fn a_broken_settings_file_loads_nothing() {
        let mut session = Session::start(Err("line 1: theme is light, dark or system".into()), false);
        assert!(matches!(&session, Session::Broken(text) if text.contains("line 1")));
        assert!(session.repos().is_none());
        assert!(session.repos_mut().is_none());
        assert_eq!(session.tick(), theme::TICK_IDLE);
        assert!(!session.is_demo());
        session.refresh();
        session.close();
    }

    #[test]
    fn a_demo_is_never_refreshed() {
        let mut session = Session::start(Err("ignored in a demo".into()), true);
        assert!(session.is_demo());
        session.refresh();
        session.tick();
        let repos = session.repos().unwrap();
        assert!(!repos.waiting());
        assert_eq!(repos.status, "12 installed. 2 are behind. 1 has new commits on other branches.");
        assert_eq!(repos.installed.len(), 12);
    }

    #[test]
    fn the_demo_is_live_and_waits_for_nothing() {
        let mut session = Session::start(Err("ignored in a demo".into()), true);
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

        let mut session = Session::start(Ok(settings), false);
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
