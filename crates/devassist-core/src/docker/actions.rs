//! What the tool can do to what it lists: the commands to run, and what a
//! confirmation says before anything is removed. All of it is plain data.

use super::cli;
use super::model::{Container, Image, Project, Volume, format_size};

/// Something to run, one `docker` command after the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// For the status line while it runs: "Stopping shop-web".
    pub doing: String,
    /// For the status line afterwards: "Stopped shop-web".
    pub done: String,
    /// The arguments of each command.
    pub steps: Vec<Vec<String>>,
    /// The output is long and worth watching, as with `compose up`.
    pub show_output: bool,
}

impl Request {
    fn new(doing: String, done: String, step: Vec<String>) -> Self {
        Self {
            doing,
            done,
            steps: vec![step],
            show_output: false,
        }
    }
}

/// "shop-web", or "3 containers".
fn named(names: &[String], kind: &str) -> String {
    match names {
        [only] => only.clone(),
        many => format!("{} {kind}s", many.len()),
    }
}

/// A line of the actions menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Start,
    Stop,
    Restart,
    Logs,
    CopyName,
    Remove,
    Down,
}

impl Verb {
    pub fn label(self) -> &'static str {
        match self {
            Verb::Start => "Start",
            Verb::Stop => "Stop",
            Verb::Restart => "Restart",
            Verb::Logs => "Show the logs",
            Verb::CopyName => "Copy the name",
            Verb::Remove => "Remove…",
            Verb::Down => "Down: stop and remove…",
        }
    }

    /// The key that runs it straight from the menu.
    pub fn key(self) -> char {
        match self {
            Verb::Start => 's',
            Verb::Stop => 'x',
            Verb::Restart => 'r',
            Verb::Logs => 'l',
            Verb::CopyName => 'c',
            Verb::Remove => 'd',
            Verb::Down => 'w',
        }
    }

    /// Shown as a warning: it removes something.
    pub fn warns(self) -> bool {
        matches!(self, Verb::Remove | Verb::Down)
    }
}

pub const CONTAINER_MENU: [Verb; 6] = [
    Verb::Start,
    Verb::Stop,
    Verb::Restart,
    Verb::Logs,
    Verb::CopyName,
    Verb::Remove,
];
pub const PROJECT_MENU: [Verb; 5] = [
    Verb::Start,
    Verb::Stop,
    Verb::Restart,
    Verb::Logs,
    Verb::Down,
];
pub const OBJECT_MENU: [Verb; 2] = [Verb::CopyName, Verb::Remove];

fn ids(containers: &[&Container]) -> Vec<String> {
    containers.iter().map(|c| c.id.clone()).collect()
}

fn names(containers: &[&Container]) -> Vec<String> {
    containers.iter().map(|c| c.name.clone()).collect()
}

/// Starts the ones that are not running. Nothing when all of them run.
pub fn start(containers: &[&Container]) -> Option<Request> {
    let stopped: Vec<&Container> = containers
        .iter()
        .copied()
        .filter(|c| !c.running())
        .collect();
    if stopped.is_empty() {
        return None;
    }
    let who = named(&names(&stopped), "container");
    Some(Request::new(
        format!("Starting {who}"),
        format!("Started {who}"),
        cli::start(&ids(&stopped)),
    ))
}

/// Stops the ones that run. Nothing when none does.
pub fn stop(containers: &[&Container]) -> Option<Request> {
    let running: Vec<&Container> = containers.iter().copied().filter(|c| c.running()).collect();
    if running.is_empty() {
        return None;
    }
    let who = named(&names(&running), "container");
    Some(Request::new(
        format!("Stopping {who}"),
        format!("Stopped {who}"),
        cli::stop(&ids(&running)),
    ))
}

pub fn restart(containers: &[&Container]) -> Option<Request> {
    if containers.is_empty() {
        return None;
    }
    let who = named(&names(containers), "container");
    Some(Request::new(
        format!("Restarting {who}"),
        format!("Restarted {who}"),
        cli::restart(&ids(containers)),
    ))
}

