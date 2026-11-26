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
use devassist_core::repos::state::{Action, Repos};
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
pub const SHOWS: [(&str, Opener); 6] =
    [("progress", show_progress), ("remove", show_remove), ("group", show_group), ("menu", show_menu), ("add", show_add), (ADD_FOLDER, show_add)];

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
        assert_eq!(show(&mut repos, "nothing").unwrap_err(), "--show knows progress, remove, group, menu, add, add-folder, not nothing");
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
    fn the_demo_opens_the_menu_on_its_first_ticked_row_for_both() {
        let mut repos = repos();
        show(&mut repos, "menu").unwrap();
        assert!(repos.mode == Mode::Menu);
        assert_eq!(repos.selected.as_deref(), Some("api"));
        assert_eq!(repos.chosen(), ["api", "web"]);
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
    }

    /// How often the process has reached out so far: programs, the clipboard, the network, folders deleted.
    fn reached_now() -> [usize; 4] {
        use devassist_core::sys::{Reach, reached};
        [Reach::Program, Reach::Clipboard, Reach::Network, Reach::Disk].map(reached)
    }

    /// The walk's window, on a fresh demo with `start` done to it first.
    fn fresh(window: &mut Window, start: fn(&mut Repos)) {
        let mut repos = repos();
        start(&mut repos);
        window.restart(crate::session::Session::demo(repos));
    }

    /// Checks, after the press `what`, that the demo ran and wrote nothing and reached nothing since `before`.
    fn still_held(window: &Window, what: &str, expect: Expect, before: [usize; 4]) {
        let repos = window.repos();
        assert!(repos.work_held(), "{what}: the demo's work is no longer held");
        assert!(!repos.busy && !repos.fetching && !repos.waiting(), "{what}: the core is busy");
        assert!(repos.queue.is_empty(), "{what}: work was queued: {}", repos.queue.len());
        let paths: Vec<PathBuf> = repos.installed.iter().map(|repo| repo.path.clone()).collect();
        let gone: &[&str] = if let Expect::Forgot(gone) = expect { gone } else { &[] };
        let kept: Vec<PathBuf> = ENTRIES.iter().filter(|entry| !gone.contains(&entry.name)).map(|entry| PathBuf::from(entry.path)).collect();
        assert_eq!(paths, kept, "{what}: the demo's rows changed");
        if matches!(expect, Expect::Held) {
            assert_eq!(repos.status, devassist_core::repos::state::HELD, "{what}: refused without saying so");
        }
        // The held core saves nothing (`save_config` returns before the write): the demo's settings file is never made.
        assert!(!repos.config_path.exists(), "{what}: the demo wrote its settings");
        assert_eq!(reached_now(), before, "{what}: a program ran, the clipboard was written, the network asked or a folder deleted");
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
        press_every_control(&mut window, before);
        run_every_line_of_the_menu(&mut window, before);
        press_every_button_of_the_dialogs(&mut window, before);
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
    fn press_every_key(window: &mut Window, before: [usize; 4]) {
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

    /// Every input of [`every_input`] in each overlay `--show` opens (the progress, remove and group dialogs and the
    /// menu), each on a fresh demo with that overlay open: whatever a key does there (a line of the menu, a button
    /// with the focus, the field), nothing runs, is written or reaches out.
    fn press_every_key_in_each_overlay(window: &mut Window, before: [usize; 4]) {
        let inputs = every_input();
        for (name, open) in SHOWS {
            for (press, release) in &inputs {
                fresh(window, open);
                assert!(window.open(), "the {name} overlay did not open");
                window.frame(press.clone());
                window.frame(release.clone());
                window.frame(Vec::new());
                still_held(window, &format!("{press:?} in the {name} overlay"), Expect::Free, before);
            }
        }
    }

    /// Every control of the toolbar, the selection bar and the top row, and the table's, each on a fresh demo.
    fn press_every_control(window: &mut Window, before: [usize; 4]) {
        // Every control of the toolbar, the selection bar and the top row, clicked on a fresh demo.
        fresh(window, unchanged);
        let layout = window.layout();
        let mut controls = Vec::new();
        let input = egui::RawInput { screen_rect: Some(crate::dialogs::testing::window()), ..Default::default() };
        let session = crate::session::Session::demo(repos());
        window
            .ctx
            .clone()
            .run_ui(input, |ui| {
                let plan = repos_screen::plan(ui.painter(), &layout, &session).unwrap();
                controls = repos_screen::control_places(ui.painter(), &plan);
            })
            .drop_without_applying_deltas();
        let words: Vec<&str> = controls.iter().map(|(words, _)| words.as_str()).collect();
        for asked in ["Update", "Move to group", "Open folder", "Remove", "x", "Update all"] {
            assert!(words.contains(&asked), "no control {asked} among {words:?}");
        }
        for (words, rect) in &controls {
            fresh(window, unchanged);
            window.click(rect.center());
            let expect = if ["Update", "Open folder", "Update all"].contains(&words.as_str()) { Expect::Held } else { Expect::Free };
            still_held(window, &format!("the control {words}"), expect, before);
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

    /// Every line of the row's menu, by its letter and by a click, each on a fresh demo.
    fn run_every_line_of_the_menu(window: &mut Window, before: [usize; 4]) {
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
                    _ => None,
                };
                match opens {
                    Some(dialog) => assert!(window.shows(dialog), "{what}: no dialog"),
                    None => assert!(!window.open(), "{what}: the menu stayed"),
                }
                still_held(window, &what, if opens.is_some() { Expect::Free } else { Expect::Held }, before);
            }
        }
    }

    /// Each dialog, opened as the core opens it on a fresh demo, and each of its buttons pressed; its keys; its scrim.
    fn press_every_button_of_the_dialogs(window: &mut Window, before: [usize; 4]) {
        // Forget takes api and web off the list (in memory only); Delete from disk is refused.
        let dialogs: [Dialog; 9] = [
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
        ];
        for (name, open, buttons) in dialogs {
            for (words, expect) in buttons {
                fresh(window, open);
                assert!(window.open(), "the {name} dialog did not open");
                assert!(window.said().contains(words), "the {name} dialog has no {words}: {:?}", window.said());
                window.press(words);
                window.frame(Vec::new());
                still_held(window, &format!("{words} in the {name} dialog"), *expect, before);
            }
            // Esc, Enter, Tab and the arrows in it too.
            for key in [Key::Tab, Key::ArrowDown, Key::ArrowUp, Key::Enter, Key::Escape] {
                fresh(window, open);
                window.key(key);
                still_held(window, &format!("{key:?} in the {name} dialog"), Expect::Free, before);
            }
        }
        // A click on each dialog's scrim.
        for open in [show_remove, show_group, show_progress, show_add] {
            fresh(window, open);
            window.click(pos(30.0, 450.0));
            still_held(window, "a click on the scrim", Expect::Free, before);
        }
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
