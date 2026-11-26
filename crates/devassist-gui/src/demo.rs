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
use devassist_core::repos::github::{PullCounts, RemoteRepo};
use devassist_core::repos::gitops::{LocalRepo, PendingPulls};
use devassist_core::repos::state::{Action, MenuItem, MigrateItem, Repos};
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
pub const SHOWS: [(&str, Opener); 10] = [
    ("progress", show_progress),
    ("remove", show_remove),
    ("group", show_group),
    ("menu", show_menu),
    ("migrate", show_migrate),
    ("add", show_add),
    (ADD_FOLDER, show_add),
    ("import", show_import),
    ("move", show_move),
    ("push", show_push),
];

/// The name `--show` opens the Add dialog by with the keyboard in its folder field, so a picture shows the folders
/// it suggests (the window gives the field the focus once the dialog is open).
pub const ADD_FOLDER: &str = "add-folder";

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

/// The drawing's remove dialog: for the ticked api and web, as the core asks.
fn show_remove(repos: &mut Repos) {
    repos.begin_remove();
}

/// The group dialog (the design draws none): moving the ticked api and web, their group in the field.
fn show_group(repos: &mut Repos) {
    repos.open_group_prompt();
}

/// The row's menu, on the first ticked row (api), for both ticked rows, as Enter on that row opens it. (The design
/// opens it on homelab, which is not ticked, and keeps the ticks; in the window the "…" of a row that is not ticked
/// clears them.)
fn show_menu(repos: &mut Repos) {
    if let Some(first) = repos.marked.first().cloned() {
        repos.pick(&first);
    }
    repos.enter_row();
}

/// The row's menu as [`show_menu`] opens it, with Migrate highlighted and its side menu open, as the drawing has it
/// (the drawing opens it on homelab, see [`show_menu`]): opened through the core's own methods.
fn show_migrate(repos: &mut Repos) {
    show_menu(repos);
    repos.highlight_menu(MenuItem::Migrate);
    repos.run_menu(MenuItem::Migrate);
}

/// The folder the demo's made-up folders are in: the drawing's `D:\code` (`/srv/code` on other systems).
pub const CODE_DIR: &str = if cfg!(windows) { r"D:\code" } else { "/srv/code" };

/// The folder the demo's Add dialog offers for a clone: the drawing's `D:\code\work`, one of [`FOLDERS`].
pub const CLONE_DIR: &str = if cfg!(windows) { r"D:\code\work" } else { "/srv/code/work" };

/// The made-up folders in [`CODE_DIR`] the demo's folder fields suggest. The demo never asks the disk for folders:
/// what it suggests is only these ([`folders_matching`]).
const FOLDERS: [&str; 4] = ["forks", "play", "work", "work-archive"];

/// The demo's made-up folders that match `text` as the core's `prompt::suggest` matches folders on disk: those in
/// the folder `text` names up to its last separator, whose names start with what follows it, ignoring case; full
/// paths, sorted. Quotes and spaces around `text` are dropped. Reads nothing.
#[must_use]
pub fn folders_matching(text: &str) -> Vec<String> {
    let same = |path: &str| path.replace(['/', '\\'], std::path::MAIN_SEPARATOR_STR).to_lowercase();
    let text = text.trim().trim_matches('"').trim();
    let Some(cut) = text.rfind(['/', '\\']) else { return Vec::new() };
    let (head, typed) = text.split_at(cut + 1);
    let parent = format!("{CODE_DIR}{}", std::path::MAIN_SEPARATOR);
    if same(head) != same(&parent) {
        return Vec::new();
    }
    let typed = typed.to_lowercase();
    FOLDERS.iter().filter(|name| name.to_lowercase().starts_with(&typed)).map(|name| format!("{parent}{name}")).collect()
}

/// What the demo's GitHub knows: a made-up list of eight, by owner, name, description, archived and private.
/// Two of them are installed (acme's api and portal); one is of another owner, so every row names its owner.
const CATALOG: [(&str, &str, &str, bool, bool); 8] = [
    (OWNER, "api", "The public HTTP API", false, false),
    (OWNER, "artcraft", "Tools for drawing and exporting canvas designs", false, false),
    (OWNER, "artcraft-docs", "The documentation site of artcraft", false, false),
    (OWNER, "artcraft-plugins", "Example plugins and the plugin template", false, false),
    (OWNER, "billing-legacy", "The billing service before the rewrite", true, false),
    (OWNER, "design-tokens", "Colours, sizes and type of every product", false, true),
    ("octo-labs", "glass-kit", "A liquid glass toolkit for egui", false, false),
    (OWNER, "portal", "The customer portal", false, false),
];

/// The repositories the demo's Add dialog opens with ticked.
const ADD_TICKED: [&str; 2] = ["acme/artcraft", "acme/artcraft-docs"];

/// The Add dialog as the drawing has it: the demo's catalog of eight found, two of them installed and two ticked, the
/// folder field at [`CLONE_DIR`]. Opened through the core (`Action::Add`), as the window opens it.
fn show_add(repos: &mut Repos) {
    repos.remotes = CATALOG
        .iter()
        .map(|&(owner, name, description, archived, private)| RemoteRepo {
            owner: owner.to_owned(),
            name: name.to_owned(),
            description: description.to_owned(),
            clone_url: format!("https://github.com/{owner}/{name}.git"),
            language: String::new(),
            archived,
            private,
            pushed: String::new(),
            direct: false,
        })
        .collect();
    // The demo's own account, so the status line counts what it found there, as the core words it.
    repos.owners = vec![OWNER.to_owned()];
    repos.clone_dir = PathBuf::from(CLONE_DIR);
    repos.run(Action::Add);
    for id in ADD_TICKED {
        repos.toggle_add_mark_of(id);
    }
    repos.ensure_selection();
}

/// The Import dialog, opened through the core (`Action::Import`) as `i` and the top row's "Import" open it, its
/// folder field at the made-up [`CODE_DIR`] (the core starts it at the clone folder, which a demo keeps made-up too).
fn show_import(repos: &mut Repos) {
    repos.clone_dir = PathBuf::from(CODE_DIR);
    repos.run(Action::Import);
}

/// The made-up folder the demo's Move dialog moves into: the drawing's `E:\work`, on another drive than the demo's
/// checkouts, so the note on another drive shows on Windows. On other systems it is `/mnt/work`, and no note shows:
/// the held core cannot tell a mount point from a path's text.
pub const MOVE_DIR: &str = if cfg!(windows) { r"E:\work" } else { "/mnt/work" };

/// The drawing's Move dialog: the ticked api and web, opened as the Migrate menu's move line opens it, its folder
/// [`MOVE_DIR`]. The held core tells where each goes, and on Windows that the drive is another, from the text alone:
/// nothing reads the disk.
fn show_move(repos: &mut Repos) {
    repos.run_migrate(MigrateItem::Folder);
    repos.set_path_input(MOVE_DIR);
}

