//! One update or clone run, as the progress dialog shows it: the
//! repositories it works through, what git said about each, and how the
//! run ended.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 500;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemState {
    Waiting,
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug)]
pub struct Item {
    pub name: String,
    pub state: ItemState,
    /// How it ended: "cloned", "up to date", or the error.
    pub note: String,
    /// Git's output for this repository.
    pub output: VecDeque<String>,
    /// Git's current progress line, such as "Receiving objects:  45% ...".
    pub progress: String,
    /// For an update: branches done, out of the branches that have
    /// something to bring in. None for a clone, and until it is known.
    pub steps: Option<(u32, u32)>,
    /// How far git's download is overall, from 0 to 1, across all of its
    /// phases. None until git reports something.
    reached: Option<f64>,
    started: Option<Instant>,
    ended: Option<Instant>,
}

impl Item {
    fn new(name: String) -> Self {
        Self {
            name,
            state: ItemState::Waiting,
            note: String::new(),
            output: VecDeque::new(),
            progress: String::new(),
            steps: None,
            reached: None,
            started: None,
            ended: None,
        }
    }

    /// How far git's clone or fetch is, in percent. This covers the whole
    /// transfer, not the phase git happens to be counting right now.
    pub fn percent(&self) -> Option<u16> {
        self.reached
            .map(|reached| (reached * 100.0).round().clamp(0.0, 100.0) as u16)
    }

    /// Takes in a line git printed. Git counts each phase from 0 to 100 on
    /// its own; the phases are joined into one figure that only goes up.
    fn note_progress(&mut self, line: &str, progress: bool) {
        if let Some(overall) = overall_progress(line) {
            self.reached = Some(self.reached.map_or(overall, |before| before.max(overall)));
        } else if progress && let Some(percent) = parse_percent(line) {
            // A phase this app does not know, or git in another language:
            // show its own percentage.
            self.reached = Some(f64::from(percent) / 100.0);
        }
    }

    /// How far this repository is, from 0 to 1: its branches when they are
    /// counted, else git's percentage.
    pub fn fraction(&self) -> Option<f64> {
        match self.steps {
            Some((done, total)) if total > 0 => Some(f64::from(done) / f64::from(total)),
            _ => self.reached,
        }
    }

    /// "2 of 3 branches updated", or "nothing new".
    pub fn steps_label(&self) -> Option<String> {
        let (done, total) = self.steps?;
        Some(match total {
            0 => "nothing new".to_string(),
            1 => format!("{done} of 1 branch updated"),
            _ => format!("{done} of {total} branches updated"),
        })
    }

    pub fn elapsed(&self) -> Option<Duration> {
        let started = self.started?;
        Some(
            self.ended
                .unwrap_or_else(Instant::now)
                .duration_since(started),
        )
    }

    fn end(&mut self, state: ItemState, note: String) {
        self.state = state;
        self.note = note;
        self.progress.clear();
        self.started.get_or_insert_with(Instant::now);
        self.ended = Some(Instant::now());
    }
}

/// What a worker thread reports while it runs.
#[derive(Debug)]
pub enum JobEvent {
    Start(usize),
    Done(usize, String),
    Failed(usize, String),
    /// A line of git output. `true` marks a progress line that replaces the
    /// previous one instead of adding to the output.
    Line(String, bool),
    /// Branches done, out of the branches with something to bring in, for
    /// the repository that is running.
    Steps(u32, u32),
}

pub struct Job {
    pub title: String,
    pub items: Vec<Item>,
    /// Repositories left out because they were already up to date.
    pub skipped: usize,
    pub result: Option<Result<String, String>>,
    pub cancelling: bool,
    pub cancelled: bool,
    /// The repository the user chose to look at. None follows the run.
    pub picked: Option<usize>,
    /// Output lines scrolled back from the newest one.
    pub scroll: usize,
    /// Output rows the dialog showed last, so scrolling knows where to stop.
    pub view: usize,
    /// The repository git output currently belongs to.
    active: Option<usize>,
    started: Instant,
    ended: Option<Instant>,
}

impl Job {
    pub fn new(title: impl Into<String>, names: Vec<String>, skipped: usize) -> Self {
        Self {
            title: title.into(),
            items: names.into_iter().map(Item::new).collect(),
            skipped,
            result: None,
            cancelling: false,
            cancelled: false,
            picked: None,
            scroll: 0,
            view: 0,
            active: None,
            started: Instant::now(),
            ended: None,
        }
    }

