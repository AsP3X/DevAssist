//! Looking for a newer version of the program, installing it, and the
//! dialog that shows both.

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::Act;
use super::update::{self, Release};
use crate::atoms::task::worker::Worker;
use crate::atoms::ui::dialog::{self, centered};
use crate::atoms::ui::hits::Hits;
use crate::atoms::ui::progress;
use crate::atoms::ui::text::fit;

/// What is known about a newer version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    /// Nobody has looked yet.
    Unknown,
    Checking,
    /// The running version is the latest.
    Current,
    Available(Release),
    Installing {
        release: Release,
        done: u64,
        total: u64,
    },
    /// This version was put in place. It runs after a restart.
    Installed(String),
    Failed(String),
}

enum Msg {
    /// The answer to "is there a newer version?".
    Found(Result<Option<Release>, String>),
    Progress(u64, u64),
    Done(Result<(), String>),
}

/// What a key did in the version dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Stay,
    Close,
    /// Quit and start the program again.
    Restart,
}

pub struct Updater {
    pub state: UpdateState,
    /// A failed look at startup stays quiet; one the user asked for says why.
    quiet: bool,
    /// Finds the GitHub token to ask with, if there is one.
    token: fn() -> Option<String>,
    worker: Worker<Msg>,
}

impl Updater {
    pub fn new(token: fn() -> Option<String>) -> Self {
        Self {
            state: UpdateState::Unknown,
            quiet: false,
            token,
            worker: Worker::default(),
        }
    }

    /// Asks GitHub for a newer version, on a thread of its own.
    pub fn look(&mut self, quiet: bool) {
        if matches!(
            self.state,
            UpdateState::Checking | UpdateState::Installing { .. } | UpdateState::Installed(_)
        ) {
            return;
        }
        self.state = UpdateState::Checking;
        self.quiet = quiet;
        let token = self.token;
        self.worker.spawn(move |tx| {
            let _ = tx.send(Msg::Found(update::check(token().as_deref())));
        });
    }

    /// The dialog was opened: look, unless an update is already known or
    /// being installed.
    pub fn opened(&mut self) {
        if matches!(
            self.state,
            UpdateState::Unknown | UpdateState::Current | UpdateState::Failed(_)
        ) {
            self.look(false);
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) -> Outcome {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Outcome::Close,
            KeyCode::Enter => self.enter(),
            _ => Outcome::Stay,
        }
    }

    /// Enter, or the first button: install what is available, restart
    /// into what was installed, or look again.
    pub fn enter(&mut self) -> Outcome {
        match self.state.clone() {
            UpdateState::Available(release) => self.install(release),
            UpdateState::Installed(_) => return Outcome::Restart,
            UpdateState::Current | UpdateState::Failed(_) | UpdateState::Unknown => {
                self.look(false);
            }
            UpdateState::Checking | UpdateState::Installing { .. } => {}
        }
        Outcome::Stay
    }

    fn install(&mut self, release: Release) {
        let total = release
            .download
            .as_ref()
            .map_or(0, |download| download.size);
        self.state = UpdateState::Installing {
            release: release.clone(),
            done: 0,
            total,
        };
        let token = self.token;
        self.worker.spawn(move |tx| {
            let result = update::install(&release, token().as_deref(), |done, total| {
                let _ = tx.send(Msg::Progress(done, total));
            });
            let _ = tx.send(Msg::Done(result));
        });
    }

    /// Takes in what the threads reported. True when something changed.
    pub fn drain(&mut self) -> bool {
        let messages = self.worker.drain();
        let arrived = !messages.is_empty();
        for message in messages {
            self.on_msg(message);
        }
        arrived
    }

    fn on_msg(&mut self, message: Msg) {
        match message {
            Msg::Found(result) => {
                self.state = match result {
                    Ok(Some(release)) => UpdateState::Available(release),
                    Ok(None) => UpdateState::Current,
                    // Nobody asked: a failed look at startup is not news.
                    Err(_) if self.quiet => UpdateState::Unknown,
                    Err(err) => UpdateState::Failed(err),
                };
            }
            Msg::Progress(done, total) => {
                if let UpdateState::Installing {
                    done: shown,
                    total: of,
                    ..
                } = &mut self.state
                {
                    *shown = done;
                    *of = total;
                }
            }
            Msg::Done(result) => {
                let version = match &self.state {
                    UpdateState::Installing { release, .. } => release.version.clone(),
                    _ => String::new(),
                };
                self.state = match result {
                    Ok(()) => UpdateState::Installed(version),
                    Err(err) => UpdateState::Failed(err),
                };
            }
        }
    }

    /// True while a look or a download is under way.
    pub fn waiting(&self) -> bool {
        matches!(
            self.state,
            UpdateState::Checking | UpdateState::Installing { .. }
        )
    }

