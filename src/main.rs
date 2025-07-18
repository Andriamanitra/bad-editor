mod bad;
mod prompt;
mod render;

use std::io::stdout;
use std::{error::Error, time::Duration};

use crossterm::cursor;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

fn main() -> Result<(), Box<dyn Error>> {
    crossterm::execute!(stdout(), EnterAlternateScreen, cursor::Hide)?;
    enable_raw_mode()?;

    // TODO: CLI
    let mut app = bad::App::new();

    const POLL_TIMEOUT: Duration = Duration::from_millis(16);

    app.render(&mut stdout())?;
    loop {
        if crossterm::event::poll(POLL_TIMEOUT)? {
            let event = crossterm::event::read()?;
            match bad::get_action(&event) {
                bad::Action::Quit => break,
                action => app.handle_action(action),
            }
            app.render(&mut stdout())?;
        }
    }

    crossterm::execute!(stdout(), LeaveAlternateScreen, cursor::Show)?;
    disable_raw_mode()?;

    Ok(())
}
