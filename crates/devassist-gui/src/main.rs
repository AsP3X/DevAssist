//! The `DevAssist` window.

use std::process::ExitCode;

use devassist_gui::assets;

fn main() -> ExitCode {
    match assets::check() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
