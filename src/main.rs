mod atoms;
mod config;
mod features;
mod shell;
mod update;

use std::io::{self, IsTerminal};

use features::repos::app::App;
use ratatui::crossterm::terminal::disable_raw_mode;
use shell::Feature;

fn main() {
    if let Err(err) = run() {
        eprintln!("devassist: {err}");
        hold_own_window();
        std::process::exit(1);
    }
}

/// Waits for Enter when the console window belongs to this program alone,
/// as it does when started from the Start menu or by a double click. Such a
/// window closes with the program, and would take the error with it.
fn hold_own_window() {
    if !owns_console() || !io::stdin().is_terminal() {
        return;
    }
    let _ = disable_raw_mode();
    eprintln!();
    eprintln!("Press Enter to close this window.");
    let _ = io::stdin().read_line(&mut String::new());
}

/// True when no other program, such as a shell, shares the console.
fn owns_console() -> bool {
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn GetConsoleProcessList(list: *mut u32, count: u32) -> u32;
        }
        let mut list = [0u32; 2];
        GetConsoleProcessList(list.as_mut_ptr(), list.len() as u32) == 1
    }
    #[cfg(not(windows))]
    false
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
