//! The `DevAssist` window.

mod assets;

use std::process::ExitCode;

fn main() -> ExitCode {
    match assets::check() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
