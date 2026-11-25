//! The Docker tool driven with no terminal and no running Docker.

#[test]
fn the_docker_tool_runs_without_an_interface() {
    use devassist_core::docker::{model::samples, state::DockerState};
    let docker = DockerState::offline(samples::lists());
    assert!(!docker.rows().is_empty());
    assert!(!docker.modal());
}