/// `s`: stops them when every one runs, and otherwise starts the ones
/// that do not.
pub fn toggle(containers: &[&Container]) -> Option<Request> {
    if containers.iter().all(|c| c.running()) {
        stop(containers)
    } else {
        start(containers)
    }
}

/// Starts the project's containers as they are. Nothing is created anew,
/// so no compose file, profile or environment file is needed.
pub fn project_start(project: &Project) -> Request {
    Request {
        doing: format!("Starting project {}", project.name),
        done: format!("Started project {}", project.name),
        steps: vec![cli::compose_start(&project.name)],
        show_output: true,
    }
}

pub fn project_stop(project: &Project) -> Request {
    Request {
        doing: format!("Stopping project {}", project.name),
        done: format!("Stopped project {}", project.name),
        steps: vec![cli::compose_stop(&project.name)],
        show_output: true,
    }
}

pub fn project_restart(project: &Project) -> Request {
    Request {
        doing: format!("Restarting project {}", project.name),
        done: format!("Restarted project {}", project.name),
        steps: vec![cli::compose_restart(&project.name)],
        show_output: true,
    }
}

/// `s` on a project: stops it when any of it runs, and starts it when
/// none does. A service that ran once and ended does not count as a
/// reason to start the rest again.
pub fn project_toggle(project: &Project) -> Request {
    if project.running > 0 {
        project_stop(project)
    } else {
        project_start(project)
    }
}

/// Something that can be removed: what `docker` calls it, what the user
/// calls it, and a note such as who uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub note: String,
    pub bytes: u64,
}

impl Item {
    fn plain(id: &str, name: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            note: String::new(),
            bytes: 0,
        }
    }
}

fn item_ids(items: &[Item]) -> Vec<String> {
    items.iter().map(|item| item.id.clone()).collect()
}

fn item_names(items: &[Item]) -> Vec<String> {
    items.iter().map(|item| item.name.clone()).collect()
}

/// A button of a confirmation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    /// Remove what can be removed without force.
    Go,
    /// Remove everything listed, stopping or untagging what is in the way.
    Force,
    Cancel,
}

/// A line of a confirmation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Told {
    pub text: String,
    /// Shown as a warning.
    pub warn: bool,
}

fn told(text: impl Into<String>) -> Told {
    Told {
        text: text.into(),
        warn: false,
    }
}

fn warned(text: impl Into<String>) -> Told {
    Told {
        text: text.into(),
        warn: true,
    }
}

/// The question asked before anything is removed. It lists by name what
/// will go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    Containers {
        stopped: Vec<Item>,
        running: Vec<Item>,
    },
    Images {
        free: Vec<Item>,
        /// Images a container was made from; the note names it.
        used: Vec<Item>,
    },
    Volumes {
        free: Vec<Item>,
        /// Volumes a container mounts: these stay.
        used: Vec<Item>,
    },
    PruneContainers {
        stopped: Vec<Item>,
    },
    PruneImages {
        dangling: Vec<Item>,
        /// Named images no container uses. They go only when `all` is set.
        unused: Vec<Item>,
        all: bool,
    },
    PruneVolumes {
        anonymous: Vec<Item>,
        /// Named volumes no container mounts. They go only when `all` is set.
        named: Vec<Item>,
        all: bool,
    },
    Down {
        project: String,
        containers: Vec<String>,
        volumes: bool,
    },
}

impl Confirm {
    pub fn containers(chosen: &[&Container]) -> Self {
        let item = |c: &&Container| Item::plain(&c.id, &c.name);
        Confirm::Containers {
            stopped: chosen.iter().filter(|c| !c.running()).map(item).collect(),
            running: chosen.iter().filter(|c| c.running()).map(item).collect(),
        }
    }

    pub fn images(chosen: &[&Image], containers: &[Container]) -> Self {
        let mut free = Vec::new();
        let mut used = Vec::new();
        for image in chosen {
            let users = image.used_by(containers);
            let mut item = Item::plain(&image.reference(), &image.name());
            if image.dangling() {
                item.name = format!("<none> {}", image.short_id());
            }
            if image.spare_name(containers) {
                item.note = "only this name goes; the image is in use under another".to_string();
                free.push(item);
            } else if image.in_use(containers) {
                item.note = match users.as_slice() {
                    [] => "used by a container".to_string(),
                    users => format!("used by {}", users.join(", ")),
                };
                used.push(item);
            } else {
                free.push(item);
            }
        }
        Confirm::Images { free, used }
    }

