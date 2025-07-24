use std::error::Error;
use std::time::{Duration, Instant};

use crossterm::ExecutableCommand;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::cursor::Hide as HideCursor;
use crossterm::cursor::Show as ShowCursor;

use crate::bad::{App, Action, get_action};

impl App {
    pub fn run(mut self, out: &mut dyn std::io::Write) -> Result<(), Box<dyn Error>> {
        out.execute(EnterAlternateScreen)?;
        out.execute(HideCursor)?;

        let result = self.enter_event_loop(out);

        let _ = out.execute(LeaveAlternateScreen);
        let _ = out.execute(ShowCursor);

        result
    }

    fn enter_event_loop(&mut self, mut out: &mut dyn std::io::Write) -> Result<(), Box<dyn Error>> {
        const POLL_TIMEOUT: Duration = Duration::from_millis(16);

        let mut need_to_render = true;
        loop {
            let frame = Instant::now();
            if need_to_render {
                self.render(&mut out)?;
                need_to_render = false;
            }
            while crossterm::event::poll(POLL_TIMEOUT.saturating_sub(frame.elapsed()))? {
                let event = crossterm::event::read()?;
                need_to_render = true;
                match get_action(&event) {
                    Action::Quit => return Ok(()),
                    action => self.handle_action(action),
                }
            }
        }
    }
}
