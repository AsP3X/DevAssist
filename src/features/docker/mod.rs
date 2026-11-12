//! Docker: containers, Compose projects, images and volumes, with live
//! logs. Everything goes through the `docker` command.

pub mod actions;
pub mod cli;
pub mod model;
pub mod state;
pub mod view;

use ratatui::Frame;
use ratatui::crossterm::event::Event;
use ratatui::layout::Rect;

use crate::shell::{Feature, Request};
pub use state::Docker;

/// The keys of the Docker tool, for the help dialog.
const HELP: &str = "tab, shift-tab next list: containers, Compose, images, volumes
arrows, j k    move
space          select several · ctrl-a selects all shown
enter          actions for the selection · on a project: fold it
s              start what is stopped, stop what runs
R              restart
l              logs, live · in the logs: / filters, end follows
d, Delete      remove the selection · Compose: take the project down
p              prune: remove everything unused in this list
r, F5          ask Docker again
/              filter the list
wheel          scroll
esc            clear a selection or a filter, or go back
q              quit

Nothing is removed without a dialog that lists what will go.
Running containers and images in use need the second button, or f.
Volumes in use are never removed.";

impl Feature for Docker {
    fn name(&self) -> &'static str {
        "Docker"
    }

    fn modal(&self) -> bool {
        Docker::modal(self)
    }

    fn help(&self) -> &'static str {
        HELP
    }

    fn shown(&mut self) {
        Docker::shown(self);
    }

    fn on_event(&mut self, event: Event) {
        Docker::on_event(self, event);
    }

    fn drain(&mut self) -> bool {
        Docker::drain(self)
    }

    fn waiting(&self) -> bool {
        Docker::waiting(self)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        view::draw(frame, self, area);
    }

    fn request(&self) -> Option<Request> {
        self.should_quit.then_some(Request::Quit)
    }
}

/// `--doctor`: one line about Docker. A missing Docker is not a failure.
pub fn doctor() {
    match cli::output(&cli::version()) {
        Ok(version) => {
            let containers = cli::output(&cli::ps())
                .map(|output| model::parse_containers(&output).len())
                .unwrap_or(0);
            println!(
                "docker    {}, {}",
                version.trim(),
                actions::count(containers, "container")
            );
        }
        Err(failure) => println!("docker    not reachable: {}", failure.text()),
    }
}