    pub fn volumes(chosen: &[&Volume], containers: &[Container]) -> Self {
        let mut free = Vec::new();
        let mut used = Vec::new();
        for volume in chosen {
            let users = volume.used_by(containers);
            let mut item = Item::plain(&volume.name, &volume.label());
            if users.is_empty() {
                free.push(item);
            } else {
                item.note = format!("used by {}", users.join(", "));
                used.push(item);
            }
        }
        Confirm::Volumes { free, used }
    }

    pub fn prune_containers(containers: &[Container]) -> Self {
        Confirm::PruneContainers {
            stopped: containers
                .iter()
                .filter(|c| !c.running())
                .map(|c| Item::plain(&c.id, &c.name))
                .collect(),
        }
    }

    pub fn prune_images(images: &[Image], containers: &[Container]) -> Self {
        let mut dangling = Vec::new();
        let mut unused = Vec::new();
        for image in images.iter().filter(|image| !image.in_use(containers)) {
            let mut item = Item::plain(&image.reference(), &image.name());
            item.bytes = image.bytes();
            item.note = image.size.clone();
            if image.dangling() {
                item.name = format!("<none> {}", image.short_id());
                dangling.push(item);
            } else {
                unused.push(item);
            }
        }
        Confirm::PruneImages {
            dangling,
            unused,
            all: false,
        }
    }

    pub fn prune_volumes(volumes: &[Volume], containers: &[Container]) -> Self {
        let mut anonymous = Vec::new();
        let mut named = Vec::new();
        for volume in volumes
            .iter()
            .filter(|volume| volume.used_by(containers).is_empty())
        {
            let item = Item::plain(&volume.name, &volume.label());
            if volume.anonymous() {
                anonymous.push(item);
            } else {
                named.push(item);
            }
        }
        Confirm::PruneVolumes {
            anonymous,
            named,
            all: false,
        }
    }

    pub fn down(project: &Project, containers: &[Container]) -> Self {
        Confirm::Down {
            project: project.name.clone(),
            containers: containers
                .iter()
                .filter(|c| c.project == project.name)
                .map(|c| c.name.clone())
                .collect(),
            volumes: false,
        }
    }

    pub fn title(&self) -> String {
        match self {
            Confirm::Containers { .. } => "Remove containers".to_string(),
            Confirm::Images { .. } => "Remove images".to_string(),
            Confirm::Volumes { .. } => "Remove volumes".to_string(),
            Confirm::PruneContainers { .. } => "Prune stopped containers".to_string(),
            Confirm::PruneImages { .. } => "Prune unused images".to_string(),
            Confirm::PruneVolumes { .. } => "Prune unused volumes".to_string(),
            Confirm::Down { project, .. } => format!("Take down {project}"),
        }
    }

    /// What `Go` removes.
    fn going(&self) -> Vec<&Item> {
        match self {
            Confirm::Containers { stopped, .. } | Confirm::PruneContainers { stopped } => {
                stopped.iter().collect()
            }
            Confirm::Images { free, .. } | Confirm::Volumes { free, .. } => free.iter().collect(),
            Confirm::PruneImages {
                dangling,
                unused,
                all,
            } => dangling
                .iter()
                .chain(unused.iter().filter(|_| *all))
                .collect(),
            Confirm::PruneVolumes {
                anonymous,
                named,
                all,
            } => anonymous
                .iter()
                .chain(named.iter().filter(|_| *all))
                .collect(),
            Confirm::Down { .. } => Vec::new(),
        }
    }

