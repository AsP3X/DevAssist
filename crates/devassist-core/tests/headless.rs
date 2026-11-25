//! The repositories tool driven with no terminal: what a second interface does.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use devassist_core::config::Settings;
use devassist_core::repos::gitops::Protocol;
use devassist_core::repos::state::{Mode, Repos};

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
        ])
        .args(["-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Settings with no account and nothing pinned, so the tool asks GitHub
/// nothing, and one checkout without a remote, so no fetch goes anywhere.
fn settings(root: &Path, checkout: PathBuf) -> Settings {
    Settings {
        title: "Test".into(),
        custom_title: None,
        owners: Vec::new(),
        owners_explicit: true,
        pinned: Vec::new(),
        protocol: Protocol::Https,
        doctor: false,
        checkouts: vec![checkout],
        clone_dir: root.to_path_buf(),
        placement: Vec::new(),
        collapsed: Vec::new(),
        update_check: false,
        watched: Vec::new(),
        watch_interval: 5,
        pull_requests: false,
        shell: Default::default(),
        look: Default::default(),
        update_check_now: false,
        config_path: root.join("devassist.toml"),
        first_run: false,
        unsaved: false,
    }
}

#[test]
fn the_repositories_tool_runs_without_an_interface() {
    let root = std::env::temp_dir().join(format!("devassist-headless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    git(&root, &["init", "-q", "-b", "main", "demo"]);
    let checkout = root.join("demo");
    std::fs::write(checkout.join("one"), "one").unwrap();
    git(&checkout, &["add", "one"]);
    git(&checkout, &["commit", "-q", "-m", "one"]);

    let mut repos = Repos::new(settings(&root, checkout));
    // The row is listed at once; its branch is known once the local read
    // of the checkout has arrived.
    let read = |repos: &Repos| repos.installed.len() == 1 && repos.installed[0].branch == "main";
    let deadline = Instant::now() + Duration::from_secs(20);
    while !read(&repos) {
        assert!(
            Instant::now() < deadline,
            "the local read did not arrive within 20 seconds"
        );
        repos.drain();
        std::thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(repos.installed.len(), 1);
    assert!(matches!(repos.mode, Mode::List));
    assert_eq!(repos.rows().len(), 1);

    // As an interface does when it closes: stops the fetch that follows
    // the first read.
    repos.quit();
    drop(repos);
    let _ = std::fs::remove_dir_all(&root);
}
