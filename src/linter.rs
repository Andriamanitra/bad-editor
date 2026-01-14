use std::collections::HashMap;
use std::error::Error;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::io::Read;
use std::num::NonZero;
use std::path::PathBuf;

use crate::MoveTarget;

type Filename = PathBuf;
type LineNo = NonZero<usize>;
type ColNo = NonZero<usize>;

pub(crate) const DEFAULT_LINTER_SCRIPT: &str = include_str!("../default_config/linters.janet");

enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug)]
pub enum LinterError {
    FilenameRequired,
    JanetNotInstalled,
    LinterScriptIOError,
    BadLinterScript(String),
    Other(String)
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            LinterError::FilenameRequired => write!(f, "linter error: filename required"),
            LinterError::JanetNotInstalled => write!(f, "linter error: janet not found in $PATH"),
            LinterError::BadLinterScript(msg) => write!(f, "linters.janet error: {msg}"),
            LinterError::Other(error_msg) => write!(f, "linter error: {error_msg}"),
            LinterError::LinterScriptIOError => write!(f, "linter error: failed to read linters.janet"),
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

    pub fn parse(line: &str) -> Option<Self> {
        static LINT_PATTERN: std::sync::OnceLock<grok::Pattern> = std::sync::OnceLock::new();
        let patt: &grok::Pattern = LINT_PATTERN.get_or_init(|| {
            let mut grok = grok::Grok::default();
            grok.add_pattern("FILENAME", r"[^:]+");
            grok.add_pattern("LINE", r"[0-9]+");
            grok.add_pattern("COLUMN", r"[0-9]*");
            grok.add_pattern("SEVERITY", r"(info|warning|error)");
            grok.add_pattern("MESSAGE", r".*$");
            grok
                .compile("%{FILENAME}:%{LINE}:%{COLUMN}:%{SEVERITY}:%{MESSAGE}", false)
                .expect("the lint grok pattern is valid")
        });
        let grok_matches = patt.match_against(line)?;
        let filename = grok_matches.get("FILENAME")?.to_string();
        let line = grok_matches.get("LINE")?.parse::<LineNo>().ok()?;
        let column = grok_matches.get("COLUMN")?.parse::<ColNo>().ok();
        let level = match grok_matches.get("SEVERITY") {
            Some("info") => Severity::Info,
            Some("warning") => Severity::Warning,
            Some("error") => Severity::Error,
            _ => return None
        };
        let message = grok_matches.get("MESSAGE")?.to_string();
        Some(Self { filename, message, line, column, level })
    }
}

pub fn run_linter_command(script_path: Option<PathBuf>, filename: Option<&str>, filetype: &str) -> Result<HashMap<Filename, Vec<Lint>>, LinterError> {
    let Some(filename) = filename else {
        return Err(LinterError::FilenameRequired)
    };

    let script = if let Some(script_path) = script_path {
        let opened = OpenOptions::new().read(true).create(false).open(script_path);
        match opened.map_err(|e| e.kind()) {
            Ok(mut file) => {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).map_err(|_| LinterError::LinterScriptIOError)?;
                Some(String::from_utf8_lossy(&buf).to_string())
            }
            Err(ErrorKind::PermissionDenied) => Err(LinterError::LinterScriptIOError)?,
            Err(_) => None
        }
    } else {
        None
    };

    let mut janet = std::process::Command::new("janet");
    if let Some(script) = script {
        janet.arg("-e");
        janet.arg(script);
    } else {
        janet.arg("-e");
        janet.arg(DEFAULT_LINTER_SCRIPT);
    };
    janet.arg("-E").arg("(lint $0 $1)").arg(filetype).arg(filename);

    match janet.output().map_err(|e| e.kind()) {
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                return Err(LinterError::BadLinterScript(stderr.into()));
            }
            let mut lints = HashMap::new();
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(lint) = Lint::parse(line) {
                    let k = PathBuf::from(&lint.filename);
                    let entry: &mut Vec<Lint> = lints.entry(k).or_default();
                    entry.push(lint);
                }
            }
            Ok(lints)
        }
        Err(ErrorKind::NotFound) => Err(LinterError::JanetNotInstalled),
        Err(err) =>  Err(LinterError::Other(err.to_string())),
    }
}
