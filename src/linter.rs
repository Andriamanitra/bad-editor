use std::collections::HashMap;
use std::error::Error;
use std::num::NonZero;
use std::path::PathBuf;

use janetrs::client::JanetClient;
use janetrs::{Janet, JanetKeyword, JanetString, JanetStruct, TaggedJanet};

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
    JanetInitError,
    JanetCompileError,
    JanetParseError,
    JanetRuntimeError,
    JanetMissingRequiredLintField { field: &'static str },
    JanetFieldWithWrongType { field: &'static str, expected_type: &'static str, actual_type: String },
    Other(String)
}

impl From<janetrs::client::Error> for LinterError {
    fn from(value: janetrs::client::Error) -> Self {
        match value {
            janetrs::client::Error::CompileError => LinterError::JanetCompileError,
            janetrs::client::Error::ParseError => LinterError::JanetParseError,
            janetrs::client::Error::RunError => LinterError::JanetRuntimeError,
            _ => LinterError::Other("unknown janetrs error".into()),
        }
    }
}

impl std::fmt::Display for LinterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self {
            LinterError::JanetInitError => write!(f, "linter error: janet interpreter failed to initialize"),
            LinterError::JanetCompileError => write!(f, "linter error: linter.janet failed to compile"),
            LinterError::JanetParseError => write!(f, "linter error: bad linter.janet"),
            LinterError::JanetRuntimeError => write!(f, "linter error: runtime error in linter.janet"),
            LinterError::Other(error_msg) => write!(f, "linter error: {error_msg}"),
            LinterError::JanetMissingRequiredLintField { field } => write!(f, "linter error: linter.janet returned a lint without {field}"),
            LinterError::JanetFieldWithWrongType { field, expected_type, actual_type } => {
                write!(f, "linter error: expected {field} to be {expected_type} but received {actual_type}")
            }
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

impl TryFrom<Janet> for Lint {
    type Error = LinterError;

    fn try_from(value: Janet) -> Result<Self, Self::Error> {
        fn get_required_field<'a>(lint: &'a JanetStruct, field: &'static str) -> Result<&'a Janet, LinterError> {
            match lint.get(JanetKeyword::new(field)) {
                Some(value) => Ok(value),
                None => Err(LinterError::JanetMissingRequiredLintField { field }),
            }
        }

        fn get_positive_number(field: &'static str, value: &Janet) -> Result<NonZero<usize>, LinterError> {
            match value.unwrap() {
                TaggedJanet::Number(num) => {
                    let u = num as usize;
                    if u > 0 && u as f64 == num {
                        Ok(NonZero::new(u).unwrap())
                    } else {
                        Err(LinterError::JanetFieldWithWrongType { field, expected_type: "NonZero<usize>", actual_type: "f64".into() })
                    }
                }
                other => {
                    let actual_type = other.kind().to_string();
                    Err(LinterError::JanetFieldWithWrongType { field, expected_type: "number", actual_type })
                }
            }
        }

        match JanetStruct::try_from(value) {
            Ok(lint_struct) => {
                let message = get_required_field(&lint_struct, "message")?;
                let message = JanetString::try_from(*message)
                    .map_err(|_| LinterError::JanetFieldWithWrongType {
                        field: "message",
                        expected_type: "string",
                        actual_type: TaggedJanet::from(*message).kind().to_string()
                    })?
                    .to_str_lossy()
                    .to_string();
                let filename = get_required_field(&lint_struct, "filename")?;
                let filename = JanetString::try_from(*filename)
                    .map_err(|_| LinterError::JanetFieldWithWrongType {
                        field: "filename",
                        expected_type: "string",
                        actual_type: TaggedJanet::from(*filename).kind().to_string()
                    })?
                    .to_str_lossy()
                    .to_string();
                let line = get_required_field(&lint_struct, "line")?;
                let line = get_positive_number("line", line)?;
                let column = match lint_struct.get(JanetKeyword::new("column")) {
                    Some(val) => Some(get_positive_number("column", val)?),
                    None => None,
                };
                let level = match lint_struct.get(JanetKeyword::new("severity")) {
                    Some(val) => match val.unwrap() {
                        TaggedJanet::Keyword(kw) => {
                            match kw.as_bytes() {
                                b"warning" => Severity::Warning,
                                b"error" => Severity::Error,
                                b"info" => Severity::Info,
                                _ => return Err(LinterError::JanetFieldWithWrongType {
                                    field: "severity",
                                    expected_type: "keyword",
                                    actual_type: "invalid keyword".into()
                                })
                            }
                        },
                        other => {
                            let actual_type = other.kind().to_string();
                            return Err(LinterError::JanetFieldWithWrongType { field: "level", expected_type: "keyword", actual_type })
                        }
                    },
                    None => Severity::Warning,
                };

                Ok(Lint { message, filename, line, column, level })
            }
            Err(_) => {
                let actual_type = TaggedJanet::from(value).kind().to_string();
                Err(LinterError::JanetFieldWithWrongType { field: "lint", expected_type: "struct", actual_type })
            }
        }
    }
}

const LINTER_SCRIPT: &str = include_str!("../defaults/linter.janet");

pub fn run_linter_command(filename: Option<&str>, filetype: &str) -> Result<HashMap<Filename, Vec<Lint>>, LinterError> {
    let filename = filename.unwrap_or_default();
    let Ok(janet) = JanetClient::init_with_default_env() else {
        return Err(LinterError::JanetInitError)
    };
    janet.run(LINTER_SCRIPT).map_err(LinterError::from)?;
    let val = janet.run(format!("(lint :{filetype} {filename:?})")).map_err(LinterError::from)?;
    match TaggedJanet::from(val) {
        TaggedJanet::Array(lints) => {
            let lints = lints.into_iter().map(Lint::try_from).collect::<Result<Vec<Lint>, LinterError>>()?;
            let mut results: HashMap<Filename, Vec<Lint>> = HashMap::new();
            for lint in lints {
                let filename = PathBuf::from(&lint.filename);
                results.entry(filename).or_default().push(lint);
            }
            Ok(results)
        },
        TaggedJanet::String(error_msg) => {
            Err(LinterError::Other(error_msg.to_str_lossy().to_string()))
        }
        other => {
            let actual_type = other.kind().to_string();
            Err(LinterError::JanetFieldWithWrongType { field: "lints", expected_type: "array", actual_type })
        }
    }
}