    /// What the dialog says, line by line.
    pub fn lines(&self) -> Vec<Told> {
        let listed = |items: &[&Item]| -> Vec<Told> {
            items
                .iter()
                .map(|item| {
                    if item.note.is_empty() {
                        told(format!("  {}", item.name))
                    } else {
                        told(format!("  {}  ·  {}", item.name, item.note))
                    }
                })
                .collect()
        };
        let going = self.going();
        let mut lines = Vec::new();
        match self {
            Confirm::Containers { running, .. } => {
                if !going.is_empty() {
                    lines.push(told("These stopped containers will be removed:"));
                    lines.extend(listed(&going));
                }
                if !running.is_empty() {
                    if !going.is_empty() {
                        lines.push(told(""));
                    }
                    lines.push(warned("These are running and stay unless you stop them:"));
                    lines.extend(listed(&running.iter().collect::<Vec<_>>()));
                }
                lines.push(told(""));
                lines.push(told(
                    "A removed container's own data is gone. Volumes stay.",
                ));
            }
            Confirm::Images { used, .. } => {
                if !going.is_empty() {
                    lines.push(told("These images will be removed:"));
                    lines.extend(listed(&going));
                }
                if !used.is_empty() {
                    if !going.is_empty() {
                        lines.push(told(""));
                    }
                    lines.push(warned("These are in use and stay unless you insist:"));
                    lines.extend(listed(&used.iter().collect::<Vec<_>>()));
                }
            }
            Confirm::Volumes { used, .. } => {
                if !going.is_empty() {
                    lines.push(warned(
                        "These volumes and all data in them will be removed:",
                    ));
                    lines.extend(listed(&going));
                }
                if !used.is_empty() {
                    if !going.is_empty() {
                        lines.push(told(""));
                    }
                    lines.push(told("These are mounted by a container and stay:"));
                    lines.extend(listed(&used.iter().collect::<Vec<_>>()));
                }
            }
            Confirm::PruneContainers { .. } => {
                if going.is_empty() {
                    lines.push(told("No container is stopped. Nothing to prune."));
                } else {
                    lines.push(told(format!(
                        "{} will be removed:",
                        count(going.len(), "stopped container")
                    )));
                    lines.extend(listed(&going));
                }
            }
            Confirm::PruneImages { .. } => {
                if going.is_empty() {
                    lines.push(told("No image is unused here. Nothing to prune."));
                } else {
                    let bytes: u64 = going.iter().map(|item| item.bytes).sum();
                    lines.push(told(format!(
                        "{} will be removed, about {}:",
                        count(going.len(), "image"),
                        format_size(bytes)
                    )));
                    lines.extend(listed(&going));
                }
            }
            Confirm::PruneVolumes { .. } => {
                if going.is_empty() {
                    lines.push(told("No volume is unused here. Nothing to prune."));
                } else {
                    lines.push(warned(format!(
                        "{} and all data in them will be removed:",
                        count(going.len(), "volume")
                    )));
                    lines.extend(listed(&going));
                }
            }
            Confirm::Down {
                project,
                containers,
                volumes,
            } => {
                lines.push(told(format!(
                    "Project {project} is stopped and its containers and networks are removed:"
                )));
                lines.extend(containers.iter().map(|name| told(format!("  {name}"))));
                lines.push(told(""));
                if *volumes {
                    lines.push(warned("Its volumes and all data in them are removed too."));
                } else {
                    lines.push(told("Its volumes stay."));
                }
            }
        }
        lines
    }

    /// The tick box: what it says and whether it is ticked.
    pub fn tick(&self) -> Option<(String, bool)> {
        match self {
            Confirm::PruneImages { unused, all, .. } if !unused.is_empty() => Some((
                format!(
                    "also the {} no container uses",
                    count(unused.len(), "named image")
                ),
                *all,
            )),
            Confirm::PruneVolumes { named, all, .. } if !named.is_empty() => Some((
                format!(
                    "also the {} no container mounts",
                    count(named.len(), "named volume")
                ),
                *all,
            )),
            Confirm::Down { volumes, .. } => {
                Some(("also remove its volumes".to_string(), *volumes))
            }
            _ => None,
        }
    }

    /// Space: ticks or unticks the box, where there is one.
    pub fn toggle(&mut self) {
        if self.tick().is_none() {
            return;
        }
        match self {
            Confirm::PruneImages { all, .. } | Confirm::PruneVolumes { all, .. } => *all = !*all,
            Confirm::Down { volumes, .. } => *volumes = !*volumes,
            _ => {}
        }
    }

