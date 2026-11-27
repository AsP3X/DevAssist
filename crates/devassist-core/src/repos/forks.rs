//! What the overview of a repository's forks says about each fork and
//! about all of them: the words, dates and numbers the terminal app and
//! the window both show.

use super::gitops::ForkInfo;

/// What the state column says about a fork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ForkState {
    /// It has commits of its own, and with them a folder of its files.
    Changes,
    /// It was fetched and has nothing of its own.
    Unchanged,
    /// Nothing was fetched from it yet.
    NotFetched,
    /// GitHub no longer has it.
    Gone,
}

impl ForkState {
    /// The state of a fork as the collection holds it.
    pub fn of(fork: &ForkInfo) -> ForkState {
        match (fork.gone, fork.folder, fork.branches) {
            (true, _, _) => ForkState::Gone,
            (false, true, _) => ForkState::Changes,
            (false, false, 0) => ForkState::NotFetched,
            (false, false, _) => ForkState::Unchanged,
        }
    }

    /// What the state column says.
    pub fn words(self) -> &'static str {
        match self {
            ForkState::Changes => "changes, folder",
            ForkState::Unchanged => "no changes",
            ForkState::NotFetched => "not fetched",
            ForkState::Gone => "gone from GitHub",
        }
    }
}

/// How a day is written.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayForm {
    /// `2026-11-23`, as the terminal app writes it.
    Digits,
    /// `23 Nov 2026`, as the window writes it.
    Words,
}

/// The day of a time in seconds since 1970, in UTC, written in `form`.
/// Empty for 0: a fork nothing was fetched from has no date.
pub fn day(seconds: i64, form: DayForm) -> String {
    if seconds == 0 {
        return String::new();
    }
    let days = seconds.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let of_era = days.rem_euclid(146_097);
    let year_of_era = (of_era - of_era / 1460 + of_era / 36_524 - of_era / 146_096) / 365;
    let of_year = of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted = (5 * of_year + 2) / 153;
    let day = of_year - (153 * shifted + 2) / 5 + 1;
    let month = if shifted < 10 {
        shifted + 3
    } else {
        shifted - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    match form {
        DayForm::Digits => format!("{year:04}-{month:02}-{day:02}"),
        DayForm::Words => {
            const MONTHS: [&str; 12] = [
                "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
            ];
            let name = usize::try_from(month - 1)
                .ok()
                .and_then(|index| MONTHS.get(index))
                .copied()
                .unwrap_or_default();
            format!("{day} {name} {year:04}")
        }
    }
}

/// How far a fork is ahead of the original, where that is worth showing:
/// only a fork with a folder, and only when it is ahead at all.
pub fn fork_ahead(fork: &ForkInfo) -> Option<u32> {
    fork.ahead.filter(|ahead| fork.folder && *ahead > 0)
}

/// The numbers of a fork overview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForkCounts {
    /// Every fork in the collection.
    pub all: usize,
    /// The forks the list shows, after the filter.
    pub shown: usize,
    /// Forks with changes, and with them a folder.
    pub changed: usize,
    /// Forks GitHub no longer has.
    pub gone: usize,
    /// The branches of every fork together.
    pub branches: u32,
    /// The pull requests of every fork together.
    pub pulls: u32,
}

