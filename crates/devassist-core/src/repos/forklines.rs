//! The lines a fetch of forks writes about how far it is, and the facts
//! the core reads back out of them. Every such line is written and read
//! here only, so the wording cannot drift between the two.

use std::path::{Path, PathBuf};

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
pub fn parse(line: &str) -> Option<ForkLine> {
    if line == GOING_ON {
        return Some(ForkLine::GoingOn);
    }
    if let Some(left) = line.strip_prefix(COUNTDOWN) {
        return read_clock(left).map(|seconds| ForkLine::Countdown { seconds });
    }
    if let Some(rest) = line.strip_prefix(FETCHING) {
        let (_, counted) = rest.split_once('(')?;
        let (done, total) = counted.strip_suffix(')')?.split_once('/')?;
        return Some(ForkLine::Fetching {
            done: done.parse().ok()?,
            total: total.parse().ok()?,
        });
    }
    if let Some((why, wait)) = line.split_once(PAUSED)
        && !why.is_empty()
    {
        let seconds = read_clock(wait.strip_suffix('.')?)?;
        return Some(ForkLine::Paused {
            why: why.to_string(),
            seconds,
        });
    }
    let (forks, into) = line.split_once(INTO)?;
    Some(ForkLine::Total {
        forks: forks.parse().ok()?,
        into: PathBuf::from(into),
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
    /// pauses.
    pub pause: Option<String>,
}

/// What the lines of the task running now in a fetch of forks said so
/// far. A new task starts from nothing.
#[derive(Clone, Debug, Default)]
pub(crate) struct ForkWatch {
    /// The collection, and the forks counted so far of how many: known
    /// once the task has said its total.
    said: Option<(PathBuf, u32, u32)>,
    /// Why the fetch pauses, and the seconds left: from the newest pause
    /// line and the newest countdown after it.
    pause: Option<(String, u64)>,
}

impl ForkWatch {
    /// Hears what the job heard: a task that starts starts from nothing.
    pub(crate) fn on(&mut self, event: &JobEvent) {
        match event {
            JobEvent::Start(_) => *self = Self::default(),
            JobEvent::Line(line, _) => self.hear(line),
            _ => {}
        }
    }

    fn hear(&mut self, line: &str) {
        match parse(line) {
            Some(ForkLine::Total { forks, into }) => {
                self.said = Some((into, 0, forks));
                self.pause = None;
            }
            Some(ForkLine::Fetching { done, total }) => {
                if let Some((_, counted, of)) = self.said.as_mut() {
                    (*counted, *of) = (done, total);
                }
                self.pause = None;
            }
            Some(ForkLine::Paused { why, seconds }) => self.pause = Some((why, seconds)),
            Some(ForkLine::Countdown { seconds }) => {
                if let Some((_, left)) = self.pause.as_mut() {
                    *left = seconds;
                }
            }
            Some(ForkLine::GoingOn) => self.pause = None,
            None => {}
        }
    }

    /// How far the task is, once it has said how many forks there are.
    pub(crate) fn progress(&self) -> Option<ForkProgress> {
        let (_, done, total) = self.said.as_ref()?;
        Some(ForkProgress {
            done: *done,
            total: *total,
            at_a_time: FORK_JOBS,
            pause: self
                .pause
                .as_ref()
                .map(|(why, left)| format!("{why}. Paused, going on in {}.", clock(*left))),
        })
    }

    /// The collection the task fetches into, once it has said so.
    pub(crate) fn into(&self) -> Option<&Path> {
        self.said.as_ref().map(|(into, _, _)| into.as_path())
    }
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
}
