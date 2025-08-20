use std::collections::HashMap;
use std::path::PathBuf;

type Filename = PathBuf;

enum Severity {
    Info,
    Warning,
    Error,
}

// TODO: implement Display for LinterError so it can be printed nicely
#[derive(Debug)]
pub enum LinterError {
    NoLinterForFileType,
    FailedToRun(std::io::Error),
}

pub struct Lint {
    pub lineno: usize,
    pub message: String,
    level: Severity,
}

impl Lint {
    pub fn info(lineno: usize, message: String) -> Self {
        let lineno = lineno.saturating_sub(1);
        Self { lineno, message, level: Severity::Info }
    }

    pub fn warning(lineno: usize, message: String) -> Self {
        let lineno = lineno.saturating_sub(1);
        Self { lineno, message, level: Severity::Warning }
    }

    pub fn error(lineno: usize, message: String) -> Self {
        let lineno = lineno.saturating_sub(1);
        Self { lineno, message, level: Severity::Error }
    }

    pub fn color(&self) -> crossterm::style::Color {
        match self.level {
            Severity::Info => crossterm::style::Color::Rgb { r: 0xDD, g: 0xCC, b: 0x88 },
            Severity::Warning => crossterm::style::Color::Rgb { r: 0xFF, g: 0xAF, b: 0 },
            Severity::Error => crossterm::style::Color::Rgb { r: 0xDB, g: 0, b: 0 },
        }
    }
}

pub fn run_linter_command(filetype: &str) -> Result<HashMap<Filename, Vec<Lint>>, LinterError> {
    // TODO: this should be configurable, not hard coded
    match filetype {
        "rust" => {
            match std::process::Command::new("cargo").args(["clippy", "--message-format=short"]).output() {
                Ok(output) => {
                    fn read_lint(line: &str) -> Option<(Filename, Lint)> {
                        match line.splitn(4, ':').collect::<Vec<_>>()[..] {
                            [fname, line, col, msg] => {
                                let line: usize = line.parse().ok()?;
                                let _col: usize = col.parse().ok()?;
                                let lint = if msg.starts_with(" warning") {
                                    Lint::warning(line, msg.to_string())
                                } else if msg.starts_with(" error") {
                                    Lint::error(line, msg.to_string())
                                } else {
                                    Lint::info(line, msg.to_string())
                                };
                                Some((PathBuf::from(fname), lint))
                            }
                            _ => None,
                        }
                    }

                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let mut results: HashMap<Filename, Vec<Lint>> = HashMap::new();
                    for (fname, lint) in stderr.lines().filter_map(read_lint) {
                        results.entry(fname).or_default().push(lint);
                    }
                    Ok(results)
                }
                Err(err) => Err(LinterError::FailedToRun(err)),
            }
        }
        _ => Err(LinterError::NoLinterForFileType),
    }
}
