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

use crate::config;
use crate::shell::{Feature, Request};
use crate::update;

impl Feature for app::App {
    fn on_event(&mut self, event: Event) {
        app::App::on_event(self, event);
    }

    fn drain(&mut self) -> bool {
        app::App::drain(self)
    }

    fn waiting(&self) -> bool {
        app::App::waiting(self)
    }

    fn draw(&mut self, frame: &mut Frame) {
        ui::draw(frame, self);
    }

    fn request(&self) -> Option<Request> {
        if !self.should_quit {
            None
        } else if self.restart {
            Some(Request::Restart)
        } else {
            Some(Request::Quit)
        }
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
