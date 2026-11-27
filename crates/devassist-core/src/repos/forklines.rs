//! The lines a fetch of forks writes about how far it is, and the facts
//! the core reads back out of them. Every such line is written and read
//! here only, so the wording cannot drift between the two.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::gitops::FORK_JOBS;
use crate::task::job::JobEvent;

/// A line of a fetch of forks that says how far it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ForkLine {
    /// How many forks GitHub listed, and the collection they go into.
    Total { forks: u32, into: PathBuf },
    /// How many of the forks are fetched, of how many.
    Fetching { done: u32, total: u32 },
    /// The fetch pauses, why, and for how long.
    Paused { why: String, seconds: u64 },
    /// What is left of the pause.
    Countdown { seconds: u64 },
    /// The pause is over.
    GoingOn,
}

const INTO: &str = " forks, into ";
const FETCHING: &str = "Fetching forks: ";
const PAUSED: &str = ". Paused for ";
const COUNTDOWN: &str = "Paused, going on in ";
const GOING_ON: &str = "Going on.";

/// "38 forks, into C:\code\api-fork": the forks GitHub listed.
pub fn total(forks: usize, into: &Path) -> String {
    format!("{forks}{INTO}{}", into.display())
}

/// "Fetching forks:  63% (24/38)": a progress line.
pub fn fetching(done: usize, total: usize) -> String {
    format!(
        "{FETCHING}{:>3}% ({done}/{total})",
        done * 100 / total.max(1)
    )
}

/// "The fetch timed out. Paused for 0:10.": `why` is a sentence without
/// its full stop.
pub fn paused(why: &str, seconds: u64) -> String {
    format!("{why}{PAUSED}{}.", clock(seconds))
}

/// "Paused, going on in 0:09": a progress line, once a second.
pub fn countdown(seconds: u64) -> String {
    format!("{COUNTDOWN}{}", clock(seconds))
}

/// "Going on.": the pause is over.
pub fn going_on() -> String {
    GOING_ON.to_string()
}

/// What a line of a fetch of forks says, when it is one of the lines
/// above. Every other line is None.
/// Each kind is tried in turn, a total before a pause, and a line that
/// is not quite one kind is tried as the others.
pub fn parse(line: &str) -> Option<ForkLine> {
    (line == GOING_ON)
        .then_some(ForkLine::GoingOn)
        .or_else(|| parse_countdown(line))
        .or_else(|| parse_fetching(line))
        .or_else(|| parse_total(line))
        .or_else(|| parse_paused(line))
}

fn parse_countdown(line: &str) -> Option<ForkLine> {
    let seconds = read_clock(line.strip_prefix(COUNTDOWN)?)?;
    Some(ForkLine::Countdown { seconds })
}

fn parse_fetching(line: &str) -> Option<ForkLine> {
    let (_, counted) = line.strip_prefix(FETCHING)?.split_once('(')?;
    let (done, total) = counted.strip_suffix(')')?.split_once('/')?;
    Some(ForkLine::Fetching {
        done: done.parse().ok()?,
        total: total.parse().ok()?,
    })
}

fn parse_total(line: &str) -> Option<ForkLine> {
    let (forks, into) = line.split_once(INTO)?;
    Some(ForkLine::Total {
        forks: forks.parse().ok()?,
        into: PathBuf::from(into),
    })
}

fn parse_paused(line: &str) -> Option<ForkLine> {
    // The reason may say anything; the clock is at the end.
    let (why, wait) = line.rsplit_once(PAUSED)?;
    if why.is_empty() {
        return None;
    }
    Some(ForkLine::Paused {
        why: why.to_string(),
        seconds: read_clock(wait.strip_suffix('.')?)?,
    })
}

/// The seconds of a clock as `clock` writes it.
fn read_clock(text: &str) -> Option<u64> {
    let parts: Vec<u64> = text
        .split(':')
        .map(|part| {
            (part.len() <= 2 && part.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| part.parse().ok())
                .flatten()
        })
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [minutes, seconds] => Some(minutes * 60 + seconds),
        [hours, minutes, seconds] => Some(hours * 3600 + minutes * 60 + seconds),
        _ => None,
    }
}

