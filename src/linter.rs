use std::collections::HashMap;
use std::error::Error;
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

#[derive(Debug)]
pub enum LinterError {
    FailedToRun(std::io::Error),
    Other(String)
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            LinterError::FailedToRun(error) => write!(f, "linter error: failed to run: {error}"),
            LinterError::Other(error_msg) => write!(f, "linter error: {error_msg}"),
        }
    }
}
impl Error for LinterError {}

pub struct Lint {
    pub message: String,
    filename: String,
    line: LineNo,
    column: Option<ColNo>,
    level: Severity,
}

impl Lint {
    fn try_from_output_line(s: &str) -> Option<Self> {
        let mut parts = s.splitn(5, ":");
        let filename = parts.next()?.to_string();
        let line: LineNo = parts.next()?.parse().ok()?;
        let column: Option<ColNo> = parts.next()?.parse().ok();
        let level = match parts.next()? {
            "info" => Severity::Info,
            "warning" => Severity::Warning,
            "error" => Severity::Error,
            _ => Severity::Warning,
        };
        let message = parts.next()?.to_string();
        Some(Self { message, filename, line, column, level })
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

const LINTER_SCRIPT: &str = include_str!("../defaults/linter.janet");

pub fn run_linter_command(filename: Option<&str>, filetype: &str) -> Result<HashMap<Filename, Vec<Lint>>, LinterError> {
    let filename = filename.unwrap_or_default();
    match Command::new("janet")
        .arg("-e")
        .arg(LINTER_SCRIPT)
        .arg("-e")
        .arg(format!("(lint :{filetype} {filename:?})"))
        .output()
    {
        Ok(output) => {
            let mut results: HashMap<Filename, Vec<Lint>> = HashMap::new();
            let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
            if !stderr.is_empty() {
                return Err(LinterError::Other(stderr))
            }
            let stdout = String::from_utf8_lossy(&output.stdout);
            for lint in stdout.lines().filter_map(|line| Lint::try_from_output_line(line.trim_end())) {
                let filename = PathBuf::from(&lint.filename);
                results.entry(filename).or_default().push(lint);
            }
            Ok(results)
        }
        Err(err) => Err(LinterError::FailedToRun(err))
    }
}
