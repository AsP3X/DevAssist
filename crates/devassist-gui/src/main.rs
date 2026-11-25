//! The `DevAssist` window.
//!
//! `devassist-gui [--config PATH] [--screenshot OUT.png] [--light|--dark] [--backdrop KEY] [--no-blur] [--no-sidebar]`

use std::process::ExitCode;

use devassist_gui::{app, assets, launch};

fn main() -> ExitCode {
    let launch = match launch::parse(std::env::args().skip(1)) {
        Ok(launch) => launch,
        Err(message) => {
            complain(&format!("{message}\n{}", launch::USAGE));
            return ExitCode::from(2);
        }
    };
    if let Err(message) = assets::check() {
        complain(&format!("DevAssist's built-in files are not usable: {message}"));
        return ExitCode::FAILURE;
    }
    match app::run(launch) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            complain(&message);
            ExitCode::FAILURE
        }
    }
}

#[expect(clippy::print_stderr, reason = "start-up errors go to the terminal that started the window")]
fn complain(message: &str) {
    eprintln!("{message}");
}