    pub fn running(&self) -> bool {
        self.result.is_none()
    }

    pub fn on(&mut self, event: JobEvent) {
        if !self.running() {
            return;
        }
        match event {
            JobEvent::Start(index) => {
                if let Some(item) = self.items.get_mut(index) {
                    item.state = ItemState::Running;
                    item.progress.clear();
                    item.steps = None;
                    item.reached = None;
                    item.started = Some(Instant::now());
                    self.active = Some(index);
                    if self.picked.is_none() {
                        self.scroll = 0;
                    }
                }
            }
            JobEvent::Done(index, note) => {
                if let Some(item) = self.items.get_mut(index) {
                    item.end(ItemState::Done, note);
                }
            }
            JobEvent::Failed(index, note) => {
                if let Some(item) = self.items.get_mut(index) {
                    item.end(ItemState::Failed, first_line(&note));
                    self.active = Some(index);
                }
            }
            JobEvent::Steps(done, total) => {
                if let Some(item) = self.active.and_then(|index| self.items.get_mut(index)) {
                    item.steps = Some((done.min(total), total));
                }
            }
            JobEvent::Line(text, progress) => {
                let index = self.active.unwrap_or(0);
                let shown = self.shown() == index;
                let Some(item) = self.items.get_mut(index) else {
                    return;
                };
                item.note_progress(&text, progress);
                if progress {
                    item.progress = text;
                    return;
                }
                item.output.push_back(text);
                while item.output.len() > OUTPUT_LIMIT {
                    item.output.pop_front();
                }
                // Keep a scrolled-back view on the same lines.
                if shown && self.scroll > 0 {
                    self.scroll += 1;
                }
            }
        }
    }

    /// Ends the run. Whatever was still running shares the run's fate.
    pub fn finish(&mut self, result: Result<String, String>, cancelled: bool) {
        if !self.running() {
            return;
        }
        for item in &mut self.items {
            match item.state {
                ItemState::Running => match &result {
                    Ok(_) => item.end(ItemState::Done, String::new()),
                    Err(_) if cancelled => item.end(ItemState::Failed, "cancelled".to_string()),
                    Err(err) => item.end(ItemState::Failed, first_line(err)),
                },
                ItemState::Waiting if result.is_err() => item.note = "not started".to_string(),
                _ => {}
            }
        }
        self.cancelling = false;
        self.cancelled = cancelled;
        self.ended = Some(Instant::now());
        self.result = Some(result);
    }

    /// The repository git is working on.
    pub fn current(&self) -> Option<&Item> {
        self.items
            .iter()
            .find(|item| item.state == ItemState::Running)
    }

    /// The repository whose details the dialog shows: the picked one, else
    /// the one that is running, else the first failure, else the last one
    /// that ran.
    pub fn shown(&self) -> usize {
        let last = self.items.len().saturating_sub(1);
        if let Some(index) = self.picked {
            return index.min(last);
        }
        let find = |state| self.items.iter().position(|item| item.state == state);
        find(ItemState::Running)
            .or_else(|| find(ItemState::Failed).filter(|_| !self.running()))
            .or(self.active)
            .unwrap_or(0)
            .min(last)
    }

    /// Moves the shown repository up or down and keeps it there.
    pub fn pick(&mut self, delta: isize) {
        let last = self.items.len().saturating_sub(1) as isize;
        self.pick_index((self.shown() as isize + delta).clamp(0, last) as usize);
    }

    pub fn pick_index(&mut self, index: usize) {
        if index < self.items.len() {
            if index != self.shown() {
                self.scroll = 0;
            }
            self.picked = Some(index);
        }
    }

    pub fn count(&self, state: ItemState) -> usize {
        self.items.iter().filter(|item| item.state == state).count()
    }

    /// Items that are done or failed.
    pub fn settled(&self) -> usize {
        self.count(ItemState::Done) + self.count(ItemState::Failed)
    }

    /// How far the whole run is, from 0 to 1. None while a single
    /// repository reports no percentage.
    pub fn fraction(&self) -> Option<f64> {
        if !self.running() {
            return Some(1.0);
        }
        let total = self.items.len().max(1) as f64;
        let part = self.current().and_then(Item::fraction);
        if self.items.len() <= 1 && part.is_none() {
            return None;
        }
        Some(((self.settled() as f64 + part.unwrap_or(0.0)) / total).clamp(0.0, 1.0))
    }