/// The sentence that sums up the forks: how many there are, and the
/// parts that are not zero.
pub(crate) fn summary(counts: &ForkCounts) -> String {
    let some = |count: u32, one: &str, many: &str| match count {
        1 => format!("1 {one}"),
        count => format!("{count} {many}"),
    };
    let mut sentence = match counts.all {
        0 => return "No forks.".to_string(),
        1 => "1 fork.".to_string(),
        all => format!("{all} forks."),
    };
    match counts.changed {
        0 => {}
        1 => sentence.push_str(" 1 has changes and a folder."),
        changed => sentence.push_str(&format!(" {changed} have changes and a folder.")),
    }
    match counts.gone {
        0 => {}
        1 => sentence.push_str(" 1 is gone from GitHub."),
        gone => sentence.push_str(&format!(" {gone} are gone from GitHub.")),
    }
    let branches = some(counts.branches, "branch", "branches");
    let pulls = some(counts.pulls, "pull request", "pull requests");
    // What is saved, and whether it is one thing.
    let saved = match (counts.branches, counts.pulls) {
        (0, 0) => None,
        (one, 0) => Some((branches, one == 1)),
        (0, one) => Some((pulls, one == 1)),
        _ => Some((format!("{branches} and {pulls}"), false)),
    };
    if let Some((what, one)) = saved {
        let verb = if one { "is" } else { "are" };
        sentence.push_str(&format!(" {what} {verb} saved."));
    }
    sentence
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fork(branches: u32, ahead: Option<u32>, folder: bool, gone: bool) -> ForkInfo {
        ForkInfo {
            owner: "bob".into(),
            name: "api".into(),
            branches,
            pulls: 0,
            ahead,
            date: 0,
            folder,
            gone,
        }
    }

    #[test]
    fn the_state_words_are_the_terminals() {
        let state = |fork: &ForkInfo| ForkState::of(fork).words();
        assert_eq!(state(&fork(3, Some(2), true, false)), "changes, folder");
        assert_eq!(state(&fork(3, Some(0), false, false)), "no changes");
        assert_eq!(state(&fork(0, None, false, false)), "not fetched");
        // Gone wins over everything else.
        assert_eq!(state(&fork(3, Some(2), true, true)), "gone from GitHub");
        assert_eq!(state(&fork(0, None, false, true)), "gone from GitHub");
        assert_eq!(
            ForkState::of(&fork(1, None, false, false)),
            ForkState::Unchanged
        );
        assert_eq!(
            ForkState::of(&fork(0, None, false, false)),
            ForkState::NotFetched
        );
        // Ahead is shown for a fork with a folder that is ahead.
        assert_eq!(fork_ahead(&fork(3, Some(7), true, false)), Some(7));
        assert_eq!(fork_ahead(&fork(3, Some(0), true, false)), None);
        assert_eq!(fork_ahead(&fork(3, Some(7), false, false)), None);
        assert_eq!(fork_ahead(&fork(3, None, true, false)), None);
    }

    #[test]
    fn a_day_is_written_in_digits_or_in_words() {
        assert_eq!(day(1_795_392_000, DayForm::Digits), "2026-11-23");
        assert_eq!(day(1_795_392_000, DayForm::Words), "23 Nov 2026");
        assert_eq!(day(0, DayForm::Digits), "");
        assert_eq!(day(0, DayForm::Words), "");
        // The terminal's days, which it drew before this moved here.
        assert_eq!(day(1_795_596_570, DayForm::Digits), "2026-11-25");
        assert_eq!(day(951_782_400, DayForm::Digits), "2000-02-29");
        assert_eq!(day(951_782_400, DayForm::Words), "29 Feb 2000");
        // No leading zero in words; days are counted in UTC.
        assert_eq!(day(1_780_358_400, DayForm::Words), "2 Jun 2026");
        assert_eq!(day(1_780_358_399, DayForm::Words), "1 Jun 2026");
        assert_eq!(day(1_780_358_400, DayForm::Digits), "2026-06-02");
        // The turn of a year.
        assert_eq!(day(1_798_761_599, DayForm::Words), "31 Dec 2026");
        assert_eq!(day(1_798_761_599, DayForm::Digits), "2026-12-31");
        assert_eq!(day(1_798_761_600, DayForm::Words), "1 Jan 2027");
        assert_eq!(day(1_798_761_600, DayForm::Digits), "2027-01-01");
        // March after a common February and after a leap one.
        assert_eq!(day(1_803_859_200, DayForm::Words), "1 Mar 2027");
        assert_eq!(day(1_835_395_200, DayForm::Words), "29 Feb 2028");
        assert_eq!(day(1_835_481_600, DayForm::Words), "1 Mar 2028");
        assert_eq!(day(1_835_481_600, DayForm::Digits), "2028-03-01");
        // Before 1970.
        assert_eq!(day(-1, DayForm::Words), "31 Dec 1969");
        assert_eq!(day(-1, DayForm::Digits), "1969-12-31");
    }

    #[test]
    fn the_summary_counts_what_is_there_and_leaves_out_zeros() {
        let counts = |all, changed, gone, branches, pulls| ForkCounts {
            all,
            shown: 0,
            changed,
            gone,
            branches,
            pulls,
        };
        assert_eq!(
            summary(&counts(38, 6, 1, 61, 9)),
            "38 forks. 6 have changes and a folder. 1 is gone from GitHub. 61 branches and 9 pull requests are saved."
        );
        assert_eq!(summary(&counts(0, 0, 0, 0, 0)), "No forks.");
        assert_eq!(summary(&counts(1, 0, 0, 0, 0)), "1 fork.");
        assert_eq!(
            summary(&counts(1, 1, 0, 1, 1)),
            "1 fork. 1 has changes and a folder. 1 branch and 1 pull request are saved."
        );
        assert_eq!(
            summary(&counts(4, 0, 2, 61, 0)),
            "4 forks. 2 are gone from GitHub. 61 branches are saved."
        );
        assert_eq!(
            summary(&counts(4, 0, 0, 0, 1)),
            "4 forks. 1 pull request is saved."
        );
    }
}
