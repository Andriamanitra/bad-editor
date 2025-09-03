use std::error::Error;
use std::fmt::Display;
use std::path::Path;
use std::process::Command;
use std::os::unix::process::CommandExt;

use crossterm::cursor::{Hide as HideCursor, Show as ShowCursor};
use crossterm::event::{
    DisableMouseCapture,
    EnableMouseCapture,
};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};

#[derive(Debug)]
pub enum ExecError {
    InvalidTemplate,
    NonUTF8Path,
    NotFound { executable: String },
    PermissionDenied { executable: String },
    Unknown(std::io::Error),
}

impl Display for ExecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecError::InvalidTemplate => f.write_str("exec error: invalid template"),
            ExecError::NonUTF8Path => f.write_str("exec error: file path must be valid UTF-8"),
            ExecError::NotFound { executable } => write!(f, "exec error: cannot execute {executable} (no such file)"),
            ExecError::PermissionDenied { executable } => write!(f, "exec error: cannot execute {executable} (permission denied)"),
            ExecError::Unknown(error) => write!(f, "exec error: {error}"),
        }
    }
}

impl Error for ExecError {}

fn execute_interactive_command(command: Command) -> Result<(), ExecError> {
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

    let executable = crate::quote_path(&command.get_program().to_string_lossy());
    let result = run_the_command(command).map_err(|err| match err.kind() {
        std::io::ErrorKind::NotFound => ExecError::NotFound { executable },
        std::io::ErrorKind::PermissionDenied => ExecError::PermissionDenied { executable },
        _ => ExecError::Unknown(err)
    });

    crossterm::execute!(std::io::stdout(), EnterAlternateScreen, HideCursor, EnableMouseCapture)
        .and_then(|_| crossterm::terminal::enable_raw_mode())
        .expect("this already succeeded when starting the editor so it should not fail now");

    result
}

fn command_from_template(template: &str, path: &Path) -> Result<Command, ExecError> {
    let filled_template = if template.contains("%f") {
        let stringified_path = path.to_str().ok_or(ExecError::NonUTF8Path)?;
        &template.replace("%f", stringified_path)
    } else {
        template
    };
    let parts = shlex::split(filled_template).ok_or(ExecError::InvalidTemplate)?;
    let (cmd, args) = parts.split_first().ok_or(ExecError::InvalidTemplate)?;
    let mut cmd = Command::new(cmd);
    cmd.args(args);
    Ok(cmd)
}

pub fn execute_interactive_command_from_template(template: &str, path: &Path) -> Result<(), ExecError> {
    let command = command_from_template(template, path)?;
    execute_interactive_command(command)?;
    Ok(())
}