    pub fn elapsed(&self) -> Duration {
        self.ended
            .unwrap_or_else(Instant::now)
            .duration_since(self.started)
    }

    /// Scrolls the shown repository's output.
    pub fn scroll_by(&mut self, delta: isize) {
        let lines = self
            .items
            .get(self.shown())
            .map_or(0, |item| item.output.len());
        let max = lines.saturating_sub(self.view.max(1));
        self.scroll = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
    }
}

pub fn parse_percent(line: &str) -> Option<u16> {
    let end = line.find('%')?;
    let digits: String = line[..end]
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .collect();
    let digits: String = digits.chars().rev().collect();
    digits.parse::<u16>().ok().filter(|value| *value <= 100)
}

/// The share of a clone or fetch each of git's phases takes, as the range
/// it covers from 0 to 1. Receiving is the download and takes most of the
/// time; resolving deltas and writing the files come after it.
const PHASES: [(&str, f64, f64); 6] = [
    ("Counting objects", 0.0, 0.04),
    ("Compressing objects", 0.04, 0.08),
    ("Receiving objects", 0.08, 0.75),
    ("Resolving deltas", 0.75, 0.96),
    ("Updating files", 0.96, 1.0),
    ("Checking out files", 0.96, 1.0),
];

/// How far the whole transfer is, from 0 to 1, judged by one progress line.
/// None for a line without a percentage or with a phase that is not known.
pub fn overall_progress(line: &str) -> Option<f64> {
    let percent = f64::from(parse_percent(line)?) / 100.0;
    let (phase, _) = split_progress(line);
    PHASES
        .iter()
        .find(|(name, _, _)| *name == phase)
        .map(|(_, from, to)| from + (to - from) * percent)
}

/// Splits git's progress line into what it is doing and the numbers after
/// the percentage: "Receiving objects:  45% (120/266), 1.2 MiB | 2 MiB/s"
/// becomes ("Receiving objects", "120/266, 1.2 MiB · 2 MiB/s").
pub fn split_progress(line: &str) -> (String, String) {
    let line = line.trim();
    let line = line.strip_prefix("remote: ").unwrap_or(line);
    let Some(percent) = line.find('%') else {
        return (line.trim_end_matches(['.', ':']).to_string(), String::new());
    };
    let phase = line[..percent]
        .trim_end_matches(|ch: char| ch.is_ascii_digit() || ch.is_whitespace())
        .trim_end_matches(':')
        .to_string();
    let rest = line[percent + 1..]
        .replace(['(', ')'], "")
        .replace(" | ", " · ")
        .trim()
        .trim_end_matches(", done.")
        .to_string();
    (phase, rest)
}

