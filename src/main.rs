mod atoms;
mod config;
mod features;
mod shell;
mod update;

use std::io::{self, IsTerminal};

use features::repos::app::App;
use shell::Feature;

fn main() {
    if let Err(err) = run() {
        eprintln!("devassist: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    match config::startup()? {
        config::Startup::Help => {
            config::print_help();
            Ok(())
        }
        config::Startup::Run(settings) if settings.doctor => features::repos::doctor(&settings),
        config::Startup::Run(settings) => {
            if !io::stdout().is_terminal() {
                eprintln!("devassist is a full-screen terminal app.");
                eprintln!("Open Windows Terminal or PowerShell and run devassist");
                std::process::exit(1);
            }
            shell::run(move || vec![Box::new(App::new(*settings)) as Box<dyn Feature>])
        }
    }
}