/// How far a fetch of forks is, as its dialog shows it: "24 of 38 forks,
/// 8 at a time", and the pause while there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForkProgress {
    /// Forks fetched, or found unchanged, so far.
    pub done: u32,
    /// The forks there are to fetch.
    pub total: u32,
    /// How many forks are fetched at the same time.
    pub at_a_time: usize,
    /// "The fetch timed out. Paused, going on in 0:12.", while the fetch
    /// pauses. It changes with time: the time left counts down on the
    /// core's own clock from what the pause lines said, so whoever shows
    /// it asks again each second. With several pauses at once it names
    /// the one that ends last, until every one has gone on; a count of
    /// forks hides it until the next pause line.
    pub pause: Option<String>,
}

/// A pause that has not gone on yet: why, and when it ends.
#[derive(Clone, Debug)]
struct Pause {
    why: String,
    ends: Instant,
}

/// What the lines of the task running now in a fetch of forks said so
/// far. A new task starts from nothing; until it says its total, and
/// once the run's last task has ended, the count of the last task that
/// said one stands, without its pause.
#[derive(Clone, Debug, Default)]
pub(crate) struct ForkWatch {
    /// The task the facts are of.
    task: Option<usize>,
    /// The collection, and the forks counted so far of how many: known
    /// once the task has said its total.
    said: Option<(PathBuf, u32, u32)>,
    /// `said` of the last task that ended, or that a new task took over
    /// from, having said its total.
    last: Option<(PathBuf, u32, u32)>,
    /// The pauses heard that have not said "Going on." yet, oldest first.
    pauses: Vec<Pause>,
    /// A count of forks came after the newest pause line.
    hidden: bool,
    /// The fetch is held (a demo's story): no worker counts its pauses
    /// down or says "Going on.", so its clock stands at its last line.
    held: bool,
    /// When the last line was heard.
    heard: Option<Instant>,
}

impl ForkWatch {
    /// A watch of a held fetch (a demo's story): its pauses show the time
    /// their lines said and never move, so it reads the same every time.
    pub(crate) fn held() -> Self {
        Self {
            held: true,
            ..Self::default()
        }
    }

    /// Hears what the job heard: a task that starts starts from nothing,
    /// and one that ends leaves only its count.
    pub(crate) fn on(&mut self, event: &JobEvent) {
        self.on_at(event, Instant::now());
    }

    /// `on`, heard at `now`.
    fn on_at(&mut self, event: &JobEvent, now: Instant) {
        match event {
            JobEvent::Start(index) => {
                *self = Self {
                    task: Some(*index),
                    last: self.said.take().or_else(|| self.last.take()),
                    held: self.held,
                    ..Self::default()
                };
            }
            JobEvent::Done(index, _) | JobEvent::Failed(index, _) if self.task == Some(*index) => {
                *self = Self {
                    last: self.said.take().or_else(|| self.last.take()),
                    held: self.held,
                    ..Self::default()
                };
            }
            JobEvent::Line(line, _) => {
                self.heard = Some(now);
                self.hear(line, now);
            }
            _ => {}
        }
    }

    fn hear(&mut self, line: &str, now: Instant) {
        let after = |seconds: u64| now + Duration::from_secs(seconds);
        match parse(line) {
            Some(ForkLine::Total { forks, into }) => {
                self.said = Some((into, 0, forks));
                self.pauses.clear();
                self.hidden = false;
            }
            Some(ForkLine::Fetching { done, total }) => {
                if let Some((_, counted, of)) = self.said.as_mut() {
                    (*counted, *of) = (done, total);
                }
                self.hidden = true;
            }
            Some(ForkLine::Paused { why, seconds }) => {
                self.pauses.push(Pause {
                    why,
                    ends: after(seconds),
                });
                self.hidden = false;
            }
            Some(ForkLine::Countdown { seconds }) => {
                // Only the job's own line counts down: its newest pause.
                if let Some(pause) = self.pauses.last_mut() {
                    pause.ends = after(seconds);
                }
                self.hidden = false;
            }
            Some(ForkLine::GoingOn) => {
                // The pause that goes on is the one due first.
                if let Some(first) = (0..self.pauses.len()).min_by_key(|&at| self.pauses[at].ends) {
                    self.pauses.remove(first);
                }
            }
            None => {}
        }
    }

    /// How far the task is, once it has said how many forks there are;
    /// before that, and once it has ended, the last count said (`last`),
    /// with no pause: a pause belongs to the task that said it.
    pub(crate) fn progress(&self) -> Option<ForkProgress> {
        self.progress_at(Instant::now())
    }

