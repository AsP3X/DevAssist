//! Putting the terminal into full-screen mode and back.

use std::io;

use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

pub type Screen = Terminal<CrosstermBackend<io::Stdout>>;

/// Switches to the full-screen view. A panic switches back first, and
/// stops the programs that were started.
pub fn setup() -> Result<Screen, Box<dyn std::error::Error>> {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        crate::atoms::sys::process::kill_active();
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

pub fn restore(terminal: &mut Screen) -> Result<(), Box<dyn std::error::Error>> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        DisableMouseCapture,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// Makes the Windows console read and write UTF-8.
pub fn enable_utf8() {
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
