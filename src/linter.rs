use std::collections::HashMap;
use std::num::NonZero;
use std::path::PathBuf;
use std::process::Command;

use crate::MoveTarget;

type Filename = PathBuf;
type LineNo = NonZero<usize>;
type ColNo = NonZero<usize>;

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
    FilenameRequired,
}

pub struct Lint {
    pub message: String,
    line: LineNo,
    column: Option<ColNo>,
    level: Severity,
}

impl Lint {
    pub fn info(line: LineNo, column: Option<ColNo>, message: String) -> Self {
        Self { line, column, message, level: Severity::Info }
    }

    pub fn warning(line: LineNo, column: Option<ColNo>, message: String) -> Self {
        Self { line, column, message, level: Severity::Warning }
    }

    pub fn error(line: LineNo, column: Option<ColNo>, message: String) -> Self {
        Self { line, column, message, level: Severity::Error }
    }

    pub fn color(&self) -> crossterm::style::Color {
        match self.level {
            Severity::Info => crossterm::style::Color::Rgb { r: 0xDD, g: 0xCC, b: 0x88 },
            Severity::Warning => crossterm::style::Color::Rgb { r: 0xFF, g: 0xAF, b: 0 },
            Severity::Error => crossterm::style::Color::Rgb { r: 0xDB, g: 0, b: 0 },
        }
    }

    /// One-based line number where this Lint is located
    pub fn lineno(&self) -> usize {
        self.line.get()
    }

    pub fn location(&self) -> Option<MoveTarget> {
        let col = self.column.unwrap_or(std::num::NonZero::<usize>::MIN);
        Some(MoveTarget::Location(self.line, col))
    }

    pub fn is_error(&self) -> bool {
        matches!(self.level, Severity::Error)
    }
}

pub fn run_linter_command(filename: Option<&str>, filetype: &str) -> Result<HashMap<Filename, Vec<Lint>>, LinterError> {
    // TODO: this should be configurable, not hard coded
    match filetype {
        "rust" => {
            fn parse_clippy_lint(line: &str) -> Option<(Filename, Lint)> {
                match line.splitn(4, ':').collect::<Vec<_>>()[..] {
                    [fname, line, col, msg] => {
                        let line: LineNo = line.parse().ok()?;
                        let col: ColNo = col.parse().ok()?;
                        let lint = if msg.starts_with(" warning") {
                            Lint::warning(line, Some(col), msg.to_string())
                        } else if msg.starts_with(" error") {
                            Lint::error(line, Some(col), msg.to_string())
                        } else {
                            Lint::info(line, Some(col), msg.to_string())
                        };
                        Some((PathBuf::from(fname), lint))
                    }
                    _ => None,
                }
            }

            match Command::new("cargo").args(["clippy", "--message-format=short"]).output() {
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let mut results: HashMap<Filename, Vec<Lint>> = HashMap::new();
                    for (fname, lint) in stderr.lines().filter_map(parse_clippy_lint) {
                        results.entry(fname).or_default().push(lint);
                    }
                    Ok(results)
                }
                Err(err) => Err(LinterError::FailedToRun(err)),
            }
        }
        "python" => {
            let Some(filename) = filename else {
                return Err(LinterError::FilenameRequired)
            };

            fn parse_ruff_lint(line: &str) -> Option<(Filename, Lint)> {
                match line.splitn(4, ':').collect::<Vec<_>>()[..] {
                    [fname, line, col, msg] => {
                        let line: LineNo = line.parse().ok()?;
                        let col: ColNo = col.parse().ok()?;
                        let lint = Lint::warning(line, Some(col), msg.to_string());
                        Some((PathBuf::from(fname), lint))
                    }
                    _ => None,
                }
            }

            fn parse_mypy_lint(line: &str) -> Option<(Filename, Lint)> {
                match line.splitn(3, ':').collect::<Vec<_>>()[..] {
                    [fname, line, msg] => {
                        let line: LineNo = line.parse().ok()?;
                        let lint = if msg.starts_with(" error") {
                            Lint::error(line, None, msg.to_string())
                        } else {
                            Lint::info(line, None, msg.to_string())
                        };
                        Some((PathBuf::from(fname), lint))
                    }
                    _ => None,
                }
            }

            let mut results: HashMap<Filename, Vec<Lint>> = HashMap::new();
            let ruff = match Command::new("uvx").args(["ruff", "check", "--output-format=concise", filename]).output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for (fname, lint) in stdout.lines().filter_map(parse_ruff_lint) {
                        results.entry(fname).or_default().push(lint);
                    }
                    Ok(())
                }
                Err(err) => Err(LinterError::FailedToRun(err)),
            };
            let mypy = match Command::new("uvx").args(["mypy", "--strict", filename]).output() {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    for (fname, lint) in stdout.lines().filter_map(parse_mypy_lint) {
                        results.entry(fname).or_default().push(lint);
                    }
                    Ok(())
                }
                Err(err) => Err(LinterError::FailedToRun(err)),
            };
            if let (Err(err), Err(_)) = (ruff, mypy) {
                return Err(err);
            }
            Ok(results)
        }
        _ => Err(LinterError::NoLinterForFileType),
    }
}