/// The made-up address the demo's Push dialog has typed: the drawing's, on Codeberg, for the demo's account.
pub const PUSH_ADDRESS: &str = "git@codeberg.org:acme/api.git";

/// The drawing's Push dialog for api, opened as the Migrate menu's push line opens it: on the ticked api alone (web's
/// tick is taken off: each repository goes to an address of its own), with [`PUSH_ADDRESS`] typed. A push asked for
/// there is refused by the held core, which looks at nothing first: the address names its host.
fn show_push(repos: &mut Repos) {
    repos.toggle_mark_of("web");
    repos.run_migrate(MigrateItem::Service);
    repos.set_service_input(PUSH_ADDRESS);
}

#[cfg(test)]
mod tests {
    use devassist_core::repos::state::Mode;
    use devassist_core::task::job::ItemState;
    use egui::{Event, Key, Modifiers};

    use crate::dialogs::Which;
    use crate::dialogs::testing::Window;
    use crate::repos_screen::{self, Command};
    use crate::{menu, table};

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
        assert_eq!(
            show(&mut repos, "nothing").unwrap_err(),
            "--show knows progress, remove, group, menu, migrate, add, add-folder, import, move, push, not nothing"
        );
    }

    #[test]
    fn the_demo_opens_the_remove_and_group_dialogs_as_the_core_asks() {
        let mut repos = repos();
        show(&mut repos, "remove").unwrap();
        assert!(repos.mode == Mode::ConfirmRemove { folders: vec!["api".to_owned(), "web".to_owned()] });
        assert!(repos.remove_lines().iter().all(|line| !line.dirty()));
        let mut repos = super::repos();
        show(&mut repos, "group").unwrap();
        assert!(repos.mode == Mode::Group);
        let prompt = repos.group_prompt.as_ref().unwrap();
        assert_eq!((prompt.repos.as_slice(), prompt.input.as_str()), (["api".to_owned(), "web".to_owned()].as_slice(), "Work"));
    }

    #[test]
    fn the_demo_opens_add_with_eight_found_two_installed_and_two_ticked() {
        let mut repos = repos();
        show(&mut repos, "add").unwrap();
        assert!(repos.mode == Mode::Add);
        let found = repos.add_list();
        assert_eq!(found.len(), 8);
        assert_eq!(found.iter().filter(|remote| repos.is_installed(remote)).count(), 2);
        assert_eq!(repos.add_marked.iter().map(RemoteRepo::id).collect::<Vec<_>>(), ADD_TICKED);
        assert_eq!(repos.add_selected.as_deref(), Some("acme/artcraft"));
        assert!(!repos.waiting() && repos.work_held());
        assert_eq!(repos.add_status().text, "8 repositories · 2 cloned · type owner/ or paste a link to list any account");
    }

    #[test]
    fn the_demo_opens_move_for_api_and_web_onto_another_drive() {
        let mut repos = repos();
        show(&mut repos, "move").unwrap();
        assert!(repos.mode == Mode::Path);
        let lines = repos.move_lines();
        assert_eq!(lines.iter().map(|line| line.name.as_str()).collect::<Vec<_>>(), ["api", "web"]);
        assert!(lines.iter().all(|line| line.to.as_ref().is_some_and(|to| to.starts_with(MOVE_DIR)) && !line.there && !line.taken));
        if cfg!(windows) {
            // The drawing's note: E: is another drive than the demo's D:, told without the disk.
            assert_eq!(repos.move_crosses_volumes(), Some(true));
        }
        assert!(repos.work_held());
    }

    #[test]
    fn the_demo_opens_push_for_api_with_an_address_typed() {
        let mut repos = repos();
        show(&mut repos, "push").unwrap();
        assert!(repos.mode == Mode::Service);
        let prompt = repos.service_prompt.as_ref().unwrap();
        assert_eq!((prompt.repo.as_str(), prompt.input.as_str(), prompt.error.as_ref()), ("api", PUSH_ADDRESS, None));
        assert_eq!(repos.marked, ["api"]);
        assert!(repos.work_held());
        // Pushed: the held core closes it and refuses the work, saying so; the tick stays.
        repos.accept_service();
        assert!(repos.mode == Mode::List && repos.service_prompt.is_none());
        assert_eq!(repos.status, devassist_core::repos::state::HELD);
        assert!(repos.queue.is_empty() && !repos.busy && repos.job.is_none());
        assert_eq!(repos.marked, ["api"]);
    }

    #[test]
    fn the_demo_opens_the_menu_on_its_first_ticked_row_for_both() {
        let mut repos = repos();
        show(&mut repos, "menu").unwrap();
        assert!(repos.mode == Mode::Menu);
        assert_eq!(repos.selected.as_deref(), Some("api"));
        assert_eq!(repos.chosen(), ["api", "web"]);
    }

    #[test]
    fn the_demo_opens_the_menu_with_its_migrate_menu_open() {
        let mut repos = repos();
        show(&mut repos, "migrate").unwrap();
        assert!(repos.mode == Mode::Menu);
        assert_eq!(repos.chosen(), ["api", "web"]);
        assert_eq!(repos.menu_highlight(), Some(devassist_core::repos::state::MenuItem::Migrate));
        assert_eq!(repos.migrate_highlight(), Some(MigrateItem::Folder));
        assert!(repos.work_held());
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

    /// What one press in the walk of [`a_demo_runs_nothing_whatever_is_pressed`] may do.
    #[derive(Clone, Copy, Debug)]
    enum Expect {
        /// It asks for work: the core refuses it and the status line says so (`HELD`).
        Held,
        /// It changes nothing but what a demo changes in memory on purpose (the ticks, the highlight, the sort, the
        /// filter, the folds, a group, a dialog or the menu opened or closed), or nothing at all.
        Free,
        /// Forget, which takes these off the list in memory; their folders are nowhere.
        Forgot(&'static [&'static str]),
        /// On a demo emptied on purpose (no repositories), nothing asked for work, as [`Expect::Free`].
        Empty,
    }

    /// How often the process has reached out so far: programs, the clipboard, the network, folders deleted; and how
    /// often this thread's dialogs asked the disk for the folders a folder field suggests (`prompt::suggest`, through
    /// `dialogs::folder_suggestions`), which a held session never does: it suggests the demo's own folders. Not
    /// counted, and the reads that remain in a held session: the checks of a typed folder when Import's or Move's
    /// Confirm or Add's Clone is pressed (`prompt::resolve`'s `is_file`, and `is_dir` for Import and Move), and the
    /// Push dialog's `is_dir` of an address that is a plain full path (`gitops::push_address`, asked for its facts as
    /// the text changes, and on Push).
    fn reached_now() -> [usize; 5] {
        use devassist_core::sys::{Reach, reached};
        let [program, clipboard, network, disk] = [Reach::Program, Reach::Clipboard, Reach::Network, Reach::Disk].map(reached);
        [program, clipboard, network, disk, crate::dialogs::disk_reads()]
    }

    /// The walk's window, on a fresh demo with `start` done to it first.
    fn fresh(window: &mut Window, start: fn(&mut Repos)) {
        let mut repos = repos();
        start(&mut repos);
        window.restart(crate::session::Session::demo(repos));
    }

    /// Checks, after the press `what`, that the demo ran and wrote nothing and reached nothing since `before`.
    fn still_held(window: &Window, what: &str, expect: Expect, before: [usize; 5]) {
        let repos = window.repos();
        assert!(repos.work_held(), "{what}: the demo's work is no longer held");
        assert!(!repos.busy && !repos.fetching && !repos.waiting(), "{what}: the core is busy");
        assert!(repos.queue.is_empty(), "{what}: work was queued: {}", repos.queue.len());
        let paths: Vec<PathBuf> = repos.installed.iter().map(|repo| repo.path.clone()).collect();
        let gone: &[&str] = if let Expect::Forgot(gone) = expect { gone } else { &[] };
        let kept: Vec<PathBuf> = ENTRIES
            .iter()
            .filter(|entry| !gone.contains(&entry.name) && !matches!(expect, Expect::Empty))
            .map(|entry| PathBuf::from(entry.path))
            .collect();
        assert_eq!(paths, kept, "{what}: the demo's rows changed");
        if matches!(expect, Expect::Held) {
            assert_eq!(repos.status, devassist_core::repos::state::HELD, "{what}: refused without saying so");
        }
        // The held core saves nothing (`save_config` returns before the write): the demo's settings file is never made.
        assert!(!repos.config_path.exists(), "{what}: the demo wrote its settings");
        assert_eq!(
            reached_now(),
            before,
            "{what}: a program ran, the clipboard was written, the network asked, a folder deleted or the disk asked for folders"
        );
    }

    /// The core's words when Update all is asked for with no repositories on the list.
    const NOTHING_TO_UPDATE: &str = "There are no repositories to update.";

    /// What the status line may say after a press.
    #[derive(Clone, Copy, Debug)]
    enum Says<'a> {
        /// The demo's own line, or that the held core refused what was asked (`HELD`).
        HeldOrNothing,
        /// That too, or this refusal of the core's in its own words: a press meant to be refused.
        OrRefused(&'a str),
    }

    /// Checks, after the press `what`, that the status line says what `says` allows, and no other words: a refusal
    /// only where the press is meant to be refused, and then in the core's exact words, and never an error otherwise.
    fn check_said(window: &Window, what: &str, says: Says<'_>) {
        let repos = window.repos();
        let quiet = (repos.status == STATUS || repos.status == devassist_core::repos::state::HELD) && !repos.status_error;
        let refused = matches!(says, Says::OrRefused(words) if repos.status == words && repos.status_error);
        assert!(quiet || refused, "{what}: the status line says {:?} (an error: {}), allowed {says:?}", repos.status, repos.status_error);
    }

    /// Checks, after the press `what`, that the status line says the demo's line or the held notice, nothing else.
    fn says_held_or_nothing(window: &Window, what: &str) {
        check_said(window, what, Says::HeldOrNothing);
    }

    /// The core's words when a push is asked for with two repositories chosen (the menu's Push on the ticked api and
    /// web): it pushes one repository at a time. Taken from the core on the demo as it starts.
    fn push_for_two_refused() -> String {
        let mut repos = repos();
        repos.run_migrate(MigrateItem::Service);
        assert!(repos.status_error && repos.service_prompt.is_none(), "the core did not refuse a push for two");
        repos.status
    }

    /// Whether `press` asks the menu's side menu (open on api and web) for its Push line: its letter typed.
    fn types_the_push_letter(press: &[Event]) -> bool {
        let letter = MigrateItem::Service.key();
        press.iter().any(|event| matches!(event, Event::Text(typed) if typed.eq_ignore_ascii_case(&letter.to_string())))
    }

    /// A key pressed with `modifiers`.
    fn key_event(key: Key, modifiers: Modifiers, pressed: bool) -> Event {
        Event::Key { key, physical_key: None, pressed, repeat: false, modifiers }
    }

    /// A dialog of the walk: its name, what opens it on a fresh demo, and its buttons with what each may do.
    type Dialog = (&'static str, fn(&mut Repos), &'static [(&'static str, Expect)]);

    /// Leaves the demo as it starts.
    fn unchanged(_: &mut Repos) {}

    #[test]
    fn a_demo_runs_nothing_whatever_is_pressed() {
        // The tests that run git on real checkouts take turns with this one: the counts are the whole process's.
        let _live = crate::session::tests::live_git();
        let before = reached_now();
        let mut window = Window::new(crate::session::Session::demo(repos()));
        press_every_key(&mut window, before);
        press_every_key_in_each_overlay(&mut window, before);
        for size in [crate::theme::WINDOW, crate::theme::MIN_WINDOW, NARROW, NARROWEST] {
            press_every_control(&mut window, before, size, false);
        }
        press_every_control(&mut window, before, crate::theme::WINDOW, true);
        press_the_keys_of_the_empty_demo(&mut window, before);
        run_every_line_of_the_menu(&mut window, before);
        run_every_line_of_the_side_menu(&mut window, before);
        press_every_button_of_the_dialogs(&mut window, before);
        click_everything_in_each_overlay(&mut window, before);
        type_into_every_field(&mut window, before);
        press_retry_in_add(&mut window, before);
        open_each_overlay_in_turn_on_one_window(before);
    }

    /// Windows narrower than the app allows, where the top row gives way: its capsule's icons only, then the main
    /// button's icon too. Their controls are the same.
    const NARROW: [f32; 2] = [900.0, 600.0];
    /// The narrower of the two.
    const NARROWEST: [f32; 2] = [780.0, 600.0];

    /// The demo with no repositories: its panel's two lines, the top row's Import and Add repository.
    fn emptied(repos: &mut Repos) {
        repos.installed.clear();
        repos.marked.clear();
        repos.selected = None;
    }

    /// The Add dialog with GitHub failed (the test's seam: the catalog's failed state) and a plain word typed, so its
    /// status line has Retry and its box the row that lists that account.
    fn show_add_failed(repos: &mut Repos) {
        show_add(repos);
        repos.remotes_state = devassist_core::repos::state::RemoteState::Failed("GitHub answered 502".to_owned());
        repos.set_add_filter("zzzz");
    }

    /// The Add dialog searched for "docs": a ticked repository is hidden, and the line above the buttons names it.
    fn show_add_searched(repos: &mut Repos) {
        show_add(repos);
        repos.set_add_filter("docs");
    }

    /// What is done to the walk's window once its overlay is open: nothing, or the keyboard given to the Add dialog's
    /// folder field, whose rows of folders then show (as `--show add-folder` does).
    type Prepare = fn(&mut Window);

    /// Leaves the overlay as it opens.
    fn as_opened(_: &mut Window) {}

    /// Gives the Add dialog's folder field the keyboard inside a frame, as the window does for `--show add-folder`; its
    /// rows show.
    fn into_the_folder_field(window: &mut Window) {
        let ctx = window.ctx.clone();
        window.between =
            Some(Box::new(move |session| assert!(crate::dialogs::focus_add_folder(&ctx, session.repos()), "the Add dialog is not open")));
        window.frame(Vec::new());
        window.frame(Vec::new());
        assert!(
            window.add_folder_rows_open(),
            "the folder field's rows did not open: {:?} {:?}",
            window.ctx.memory(egui::Memory::focused),
            window.overlay_rect()
        );
    }

    /// The overlays the walk opens, by name, each with what opens it on a fresh demo and what is done once it is open:
    /// every overlay `--show` opens, and the Add dialog with GitHub failed (Retry) and searched (a hidden tick).
    fn walked_overlays() -> Vec<(&'static str, Opener, Prepare)> {
        let mut overlays: Vec<(&'static str, Opener, Prepare)> =
            SHOWS.iter().map(|&(name, open)| (name, open, if name == ADD_FOLDER { into_the_folder_field as Prepare } else { as_opened })).collect();
        overlays.push(("add failed", show_add_failed, as_opened));
        overlays.push(("add searched", show_add_searched, as_opened));
        overlays
    }

    /// The walk's window on a fresh demo with the overlay `open` opened and `prepare` done, showing what the walk's
    /// step says it shows ([`shows_what_it_claims`]).
    fn fresh_overlay(window: &mut Window, name: &str, open: Opener, prepare: Prepare) {
        fresh(window, open);
        assert!(window.open(), "the {name} overlay did not open");
        prepare(window);
        shows_what_it_claims(window, name);
    }

    /// Whether the last frame painted a text that starts with `words`.
    fn painted(window: &Window, words: &str) -> bool {
        window.texts.iter().any(|(said, _, _)| said.starts_with(words))
    }

    /// Checks that the overlay `name` of [`walked_overlays`] shows what its step presses: Add failed its Retry and the
    /// row that lists the typed account (the Add dialog's own words, `List github.com/{word}`); Add searched the line
    /// that names the ticked repository the search hides; the menu with its side menu that pane and its two lines;
    /// the Add dialog's folder field its rows; each dialog its own kind.
    fn shows_what_it_claims(window: &Window, name: &str) {
        let dialog = match name {
            "progress" => Which::Progress,
            "remove" => Which::Remove,
            "group" => Which::Group,
            "menu" | "migrate" => Which::Menu,
            "import" | "move" => Which::Folder,
            "push" => Which::Push,
            _ => Which::Add,
        };
        assert!(window.shows(dialog), "the {name} overlay is no {dialog:?}");
        match name {
            "add failed" => {
                assert!(window.add_retry().is_some(), "add failed: no Retry");
                assert!(painted(window, "List github.com/zzzz"), "add failed: no row that lists zzzz");
            }
            "add searched" => assert!(painted(window, "Also ticked: acme/artcraft"), "add searched: no line of the hidden tick"),
            "migrate" => {
                assert!(window.submenu_pane().is_some(), "migrate: the side menu is not drawn");
                for item in MigrateItem::ALL {
                    assert!(painted(window, item.label()), "migrate: the side menu has no {}", item.label());
                }
            }
            ADD_FOLDER => assert!(window.add_folder_rows_open(), "add-folder: the folder field's rows are not shown"),
            _ => {}
        }
    }

    /// Every key there is, alone and with Shift, Ctrl or Alt, and every letter, figure and sign typed, each with
    /// what lets it go; and the filter with a letter typed into it in the same frame.
    fn every_input() -> Vec<(Vec<Event>, Vec<Event>)> {
        let modifiers = [Modifiers::NONE, Modifiers::SHIFT, Modifiers::COMMAND, Modifiers::ALT];
        let mut inputs: Vec<(Vec<Event>, Vec<Event>)> =
            Key::ALL.iter().flat_map(|&key| modifiers.map(|held| (vec![key_event(key, held, true)], vec![key_event(key, held, false)]))).collect();
        inputs.extend((' '..='~').map(|typed| (vec![Event::Text(typed.to_string())], Vec::new())));
        inputs.push((vec![Event::Text("/".to_owned()), Event::Text("u".to_owned())], Vec::new()));
        inputs
    }

    /// Every key there is, and every letter, figure and sign typed, each on a fresh demo.
    fn press_every_key(window: &mut Window, before: [usize; 5]) {
        let on_api: fn(&mut Repos) = |repos| repos.pick("api");
        // Every input, on a demo whose highlight is on a group's header and on a ticked row: whatever the screen's
        // keys are now or later, each is pressed. Each starts from a fresh demo, and is let go after.
        let inputs = every_input();
        let mut kinds = [false; Command::KINDS];
        for start in [unchanged, on_api] {
            for (press, release) in &inputs {
                fresh(window, start);
                let commands = repos_screen::commands(press);
                for command in &commands {
                    kinds[command.kind()] = true;
                }
                window.frame(press.clone());
                window.frame(release.clone());
                window.frame(Vec::new());
                // The keys that ask for work: the rest move, tick, sort, filter, fold, or open a dialog or the menu.
                let asks = commands
                    .iter()
                    .any(|command| matches!(command, Command::UpdateChosen | Command::UpdateAll | Command::Watch | Command::Open | Command::Refresh));
                still_held(window, &format!("{press:?}"), if asks { Expect::Held } else { Expect::Free }, before);
            }
        }
        let missed: Vec<usize> = (0..Command::KINDS).filter(|kind| !kinds[*kind]).collect();
        assert!(missed.is_empty(), "the walk pressed no key for the commands {missed:?}");
    }

    /// Every input of [`every_input`] in each overlay of [`walked_overlays`] (the progress, remove and group dialogs,
    /// the menu and its side menu, Add with its search and with its folder field, Import, Move and Push), each on a
    /// fresh demo with that overlay open: whatever a key does there (a line of the menu, a button with the focus, a
    /// field), nothing runs, is written or reaches out. Enter in the Add dialog's search and in the Push dialog's field
    /// asks for the clone and the push, which the held core refuses, saying so.
    fn press_every_key_in_each_overlay(window: &mut Window, before: [usize; 5]) {
        let inputs = every_input();
        let enter = vec![key_event(Key::Enter, Modifiers::NONE, true)];
        let push_refused = push_for_two_refused();
        for (name, open, prepare) in walked_overlays() {
            for (press, release) in &inputs {
                fresh_overlay(window, name, open, prepare);
                window.frame(press.clone());
                window.frame(release.clone());
                window.frame(Vec::new());
                let what = format!("{press:?} in the {name} overlay");
                let asks = *press == enter && ["add", "push"].contains(&name);
                still_held(window, &what, if asks { Expect::Held } else { Expect::Free }, before);
                // The older overlays change what a demo changes in memory, and say so (a group moved). In the side menu
                // on api and web, its Push letter is refused: one repository is pushed at a time.
                if name == "migrate" && types_the_push_letter(press) {
                    check_said(window, &what, Says::OrRefused(&push_refused));
                } else if !OLDER_OVERLAYS.contains(&name) {
                    says_held_or_nothing(window, &what);
                }
            }
        }
    }

    /// The overlays before this plan's: their keys are swept too, but they may say what they changed in memory.
    const OLDER_OVERLAYS: [&str; 4] = ["progress", "remove", "group", "menu"];

    /// Every control of the toolbar, the selection bar and the top row, clicked and double-clicked, each on a fresh
    /// demo (`empty`: with no repositories), in a window of `size`; and, on the demo as it starts in the design's
    /// window, the table's.
    fn press_every_control(window: &mut Window, before: [usize; 5], size: [f32; 2], empty: bool) {
        window.size = size;
        let start: fn(&mut Repos) = if empty { emptied } else { unchanged };
        fresh(window, start);
        let layout = window.layout();
        let mut controls = Vec::new();
        let mut top = None;
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(size[0], size[1]));
        let input = egui::RawInput { screen_rect: Some(screen), ..Default::default() };
        let mut started = repos();
        start(&mut started);
        let session = crate::session::Session::demo(started);
        window
            .ctx
            .clone()
            .run_ui(input, |ui| {
                let plan = repos_screen::plan(ui.painter(), &layout, &session).unwrap();
                controls = repos_screen::control_places(ui.painter(), &plan);
                top = repos_screen::top_row(&plan);
            })
            .drop_without_applying_deltas();
        let (capsule, main) = top.unwrap_or_else(|| panic!("{size:?}: no top row"));
        let words: Vec<&str> = controls.iter().map(|(words, _)| words.as_str()).collect();
        let asked: &[&str] = if empty { &["Add repository"] } else { &["Update", "Move to group", "Open folder", "Remove", "x", "Add repository"] };
        for asked in asked {
            assert!(words.contains(asked), "{size:?}: no control {asked} among {words:?}");
        }
        // The top row's capsule (Import and Update all, by their words or their icons) and its main button.
        assert!(controls.len() >= asked.len() + 2, "{size:?}: the top row's capsule is missing among {words:?}");
        for (words, rect) in &controls {
            for double in [false, true] {
                fresh(window, start);
                window.click(rect.center());
                if double {
                    window.click(rect.center());
                }
                let what = format!("the control {words} at {size:?}{}", if double { ", double-clicked" } else { "" });
                let expect = match (["Update", "Open folder", "Update all"].contains(&words.as_str()), empty) {
                    (_, true) => Expect::Empty,
                    (true, false) => Expect::Held,
                    (false, false) => Expect::Free,
                };
                still_held(window, &what, expect, before);
                // The top row's controls are this plan's; the others may say what they changed in memory (a sort).
                // Each landed: "Add repository" opens Add, the capsule's first ("Import", or its icon) Import, its
                // second ("Update all") is refused by the held core, saying so.
                if *rect == main {
                    assert!(window.shows(Which::Add), "{what}: no Add dialog");
                } else if capsule.contains_rect(*rect) {
                    let first = controls.iter().find(|(_, place)| capsule.contains_rect(*place)).is_some_and(|(_, place)| place == rect);
                    if first {
                        assert!(window.shows(Which::Folder) && window.repos().prompt.is_some(), "{what}: no Import dialog");
                    } else {
                        // With repositories the held core refuses the update; with none the core refuses it for that.
                        assert!(!window.open(), "{what}: a dialog opened");
                        let words = if empty { NOTHING_TO_UPDATE } else { devassist_core::repos::state::HELD };
                        assert_eq!(window.repos().status, words, "{what}: Update all was not refused");
                    }
                }
                if capsule.contains_rect(*rect) || *rect == main {
                    check_said(window, &what, if empty { Says::OrRefused(NOTHING_TO_UPDATE) } else { Says::HeldOrNothing });
                }
            }
        }
        window.size = crate::theme::WINDOW;
        if empty || size != crate::theme::WINDOW {
            return;
        }
        // The table: a group's header, a row, its tick, the status bar (which brings a run back; there is none).
        let lines = crate::repos_view::lines(window.repos());
        let header = table::dots_rect(layout.panel, &lines, 0.0, "group::home").unwrap();
        let row = table::dots_rect(layout.panel, &lines, 0.0, "homelab").unwrap();
        let table_presses = [
            ("a group's header", pos(header.center().x - 400.0, header.center().y)),
            ("a row", pos(row.center().x - 400.0, row.center().y)),
            ("a row's tick", pos(layout.panel.left() + crate::columns::columns(layout.panel.width()).check.left + 8.0, row.center().y)),
            ("the status bar", layout.status.center()),
        ];
        for (what, at) in table_presses {
            fresh(window, unchanged);
            window.click(at);
            still_held(window, what, Expect::Free, before);
        }
    }

    /// Every input of [`every_input`] on the demo with no repositories, each on a fresh one: nothing runs, is written
    /// or reaches out; and `a` and `i` open Add and Import there too.
    fn press_the_keys_of_the_empty_demo(window: &mut Window, before: [usize; 5]) {
        for (press, release) in every_input() {
            fresh(window, emptied);
            window.frame(press.clone());
            window.frame(release);
            window.frame(Vec::new());
            still_held(window, &format!("{press:?} on the empty demo"), Expect::Empty, before);
        }
        for (key, letter, dialog) in [(Key::A, "a", Which::Add), (Key::I, "i", Which::Folder)] {
            fresh(window, emptied);
            window.frame(vec![key_event(key, Modifiers::NONE, true), Event::Text(letter.to_owned())]);
            window.frame(vec![key_event(key, Modifiers::NONE, false)]);
            window.frame(Vec::new());
            assert!(window.shows(dialog), "{letter} on the empty demo opened no {dialog:?}");
            still_held(window, &format!("{letter} on the empty demo"), Expect::Empty, before);
        }
    }

    /// Every line of the row's menu, by its letter and by a click, each on a fresh demo.
    fn run_every_line_of_the_menu(window: &mut Window, before: [usize; 5]) {
        // The row's menu, opened by its "…" on the ticked api (so it is for api and web): every line it shows, by
        // its letter and by a click. A line that asks for work is refused; Move to a group and Remove open their
        // dialogs, whose buttons come next.
        for item in menu::shown().iter().copied() {
            for by_click in [false, true] {
                fresh(window, unchanged);
                let lines = crate::repos_view::lines(window.repos());
                let dots = table::dots_rect(window.layout().panel, &lines, 0.0, "api").unwrap();
                window.click(dots.center());
                window.frame(Vec::new());
                assert!(window.shows(Which::Menu), "the menu did not open");
                let what = format!("the menu's {item:?} by {}", if by_click { "a click" } else { "its letter" });
                if by_click {
                    let at = menu::line_rects(window.menu_pane().unwrap())[menu::shown().iter().position(|shown| *shown == item).unwrap()];
                    window.click(at.center());
                } else {
                    let letter = item.key().to_string();
                    let code = Key::from_name(&letter.to_uppercase()).unwrap();
                    window.frame(vec![key_event(code, Modifiers::NONE, true), Event::Text(letter)]);
                    window.frame(vec![key_event(code, Modifiers::NONE, false)]);
                }
                window.frame(Vec::new());
                let opens = match item {
                    devassist_core::repos::state::MenuItem::Group => Some(Which::Group),
                    devassist_core::repos::state::MenuItem::Remove => Some(Which::Remove),
                    // Migrate opens its side menu, and the menu stays.
                    devassist_core::repos::state::MenuItem::Migrate => Some(Which::Menu),
                    _ => None,
                };
                match opens {
                    Some(dialog) => assert!(window.shows(dialog), "{what}: no dialog"),
                    None => assert!(!window.open(), "{what}: the menu stayed"),
                }
                if item == devassist_core::repos::state::MenuItem::Migrate {
                    assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "{what}: no side menu");
                }
                still_held(window, &what, if opens.is_some() { Expect::Free } else { Expect::Held }, before);
            }
        }
    }

    /// Every line of the menu's side menu (Migrate), by its letter and by a click, each on a fresh demo with the menu
    /// opened on a ticked row (for api and web) and on one alone (homelab): Move opens its dialog, Push its dialog for
    /// one row and the core's refusal for two. Nothing runs; the dialogs' buttons come next.
    fn run_every_line_of_the_side_menu(window: &mut Window, before: [usize; 5]) {
        for (row, rows) in [("api", 2), ("homelab", 1)] {
            for item in MigrateItem::ALL {
                for by_click in [false, true] {
                    fresh(window, unchanged);
                    let lines = crate::repos_view::lines(window.repos());
                    let dots = table::dots_rect(window.layout().panel, &lines, 0.0, row).unwrap();
                    window.click(dots.center());
                    window.frame(Vec::new());
                    assert_eq!(window.repos().chosen().len(), rows);
                    window.frame(vec![key_event(Key::T, Modifiers::NONE, true), Event::Text("t".to_owned())]);
                    window.frame(vec![key_event(Key::T, Modifiers::NONE, false)]);
                    window.frame(Vec::new());
                    assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "the side menu did not open");
                    let what = format!("the side menu's {item:?} on {row} by {}", if by_click { "a click" } else { "its letter" });
                    if by_click {
                        let index = MigrateItem::ALL.iter().position(|known| *known == item).unwrap();
                        let at = menu::side_line_rects(window.submenu_pane().unwrap())[index];
                        window.click(at.center());
                    } else {
                        let letter = item.key().to_string();
                        let code = Key::from_name(&letter.to_uppercase()).unwrap();
                        window.frame(vec![key_event(code, Modifiers::NONE, true), Event::Text(letter)]);
                        window.frame(vec![key_event(code, Modifiers::NONE, false)]);
                    }
                    window.frame(Vec::new());
                    match (item, rows) {
                        (MigrateItem::Folder, _) => assert!(window.shows(Which::Folder), "{what}: no dialog"),
                        (MigrateItem::Service, 1) => assert!(window.shows(Which::Push), "{what}: no dialog"),
                        (MigrateItem::Service, _) => assert!(!window.open() && window.repos().status_error, "{what}: not refused"),
                    }
                    still_held(window, &what, Expect::Free, before);
                }
            }
        }
    }

    /// Each dialog of the walk, as the core opens it on a fresh demo, and its buttons with what each may do.
    fn walked_dialogs() -> [Dialog; 17] {
        // Forget takes api and web off the list (in memory only); Delete from disk is refused.
        [
            ("add", show_add, &[("Cancel", Expect::Free), ("Clone 2 repositories", Expect::Held)]),
            (
                "add on one",
                |repos| {
                    show_add(repos);
                    repos.toggle_add_mark_of("acme/artcraft");
                    repos.toggle_add_mark_of("acme/artcraft-docs");
                },
                &[("Clone acme/artcraft", Expect::Held)],
            ),
            (
                "add failed",
                |repos| {
                    show_add(repos);
                    repos.remotes_state = devassist_core::repos::state::RemoteState::Failed("GitHub answered 502".to_owned());
                },
                &[("Cancel", Expect::Free)],
            ),
            ("remove", show_remove, &[("Cancel", Expect::Free), ("Forget", Expect::Forgot(&["api", "web"])), ("Delete from disk", Expect::Held)]),
            // The made-up folder is refused, or, should it be there, the held core refuses the import: either way nothing
            // changes.
            ("import", show_import, &[("Cancel", Expect::Free), ("Import", Expect::Free)]),
            (
                "import a folder that is there",
                |repos| {
                    show_import(repos);
                    repos.set_path_input(&std::env::temp_dir().display().to_string());
                },
                &[("Import", Expect::Held)],
            ),
            // The made-up folder on another drive is looked at once (the core's check of the folder), then the held
            // core refuses the move.
            ("move", show_move, &[("Cancel", Expect::Free), ("Move 2 repositories", Expect::Held)]),
            (
                "move where they are",
                |repos| {
                    show_move(repos);
                    repos.set_path_input(CLONE_DIR);
                },
                &[("Move", Expect::Free)],
            ),
            (
                "move into a folder that is no full path",
                |repos| {
                    show_move(repos);
                    repos.set_path_input("work");
                },
                &[("Cancel", Expect::Free), ("Move", Expect::Free)],
            ),
            // The held core checks the address by its text, then refuses the push: nothing is run or reached.
            ("push", show_push, &[("Cancel", Expect::Free), ("Push", Expect::Held)]),
            (
                "push to an address Git cannot push to",
                |repos| {
                    show_push(repos);
                    repos.set_service_input("my server");
                },
                &[("Push", Expect::Free)],
            ),
            (
                "push without an address",
                |repos| {
                    show_push(repos);
                    repos.set_service_input("");
                },
                &[("Cancel", Expect::Free), ("Push", Expect::Free)],
            ),
            ("group", show_group, &[("Cancel", Expect::Free), ("Move", Expect::Free)]),
            (
                "group into Tools",
                |repos| {
                    show_group(repos);
                    repos.set_group_name("Tools");
                },
                &[("Move", Expect::Free)],
            ),
            (
                "group without a name",
                |repos| {
                    show_group(repos);
                    repos.set_group_name("");
                },
                &[("Move", Expect::Free)],
            ),
            ("progress", show_progress, &[("Cancel", Expect::Held), ("Hide", Expect::Free)]),
            (
                "progress cancelling",
                |repos| {
                    show_progress(repos);
                    repos.job.as_mut().unwrap().cancelling = true;
                },
                &[("Cancelling…", Expect::Free), ("Hide", Expect::Free)],
            ),
        ]
    }

    /// Each dialog, opened as the core opens it on a fresh demo, and each of its buttons pressed; its keys; its scrim.
    fn press_every_button_of_the_dialogs(window: &mut Window, before: [usize; 5]) {
        let dialogs = walked_dialogs();
        for (name, open, buttons) in dialogs {
            for (words, expect) in buttons {
                for double in [false, true] {
                    fresh(window, open);
                    assert!(window.open(), "the {name} dialog did not open");
                    let said = window.said();
                    assert!(said.iter().any(|said| said == words), "the {name} dialog has no {words}: {said:?}");
                    let (place, _) = window.buttons().into_iter().find(|(_, said)| said == words).unwrap();
                    window.click(place.center());
                    if double {
                        // The second click lands where the button was: on the dialog, or on the screen once it closed.
                        window.click(place.center());
                    }
                    window.frame(Vec::new());
                    let what = format!("{words} in the {name} dialog{}", if double { ", double-clicked" } else { "" });
                    still_held(window, &what, *expect, before);
                }
            }
            // Esc, Enter, Tab and the arrows in it too.
            for key in [Key::Tab, Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Escape] {
                fresh(window, open);
                tap(window, key, None);
                still_held(window, &format!("{key:?} in the {name} dialog"), Expect::Free, before);
            }
        }
        // A click on each dialog's scrim.
        for open in [show_remove, show_group, show_progress, show_add, show_import, show_move, show_push] {
            fresh(window, open);
            window.click(pos(30.0, 450.0));
            still_held(window, "a click on the scrim", Expect::Free, before);
        }
    }

    /// Every place of each of this plan's overlays of [`walked_overlays`] (the menu with its side menu open, Add with
    /// its search, with its folder field and its rows, failed and searched, Import, Move and Push) a person can point at: the middle of every text it draws
    /// (each button's words, each row's name, description and note, each field's text or hint, each folder it
    /// suggests, each line and key of the menus, Retry, the row that lists an account) and the Add dialog's checkboxes,
    /// each clicked and double-clicked on a fresh demo with that overlay open. Nothing runs, is written or reaches out,
    /// and the status line says only the demo's words, or that the held core refused what was asked.
    fn click_everything_in_each_overlay(window: &mut Window, before: [usize; 5]) {
        let push_refused = push_for_two_refused();
        // The older overlays' buttons are pressed by name ([`press_every_button_of_the_dialogs`]).
        for (name, open, prepare) in walked_overlays().into_iter().filter(|(name, ..)| !OLDER_OVERLAYS.contains(name)) {
            fresh_overlay(window, name, open, prepare);
            let room = window.overlay_rect().unwrap_or_else(|| panic!("the {name} overlay has no place"));
            let mut points: Vec<egui::Pos2> =
                window.texts.iter().map(|(_, rect, _)| rect.center()).filter(|at| room.contains(*at)).chain(window.add_boxes()).collect();
            points.dedup_by(|a, b| a.distance(*b) < 2.0);
            assert!(points.len() >= 4, "the {name} overlay drew almost nothing: {points:?}");
            if name.starts_with("add") && name != "add failed" {
                assert!(!window.add_boxes().is_empty(), "the {name} overlay has no checkbox");
            }
            // The side menu's Push line, on api and web: refused, one repository is pushed at a time.
            let push_line = window
                .submenu_pane()
                .map(|side| menu::side_line_rects(side)[MigrateItem::ALL.iter().position(|item| *item == MigrateItem::Service).unwrap()]);
            for at in points {
                for double in [false, true] {
                    fresh_overlay(window, name, open, prepare);
                    window.click(at);
                    if double {
                        window.click(at);
                    }
                    window.frame(Vec::new());
                    let what = format!("a {}click at {at:?} in the {name} overlay", if double { "double " } else { "" });
                    still_held(window, &what, Expect::Free, before);
                    if push_line.is_some_and(|line| line.contains(at)) {
                        check_said(window, &what, Says::OrRefused(&push_refused));
                    } else {
                        says_held_or_nothing(window, &what);
                    }
                }
            }
        }
    }

    /// Words typed into every field of the new dialogs, each on a fresh demo with the field's dialog open (the field
    /// has the keyboard), after its text and over it, then Enter: Add's search and folder field, Import's, Move's and
    /// Push's fields. Folders under the demo's made-up one are suggested from the demo's own list; nothing runs, is
    /// written or reaches out.
    fn type_into_every_field(window: &mut Window, before: [usize; 5]) {
        let sep = std::path::MAIN_SEPARATOR;
        let typed = [
            "work".to_owned(),
            format!("{sep}"),
            format!("{CODE_DIR}{sep}"),
            format!("{CODE_DIR}{sep}w"),
            "acme/".to_owned(),
            "zzzz".to_owned(),
            "~/code".to_owned(),
            "\"quoted folder\"".to_owned(),
            "git@codeberg.org:acme/web.git".to_owned(),
            "https://gitlab.com/acme/web.git".to_owned(),
        ];
        let fields: [(&str, Opener, Prepare); 5] = [
            ("add", show_add, as_opened),
            (ADD_FOLDER, show_add, into_the_folder_field),
            ("import", show_import, as_opened),
            ("move", show_move, as_opened),
            ("push", show_push, as_opened),
        ];
        for (name, open, prepare) in fields {
            for words in &typed {
                for over in [false, true] {
                    fresh_overlay(window, name, open, prepare);
                    if over {
                        window.frame(vec![key_event(Key::A, Modifiers::COMMAND, true)]);
                        window.frame(vec![key_event(Key::A, Modifiers::COMMAND, false)]);
                    }
                    let was = field_text(window, name);
                    window.frame(vec![Event::Text(words.clone())]);
                    window.frame(Vec::new());
                    let what = format!("{words:?} typed into the {name} dialog{}", if over { " over its text" } else { "" });
                    // The words reached the field's owner: the core's search, folder or address, or Add's folder.
                    let now = field_text(window, name);
                    let wanted = if over { words.clone() } else { format!("{was}{words}") };
                    assert_eq!(now, wanted, "{what}: the text did not reach the field (it held {was:?})");
                    still_held(window, &what, Expect::Free, before);
                    tap(window, Key::Enter, None);
                    let what = format!("{what}, then Enter");
                    still_held(window, &what, Expect::Free, before);
                    says_held_or_nothing(window, &what);
                }
            }
        }
    }

    /// The text of the field the dialog `name` of [`type_into_every_field`] has the keyboard in, where its owner keeps
    /// it: the core's search (`add_filter`), Add's folder field (the dialog's own), the core's folder question
    /// (Import, Move) or its push address.
    fn field_text(window: &Window, name: &str) -> String {
        let repos = window.repos();
        match name {
            "add" => repos.add_filter.clone(),
            ADD_FOLDER => window.add_folder_text().expect("Add is not open"),
            "import" | "move" => repos.prompt.as_ref().expect("no folder question").input.clone(),
            _ => repos.service_prompt.as_ref().expect("no push question").input.clone(),
        }
    }

    /// Retry in the Add dialog, with GitHub failed (the test's seam: the catalog's failed state): clicked,
    /// double-clicked, and pressed by Enter once Tab gave it the focus. The held core asks GitHub nothing and says so
    /// (`HELD`: the press landed on Retry), and Add stays open on its failed state.
    fn press_retry_in_add(window: &mut Window, before: [usize; 5]) {
        for by in ["a click", "a double click", "Enter"] {
            fresh(window, show_add_failed);
            let retry = window.add_retry().expect("no Retry beside the failed status");
            assert_ne!(window.repos().status, devassist_core::repos::state::HELD, "held before Retry");
            match by {
                "a click" => window.click(retry.center()),
                "a double click" => {
                    window.click(retry.center());
                    window.click(retry.center());
                }
                _ => {
                    tap(window, Key::Tab, None);
                    tap(window, Key::Enter, None);
                }
            }
            window.frame(Vec::new());
            let what = format!("Retry in Add by {by}");
            assert!(window.shows(Which::Add), "{what}: Add closed");
            let failed = matches!(window.repos().remotes_state, devassist_core::repos::state::RemoteState::Failed(_));
            assert!(failed, "{what}: the catalog is no longer failed");
            still_held(window, &what, Expect::Held, before);
        }
    }

    /// A press of `key` (typing `text`), its release, and a frame.
    fn tap(window: &mut Window, key: Key, text: Option<&str>) {
        let mut press = vec![key_event(key, Modifiers::NONE, true)];
        press.extend(text.map(|text| Event::Text(text.to_owned())));
        window.frame(press);
        window.frame(vec![key_event(key, Modifiers::NONE, false)]);
        window.frame(Vec::new());
    }

    /// A second later on egui's clock, so the next click is no double click with the last.
    fn pause(window: &mut Window) {
        let now = window.ctx.input(|input| input.time);
        window.frame_at(Vec::new(), Some(now + 1.0));
    }

    /// Opens the menu by the "…" of `row` and its side menu by its letter, on `window` as it is.
    fn open_side_menu_on(window: &mut Window, row: &str) {
        pause(window);
        let lines = crate::repos_view::lines(window.repos());
        let dots = table::dots_rect(window.layout().panel, &lines, 0.0, row).unwrap();
        window.click(dots.center());
        window.frame(Vec::new());
        assert!(window.shows(Which::Menu), "no menu on {row}");
        assert_eq!(window.repos().menu_highlight(), Some(MenuItem::Update), "the menu on {row} opened on a stale line");
        assert_eq!(window.repos().migrate_highlight(), None, "the menu on {row} opened with its side menu");
        tap(window, Key::T, Some("t"));
        assert_eq!(window.repos().migrate_highlight(), Some(MigrateItem::Folder), "the side menu on {row} did not open");
        assert!(window.submenu_pane().is_some(), "the side menu on {row} is not drawn");
    }

    /// One window, never restarted, through every new overlay in a row, each closed before the next opens: the menu
    /// with its side menu (closed by Esc held over several frames, then the menu by a fresh Esc), Add (typed into, its
    /// folder field's rows closed by Esc, then closed by Esc), Add again (its search empty, its folder field's rows
    /// shown again), Import, Move, Push, and the menu again. Each opens clean (no stale highlight, no closed rows, a
    /// fresh key acts in it), and nothing runs, is written or reaches out.
    fn open_each_overlay_in_turn_on_one_window(before: [usize; 5]) {
        // The clone folder the demo's Add offers, so Add's folder field suggests the demo's folders as it opens.
        let mut start = repos();
        start.clone_dir = PathBuf::from(CLONE_DIR);
        let mut window = Window::new(crate::session::Session::demo(start));
        window.frame(Vec::new());
        let window = &mut window;
        // The menu and its side menu; Esc held closes the side menu only, a fresh Esc the menu.
        open_side_menu_on(window, "homelab");
        window.frame(vec![key_event(Key::Escape, Modifiers::NONE, true)]);
        for _ in 0..4 {
            window.frame(vec![Event::Key { key: Key::Escape, physical_key: None, pressed: true, repeat: true, modifiers: Modifiers::NONE }]);
            assert!(window.shows(Which::Menu) && window.repos().migrate_highlight().is_none(), "a held Esc closed more than the side menu");
        }
        window.frame(vec![key_event(Key::Escape, Modifiers::NONE, false)]);
        tap(window, Key::Escape, None);
        assert!(!window.open(), "a fresh Esc left the menu open");
        still_held(window, "the menu in turn", Expect::Free, before);
        // Add: typed into, its folder field's rows closed by Esc, closed by Esc.
        for round in 0..2 {
            tap(window, Key::A, Some("a"));
            assert!(window.shows(Which::Add), "a opened no Add ({round})");
            assert_eq!(window.repos().add_filter, "", "Add opened with a stale search ({round})");
            tap(window, Key::R, Some("r"));
            assert_eq!(window.repos().add_filter, "r", "a fresh letter did not type ({round})");
            tap(window, Key::Tab, None);
            assert!(window.add_folder_rows_open(), "Add's folder field's rows stayed closed ({round})");
            tap(window, Key::Escape, None);
            assert!(window.shows(Which::Add) && !window.add_folder_rows_open(), "Esc did not close the rows ({round})");
            tap(window, Key::Escape, None);
            assert!(!window.open(), "a fresh Esc left Add open ({round})");
            still_held(window, &format!("Add in turn ({round})"), Expect::Free, before);
        }
        // Import: typed into, closed by Esc.
        tap(window, Key::I, Some("i"));
        assert!(window.shows(Which::Folder), "i opened no Import");
        let typed = window.repos().prompt.as_ref().unwrap().input.clone();
        tap(window, Key::X, Some("x"));
        assert_eq!(window.repos().prompt.as_ref().unwrap().input, format!("{typed}x"), "a fresh letter did not type in Import");
        tap(window, Key::Escape, None);
        assert!(!window.open(), "Esc left Import open");
        still_held(window, "Import in turn", Expect::Free, before);
        // Move, by the side menu on the ticked api (api and web), and Push by the side menu on homelab alone.
        for (row, item, dialog) in [("api", MigrateItem::Folder, Which::Folder), ("homelab", MigrateItem::Service, Which::Push)] {
            open_side_menu_on(window, row);
            let letter = item.key().to_string();
            tap(window, Key::from_name(&letter.to_uppercase()).unwrap(), Some(&letter));
            assert!(window.shows(dialog), "the side menu's {item:?} opened no {dialog:?}");
            tap(window, Key::Escape, None);
            if dialog == Which::Folder {
                // Move's field opens at the demo's clone folder, whose folders it suggests: Esc closes those rows first.
                assert!(window.shows(dialog), "the first Esc closed Move, not its rows");
                tap(window, Key::Escape, None);
            }
            assert!(!window.open(), "Esc left {dialog:?} open");
            still_held(window, &format!("{dialog:?} in turn"), Expect::Free, before);
            says_held_or_nothing(window, &format!("{dialog:?} in turn"));
        }
        // The menu again: on its first line, its side menu closed, and a fresh Esc closes it.
        open_side_menu_on(window, "homelab");
        tap(window, Key::Escape, None);
        tap(window, Key::Escape, None);
        assert!(!window.open(), "a fresh Esc left the menu open at the end");
        still_held(window, "the menu again", Expect::Free, before);
        says_held_or_nothing(window, "the menu again");
    }

    fn pos(x: f32, y: f32) -> egui::Pos2 {
        egui::pos2(x, y)
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
