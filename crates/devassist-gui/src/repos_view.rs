//! What each row of the repositories table says, decided from the core's state and tested here, so the table only
//! draws it.
//!
//! Every word a row says is chosen here, from the core's own fields and methods; the core's `sync_label` is not
//! parsed. Reading only: nothing here changes the core's state.

use devassist_core::repos::gitops::LocalRepo;
use devassist_core::repos::state::{Repos, Row, group_key};

use crate::assets::Icon;

/// A shorter path for a narrow column, the terminal app's rule, shared through the core.
pub use devassist_core::text::short_path as shorten_path;

/// The name of the group that holds the repositories that are in no group.
const NO_GROUP: &str = "No group";
/// A repository that has nothing behind, ahead or new.
const UP_TO_DATE: &str = "Up to date";
/// A checked-out commit that is on no branch.
const DETACHED: &str = "Detached";
/// A branch whose upstream the remote no longer has.
const UPSTREAM_GONE: &str = "Upstream gone";
/// A branch with no upstream to measure against.
const NO_UPSTREAM: &str = "No upstream";
/// The last fetch failed, so the counts are those of the last check that worked.
const FETCH_FAILED: &str = "Fetch failed";

/// One line of the table: a group's header or a repository.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// The header of a group.
    Group(GroupLine),
    /// A repository.
    Repo(RepoLine),
}

/// The header of a group.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupLine {
    /// The row's key in the core: the one that names the group when it is highlighted.
    pub key: String,
    /// What the header shows as the group's name: [`NO_GROUP`] for the repositories that are in no group.
    pub name: String,
    /// How many repositories are in it and how many have something to bring in.
    pub note: String,
    /// Its repositories are not shown.
    pub collapsed: bool,
    /// It is the highlighted row.
    pub chosen: bool,
}

/// One repository, in the words and numbers its row shows.
#[derive(Clone, Debug, PartialEq)]
pub struct RepoLine {
    /// The row's key in the core: the folder label, which also names the row in the core's calls.
    pub key: String,
    /// What the overview shows as its name: its folder label (two checkouts of one folder name show as `work/api`
    /// and `home/api`).
    pub name: String,
    /// It is pulled on a timer.
    pub watched: bool,
    /// It is ticked.
    pub ticked: bool,
    /// It is the highlighted row.
    pub chosen: bool,
    /// The checkout's full path.
    pub folder: String,
    /// The branch that is checked out.
    pub branch: String,
    /// Whether the checkout has uncommitted changes.
    pub changes: Changes,
    /// Its pull requests.
    pub pulls: Pulls,
    /// How it stands to its remote.
    pub origin: Origin,
}

/// Whether a checkout has uncommitted changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Changes {
    /// None.
    Clean,
    /// Some.
    Uncommitted,
    /// Not known: the checkout has not been read, or could not be.
    Unknown,
}

/// A repository's pull requests: on GitHub by state, and what its clone has not saved.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pulls {
    /// Open on GitHub.
    pub open: u32,
    /// Merged on GitHub.
    pub merged: u32,
    /// Closed without a merge on GitHub.
    pub closed: u32,
    /// On the remote and not saved in the clone.
    pub new: u32,
    /// Saved in the clone, with commits that changed on the remote since.
    pub changed: u32,
    /// GitHub has answered with counts for the repository. When it has not (no sign-in, not on GitHub), the
    /// counts of GitHub are zero.
    pub known: bool,
}

/// How a repository stands to its remote.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// Not read yet.
    Reading,
    /// A fetch or an update is running: how far, and its words.
    Busy {
        /// How far it is, from 0 to 1.
        share: f32,
        /// Its state in a few words.
        text: String,
    },
    /// It could not be read or fetched: the reason.
    Failed(String),
    /// Commits behind and ahead, drawn on the rule, and the words beside it.
    Measured {
        /// Commits the remote has that the checkout has not.
        behind: u32,
        /// Commits the checkout has that the remote has not.
        ahead: u32,
        /// The words beside the rule.
        text: String,
        /// How loudly the words are said.
        tone: Tone,
    },
}

/// How loudly a text is said.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Nothing to say.
    Quiet,
    /// Something to read.
    Plain,
    /// Something to look at.
    Warning,
}

