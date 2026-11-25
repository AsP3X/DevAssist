//! The drawing's story, as the core holds it: twelve repositories in four groups, with their pull requests,
//! ticks and watch, for screenshots and for comparing the window with the design. Nothing here reads a disk or
//! asks a network.
//!
//! The values are those of the design's scripts (`v2-data.mjs`, and the `pulls` table, the renamed
//! `customer-portal-i18n` of `v5-app.mjs`; the paths are the full ones of `v2-data.mjs`, the table shortens them), mapped onto the core's own
//! rows so that the core's own words (`sync_label`, `remote_label`, `state_label`) give the drawing's.

use std::collections::HashMap;
use std::path::PathBuf;

use devassist_core::config::Settings;
use devassist_core::repos::github::PullCounts;
use devassist_core::repos::gitops::{LocalRepo, PendingPulls};
use devassist_core::repos::state::Repos;
use devassist_core::task::job::{Job, JobEvent};

/// The folder, under the system's temporary folder, of the demo's settings path, which is never written.
const SETTINGS_FOLDER: &str = "devassist-gui-demo";
/// The settings file's name in it.
const SETTINGS_FILE: &str = "devassist.toml";
/// The account every demo repository belongs to.
const OWNER: &str = "acme";
/// What the demo says when it has loaded: a fixed picture, not a computed state.
const STATUS: &str = "12 installed. 2 are behind. 1 has new commits on other branches.";
/// What the one that failed says.
const NOT_FOUND: &str = "Not found on GitHub";

/// A repository's pull requests: open, merged, closed without a merge, new to the clone, changed since saved.
type Pulls = (u32, u32, u32, u32, u32);

/// What GitHub says of a repository without pull requests: counts of zero, which the table reads as "None". Every
/// repository of the drawing but the one GitHub does not know has counts.
const NO_PULLS: Pulls = (0, 0, 0, 0, 0);

