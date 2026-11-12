//! What git's output says about how far a clone, fetch or update is.

use crate::atoms::task::job::{Job, parse_percent};

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

/// A job whose progress is read from git's output.
pub fn job(title: impl Into<String>, names: Vec<String>, skipped: usize) -> Job {
    Job::new(title, names, skipped).with_reader(reader)
}

/// Git counts each phase from 0 to 100 on its own; the phases are joined
/// into one figure that only goes up.
fn reader(line: &str, progress: bool, before: Option<f64>) -> Option<f64> {
    if let Some(overall) = overall_progress(line) {
        Some(before.map_or(overall, |before| before.max(overall)))
    } else if progress && let Some(percent) = parse_percent(line) {
        // A phase this app does not know, or git in another language:
        // show its own percentage.
        Some(f64::from(percent) / 100.0)
    } else {
        before
    }
}

/// How far the whole transfer is, from 0 to 1, judged by one progress line.
/// None for a line without a percentage or with a phase that is not known.
fn overall_progress(line: &str) -> Option<f64> {
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

/// "2 of 3 branches updated", or "nothing new". None while the branches
/// are not counted.
pub fn steps_label(steps: Option<(u32, u32)>) -> Option<String> {
    let (done, total) = steps?;
    Some(match total {
        0 => "nothing new".to_string(),
        1 => format!("{done} of 1 branch updated"),
        _ => format!("{done} of {total} branches updated"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atoms::task::job::{ItemState, JobEvent};

    fn job(names: &[&str]) -> Job {
        super::job(
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
        let mut job = super::job("Cloning", vec!["tool".into()], 0);
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
            steps_label(job.items[0].steps).unwrap(),
            "2 of 3 branches updated"
        );
        assert!((job.fraction().unwrap() - 1.0 / 3.0).abs() < 1e-9);

        // The count stays once the repository is done.
        job.on(JobEvent::Done(0, "updated".into()));
        assert_eq!(job.items[0].steps, Some((2, 3)));

        job.on(JobEvent::Start(1));
        assert_eq!(job.items[1].steps, None);
        assert_eq!(steps_label(job.items[1].steps), None);
        job.on(JobEvent::Steps(0, 0));
        assert_eq!(steps_label(job.items[1].steps).unwrap(), "nothing new");
        assert_eq!(job.items[1].fraction(), None);
        job.on(JobEvent::Steps(0, 1));
        assert_eq!(
            steps_label(job.items[1].steps).unwrap(),
            "0 of 1 branch updated"
        );
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
}