/// What the status line says.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    /// The core's last message.
    pub text: String,
    /// The message is an error.
    pub error: bool,
    /// What is switched on, each with its icon.
    pub notes: Vec<(Icon, String)>,
    /// The message is kept whole and the notes give way, the last first; otherwise the notes are kept and the
    /// message is cut.
    pub keep_text: bool,
    /// The place among `notes` of the one that is a folder's path: cut from its left so its last folder shows, all
    /// of it on hover.
    pub path: Option<usize>,
}

/// The table's lines, in the core's order: with groups, each header and the repositories under it that are not
/// folded away. A selected or ticked folder the core no longer lists is no line, and an empty list is no lines.
#[must_use]
pub fn lines(repos: &Repos) -> Vec<Line> {
    repos
        .rows()
        .into_iter()
        .map(|row| match row {
            Row::Group { name, repos: members, incoming, collapsed } => Line::Group(GroupLine {
                chosen: repos.selected.as_deref() == Some(group_key(&name).as_str()),
                key: group_key(&name),
                note: group_note(&name, members, incoming),
                name: if name.is_empty() { NO_GROUP.to_owned() } else { name },
                collapsed,
            }),
            Row::Repo(repo) => Line::Repo(repo_line(repos, repo)),
        })
        .collect()
}

/// What the status line shows: the core's message, and what is switched on.
#[must_use]
pub fn status(repos: &Repos) -> Status {
    let mut notes = Vec::new();
    match repos.watched_count() {
        0 => {}
        1 => notes.push((Icon::Eye, "Watching 1 repository".to_owned())),
        many => notes.push((Icon::Eye, format!("Watching {many} repositories"))),
    }
    if repos.pull_requests {
        notes.push((Icon::GitPullRequest, "Pull request backup is on".to_owned()));
    }
    Status { text: repos.status.clone(), error: repos.status_error, notes, ..Status::default() }
}

/// How many repositories a group holds, and how many of them wait for an update.
fn group_note(name: &str, members: usize, incoming: usize) -> String {
    let count = if members == 1 { "1 repository".to_owned() } else { format!("{members} repositories") };
    if incoming > 0 {
        format!("{count}, {incoming} to update")
    } else if name.is_empty() {
        count
    } else {
        format!("{count}, all up to date")
    }
}

/// The line of one repository.
fn repo_line(repos: &Repos, repo: &LocalRepo) -> RepoLine {
    RepoLine {
        key: repo.folder.clone(),
        name: repo.folder.clone(),
        watched: repos.is_watched(&repo.path),
        ticked: repos.is_marked_local(&repo.folder),
        chosen: repos.selected.as_deref() == Some(repo.folder.as_str()),
        folder: repo.path.display().to_string(),
        branch: repo.branch.clone(),
        changes: changes(repo),
        pulls: pulls(repos, repo),
        origin: origin(repos, repo),
    }
}

/// Whether the checkout has uncommitted changes, as far as the core has read it.
fn changes(repo: &LocalRepo) -> Changes {
    if !repo.ready || repo.error.is_some() {
        Changes::Unknown
    } else if repo.dirty {
        Changes::Uncommitted
    } else {
        Changes::Clean
    }
}

/// The pull requests of a repository. Without counts from GitHub the counts of GitHub are zero; what the clone has
/// not saved comes from its own last fetch, and is said either way.
fn pulls(repos: &Repos, repo: &LocalRepo) -> Pulls {
    let counts = repos.pull_counts(repo);
    let pending = repos.pending_pulls(repo);
    let counts_or_zero = counts.unwrap_or_default();
    Pulls {
        open: counts_or_zero.open,
        merged: counts_or_zero.merged,
        closed: counts_or_zero.closed,
        new: pending.new,
        changed: pending.changed,
        known: counts.is_some(),
    }
}

