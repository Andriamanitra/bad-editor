use std::io::stdout;

use bad_editor::{App, cli};
use crossterm::ExecutableCommand;
use crossterm::cursor::{Hide as HideCursor, Show as ShowCursor};
use crossterm::event::{
    DisableMouseCapture,
    EnableMouseCapture,
    KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

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
        let _ = stdout().execute(PopKeyboardEnhancementFlags);
        let _ = stdout().execute(DisableMouseCapture);
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = App::new();
    app.load_runtime_syntaxes();

    let args = cli::parse_cli_args();
    if let Some(file_loc) = args.get_one::<cli::FilePathWithOptionalLocation>("file") {
        app.open_file_pane(file_loc);
    }

    // TerminalGuard ensures raw mode gets disabled if the app crashes.
    // Drop runs when variable leaves the scope, even on panic.
    let terminal_guard = TerminalGuard::acquire()?;
    stdout().execute(HideCursor)?;
    stdout().execute(EnterAlternateScreen)?;
    stdout().execute(EnableMouseCapture)?;
    stdout().execute(PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;

    app.run(&mut stdout())?;

    drop(terminal_guard);

    // the backtrace from panicking is in the alternate screen
    // so we only want to execute this when exiting normally
    stdout().execute(LeaveAlternateScreen)?;

    Ok(())
}