    /// The buttons, the one Enter presses first.
    pub fn buttons(&self) -> Vec<(&'static str, Choice)> {
        let go = !self.going().is_empty();
        let mut buttons = Vec::new();
        match self {
            Confirm::Containers { running, .. } => {
                if go {
                    buttons.push(("Remove", Choice::Go));
                }
                if !running.is_empty() {
                    buttons.push(("Stop and remove all", Choice::Force));
                }
            }
            Confirm::Images { used, .. } => {
                if go {
                    buttons.push(("Remove", Choice::Go));
                }
                if !used.is_empty() {
                    buttons.push(("Remove anyway", Choice::Force));
                }
            }
            Confirm::Down { .. } => buttons.push(("Take down", Choice::Go)),
            _ => {
                if go {
                    buttons.push(("Remove", Choice::Go));
                }
            }
        }
        buttons.push((
            if buttons.is_empty() {
                "Close"
            } else {
                "Cancel"
            },
            Choice::Cancel,
        ));
        buttons
    }

    /// What Enter does: `Go` where the dialog offers it, and otherwise
    /// nothing. Force is never what Enter does.
    pub fn default_choice(&self) -> Choice {
        if self
            .buttons()
            .iter()
            .any(|(_, choice)| *choice == Choice::Go)
        {
            Choice::Go
        } else {
            Choice::Cancel
        }
    }

    /// What a button runs. Nothing for Cancel, or for a button the dialog
    /// does not have.
    pub fn request(&self, choice: Choice) -> Option<Request> {
        if !self.buttons().iter().any(|(_, offered)| *offered == choice) {
            return None;
        }
        let going: Vec<Item> = self.going().into_iter().cloned().collect();
        match (self, choice) {
            (_, Choice::Cancel) => None,
            (Confirm::Containers { running, .. }, Choice::Force) => {
                let all: Vec<Item> = going.iter().chain(running).cloned().collect();
                let who = named(&item_names(&all), "container");
                Some(Request::new(
                    format!("Removing {who}"),
                    format!("Removed {who}"),
                    cli::rm(&item_ids(&all), true),
                ))
            }
            (Confirm::Containers { .. } | Confirm::PruneContainers { .. }, Choice::Go) => {
                let who = named(&item_names(&going), "container");
                Some(Request::new(
                    format!("Removing {who}"),
                    format!("Removed {who}"),
                    cli::rm(&item_ids(&going), false),
                ))
            }
            (Confirm::Images { used, .. }, Choice::Force) => {
                let all: Vec<Item> = going.iter().chain(used).cloned().collect();
                let who = named(&item_names(&all), "image");
                Some(Request::new(
                    format!("Removing {who}"),
                    format!("Removed {who}"),
                    cli::rmi(&item_ids(&all), true),
                ))
            }
            (Confirm::Images { .. } | Confirm::PruneImages { .. }, Choice::Go) => {
                let who = named(&item_names(&going), "image");
                Some(Request::new(
                    format!("Removing {who}"),
                    format!("Removed {who}"),
                    cli::rmi(&item_ids(&going), false),
                ))
            }
            (Confirm::Volumes { .. } | Confirm::PruneVolumes { .. }, Choice::Go) => {
                let who = named(&item_names(&going), "volume");
                Some(Request::new(
                    format!("Removing {who}"),
                    format!("Removed {who}"),
                    cli::volume_rm(&item_ids(&going)),
                ))
            }
            (
                Confirm::Down {
                    project, volumes, ..
                },
                Choice::Go,
            ) => Some(Request {
                doing: format!("Taking down {project}"),
                done: format!("Took down {project}"),
                steps: vec![cli::compose_down(project, *volumes)],
                show_output: true,
            }),
            (_, Choice::Force) => None,
        }
    }
}

