mod app;
mod atoms;
mod config;
mod github;
mod gitops;
mod gitprogress;
mod settings;
mod ui;
mod update;

use std::io::{self, IsTerminal};
use std::time::Duration;

use app::App;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{self, DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

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
        config::Startup::Run(settings) if settings.doctor => doctor(&settings),
        config::Startup::Run(settings) => run_tui(*settings),
    }
}

fn run_tui(settings: config::Settings) -> Result<(), Box<dyn std::error::Error>> {
    if !io::stdout().is_terminal() {
        eprintln!("devassist is a full-screen terminal app.");
        eprintln!("Open Windows Terminal or PowerShell and run devassist");
        std::process::exit(1);
    }
    enable_utf8();
    let mut app = App::new(settings);
    let mut terminal = setup_terminal()?;
    let mut dirty = true;
    let result = loop {
        if dirty || app.waiting() {
            terminal.draw(|frame| ui::draw(frame, &mut app))?;
            dirty = false;
        }
        if app.should_quit {
            break Ok(());
        }
        let wait = if app.waiting() { 40 } else { 200 };
        if event::poll(Duration::from_millis(wait))? {
            while event::poll(Duration::from_millis(0))? {
                app.on_event(event::read()?);
            }
            dirty = true;
        }
        if app.drain() {
            dirty = true;
        }
    };
    restore_terminal(&mut terminal)?;
    if app.restart {
        // An update was installed: hand the terminal to the new version.
        let exe = std::env::current_exe()?;
        let status = std::process::Command::new(exe)
            .args(std::env::args_os().skip(1))
            .status()?;
        std::process::exit(status.code().unwrap_or(0));
    }
    result
}

fn doctor(settings: &config::Settings) -> Result<(), Box<dyn std::error::Error>> {
    println!("version   {}", update::VERSION);
    match update::check(github::token().as_deref()) {
        Ok(Some(release)) => println!("update    {} is available", release.version),
        Ok(None) => println!("update    none, this is the latest"),
        Err(err) => println!("update    could not look: {err}"),
    }
    println!("settings  {}", settings.config_path.display());
    println!("clone dir {}", settings.clone_dir.display());
    println!("title     {}", settings.title);
    println!("protocol  {}", settings.protocol);
    if settings.owners.is_empty() {
        println!("owners    (none)");
    } else {
        println!("owners    {}", settings.owners.join(", "));
    }
    if !settings.pinned.is_empty() {
        let pinned: Vec<String> = settings.pinned.iter().map(|spec| spec.id()).collect();
        println!("pinned    {}", pinned.join(", "));
    }
    let installed = gitops::load(&settings.checkouts);
    println!("listed    {} local checkouts", installed.len());
    let catalog = github::fetch_catalog(&settings.owners, &settings.pinned);
    println!("github    {} repositories", catalog.repos.len());
    for err in &catalog.errors {
        println!("github error: {err}");
    }
    let mut failed = 0;
    for repo in &installed {
        match gitops::status(&repo.path) {
            Ok(info) => {
                let mut view = repo.clone();
                view.apply(info);
                println!(
                    "  {:<36} {:<16} {:<8} {}",
                    format!("{}/{}", view.owner, view.folder),
                    view.branch,
                    view.state_label(),
                    view.sync_label()
                );
            }
            Err(err) => {
                failed += 1;
                println!(
                    "  {:<36} status failed: {err}",
                    format!("{}/{}", repo.owner, repo.folder)
                );
            }
        }
    }
    if !catalog.errors.is_empty() || failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>, Box<dyn std::error::Error>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        atoms::sys::process::kill_active();
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), DisableMouseCapture, LeaveAlternateScreen);
        hook(info);
    }));
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn enable_utf8() {
    #[cfg(windows)]
    unsafe {
        unsafe extern "system" {
            fn SetConsoleOutputCP(code_page: u32) -> i32;
            fn SetConsoleCP(code_page: u32) -> i32;
        }
        SetConsoleOutputCP(65001);
        SetConsoleCP(65001);
    }
}
