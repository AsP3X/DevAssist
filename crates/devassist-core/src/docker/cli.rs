//! Every `docker` command the tool runs, as its arguments.

use std::process::Command;

use crate::sys::process::{self, RunError};

/// One JSON object per line.
const JSON: &str = "{{json .}}";

/// How many lines of a log are shown before the new ones.
const LOG_TAIL: &str = "500";

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| part.to_string()).collect()
}

fn with(parts: &[&str], names: &[String]) -> Vec<String> {
    let mut all = args(parts);
    all.extend(names.iter().cloned());
    all
}

pub fn ps() -> Vec<String> {
    args(&["ps", "-a", "--no-trunc", "--format", JSON])
}

pub fn images() -> Vec<String> {
    args(&["image", "ls", "--no-trunc", "--format", JSON])
}

pub fn volumes() -> Vec<String> {
    args(&["volume", "ls", "--format", JSON])
}

pub fn projects() -> Vec<String> {
    args(&["compose", "ls", "-a", "--format", "json"])
}

pub fn events() -> Vec<String> {
    args(&["events", "--format", JSON])
}

/// The engine's version: the cheapest question that needs the engine.
pub fn version() -> Vec<String> {
    args(&["version", "--format", "{{.Server.Version}}"])
}

pub fn logs(container: &str) -> Vec<String> {
    args(&["logs", "--follow", "--tail", LOG_TAIL, container])
}

pub fn project_logs(project: &str) -> Vec<String> {
    args(&[
        "compose",
        "-p",
        project,
        "logs",
        "--follow",
        "--no-color",
        "--tail",
        LOG_TAIL,
    ])
}

pub fn start(ids: &[String]) -> Vec<String> {
    with(&["start"], ids)
}

pub fn stop(ids: &[String]) -> Vec<String> {
    with(&["stop"], ids)
}

pub fn restart(ids: &[String]) -> Vec<String> {
    with(&["restart"], ids)
}

/// Removes containers; with `force`, running ones too.
pub fn rm(ids: &[String], force: bool) -> Vec<String> {
    if force {
        with(&["rm", "--force"], ids)
    } else {
        with(&["rm"], ids)
    }
}

/// Removes images; with `force`, ones a container was made from too.
pub fn rmi(references: &[String], force: bool) -> Vec<String> {
    if force {
        with(&["rmi", "--force"], references)
    } else {
        with(&["rmi"], references)
    }
}

pub fn volume_rm(names: &[String]) -> Vec<String> {
    with(&["volume", "rm"], names)
}

/// Starts a project's containers as they are.
pub fn compose_start(project: &str) -> Vec<String> {
    args(&["compose", "-p", project, "start"])
}

pub fn compose_stop(project: &str) -> Vec<String> {
    args(&["compose", "-p", project, "stop"])
}

pub fn compose_restart(project: &str) -> Vec<String> {
    args(&["compose", "-p", project, "restart"])
}

/// Stops a project and removes its containers and networks; with
/// `volumes`, its volumes too.
pub fn compose_down(project: &str, volumes: bool) -> Vec<String> {
    let mut all = args(&["compose", "-p", project, "down"]);
    if volumes {
        all.push("--volumes".to_string());
    }
    all
}

/// `docker` with these arguments, ready to run without a console window.
pub fn command(arguments: &[String]) -> Command {
    #[cfg(feature = "testing")]
    crate::sys::note(crate::sys::Reach::Program);
    let mut cmd = Command::new("docker");
    cmd.args(arguments);
    process::quiet(&mut cmd);
    cmd
}

/// Why a command gave nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// There is no `docker` program.
    Missing,
    /// Docker said this.
    Failed(String),
}

impl Failure {
    pub fn from_run(err: RunError) -> Self {
        match err {
            RunError::Start(err) if err.kind() == std::io::ErrorKind::NotFound => Failure::Missing,
            RunError::Start(err) => Failure::Failed(err.to_string()),
            RunError::Wait(text) | RunError::Failed(text) => Failure::Failed(text),
            RunError::Exit(status) => Failure::Failed(format!("docker ended with {status}")),
        }
    }

    pub fn text(&self) -> String {
        match self {
            Failure::Missing => "Docker is not installed.".to_string(),
            Failure::Failed(text) => text.clone(),
        }
    }
}

/// Runs `docker` with these arguments and returns what it printed.
pub fn output(arguments: &[String]) -> Result<String, Failure> {
    process::capture(command(arguments)).map_err(Failure::from_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    fn line(arguments: Vec<String>) -> String {
        arguments.join(" ")
    }

    #[test]
    fn the_lists_are_asked_for_as_json() {
        assert_eq!(line(ps()), "ps -a --no-trunc --format {{json .}}");
        assert_eq!(line(images()), "image ls --no-trunc --format {{json .}}");
        assert_eq!(line(volumes()), "volume ls --format {{json .}}");
        assert_eq!(line(projects()), "compose ls -a --format json");
        assert_eq!(line(events()), "events --format {{json .}}");
    }

    #[test]
    fn force_is_only_passed_when_asked_for() {
        let ids = names(&["a1", "b2"]);
        assert_eq!(line(start(&ids)), "start a1 b2");
        assert_eq!(line(stop(&ids)), "stop a1 b2");
        assert_eq!(line(restart(&ids)), "restart a1 b2");
        assert_eq!(line(rm(&ids, false)), "rm a1 b2");
        assert_eq!(line(rm(&ids, true)), "rm --force a1 b2");
        assert_eq!(line(rmi(&ids, false)), "rmi a1 b2");
        assert_eq!(line(rmi(&ids, true)), "rmi --force a1 b2");
        assert_eq!(line(volume_rm(&ids)), "volume rm a1 b2");
    }

    #[test]
    fn compose_names_the_project_and_keeps_volumes_unless_told() {
        assert_eq!(line(compose_start("shop")), "compose -p shop start");
        assert_eq!(line(compose_stop("shop")), "compose -p shop stop");
        assert_eq!(line(compose_restart("shop")), "compose -p shop restart");
        assert_eq!(line(compose_down("shop", false)), "compose -p shop down");
        assert_eq!(
            line(compose_down("shop", true)),
            "compose -p shop down --volumes"
        );
        assert_eq!(
            line(project_logs("shop")),
            "compose -p shop logs --follow --no-color --tail 500"
        );
        assert_eq!(line(logs("web")), "logs --follow --tail 500 web");
    }

    #[test]
    fn a_missing_program_is_told_apart_from_a_failure() {
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert_eq!(
            Failure::from_run(RunError::Start(missing)),
            Failure::Missing
        );
        assert_eq!(
            Failure::from_run(RunError::Failed("no engine".into())),
            Failure::Failed("no engine".into())
        );
        assert_eq!(Failure::Missing.text(), "Docker is not installed.");
    }
}