/// "1 image", "3 images".
pub fn count(number: usize, what: &str) -> String {
    match number {
        1 => format!("1 {what}"),
        _ => format!("{number} {what}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::model::samples;

    fn texts(confirm: &Confirm) -> Vec<String> {
        confirm.lines().into_iter().map(|line| line.text).collect()
    }

    fn labels(confirm: &Confirm) -> Vec<&'static str> {
        confirm
            .buttons()
            .into_iter()
            .map(|(label, _)| label)
            .collect()
    }

    #[test]
    fn start_and_stop_only_touch_what_needs_it() {
        let lists = samples::lists();
        let all: Vec<&Container> = lists.containers.iter().collect();
        let start = start(&all).unwrap();
        assert_eq!(start.doing, "Starting 2 containers");
        assert_eq!(start.steps[0][0], "start");
        assert_eq!(start.steps[0].len(), 3);
        let stop = stop(&all).unwrap();
        assert_eq!(stop.done, "Stopped 2 containers");

        // `s` on one container does the opposite of what it is doing.
        let web: Vec<&Container> = all
            .iter()
            .copied()
            .filter(|c| c.name == "shop-web")
            .collect();
        assert_eq!(toggle(&web).unwrap().doing, "Stopping shop-web");
        let admin: Vec<&Container> = all
            .iter()
            .copied()
            .filter(|c| c.name == "shop-admin")
            .collect();
        assert_eq!(toggle(&admin).unwrap().doing, "Starting shop-admin");
        assert!(super::stop(&admin).is_none());
        // A mixed selection is brought up, not down.
        assert_eq!(toggle(&all).unwrap().doing, "Starting 2 containers");
    }

    #[test]
    fn removing_containers_leaves_running_ones_unless_forced() {
        let lists = samples::lists();
        let all: Vec<&Container> = lists.containers.iter().collect();
        let confirm = Confirm::containers(&all);
        let lines = texts(&confirm);
        assert!(lines.contains(&"These stopped containers will be removed:".to_string()));
        assert!(lines.contains(&"  backups".to_string()));
        assert!(lines.contains(&"These are running and stay unless you stop them:".to_string()));
        assert!(lines.contains(&"  shop-web".to_string()));
        assert_eq!(
            labels(&confirm),
            ["Remove", "Stop and remove all", "Cancel"]
        );

        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0][0], "rm");
        assert_eq!(go.steps[0].len(), 3);
        assert!(!go.steps[0].contains(&"--force".to_string()));
        let force = confirm.request(Choice::Force).unwrap();
        assert_eq!(&force.steps[0][..2], ["rm", "--force"]);
        assert_eq!(force.steps[0].len(), 6);
        assert_eq!(confirm.request(Choice::Cancel), None);

        // Only running ones chosen: there is nothing to remove without force.
        let running: Vec<&Container> = all.iter().copied().filter(|c| c.running()).collect();
        let confirm = Confirm::containers(&running);
        assert_eq!(labels(&confirm), ["Stop and remove all", "Cancel"]);
        // Enter never forces: it closes the dialog.
        assert_eq!(confirm.default_choice(), Choice::Cancel);
        assert!(confirm.request(Choice::Force).is_some());
        assert_eq!(confirm.request(Choice::Go), None);
    }

    #[test]
    fn removing_an_image_in_use_needs_the_second_button() {
        let lists = samples::lists();
        let all: Vec<&Image> = lists.images.iter().collect();
        let confirm = Confirm::images(&all, &lists.containers);
        let lines = texts(&confirm);
        assert!(lines.contains(&"  old-admin:latest".to_string()));
        assert!(lines.contains(&"  <none> 8524458b60e3".to_string()));
        assert!(lines.contains(&"  postgres:16-alpine  ·  used by shop-postgres".to_string()));
        assert_eq!(labels(&confirm), ["Remove", "Remove anyway", "Cancel"]);
        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0].len(), 3);
        assert_eq!(go.steps[0][1], "old-admin:latest");
        assert!(go.steps[0][2].starts_with("sha256:8524"));
        let force = confirm.request(Choice::Force).unwrap();
        assert_eq!(&force.steps[0][..2], ["rmi", "--force"]);
        assert_eq!(force.steps[0].len(), 6);
    }

    #[test]
    fn a_volume_in_use_is_named_and_left_out() {
        let lists = samples::lists();
        let all: Vec<&Volume> = lists.volumes.iter().collect();
        let confirm = Confirm::volumes(&all, &lists.containers);
        let lines = texts(&confirm);
        assert!(lines.contains(&"  old_cache".to_string()));
        assert!(lines.contains(&"  shop_pg_data  ·  used by shop-postgres".to_string()));
        assert_eq!(labels(&confirm), ["Remove", "Cancel"]);
        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0][..3], ["volume", "rm", "old_cache"]);
        assert_eq!(go.steps[0].len(), 4);
        assert_eq!(confirm.request(Choice::Force), None);

        // Nothing but volumes in use: the dialog can only be closed.
        let used: Vec<&Volume> = all
            .iter()
            .copied()
            .filter(|v| v.name == "shop_pg_data")
            .collect();
        let confirm = Confirm::volumes(&used, &lists.containers);
        assert_eq!(labels(&confirm), ["Close"]);
        assert_eq!(confirm.request(Choice::Go), None);
    }

    #[test]
    fn prune_says_how_much_goes_and_named_things_need_the_tick() {
        let lists = samples::lists();
        let mut confirm = Confirm::prune_images(&lists.images, &lists.containers);
        assert_eq!(
            texts(&confirm),
            [
                "1 image will be removed, about 1.2GB:",
                "  <none> 8524458b60e3  ·  1.2GB"
            ]
        );
        assert_eq!(
            confirm.tick(),
            Some((
                "also the 1 named image no container uses".to_string(),
                false
            ))
        );
        confirm.toggle();
        assert_eq!(texts(&confirm)[0], "2 images will be removed, about 1.3GB:");
        assert_eq!(confirm.request(Choice::Go).unwrap().steps[0].len(), 3);

        let mut confirm = Confirm::prune_volumes(&lists.volumes, &lists.containers);
        assert_eq!(
            texts(&confirm),
            [
                "1 volume and all data in them will be removed:",
                "  4fb0c2a2af7e"
            ]
        );
        confirm.toggle();
        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0].len(), 4);
        assert!(go.steps[0].contains(&"old_cache".to_string()));
        assert!(!go.steps[0].contains(&"shop_pg_data".to_string()));

        let confirm = Confirm::prune_containers(&lists.containers);
        assert_eq!(texts(&confirm)[0], "2 stopped containers will be removed:");
        assert_eq!(confirm.tick(), None);

        // With nothing to prune, there is nothing to press but Close.
        let confirm = Confirm::prune_containers(&[]);
        assert_eq!(
            texts(&confirm),
            ["No container is stopped. Nothing to prune."]
        );
        assert_eq!(labels(&confirm), ["Close"]);
    }

    #[test]
    fn taking_a_project_down_keeps_its_volumes_unless_ticked() {
        let lists = samples::lists();
        let shop = lists.projects.iter().find(|p| p.name == "shop").unwrap();
        let mut confirm = Confirm::down(shop, &lists.containers);
        let lines = texts(&confirm);
        assert!(lines.contains(&"  shop-postgres".to_string()));
        assert!(lines.contains(&"Its volumes stay.".to_string()));
        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0].join(" "), "compose -p shop down");
        assert!(go.show_output);

        confirm.toggle();
        assert!(
            texts(&confirm)
                .contains(&"Its volumes and all data in them are removed too.".to_string())
        );
        let go = confirm.request(Choice::Go).unwrap();
        assert_eq!(go.steps[0].join(" "), "compose -p shop down --volumes");
    }

    #[test]
    fn s_on_a_project_stops_it_while_any_of_it_runs_and_starts_it_otherwise() {
        let lists = samples::lists();
        // Two of three run: the third may be a job that ran once and ended.
        let shop = lists.projects.iter().find(|p| p.name == "shop").unwrap();
        let stop = project_toggle(shop);
        assert_eq!(stop.doing, "Stopping project shop");
        assert_eq!(stop.steps[0].join(" "), "compose -p shop stop");
        let blog = lists.projects.iter().find(|p| p.name == "blog").unwrap();
        let start = project_toggle(blog);
        assert_eq!(start.doing, "Starting project blog");
        assert_eq!(start.steps[0].join(" "), "compose -p blog start");
        assert!(start.show_output);
    }
}