    /// `progress`, at `now`.
    fn progress_at(&self, now: Instant) -> Option<ForkProgress> {
        let Some((_, done, total)) = self.said.as_ref() else {
            let (_, done, total) = self.last.as_ref()?;
            return Some(ForkProgress {
                done: *done,
                total: *total,
                at_a_time: at_a_time(*total),
                pause: None,
            });
        };
        // Held, the clock stands where the last line left it.
        let now = match self.heard {
            Some(heard) if self.held => heard,
            _ => now,
        };
        let pause = self
            .pauses
            .iter()
            .max_by_key(|pause| pause.ends)
            .filter(|_| !self.hidden)
            .map(|pause| {
                let left = pause.ends.saturating_duration_since(now);
                // A second begun is a second left: it reads 0:12 when heard.
                let seconds = left.as_secs() + u64::from(left.subsec_nanos() > 0);
                format!("{}. Paused, going on in {}.", pause.why, clock(seconds))
            });
        Some(ForkProgress {
            done: *done,
            total: *total,
            at_a_time: at_a_time(*total),
            pause,
        })
    }

    /// The collection the task fetches into, once it has said so; before
    /// that, and once it has ended, that of the last count (`progress`).
    pub(crate) fn folder(&self) -> Option<&Path> {
        self.said
            .as_ref()
            .or(self.last.as_ref())
            .map(|(into, _, _)| into.as_path())
    }
}

/// How many of `total` forks are fetched at the same time.
fn at_a_time(total: u32) -> usize {
    FORK_JOBS.min(total as usize).max(1)
}