    /// What the top line says about a newer version, if there is news.
    pub fn notice(&self) -> Option<String> {
        match &self.state {
            UpdateState::Available(release) => {
                Some(format!("version {} is out, press v", release.version))
            }
            UpdateState::Installed(version) => {
                Some(format!("version {version} runs after a restart, press v"))
            }
            _ => None,
        }
    }

    pub fn draw(
        &self,
        frame: &mut Frame,
        screen: Rect,
        tick: usize,
        mouse: Option<(u16, u16)>,
        hits: &mut Hits<Act>,
    ) {
        let popup = centered(
            screen,
            72.min(screen.width.saturating_sub(2)),
            16.min(screen.height.saturating_sub(2)),
        );
        let accent = match &self.state {
            UpdateState::Failed(_) => Color::Red,
            UpdateState::Available(_) | UpdateState::Installed(_) => Color::Green,
            _ => Color::Cyan,
        };
        let inner = dialog::window(
            frame,
            popup,
            &format!(" DevAssist {} ", update::VERSION),
            accent,
        );
        let text = Rect {
            x: inner.x + 1,
            y: inner.y,
            width: inner.width.saturating_sub(2),
            height: inner.height.saturating_sub(1),
        };
        let width = text.width as usize;
        let dim = Style::default().fg(Color::DarkGray);
        let bold = Style::default().fg(accent).add_modifier(Modifier::BOLD);
        let megabytes = |bytes: u64| format!("{:.1} MB", bytes as f64 / 1_048_576.0);
        let mut lines: Vec<Line> = vec![Line::from("")];
        let buttons: Vec<(&str, Act)> = match &self.state {
            UpdateState::Unknown | UpdateState::Checking => {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", progress::spinner(tick)),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw("Looking for a newer version…"),
                ]));
                vec![("Close", Act::Close)]
            }
            UpdateState::Current => {
                lines.push(Line::from(Span::styled(
                    "✓ This is the latest version.",
                    bold,
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("enter looks again", dim)));
                vec![("Look again", Act::Confirm), ("Close", Act::Close)]
            }
            UpdateState::Available(release) => {
                lines.push(Line::from(Span::styled(
                    format!("Version {} is available.", release.version),
                    bold,
                )));
                lines.push(Line::from(""));
                // What is new, as far as there is room for it.
                let room = (text.height as usize).saturating_sub(lines.len() + 3);
                let notes: Vec<&str> = release
                    .notes
                    .lines()
                    .map(str::trim_end)
                    .filter(|line| !line.trim().is_empty())
                    .collect();
                for line in notes.iter().take(room) {
                    lines.push(Line::from(Span::styled(
                        fit(line, width),
                        Style::default().fg(Color::Gray),
                    )));
                }
                if notes.len() > room {
                    lines.push(Line::from(Span::styled("…", dim)));
                }
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    fit("enter downloads it and replaces this program.", width),
                    dim,
                )));
                vec![("Install", Act::Confirm), ("Later", Act::Close)]
            }
            UpdateState::Installing {
                release,
                done,
                total,
            } => {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("{} ", progress::spinner(tick)),
                        Style::default().fg(Color::Cyan),
                    ),
                    Span::raw(format!("Downloading version {}…", release.version)),
                ]));
                lines.push(Line::from(""));
                let fraction = (*total > 0).then(|| *done as f64 / *total as f64);
                lines.push(Line::from(progress::bar(
                    width,
                    fraction,
                    Color::Cyan,
                    tick,
                )));
                lines.push(Line::from(Span::styled(
                    format!("{} of {}", megabytes(*done), megabytes(*total)),
                    dim,
                )));
                vec![("Hide", Act::Close)]
            }
            UpdateState::Installed(version) => {
                lines.push(Line::from(Span::styled(
                    format!("✓ Version {version} is installed."),
                    bold,
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "It runs the next time DevAssist starts. enter restarts now.",
                ));
                vec![("Restart now", Act::Confirm), ("Later", Act::Close)]
            }
            UpdateState::Failed(err) => {
                lines.push(Line::from(Span::styled("✗ That did not work.", bold)));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled(
                    err.clone(),
                    Style::default().fg(Color::Red),
                )));
                lines.push(Line::from(""));
                lines.push(Line::from(Span::styled("enter tries again", dim)));
                vec![("Try again", Act::Confirm), ("Close", Act::Close)]
            }
        };
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text);
        let row = Rect {
            x: inner.x,
            y: inner.y + inner.height.saturating_sub(1),
            width: inner.width,
            height: 1,
        };
        dialog::buttons(frame, row, &buttons, Act::Confirm, mouse, hits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::crossterm::event::KeyModifiers;

    fn updater() -> Updater {
        Updater::new(|| None)
    }

    fn release(version: &str) -> Release {
        Release {
            version: version.into(),
            notes: "## What is new\n\n- Groups fold\n- Faster clones".into(),
            page: format!("https://github.com/AsP3X/DevAssist/releases/tag/v{version}"),
            download: Some(update::Asset {
                name: format!("devassist-{version}-x64.exe"),
                url: "https://api.github.com/assets/1".into(),
                size: 4_194_304,
            }),
            sums: None,
        }
    }

    fn press(updater: &mut Updater, code: KeyCode) -> Outcome {
        updater.on_key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn render(updater: &Updater) -> String {
        let (width, height) = (120, 30);
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| updater.draw(frame, frame.area(), 0, None, &mut Hits::default()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut out = String::new();
        for y in 0..height {
            for x in 0..width {
                out.push_str(buffer.cell((x, y)).unwrap().symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn a_newer_version_is_announced_and_installing_ends_in_a_restart() {
        let mut updater = updater();
        assert_eq!(updater.state, UpdateState::Unknown);
        assert_eq!(updater.notice(), None);

        // The look at startup found one: the top line says so.
        updater.state = UpdateState::Checking;
        assert!(updater.waiting());
        updater.on_msg(Msg::Found(Ok(Some(release("9.0.0")))));
        assert_eq!(updater.state, UpdateState::Available(release("9.0.0")));
        assert_eq!(
            updater.notice().as_deref(),
            Some("version 9.0.0 is out, press v")
        );

        // While it downloads, progress arrives; then it is in place.
        updater.state = UpdateState::Installing {
            release: release("9.0.0"),
            done: 0,
            total: 4_000_000,
        };
        assert!(updater.waiting());
        updater.on_msg(Msg::Progress(1_000_000, 4_000_000));
        assert!(matches!(
            updater.state,
            UpdateState::Installing {
                done: 1_000_000,
                ..
            }
        ));
        updater.on_msg(Msg::Done(Ok(())));
        assert_eq!(updater.state, UpdateState::Installed("9.0.0".into()));
        assert_eq!(
            updater.notice().as_deref(),
            Some("version 9.0.0 runs after a restart, press v")
        );

        // Enter in the version dialog restarts into it.
        assert_eq!(press(&mut updater, KeyCode::Enter), Outcome::Restart);
    }

    #[test]
    fn a_failed_look_is_quiet_at_startup_and_said_when_asked_for() {
        let mut updater = updater();
        updater.state = UpdateState::Checking;
        updater.quiet = true;
        updater.on_msg(Msg::Found(Err("could not reach GitHub".into())));
        assert_eq!(updater.state, UpdateState::Unknown);

        updater.state = UpdateState::Checking;
        updater.quiet = false;
        updater.on_msg(Msg::Found(Err("could not reach GitHub".into())));
        assert_eq!(
            updater.state,
            UpdateState::Failed("could not reach GitHub".into())
        );

        updater.state = UpdateState::Checking;
        updater.on_msg(Msg::Found(Ok(None)));
        assert_eq!(updater.state, UpdateState::Current);

        // A failed install says why and can be tried again.
        updater.state = UpdateState::Installing {
            release: release("9.0.0"),
            done: 10,
            total: 20,
        };
        updater.on_msg(Msg::Done(Err("the download stopped".into())));
        assert_eq!(
            updater.state,
            UpdateState::Failed("the download stopped".into())
        );

        // Esc leaves the dialog without doing anything.
        assert_eq!(press(&mut updater, KeyCode::Esc), Outcome::Close);
        assert_eq!(press(&mut updater, KeyCode::Char('x')), Outcome::Stay);
    }

    #[test]
    fn the_version_dialog_walks_through_an_update() {
        let mut updater = updater();
        updater.state = UpdateState::Available(release("9.0.0"));
        let screen = render(&updater);
        assert!(screen.contains(&format!("╭ DevAssist {} ", update::VERSION)));
        assert!(screen.contains("Version 9.0.0 is available."));
        assert!(screen.contains("- Groups fold"));
        assert!(screen.contains(" Install   Later "));

        updater.state = UpdateState::Installing {
            release: release("9.0.0"),
            done: 1_048_576,
            total: 4_194_304,
        };
        let screen = render(&updater);
        assert!(screen.contains("Downloading version 9.0.0…"));
        assert!(screen.contains("1.0 MB of 4.0 MB"));

        updater.state = UpdateState::Installed("9.0.0".into());
        let screen = render(&updater);
        assert!(screen.contains("✓ Version 9.0.0 is installed."));
        assert!(screen.contains(" Restart now   Later "));

        updater.state = UpdateState::Failed("DevAssist cannot write to C:\\Program Files".into());
        let screen = render(&updater);
        assert!(screen.contains("✗ That did not work."));
        assert!(screen.contains("DevAssist cannot write to"));
        assert!(screen.contains(" Try again   Close "));

        updater.state = UpdateState::Current;
        let screen = render(&updater);
        assert!(screen.contains("✓ This is the latest version."));
    }
}