/// How the repository stands to its remote: busy when a job has it, else what the last check found.
fn origin(repos: &Repos, repo: &LocalRepo) -> Origin {
    if let Some((share, text)) = repos.job_share_of(&repo.folder) {
        return Origin::Busy { share, text };
    }
    if !repo.ready {
        return Origin::Reading;
    }
    if let Some(reason) = &repo.error {
        return Origin::Failed(reason.clone());
    }
    let (ahead, behind) = repo.sync_counts();
    if repo.warning.is_some() {
        return Origin::Measured { behind, ahead, text: FETCH_FAILED.to_owned(), tone: Tone::Warning };
    }
    let remote = repo.remote_label();
    let (local, tone) = match (ahead, behind) {
        (0, 0) => match no_counts(repo) {
            Some(state) => (Some(state.to_owned()), Tone::Warning),
            None => (None, if remote.is_some() { Tone::Plain } else { Tone::Quiet }),
        },
        (ahead, 0) => (Some(format!("{ahead} ahead")), Tone::Plain),
        (0, behind) => (Some(format!("{behind} behind")), Tone::Warning),
        (ahead, behind) => (Some(format!("{ahead} ahead, {behind} behind")), Tone::Plain),
    };
    let text = match (local, remote) {
        (Some(local), Some(remote)) => format!("{local}, {remote}"),
        (Some(only), None) | (None, Some(only)) => only,
        (None, None) => UP_TO_DATE.to_owned(),
    };
    Origin::Measured { behind, ahead, text, tone }
}

