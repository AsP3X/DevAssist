//! Local clones of GitHub repositories: the overview, and adding,
//! updating, grouping and removing them.

pub mod app;
pub mod github;
pub mod gitops;
pub mod gitprogress;
pub mod settings;
pub mod ui;

use ratatui::Frame;
use ratatui::crossterm::event::Event;
use ratatui::layout::Rect;

use crate::shell::config;
use crate::shell::update;
use crate::shell::{Feature, Request};

/// The keys of the overview, for the help dialog.
const HELP: &str = "arrows, j k    move
space          select several · ctrl-a selects all shown
m              put the selection into a group · on a group: rename it
enter          actions for the selection · on a group: fold it
u              update the selection
U              update every repository
w              pull the selection automatically on a timer, or stop that
f              the forks of the repository, once they are cloned
a              add any GitHub repository, into a folder you choose
i              import clones you already have, from any folder
d, Delete      delete the selection, or only take it off the list
r, F5          fetch and refresh
s              sort by name or by attention
o              open the selection in the file manager
c              settings: accounts, protocol, pull interval, pull requests, title
l              show the last update or clone
/              filter the installed list
wheel          scroll
esc            clear a selection, cancel, or go back
q              quit

In Add, type owner/repo or paste a GitHub URL.
owner/ or a link to an account lists all of its repositories.
Space selects several repositories. Enter clones them all.
Starting and Refresh fetch. Update fast-forwards every branch.
Remove asks before it deletes anything.
An action started while another runs waits and runs after it.";

impl Feature for app::App {
    fn name(&self) -> &'static str {
        "Repos"
    }

    fn about(&self) -> &'static str {
        "Local clones of GitHub repos: add, update, group, remove"
    }

    /// How many clones there are and how many have something to bring in.
    fn status(&self) -> Option<String> {
        let clones = self.installed.len();
        let behind = self
            .installed
            .iter()
            .filter(|repo| repo.sync_counts().1 > 0)
            .count();
        let mut status = match clones {
            1 => "1 clone".to_string(),
            _ => format!("{clones} clones"),
        };
        if behind > 0 {
            status.push_str(&format!(" · {behind} behind"));
        }
        Some(status)
    }

    fn modal(&self) -> bool {
        // The fork overview has keys of its own, such as its filter.
        self.mode != app::Mode::List || self.forks.is_some()
    }

    fn help(&self) -> &'static str {
        HELP
    }

    fn shown(&mut self) {}

    fn on_event(&mut self, event: Event) {
        app::App::on_event(self, event);
    }

    fn drain(&mut self) -> bool {
        app::App::drain(self)
    }

    fn waiting(&self) -> bool {
        app::App::waiting(self)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        ui::draw(frame, self, area);
    }

    fn request(&self) -> Option<Request> {
        self.should_quit.then_some(Request::Quit)
    }

    fn closing(&mut self) {
        // Stops git if this tool started some, however the program ends.
        app::App::quit(self);
    }
}

/// `--doctor`: prints the version, the settings and every listed
/// repository's state, without the full-screen view.
pub fn doctor(settings: &config::Settings) -> Result<(), Box<dyn std::error::Error>> {
    println!("version   {}", update::VERSION);
    match update::check(github::token().as_deref()) {
        Ok(Some(release)) => println!("update    {} is available", release.version),
        Ok(None) => println!("update    none, this is the latest"),
        Err(err) => println!("update    could not look: {err}"),
    }
    println!("settings  {}", settings.config_path.display());
    println!("clone dir {}", settings.clone_dir.display());
    println!("title     {}", settings.title);
    println!("protocol  {}", settings.protocol);
    if settings.owners.is_empty() {
        println!("owners    (none)");
    } else {
        println!("owners    {}", settings.owners.join(", "));
    }
    if !settings.pinned.is_empty() {
        let pinned: Vec<String> = settings.pinned.iter().map(|spec| spec.id()).collect();
        println!("pinned    {}", pinned.join(", "));
    }
    let installed = gitops::load(&settings.checkouts);
    println!("listed    {} local checkouts", installed.len());
    let catalog = github::fetch_catalog(&settings.owners, &settings.pinned);
    println!("github    {} repositories", catalog.repos.len());
    for err in &catalog.errors {
        println!("github error: {err}");
    }
    let mut failed = 0;
    for repo in &installed {
        match gitops::status(&repo.path) {
            Ok(info) => {
                let mut view = repo.clone();
                view.apply(info);
                println!(
                    "  {:<36} {:<16} {:<8} {}",
                    format!("{}/{}", view.owner, view.folder),
                    view.branch,
                    view.state_label(),
                    view.sync_label()
                );
            }
            Err(err) => {
                failed += 1;
                println!(
                    "  {:<36} status failed: {err}",
                    format!("{}/{}", repo.owner, repo.folder)
                );
            }
        }
    }
    if !catalog.errors.is_empty() || failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