/// One repository of the drawing.
struct Entry {
    name: &'static str,
    group: &'static str,
    path: &'static str,
    branch: &'static str,
    ahead: u32,
    behind: u32,
    /// Has uncommitted changes.
    dirty: bool,
    /// New commits on another branch: how many, and on which.
    fresh: Option<(u32, &'static str)>,
    /// What the core says went wrong with it.
    error: Option<&'static str>,
    watched: bool,
    ticked: bool,
    pulls: Option<Pulls>,
}

impl Entry {
    const fn new(name: &'static str, group: &'static str, path: &'static str, branch: &'static str) -> Entry {
        Entry {
            name,
            group,
            path,
            branch,
            ahead: 0,
            behind: 0,
            dirty: false,
            fresh: None,
            error: None,
            watched: false,
            ticked: false,
            pulls: Some(NO_PULLS),
        }
    }
}

/// The drawing's twelve, in its order.
const ENTRIES: [Entry; 12] = [
    Entry { pulls: Some((0, 2, 0, 0, 0)), ..Entry::new("dotfiles", "home", "D:\\code\\home\\dotfiles", "main") },
    Entry { watched: true, ..Entry::new("homelab", "home", "D:\\code\\home\\homelab", "main") },
    Entry { ahead: 1, ..Entry::new("recipes", "home", "D:\\code\\home\\recipes", "main") },
    Entry { behind: 4, ticked: true, pulls: Some((2, 14, 3, 2, 0)), ..Entry::new("api", "Work", "D:\\code\\work\\api", "main") },
    Entry::new("customer-portal-i18n", "Work", "D:\\code\\work\\customer-portal-i18n", "main"),
    Entry { dirty: true, pulls: Some((3, 21, 0, 0, 1)), ..Entry::new("portal", "Work", "D:\\code\\work\\portal", "feature/login") },
    Entry { fresh: Some((3, "dev")), ticked: true, pulls: Some((1, 37, 0, 0, 0)), ..Entry::new("web", "Work", "D:\\code\\work\\web", "main") },
    Entry { ahead: 1, behind: 2, pulls: Some((0, 9, 1, 0, 0)), ..Entry::new("DevAssist", "Tools", "D:\\code\\DevAssist", "master") },
    Entry { pulls: Some((1, 6, 0, 0, 0)), ..Entry::new("release-scripts", "Tools", "D:\\code\\tools\\release-scripts", "main") },
    Entry { ahead: 1, ..Entry::new("notes", "", "D:\\code\\notes", "main") },
    Entry { error: Some(NOT_FOUND), pulls: None, ..Entry::new("old-blog", "", "E:\\archive\\old-blog", "main") },
    Entry { dirty: true, ..Entry::new("sandbox", "", "D:\\code\\sandbox", "experiment/wasm") },
];

/// The row the core holds for `entry`.
fn local(entry: &Entry) -> LocalRepo {
    let folder = entry.path.rsplit('\\').next().unwrap_or(entry.path).to_owned();
    LocalRepo {
        folder,
        owner: OWNER.to_owned(),
        repo: entry.name.to_owned(),
        origin: format!("https://github.com/{OWNER}/{}.git", entry.name),
        path: PathBuf::from(entry.path),
        group: entry.group.to_owned(),
        branch: entry.branch.to_owned(),
        dirty: entry.dirty,
        ahead: Some(entry.ahead),
        behind: Some(entry.behind),
        total_ahead: entry.ahead,
        total_behind: entry.behind,
        all_branches_current: true,
        behind_branches: u32::from(entry.behind > 0),
        remote_new: entry.fresh.map_or(0, |(count, _)| count),
        remote_branches: entry.fresh.map(|(_, branch)| branch.to_owned()).into_iter().collect(),
        ready: true,
        fetched: true,
        error: entry.error.map(str::to_owned),
        ..LocalRepo::default()
    }
}

/// The demo's settings: every one at its default (the default look among them), with the drawing's checkouts, and
/// a settings path under the system's temporary folder, never the person's settings file. Nothing is written
/// there: the demo's [`repos`] hold all work, and a held core writes no settings. Reads nothing.
#[must_use]
pub fn settings() -> Settings {
    // The core keeps a watched repository only while it is among the checkouts, also when it saves a fold.
    let checkouts = ENTRIES.iter().map(|entry| PathBuf::from(entry.path)).collect();
    Settings::for_folder(checkouts, std::env::temp_dir().join(SETTINGS_FOLDER).join(SETTINGS_FILE))
}

/// The drawing's twelve repositories in four groups, with its pull request counts, ticks and watch, reading no disk
/// and no network, with the demo's [`settings`]. All work is held (`Repos::hold_work`): whatever a person asks of
/// them, nothing is run, fetched, pulled, opened or written, and the status line says so.
#[must_use]
pub fn repos() -> Repos {
    let mut repos = Repos::offline(settings());
    repos.hold_work();
    // The core keeps pull request counts under the owner and the repository, in lower case.
    let mut pulls = HashMap::new();
    for entry in &ENTRIES {
        let row = local(entry);
        if let Some((open, merged, closed, new, changed)) = entry.pulls {
            pulls.insert(format!("{OWNER}/{}", entry.name).to_lowercase(), PullCounts { open, merged, closed });
            if new + changed > 0 {
                repos.pulls_pending.insert(row.path.clone(), PendingPulls { new, changed });
            }
        }
        if entry.ticked {
            repos.marked.push(row.folder.clone());
        }
        if entry.watched {
            repos.watched.push(row.path.clone());
        }
        if !row.group.is_empty() {
            repos.placement.push((row.path.clone(), row.group.clone()));
        }
        repos.installed.push(row);
    }
    repos.pulls = pulls;
    STATUS.clone_into(&mut repos.status);
    repos
}

/// The overlays `--show` can open in a demo, by name, each with what sets it up in the demo's repositories.
/// Another overlay is one more line here.
pub const SHOWS: [(&str, Opener); 1] = [("progress", show_progress)];

/// What sets an overlay up in the demo's repositories.
pub type Opener = fn(&mut Repos);

/// Opens the overlay `name` (one of [`SHOWS`]) in the demo's `repos`, through the core's own methods.
///
/// # Errors
/// A sentence naming the overlays there are, when `name` is none of them.
pub fn show(repos: &mut Repos, name: &str) -> Result<(), String> {
    let (_, open) = SHOWS.iter().find(|(known, _)| *known == name).ok_or_else(|| show_error(name))?;
    open(repos);
    Ok(())
}

/// What `--show` says of a name it does not know.
#[must_use]
pub fn show_error(name: &str) -> String {
    let names: Vec<&str> = SHOWS.iter().map(|(name, _)| *name).collect();
    format!("--show knows {}, not {name}", names.join(", "))
}

/// The drawing's update: api at three of its four branches, web waiting.
const RUN_TITLE: &str = "Updating 2 repositories";
/// The repositories of the drawing's update.
const RUN_NAMES: [&str; 2] = ["api", "web"];
/// How far api is: branches done, of the branches with news.
const RUN_STEPS: (u32, u32) = (3, 4);
/// What git printed for api so far, as the drawing has it.
const RUN_OUTPUT: [&str; 7] = [
    "From https://github.com/acme/api",
    "   4c1e2aa..9d03b71  main        -> origin/main",
    "   77ab120..0e5f9c3  release/2.4 -> origin/release/2.4",
    " * [new branch]      hotfix/tls-renewal -> origin/hotfix/tls-renewal",
    "main: fast-forwarded 4 commits",
    "release/2.4: fast-forwarded 1 commit",
    "feature/audit-log: left alone, it has your commits",
];

/// The drawing's update in progress, as the core keeps a run: made with the core's `Job` and its events, so nothing
/// runs. The core is not busy with it (no worker), so it waits for nothing and never ends; its dialog is open.
fn show_progress(repos: &mut Repos) {
    let mut job = Job::new(RUN_TITLE, RUN_NAMES.map(str::to_owned).to_vec(), 0);
    job.on(JobEvent::Start(0));
    job.on(JobEvent::Steps(RUN_STEPS.0, RUN_STEPS.1));
    for line in RUN_OUTPUT {
        job.on(JobEvent::Line(line.to_owned(), false));
    }
    repos.job = Some(job);
    repos.show_job();
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::Mode;
    use devassist_core::task::job::ItemState;

    use super::*;

    #[test]
    fn the_demos_run_is_the_drawings_and_waits_for_nothing() {
        let mut repos = repos();
        show(&mut repos, "progress").unwrap();
        assert!(repos.mode == Mode::Progress);
        let job = repos.job.as_ref().unwrap();
        assert_eq!(job.title, "Updating 2 repositories");
        assert_eq!(job.summary().to_string(), "1 of 2  api");
        assert_eq!((job.items[0].short_state(), job.items[0].bar_fill()), ("3/4".to_owned(), Some(0.75)));
        assert_eq!((job.items[1].state, job.items[1].short_state()), (ItemState::Waiting, "waiting".to_owned()));
        assert_eq!(job.items[0].output.len(), 7);
        assert!(job.running(), "it shows a run that goes on");
        // Yet the core waits for nothing: the window sleeps, and the dialog stays.
        assert!(!repos.busy && !repos.waiting() && repos.work_held());
        for _ in 0..5 {
            repos.drain();
        }
        assert!(repos.mode == Mode::Progress && !repos.waiting());
        assert_eq!(show(&mut repos, "nothing").unwrap_err(), "--show knows progress, not nothing");
    }

    #[test]
    fn the_demo_is_the_drawings_story() {
        let repos = repos();
        let api = repos.installed.iter().find(|r| r.folder == "api").unwrap();
        assert_eq!((api.sync_counts(), api.group.as_str()), ((0, 4), "Work"));
        assert!(repos.is_marked_local("api") && repos.is_marked_local("web"));
        assert_eq!(repos.watched_count(), 1);
        assert_eq!(repos.status, "12 installed. 2 are behind. 1 has new commits on other branches.");
        assert!(repos.config_path.starts_with(std::env::temp_dir())); // never the real settings, though nothing is written
        assert!(repos.work_held());
    }

    #[test]
    fn the_core_words_the_demo_as_the_drawing_does() {
        let repos = repos();
        let by = |name: &str| repos.installed.iter().find(|r| r.repo == name).unwrap();
        assert_eq!(by("api").sync_label(), "behind 4");
        assert_eq!(by("web").sync_label(), "3 new on dev");
        assert_eq!(by("DevAssist").sync_label(), "ahead 1, behind 2");
        assert_eq!(by("recipes").sync_label(), "ahead 1");
        assert_eq!(by("homelab").sync_label(), "up to date");
        assert_eq!(by("old-blog").sync_label(), "Not found on GitHub");
        assert_eq!(by("old-blog").state_label(), "error");
        assert_eq!(by("portal").state_label(), "dirty");
        assert_eq!(by("sandbox").state_label(), "dirty");
        assert_eq!(by("dotfiles").state_label(), "clean");
        assert_eq!(by("web").remote_label().as_deref(), Some("3 new on dev"));
        assert_eq!(by("customer-portal-i18n").folder, "customer-portal-i18n");
        assert_eq!(by("release-scripts").folder, "release-scripts");
        assert_eq!(by("DevAssist").folder, "DevAssist");
        assert!(repos.installed.iter().all(|r| r.ready));
        assert!(repos.installed.iter().all(|r| !r.path.to_string_lossy().contains('\u{2026}')));
    }

    #[test]
    fn the_demo_has_the_drawings_pull_requests_and_nothing_to_wait_for() {
        let repos = repos();
        let by = |name: &str| repos.installed.iter().find(|r| r.repo == name).unwrap();
        assert_eq!(repos.pull_counts(by("api")), Some(PullCounts { open: 2, merged: 14, closed: 3 }));
        assert_eq!(repos.pending_pulls(by("api")), PendingPulls { new: 2, changed: 0 });
        assert_eq!(repos.pending_pulls(by("portal")), PendingPulls { new: 0, changed: 1 });
        // Zeros where the drawing says "None"; no counts only for the one GitHub does not know.
        assert_eq!(repos.pull_counts(by("notes")), Some(PullCounts::default()));
        assert_eq!(repos.pull_counts(by("homelab")), Some(PullCounts::default()));
        assert_eq!(repos.pull_counts(by("old-blog")), None);
        assert_eq!(repos.installed.iter().filter(|r| repos.pull_counts(r).is_none()).count(), 1);
        assert!(!repos.pending_pulls(by("web")).any());
        assert!(!repos.waiting());
        assert_eq!(repos.groups(), ["home", "Tools", "Work"]);
        assert_eq!(repos.installed.iter().filter(|r| r.group.is_empty()).count(), 3);
    }
}