/// What a checkout with nothing ahead and nothing behind has to say about its branch, when it is not simply up to
/// date. The order is the core's.
fn no_counts(repo: &LocalRepo) -> Option<&'static str> {
    if repo.branch == "detached" {
        Some(DETACHED)
    } else if repo.upstream_gone {
        Some(UPSTREAM_GONE)
    } else if repo.all_branches_current {
        None
    } else {
        Some(NO_UPSTREAM)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use devassist_core::repos::github::PullCounts;
    use devassist_core::repos::gitops::{LocalRepo, PendingPulls};
    use devassist_core::repos::state::Repos;
    use devassist_core::repos::state::testing::{installed, test_settings};
    use devassist_core::task::job::{Job, JobEvent};

    use super::*;
    use crate::assets::Icon;

    /// A repository of `acme` in `group` (empty for none), checked out at a path of its own.
    fn repo(name: &str, group: &str) -> LocalRepo {
        let mut row = installed("acme", name);
        row.group = group.to_owned();
        row.path = PathBuf::from("/srv/code").join(name);
        row
    }

    fn repos_of(rows: Vec<LocalRepo>) -> Repos {
        let mut repos = Repos::offline(test_settings());
        repos.installed = rows;
        repos
    }

    fn behind(mut row: LocalRepo, count: u32) -> LocalRepo {
        row.behind = Some(count);
        row.total_behind = count;
        row
    }

    fn ahead(mut row: LocalRepo, count: u32) -> LocalRepo {
        row.ahead = Some(count);
        row.total_ahead = count;
        row
    }

    fn only(repos: &Repos) -> RepoLine {
        let mut lines = lines(repos);
        assert_eq!(lines.len(), 1, "one repository, no groups");
        match lines.remove(0) {
            Line::Repo(line) => line,
            Line::Group(group) => panic!("expected a repository, got the group {}", group.name),
        }
    }

    fn origin_of(row: LocalRepo) -> Origin {
        only(&repos_of(vec![row])).origin
    }

    fn measured(behind: u32, ahead: u32, text: &str, tone: Tone) -> Origin {
        Origin::Measured { behind, ahead, text: text.to_owned(), tone }
    }

    fn group_named<'a>(all: &'a [Line], name: &str) -> &'a GroupLine {
        all.iter()
            .find_map(|line| match line {
                Line::Group(group) if group.name == name => Some(group),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no group {name}"))
    }

    #[test]
    fn groups_say_how_many_and_how_many_wait() {
        let mut repos = repos_of(vec![
            repo("dotfiles", "home"),
            behind(repo("api", "Work"), 4),
            repo("docs", "Work"),
            repo("sandbox", ""),
            behind(repo("notes", ""), 1),
        ]);
        let all = lines(&repos);
        let home = group_named(&all, "home");
        assert_eq!((home.key.as_str(), home.note.as_str()), ("group::home", "1 repository, all up to date"));
        assert_eq!(group_named(&all, "Work").note, "2 repositories, 1 to update");
        let none = group_named(&all, "No group");
        assert_eq!((none.key.as_str(), none.note.as_str()), ("group::", "2 repositories, 1 to update"));
        assert!(all.iter().all(|line| !matches!(line, Line::Group(group) if group.collapsed)));
        // The group without a name says only how many when nothing waits, and one is one.
        repos.installed.retain(|row| row.folder != "notes");
        let all = lines(&repos);
        assert_eq!(group_named(&all, "No group").note, "1 repository");
        // A folded group is its header alone, and says so.
        repos.collapsed.push("Work".to_owned());
        let all = lines(&repos);
        assert!(group_named(&all, "Work").collapsed);
        let keys: Vec<&str> = all
            .iter()
            .filter_map(|line| match line {
                Line::Repo(repo) => Some(repo.key.as_str()),
                Line::Group(_) => None,
            })
            .collect();
        assert_eq!(keys, ["dotfiles", "sandbox"]);
    }

    #[test]
    fn a_row_behind_is_measured_and_warns() {
        assert_eq!(origin_of(behind(repo("api", ""), 4)), measured(4, 0, "4 behind", Tone::Warning));
    }

    #[test]
    fn ahead_and_behind_are_both_said() {
        assert_eq!(origin_of(ahead(repo("api", ""), 1)), measured(0, 1, "1 ahead", Tone::Plain));
        assert_eq!(origin_of(ahead(behind(repo("api", ""), 2), 1)), measured(2, 1, "1 ahead, 2 behind", Tone::Plain));
        assert_eq!(origin_of(repo("api", "")), measured(0, 0, "Up to date", Tone::Quiet));
    }

    #[test]
    fn news_on_other_branches_is_said_beside_up_to_date() {
        let mut web = repo("web", "");
        web.remote_new = 3;
        web.remote_branches = vec!["dev".to_owned()];
        assert_eq!(origin_of(web.clone()), measured(0, 0, "3 new on dev", Tone::Plain));
        // Beside a count it follows the count.
        assert_eq!(origin_of(behind(web.clone(), 2)), measured(2, 0, "2 behind, 3 new on dev", Tone::Warning));
        assert_eq!(origin_of(ahead(web.clone(), 1)), measured(0, 1, "1 ahead, 3 new on dev", Tone::Plain));
        web.remote_branches.push("beta".to_owned());
        assert_eq!(origin_of(web), measured(0, 0, "3 new on 2 branches", Tone::Plain));
    }

    #[test]
    fn the_states_without_counts_warn_in_the_cores_words() {
        let mut detached = repo("a", "");
        detached.branch = "detached".to_owned();
        assert_eq!(origin_of(detached), measured(0, 0, "Detached", Tone::Warning));
        let mut gone = repo("b", "");
        gone.upstream_gone = true;
        assert_eq!(origin_of(gone), measured(0, 0, "Upstream gone", Tone::Warning));
        // Counts win over the states: a checkout with commits says them, not that its upstream is gone.
        let mut ahead_and_gone = ahead(repo("e", ""), 1);
        ahead_and_gone.upstream_gone = true;
        assert_eq!(origin_of(ahead_and_gone), measured(0, 1, "1 ahead", Tone::Plain));
        let mut detached_with_commits = ahead(repo("f", ""), 2);
        detached_with_commits.branch = "detached".to_owned();
        assert_eq!(origin_of(detached_with_commits), measured(0, 2, "2 ahead", Tone::Plain));
        let mut none = repo("c", "");
        none.all_branches_current = false;
        assert_eq!(origin_of(none.clone()), measured(0, 0, "No upstream", Tone::Warning));
        none.remote_new = 1;
        none.remote_branches = vec!["dev".to_owned()];
        assert_eq!(origin_of(none), measured(0, 0, "No upstream, 1 new on dev", Tone::Warning));
        let mut failed = behind(repo("d", ""), 2);
        failed.warning = Some("could not reach the remote".to_owned());
        assert_eq!(origin_of(failed), measured(2, 0, "Fetch failed", Tone::Warning));
    }

    #[test]
    fn a_row_that_failed_says_why() {
        let mut old = repo("old-blog", "");
        old.error = Some("Not found on GitHub".to_owned());
        old.dirty = true;
        let line = only(&repos_of(vec![old]));
        assert_eq!(line.origin, Origin::Failed("Not found on GitHub".to_owned()));
        assert_eq!(line.changes, Changes::Unknown);
    }

    #[test]
    fn a_row_not_read_yet_says_so() {
        let mut fresh = repo("api", "");
        fresh.ready = false;
        let line = only(&repos_of(vec![fresh]));
        assert_eq!(line.origin, Origin::Reading);
        assert_eq!(line.changes, Changes::Unknown);
        assert_eq!(only(&repos_of(vec![repo("api", "")])).changes, Changes::Clean);
        let mut dirty = repo("api", "");
        dirty.dirty = true;
        assert_eq!(only(&repos_of(vec![dirty])).changes, Changes::Uncommitted);
    }

    #[test]
    fn a_running_job_shows_on_its_rows() {
        let mut repos = repos_of(vec![repo("api", ""), repo("docs", ""), repo("web", "")]);
        let mut job = Job::new("Updating", vec!["api".to_owned(), "docs".to_owned()], 0);
        job.on(JobEvent::Start(0));
        job.on(JobEvent::Steps(3, 4));
        repos.job = Some(job);
        let origins: Vec<Origin> = lines(&repos)
            .into_iter()
            .map(|line| match line {
                Line::Repo(repo) => repo.origin,
                Line::Group(_) => panic!("no groups here"),
            })
            .collect();
        assert_eq!(origins[0], Origin::Busy { share: 0.75, text: "3/4".to_owned() });
        assert_eq!(origins[1], Origin::Busy { share: 0.0, text: "waiting".to_owned() });
        assert_eq!(origins[2], measured(0, 0, "Up to date", Tone::Quiet));
    }

    #[test]
    fn pull_requests_are_counted_by_state_and_marked_when_some_wait() {
        let mut repos = repos_of(vec![repo("api", ""), repo("notes", "")]);
        repos.pulls.insert("acme/api".to_owned(), PullCounts { open: 2, merged: 14, closed: 3 });
        repos.pulls_pending.insert(PathBuf::from("/srv/code/api"), PendingPulls { new: 2, changed: 1 });
        let all = lines(&repos);
        let Line::Repo(api) = &all[0] else { panic!("api first") };
        let Line::Repo(notes) = &all[1] else { panic!("notes second") };
        assert_eq!(api.pulls, Pulls { open: 2, merged: 14, closed: 3, new: 2, changed: 1, known: true });
        // No counts from GitHub: not known, and every number is zero.
        assert_eq!(notes.pulls, Pulls { open: 0, merged: 0, closed: 0, new: 0, changed: 0, known: false });
    }

    #[test]
    fn the_chosen_and_ticked_rows_are_marked() {
        let mut repos = repos_of(vec![repo("api", "Work"), repo("docs", "Work"), repo("web", "")]);
        repos.selected = Some("docs".to_owned());
        repos.marked.push("api".to_owned());
        repos.watched.push(PathBuf::from("/srv/code/web"));
        let all = lines(&repos);
        let marks: Vec<(String, bool, bool, bool)> = all
            .iter()
            .filter_map(|line| match line {
                Line::Repo(r) => Some((r.name.clone(), r.watched, r.ticked, r.chosen)),
                Line::Group(_) => None,
            })
            .collect();
        assert_eq!(marks, [("api".to_owned(), false, true, false), ("docs".to_owned(), false, false, true), ("web".to_owned(), true, false, false),]);
        assert!(all.iter().all(|line| !matches!(line, Line::Group(group) if group.chosen)));
        // A highlighted header is the chosen one.
        repos.selected = Some("group::Work".to_owned());
        let all = lines(&repos);
        assert!(group_named(&all, "Work").chosen);
        assert!(!group_named(&all, "No group").chosen);
    }

    #[test]
    fn a_row_says_its_name_and_its_folder() {
        let mut same = repo("api", "");
        same.folder = "work/api".to_owned();
        same.path = PathBuf::from("/srv/code/work/api");
        let line = only(&repos_of(vec![same]));
        assert_eq!((line.key.as_str(), line.name.as_str(), line.branch.as_str()), ("work/api", "work/api", "main"));
        assert_eq!(line.folder, PathBuf::from("/srv/code/work/api").display().to_string());
    }

    #[test]
    fn a_long_path_loses_its_middle() {
        assert_eq!(shorten_path(r"D:\code\work\api", 40), r"D:\code\work\api");
        assert_eq!(shorten_path(r"D:\code\tools\release-scripts", 20), r"D:\…\release-scripts");
        // The terminal app's rule: with the last part too long, as much of its end as fits.
        assert_eq!(shorten_path(r"D:\a-very-long-single-folder-name", 12), r"D:\…der-name");
        assert_eq!(shorten_path("", 12), "");
        assert_eq!(shorten_path("/home/me/code/api", 12), "/…/code/api");
    }

    #[test]
    fn the_status_says_what_the_core_says_and_what_is_switched_on() {
        let mut repos = repos_of(vec![repo("api", ""), repo("docs", ""), repo("web", "")]);
        repos.status = "3 installed. 1 is behind.".to_owned();
        let quiet = status(&repos);
        assert_eq!(quiet.text, "3 installed. 1 is behind.");
        assert!(!quiet.error);
        assert_eq!(quiet.notes, []);
        repos.status = "Could not read the settings".to_owned();
        repos.status_error = true;
        repos.watched.push(PathBuf::from("/srv/code/api"));
        let one = status(&repos);
        assert_eq!((one.text.as_str(), one.error), ("Could not read the settings", true));
        assert_eq!(one.notes, [(Icon::Eye, "Watching 1 repository".to_owned())]);
        repos.watched.push(PathBuf::from("/srv/code/docs"));
        repos.pull_requests = true;
        assert_eq!(
            status(&repos).notes,
            [(Icon::Eye, "Watching 2 repositories".to_owned()), (Icon::GitPullRequest, "Pull request backup is on".to_owned()),]
        );
        repos.watched.clear();
        assert_eq!(status(&repos).notes, [(Icon::GitPullRequest, "Pull request backup is on".to_owned())]);
    }

    #[test]
    fn a_repository_that_left_the_list_leaves_no_line() {
        let mut repos = repos_of(vec![repo("api", "Work"), repo("docs", "Work")]);
        repos.selected = Some("gone".to_owned());
        repos.marked.push("gone".to_owned());
        repos.watched.push(PathBuf::from("/srv/code/gone"));
        // The core counts the watched paths, listed or not.
        assert_eq!(status(&repos).notes, [(Icon::Eye, "Watching 1 repository".to_owned())]);
        let all = lines(&repos);
        assert_eq!(all.len(), 3, "a header and two rows");
        assert!(all.iter().all(|line| match line {
            Line::Repo(r) => r.key != "gone" && !r.chosen && !r.ticked && !r.watched,
            Line::Group(g) => !g.chosen,
        }));
        // A group that is gone is no header either.
        repos.selected = Some("group::Away".to_owned());
        assert!(lines(&repos).iter().all(|line| !matches!(line, Line::Group(g) if g.chosen)));
    }

    #[test]
    fn nothing_that_gets_through_the_filter_is_no_lines() {
        let mut repos = repos_of(Vec::new());
        assert_eq!(lines(&repos), []);
        repos.installed = vec![repo("api", "Work"), repo("docs", "")];
        repos.set_list_filter("zzz");
        assert_eq!(lines(&repos), []);
    }

    #[test]
    fn the_drawings_twelve_read_as_the_design_has_them() {
        let all = lines(&crate::demo::repos());
        assert_eq!(all.len(), 12 + 4, "twelve rows under four headers");
        let by = |name: &str| {
            all.iter()
                .find_map(|line| match line {
                    Line::Repo(repo) if repo.name == name => Some(repo),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("no row {name}"))
        };
        assert_eq!(by("api").origin, measured(4, 0, "4 behind", Tone::Warning));
        assert_eq!(by("api").pulls, Pulls { open: 2, merged: 14, closed: 3, new: 2, changed: 0, known: true });
        assert!(by("api").ticked && by("web").ticked);
        assert_eq!(by("web").origin, measured(0, 0, "3 new on dev", Tone::Plain));
        assert_eq!(by("DevAssist").origin, measured(2, 1, "1 ahead, 2 behind", Tone::Plain));
        assert_eq!(by("old-blog").origin, Origin::Failed("Not found on GitHub".to_owned()));
        assert_eq!(by("sandbox").changes, Changes::Uncommitted);
        assert!(by("homelab").watched);
    }
}
