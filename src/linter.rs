use std::collections::HashMap;
use std::error::Error;
use std::num::NonZero;
use std::path::PathBuf;

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
    Other(String)
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
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
    Err(LinterError::Other("linter module not implemented".into()))
}
