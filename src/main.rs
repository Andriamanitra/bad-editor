use std::io::stdout;

use bad_editor::bad;
use crossterm::ExecutableCommand;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::cursor::Hide as HideCursor;
use crossterm::cursor::Show as ShowCursor;

struct TerminalGuard;
impl TerminalGuard {
    fn acquire() -> Result<Self, Box<dyn std::error::Error>> {
        crossterm::terminal::enable_raw_mode()?;
        Ok(Self)
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = stdout().execute(ShowCursor);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // TODO: CLI
    let app = bad::App::new();

    // TerminalGuard ensures raw mode gets disabled if the app crashes.
    // Drop runs when variable leaves the scope, even on panic.
    let terminal_guard = TerminalGuard::acquire()?;
    stdout().execute(HideCursor)?;
    stdout().execute(EnterAlternateScreen)?;

    app.run(&mut stdout())?;

    drop(terminal_guard);

    // the backtrace from panicking is in the alternate screen
    // so we only want to execute this when exiting normally
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}
