use std::process::Command;
use std::os::unix::process::CommandExt;

use crossterm::cursor::{Hide as HideCursor, Show as ShowCursor};
use crossterm::event::{
    DisableMouseCapture,
    EnableMouseCapture,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

pub fn execute_interactive_command(command: Command) -> std::io::Result<()> {
    fn run_the_command(mut command: Command) -> std::io::Result<()> {
        let status = unsafe {
            // We need to ignore SIGTTOU to be able to call tcsetpgrp from a
            // member of a background process group or we'll get a nasty crash
            // with no error message!
            libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            
            // It is important for the child to be in a new process group so it
            // can become the foreground process group on its own. 0 is a sentinel
            // value for creating a new process group.
            // FIXME: If the editor is killed by a signal while a command is running,
            // the child process should also be killed.
            let mut child = command.process_group(0).spawn()?;

            let old_foreground_process_group = libc::tcgetpgrp(0);

            // Make child the foreground process group for the terminal
            libc::tcsetpgrp(0, child.id() as i32);

            // We can ignore the return value because subsequent calls to
            // child.wait() will return the same thing.
            let _ = child.wait();

            // Make the editor's process group the foreground process group again.
            libc::tcsetpgrp(0, old_foreground_process_group);

            // It's good practice to restore the signal handler; not sure if it
            // ever actually matters though.
            libc::signal(libc::SIGTTOU, libc::SIG_DFL);

            child.wait()
        };

        match status?.code() {
            Some(code) => println!("\nExited with status code {code} (press Enter to return to editor)"),
            None => println!("\nProcess terminated by signal (press Enter to return to editor)")
        }
        
        let mut buf = String::new();
        std::io::stdin().read_line(&mut buf)?;
        Ok(())
    }

    crossterm::execute!(std::io::stdout(), LeaveAlternateScreen, ShowCursor, DisableMouseCapture)
        .and_then(|_| crossterm::terminal::disable_raw_mode())
        .expect("this already succeeded when starting the editor so it should not fail now");

    let _ = run_the_command(command);

    crossterm::execute!(std::io::stdout(), EnterAlternateScreen, HideCursor, EnableMouseCapture)
        .and_then(|_| crossterm::terminal::enable_raw_mode())
        .expect("this already succeeded when starting the editor so it should not fail now");

    Ok(())
}