/// "1:05" for 65 seconds, "1:01:05" from an hour on.
pub(crate) fn clock(seconds: u64) -> String {
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fork_lines_read_as_they_always_did() {
        let into = PathBuf::from("code").join("api-fork");
        assert_eq!(
            total(38, &into),
            format!("38 forks, into {}", into.display())
        );
        assert_eq!(fetching(24, 38), "Fetching forks:  63% (24/38)");
        assert_eq!(fetching(0, 4), "Fetching forks:   0% (0/4)");
        assert_eq!(fetching(4, 4), "Fetching forks: 100% (4/4)");
        assert_eq!(
            paused("The fetch timed out", 10),
            "The fetch timed out. Paused for 0:10."
        );
        assert_eq!(
            paused("GitHub's limit is reached", 3665),
            "GitHub's limit is reached. Paused for 1:01:05."
        );
        assert_eq!(countdown(9), "Paused, going on in 0:09");
        assert_eq!(going_on(), "Going on.");
    }

    #[test]
    fn every_fork_line_is_read_back_as_it_was_written() {
        let into = PathBuf::from("code").join("my api-fork");
        assert_eq!(
            parse(&total(38, &into)),
            Some(ForkLine::Total {
                forks: 38,
                into: into.clone()
            })
        );
        assert_eq!(
            parse(&total(1, Path::new("a, into b"))),
            Some(ForkLine::Total {
                forks: 1,
                into: PathBuf::from("a, into b")
            })
        );
        for (done, total) in [(0, 4), (24, 38), (38, 38), (1234, 5000)] {
            assert_eq!(
                parse(&fetching(done, total)),
                Some(ForkLine::Fetching {
                    done: done as u32,
                    total: total as u32
                })
            );
        }
        for (why, seconds) in [
            ("The fetch timed out", 10),
            ("GitHub's limit is reached", 250),
            ("GitHub did not answer", 3665),
        ] {
            assert_eq!(
                parse(&paused(why, seconds)),
                Some(ForkLine::Paused {
                    why: why.to_string(),
                    seconds
                })
            );
        }
        for seconds in [1, 9, 12, 59, 60, 250, 3600, 3665] {
            assert_eq!(
                parse(&countdown(seconds)),
                Some(ForkLine::Countdown { seconds })
            );
        }
        assert_eq!(parse(&going_on()), Some(ForkLine::GoingOn));
        // The fetch's other lines are none of these.
        for other in [
            "Asking GitHub for the forks of acme/api.",
            "Listing forks: 38",
            "Fetching acme/api, the original.",
            "Fetching bob/api.",
            "It has no forks.",
            "forks, into somewhere",
            "many forks, into somewhere",
            "Fetching forks: 63% (24/)",
            "Paused for 0:10.",
            "Paused, going on in soon",
            "Going on",
            "",
        ] {
            assert_eq!(parse(other), None, "{other}");
        }
    }

    #[test]
    fn a_line_that_looks_like_two_kinds_reads_as_the_one_it_is() {
        // A collection whose path reads like a pause is still a total.
        let into = PathBuf::from("code").join("api. Paused for 0:10.");
        assert_eq!(
            parse(&total(38, &into)),
            Some(ForkLine::Total {
                forks: 38,
                into: into.clone()
            })
        );
        // A reason that says "Paused" is still the reason.
        let why = "Paused by GitHub. Paused for a while";
        assert_eq!(
            parse(&paused(why, 10)),
            Some(ForkLine::Paused {
                why: why.to_string(),
                seconds: 10
            })
        );
        // A clock that cannot be read leaves the line to the other kinds.
        for why in ["Paused, going on in soon", "Fetching forks: (x"] {
            assert_eq!(
                parse(&paused(why, 5)),
                Some(ForkLine::Paused {
                    why: why.to_string(),
                    seconds: 5
                }),
                "{why}"
            );
        }
        // Clocks at their edges, and a count of none.
        for seconds in [600, 3599] {
            assert_eq!(
                parse(&countdown(seconds)),
                Some(ForkLine::Countdown { seconds })
            );
            assert_eq!(
                parse(&paused("The fetch timed out", seconds)),
                Some(ForkLine::Paused {
                    why: "The fetch timed out".into(),
                    seconds
                })
            );
        }
        assert_eq!(countdown(600), "Paused, going on in 10:00");
        assert_eq!(countdown(3599), "Paused, going on in 59:59");
        assert_eq!(fetching(0, 0), "Fetching forks:   0% (0/0)");
        assert_eq!(
            parse(&fetching(0, 0)),
            Some(ForkLine::Fetching { done: 0, total: 0 })
        );
    }

    /// A watch that has heard `lines` of the task at index 0, each at the
    /// given second after `start`.
    fn heard(start: Instant, lines: &[(u64, &str)]) -> ForkWatch {
        let mut watch = ForkWatch::default();
        watch.on_at(&JobEvent::Start(0), start);
        for (second, line) in lines {
            watch.on_at(
                &JobEvent::Line((*line).to_string(), false),
                at(start, *second),
            );
        }
        watch
    }

    fn at(start: Instant, second: u64) -> Instant {
        start + std::time::Duration::from_secs(second)
    }

    fn pause_at(watch: &ForkWatch, start: Instant, second: u64) -> Option<String> {
        watch.progress_at(at(start, second))?.pause
    }

    #[test]
    fn a_pause_counts_down_on_its_own_clock() {
        let start = Instant::now();
        let total = "38 forks, into api-fork";
        // A fork's own timeout: no countdown line ever comes.
        let watch = heard(
            start,
            &[(0, total), (1, "The fetch timed out. Paused for 0:10.")],
        );
        let said = |second| pause_at(&watch, start, second);
        assert_eq!(
            said(1).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:10.")
        );
        assert_eq!(
            said(4).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:07.")
        );
        assert_eq!(
            said(30).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:00.")
        );
        // The design's case: a countdown line sets the clock again.
        let watch = heard(
            start,
            &[
                (0, total),
                (2, "The fetch timed out. Paused for 0:30."),
                (20, "Paused, going on in 0:12"),
            ],
        );
        assert_eq!(
            pause_at(&watch, start, 20).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:12.")
        );
        assert_eq!(
            pause_at(&watch, start, 25).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:07.")
        );
    }

    #[test]
    fn a_held_watch_shows_its_pause_as_it_was_said() {
        let start = Instant::now();
        let mut watch = ForkWatch::held();
        watch.on_at(&JobEvent::Start(0), start);
        for (second, line) in [
            (0, "38 forks, into api-fork"),
            (2, "The fetch timed out. Paused for 0:30."),
            (20, "Paused, going on in 0:12"),
        ] {
            watch.on_at(&JobEvent::Line(line.to_string(), false), at(start, second));
        }
        // No worker says "Going on.": the pause stands as its last line
        // said it, however late it is asked.
        for second in [20, 25, 60, 3600] {
            assert_eq!(
                pause_at(&watch, start, second).as_deref(),
                Some("The fetch timed out. Paused, going on in 0:12."),
                "{second}"
            );
        }
    }

    #[test]
    fn overlapping_pauses_show_the_one_that_ends_last() {
        let start = Instant::now();
        let total = "38 forks, into api-fork";
        let mut watch = heard(
            start,
            &[
                (0, total),
                (1, "The fetch timed out. Paused for 0:10."),
                (2, "GitHub did not answer. Paused for 0:30."),
            ],
        );
        assert_eq!(
            pause_at(&watch, start, 2).as_deref(),
            Some("GitHub did not answer. Paused, going on in 0:30.")
        );
        // The first goes on; the second still pauses.
        watch.on_at(&JobEvent::Line("Going on.".into(), false), at(start, 11));
        assert_eq!(
            pause_at(&watch, start, 11).as_deref(),
            Some("GitHub did not answer. Paused, going on in 0:21.")
        );
        watch.on_at(&JobEvent::Line("Going on.".into(), false), at(start, 32));
        assert_eq!(pause_at(&watch, start, 32), None);
        // The longer one is shown, whichever came first.
        let watch = heard(
            start,
            &[
                (0, total),
                (1, "The fetch timed out. Paused for 0:40."),
                (2, "GitHub did not answer. Paused for 0:05."),
            ],
        );
        assert_eq!(
            pause_at(&watch, start, 2).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:39.")
        );
    }

    #[test]
    fn a_count_hides_a_pause_until_the_next_pause_line() {
        let start = Instant::now();
        let mut watch = heard(
            start,
            &[
                (0, "38 forks, into api-fork"),
                (1, "The fetch timed out. Paused for 0:30."),
            ],
        );
        watch.on_at(
            &JobEvent::Line("Fetching forks:  65% (25/38)".into(), true),
            at(start, 3),
        );
        let progress = watch.progress_at(at(start, 3)).unwrap();
        assert_eq!(
            (progress.done, progress.total, progress.pause),
            (25, 38, None)
        );
        // Another pause brings it back, with the one that ends last.
        watch.on_at(
            &JobEvent::Line("GitHub did not answer. Paused for 0:05.".into(), false),
            at(start, 5),
        );
        assert_eq!(
            pause_at(&watch, start, 5).as_deref(),
            Some("The fetch timed out. Paused, going on in 0:26.")
        );
    }

    #[test]
    fn a_task_that_ends_leaves_its_count_and_no_pause() {
        let start = Instant::now();
        let lines = [
            (0, "38 forks, into api-fork"),
            (1, "Fetching forks:  63% (24/38)"),
            (2, "The fetch timed out. Paused for 0:30."),
        ];
        let counted = |watch: &ForkWatch| {
            watch
                .progress_at(at(start, 3))
                .map(|progress| (progress.done, progress.total, progress.pause))
        };
        let mut watch = heard(start, &lines);
        // The end of another task changes nothing.
        watch.on_at(&JobEvent::Done(1, "done".into()), at(start, 3));
        assert!(counted(&watch).is_some_and(|(.., pause)| pause.is_some()));
        watch.on_at(&JobEvent::Done(0, "38 forks".into()), at(start, 3));
        assert_eq!(counted(&watch), Some((24, 38, None)));
        assert_eq!(watch.folder(), Some(Path::new("api-fork")));
        let mut watch = heard(start, &lines);
        watch.on_at(&JobEvent::Failed(0, "fatal".into()), at(start, 3));
        assert_eq!(counted(&watch), Some((24, 38, None)));
        // The next task's lines are its own once it says its total; until
        // then the last count stands.
        watch.on_at(&JobEvent::Start(1), at(start, 4));
        assert_eq!(counted(&watch), Some((24, 38, None)));
        watch.on_at(
            &JobEvent::Line("3 forks, into docs-fork".into(), false),
            at(start, 4),
        );
        assert_eq!(counted(&watch), Some((0, 3, None)));
        assert_eq!(watch.folder(), Some(Path::new("docs-fork")));
        // A task that ends before it said a total leaves the count before.
        watch.on_at(&JobEvent::Done(1, "3 forks".into()), at(start, 5));
        watch.on_at(&JobEvent::Start(2), at(start, 5));
        watch.on_at(&JobEvent::Failed(2, "fatal".into()), at(start, 5));
        assert_eq!(counted(&watch), Some((0, 3, None)));
    }

    #[test]
    fn as_many_run_at_a_time_as_there_are_forks_up_to_the_limit() {
        let start = Instant::now();
        let at_a_time = |line: &str| {
            heard(start, &[(0, line)])
                .progress_at(start)
                .map(|progress| progress.at_a_time)
        };
        assert_eq!(at_a_time("3 forks, into api-fork"), Some(3));
        assert_eq!(at_a_time("38 forks, into api-fork"), Some(FORK_JOBS));
        assert_eq!(at_a_time("0 forks, into api-fork"), Some(1));
    }
}
