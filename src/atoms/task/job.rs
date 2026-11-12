//! One background run, as a progress dialog shows it: the items it works
//! through, what was printed for each, and how the run ended.

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
    /// How it ended: a few words, or the error.
    pub note: String,
    /// The output printed for this item.
    pub output: VecDeque<String>,
    /// The current progress line. It replaces the one before it.
    pub progress: String,
    /// Steps done, out of the steps there are. None when the work is not
    /// counted in steps, and until the count is known.
    pub steps: Option<(u32, u32)>,
    /// How far the item is, from 0 to 1, as the job's reader judged it.
    /// None until something is reported.
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

    /// How far the item is, in percent.
    pub fn percent(&self) -> Option<u16> {
        self.reached
            .map(|reached| (reached * 100.0).round().clamp(0.0, 100.0) as u16)
    }

    /// How far this item is, from 0 to 1: its steps when they are counted,
    /// else what the reader judged.
    pub fn fraction(&self) -> Option<f64> {
        match self.steps {
            Some((done, total)) if total > 0 => Some(f64::from(done) / f64::from(total)),
            _ => self.reached,
        }
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
    /// A line of output. `true` marks a progress line that replaces the
    /// previous one instead of adding to the output.
    Line(String, bool),
    /// Steps done, out of the steps there are, for the item that is
    /// running.
    Steps(u32, u32),
}

/// Reads how far an item is, from 0 to 1, out of a line of its output.
/// `progress` is true for a progress line; `before` is what was known.
pub type Reader = fn(line: &str, progress: bool, before: Option<f64>) -> Option<f64>;

/// The reader a job has unless it is given another: a progress line with
/// a percentage is that far.
pub fn percent_reader(line: &str, progress: bool, before: Option<f64>) -> Option<f64> {
    if progress && let Some(percent) = parse_percent(line) {
        Some(f64::from(percent) / 100.0)
    } else {
        before
    }
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
    /// The item that output currently belongs to.
    active: Option<usize>,
    started: Instant,
    ended: Option<Instant>,
    reader: Reader,
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
            reader: percent_reader,
        }
    }

    /// Reads progress with `reader` instead of the plain percentage.
    pub fn with_reader(mut self, reader: Reader) -> Self {
        self.reader = reader;
        self
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
                let reader = self.reader;
                let Some(item) = self.items.get_mut(index) else {
                    return;
                };
                item.reached = reader(&text, progress, item.reached);
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

    /// The item that is running.
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

    #[test]
    fn a_progress_line_with_a_percentage_is_that_far() {
        let mut job = job(&["one"]);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Line("Pulling layer:  40%".into(), true));
        assert_eq!(job.items[0].percent(), Some(40));
        // Output that merely contains a percentage is not progress.
        job.on(JobEvent::Line("similarity index 90%".into(), false));
        assert_eq!(job.items[0].percent(), Some(40));
        assert_eq!(job.items[0].progress, "Pulling layer:  40%");
    }

    #[test]
    fn a_job_reads_progress_with_the_reader_it_is_given() {
        fn halfway(_: &str, _: bool, _: Option<f64>) -> Option<f64> {
            Some(0.5)
        }
        let mut job = job(&["one", "two"]).with_reader(halfway);
        job.on(JobEvent::Start(0));
        assert_eq!(job.fraction(), Some(0.0));
        job.on(JobEvent::Line("anything".into(), false));
        assert_eq!(job.items[0].percent(), Some(50));
        assert!((job.fraction().unwrap() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn steps_count_before_the_percentage() {
        let mut job = job(&["one", "two"]);
        job.on(JobEvent::Steps(1, 2));
        assert_eq!(job.items[0].steps, None);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Steps(0, 3));
        job.on(JobEvent::Line("Working:  80%".into(), true));
        assert_eq!(job.items[0].fraction(), Some(0.0));
        job.on(JobEvent::Steps(5, 3));
        assert_eq!(job.items[0].steps, Some((3, 3)));
        job.on(JobEvent::Done(0, "ok".into()));
        job.on(JobEvent::Start(1));
        assert_eq!(job.items[1].steps, None);
    }
}