pub fn clock(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else {
        format!("{}m {:02}s", secs / 60, secs % 60)
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job(names: &[&str]) -> Job {
        Job::new(
            "Cloning",
            names.iter().map(|name| (*name).to_string()).collect(),
            0,
        )
    }

    #[test]
    fn a_clone_counts_every_phase_and_never_goes_back() {
        let mut job = job(&["tool"]);
        job.on(JobEvent::Start(0));
        assert_eq!(job.items[0].percent(), None);
        let mut seen = Vec::new();
        for (line, progress) in [
            ("Cloning into 'tool'...", false),
            ("remote: Enumerating objects: 266, done.", false),
            ("remote: Counting objects:  50% (133/266)", true),
            ("remote: Counting objects: 100% (266/266), done.", false),
            ("remote: Compressing objects: 100% (120/120), done.", false),
            ("Receiving objects:   0% (1/266)", true),
            (
                "Receiving objects:  50% (133/266), 1.20 MiB | 2.00 MiB/s",
                true,
            ),
            (
                "Receiving objects: 100% (266/266), 2.40 MiB | 2.00 MiB/s, done.",
                false,
            ),
            // The count starts over, the bar must not.
            ("Resolving deltas:   0% (0/90)", true),
            ("Resolving deltas:  50% (45/90)", true),
            ("Resolving deltas: 100% (90/90), done.", false),
            ("Updating files: 100% (40/40), done.", false),
        ] {
            job.on(JobEvent::Line(line.into(), progress));
            seen.push(job.items[0].percent());
        }
        assert_eq!(
            seen,
            [
                None,
                None,
                Some(2),
                Some(4),
                Some(8),
                Some(8),
                Some(42),
                Some(75),
                Some(75),
                Some(86),
                Some(96),
                Some(100),
            ]
        );
        assert_eq!(job.fraction(), Some(1.0));
        // The line under the bar still says what git is doing right now.
        assert_eq!(job.items[0].progress, "Resolving deltas:  50% (45/90)");

        // A late line of an earlier phase does not pull the bar back.
        let mut job = super::Job::new("Cloning", vec!["tool".into()], 0);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Line(
            "Resolving deltas:  50% (45/90)".into(),
            true,
        ));
        job.on(JobEvent::Line(
            "Receiving objects: 100% (266/266), done.".into(),
            false,
        ));
        assert_eq!(job.items[0].percent(), Some(86));
    }

    #[test]
    fn progress_is_joined_across_phases() {
        assert_eq!(overall_progress("Cloning into 'tool'..."), None);
        assert_eq!(
            overall_progress("Receiving objects:   0% (0/9)"),
            Some(0.08)
        );
        assert_eq!(overall_progress("Resolving deltas:   0% (0/9)"), Some(0.75));
        assert_eq!(
            overall_progress("Resolving deltas: 100% (9/9), done."),
            Some(0.96)
        );
        assert_eq!(
            overall_progress("remote: Compressing objects: 100% (9/9), done."),
            Some(0.08)
        );
        assert_eq!(
            overall_progress("Updating files: 100% (9/9), done."),
            Some(1.0)
        );
        assert_eq!(overall_progress("Something else:  40% (4/10)"), None);

        // An unknown phase shows its own percentage while it is counting.
        let mut job = job(&["tool"]);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Line("Something else:  40% (4/10)".into(), true));
        assert_eq!(job.items[0].percent(), Some(40));
        // Output that merely contains a percentage is not progress.
        job.on(JobEvent::Line("similarity index 90%".into(), false));
        assert_eq!(job.items[0].percent(), Some(40));
    }

    #[test]
    fn steps_count_the_branches_of_the_running_repository() {
        let mut job = job(&["a/one", "a/two"]);
        job.on(JobEvent::Steps(1, 2));
        assert_eq!(job.items[0].steps, None);

        job.on(JobEvent::Start(0));
        job.on(JobEvent::Steps(0, 3));
        assert_eq!(job.items[0].fraction(), Some(0.0));
        job.on(JobEvent::Line(
            "Receiving objects:  80% (8/10)".into(),
            true,
        ));
        // Branches count, not what git is downloading.
        assert_eq!(job.items[0].fraction(), Some(0.0));
        job.on(JobEvent::Steps(2, 3));
        assert_eq!(
            job.items[0].steps_label().unwrap(),
            "2 of 3 branches updated"
        );
        assert!((job.fraction().unwrap() - 1.0 / 3.0).abs() < 1e-9);

        // The count stays once the repository is done.
        job.on(JobEvent::Done(0, "updated".into()));
        assert_eq!(job.items[0].steps, Some((2, 3)));

        job.on(JobEvent::Start(1));
        assert_eq!(job.items[1].steps, None);
        assert_eq!(job.items[1].steps_label(), None);
        job.on(JobEvent::Steps(0, 0));
        assert_eq!(job.items[1].steps_label().unwrap(), "nothing new");
        assert_eq!(job.items[1].fraction(), None);
        job.on(JobEvent::Steps(0, 1));
        assert_eq!(job.items[1].steps_label().unwrap(), "0 of 1 branch updated");
    }

    #[test]
    fn reads_the_percentage_from_git_progress() {
        assert_eq!(
            parse_percent("Receiving objects:  45% (120/266), 1.20 MiB | 2.00 MiB/s"),
            Some(45)
        );
        assert_eq!(
            parse_percent("remote: Counting objects: 100% (266/266), done."),
            Some(100)
        );
        assert_eq!(parse_percent("Cloning into 'tool'..."), None);
        assert_eq!(parse_percent("%"), None);
    }

    #[test]
    fn splits_a_progress_line_into_phase_and_numbers() {
        assert_eq!(
            split_progress("Receiving objects:  45% (120/266), 1.20 MiB | 2.00 MiB/s"),
            (
                "Receiving objects".to_string(),
                "120/266, 1.20 MiB · 2.00 MiB/s".to_string()
            )
        );
        assert_eq!(
            split_progress("remote: Counting objects: 100% (266/266), done."),
            ("Counting objects".to_string(), "266/266".to_string())
        );
        assert_eq!(
            split_progress("Updating files..."),
            ("Updating files".to_string(), String::new())
        );
    }

    #[test]
    fn a_batch_keeps_output_and_progress_per_repository() {
        let mut job = job(&["a/one", "a/two"]);
        assert_eq!(job.fraction(), Some(0.0));
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Line("Receiving objects:  50% (1/2)".into(), true));
        // Half of the download is not half of the clone: deltas come after.
        assert_eq!(job.current().unwrap().percent(), Some(42));
        assert!((job.fraction().unwrap() - 0.415 / 2.0).abs() < 1e-9);
        job.on(JobEvent::Line("Cloning into 'one'...".into(), false));
        job.on(JobEvent::Done(0, "cloned".into()));
        job.on(JobEvent::Start(1));
        job.on(JobEvent::Line("Cloning into 'two'...".into(), false));
        job.on(JobEvent::Line("fatal: not found".into(), false));
        job.on(JobEvent::Failed(1, "fatal: not found\nmore".into()));
        assert_eq!(job.items[0].output, ["Cloning into 'one'..."]);
        assert_eq!(job.items[1].output.len(), 2);
        assert!(job.items[0].progress.is_empty());
        assert_eq!(job.settled(), 2);
        assert_eq!(job.items[1].note, "fatal: not found");
        job.finish(Err("Cloned 1, 1 failed: a/two".into()), false);
        assert!(!job.running());
        assert_eq!(job.fraction(), Some(1.0));
        assert_eq!(job.items[0].state, ItemState::Done);
    }

    #[test]
    fn the_dialog_follows_the_run_until_a_repository_is_picked() {
        let mut job = job(&["one", "two", "three"]);
        assert_eq!(job.shown(), 0);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Done(0, "cloned".into()));
        assert_eq!(job.shown(), 0);
        job.on(JobEvent::Start(1));
        assert_eq!(job.shown(), 1);
        job.pick(-1);
        assert_eq!(job.shown(), 0);
        job.on(JobEvent::Failed(1, "fatal".into()));
        job.on(JobEvent::Start(2));
        assert_eq!(job.shown(), 0);
        job.pick(5);
        assert_eq!(job.shown(), 2);
        job.picked = None;
        job.finish(Ok("done".into()), false);
        assert_eq!(job.shown(), 1, "a finished run shows its first failure");
    }

    #[test]
    fn one_repository_without_a_percentage_has_no_fraction() {
        let mut job = job(&["tool"]);
        job.on(JobEvent::Start(0));
        assert_eq!(job.fraction(), None);
        job.on(JobEvent::Line("Already up to date.".into(), false));
        assert_eq!(job.items[0].output[0], "Already up to date.");
        job.finish(Ok("Updated tool.".into()), false);
        assert_eq!(job.items[0].state, ItemState::Done);
        assert!(job.items[0].elapsed().is_some());
    }

    #[test]
    fn cancelling_marks_the_running_and_the_waiting_repositories() {
        let mut job = job(&["one", "two"]);
        job.on(JobEvent::Start(0));
        job.finish(Err("cancelled".into()), true);
        assert!(job.cancelled);
        assert_eq!(job.items[0].state, ItemState::Failed);
        assert_eq!(job.items[0].note, "cancelled");
        assert_eq!(job.items[1].state, ItemState::Waiting);
        assert_eq!(job.items[1].note, "not started");
        job.on(JobEvent::Start(1));
        assert_eq!(job.items[1].state, ItemState::Waiting);
    }

    #[test]
    fn scrolling_stops_at_the_oldest_line_and_resets_on_another_repository() {
        let mut job = job(&["tool", "other"]);
        job.on(JobEvent::Start(0));
        for index in 0..10 {
            job.on(JobEvent::Line(format!("line {index}"), false));
        }
        job.view = 4;
        job.scroll_by(100);
        assert_eq!(job.scroll, 6);
        job.on(JobEvent::Line("line 10".into(), false));
        assert_eq!(job.scroll, 7);
        job.scroll_by(-100);
        assert_eq!(job.scroll, 0);
        job.scroll_by(3);
        job.pick(1);
        assert_eq!(job.scroll, 0);
        assert_eq!(clock(Duration::from_secs(65)), "1m 05s");
    }
}
