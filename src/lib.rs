mod app;
pub mod cli;
mod clipboard;
mod cursor;
mod editing;
mod exec;
mod highlighter;
mod linter;
mod pane;
mod prompt;
mod prompt_completer;
mod render;
mod rope_ext;
mod ropebuffer;
mod run;
mod completer;

use std::num::NonZeroUsize;
use std::path::PathBuf;

pub use app::App;
pub use cursor::MultiCursor;
pub use pane::{Pane, PaneAction};
pub use rope_ext::RopeExt;

use crate::cli::FilePathWithOptionalLocation;

#[derive(Debug, Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}

#[derive(Debug, Clone, Copy)]
pub enum IndentKind {
    Spaces,
    Tabs,
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Quit,
    Esc,
    Resize(u16, u16),
    Command(String),
    CommandPrompt,
    CommandPromptEdit(String),
    SetInfo(String),
    HandledByPane(PaneAction),
    Save,
    SaveAs(PathBuf),
    Open(FilePathWithOptionalLocation),
    Cut,
    Copy,
    Paste,
    NewPane,
    ClosePane,
    GoToPane(usize),
    NextPane,
    PreviousPane,
}

#[derive(Debug, Clone, Copy)]
pub enum MoveTarget {
    Up(usize),
    Down(usize),
    Left(usize),
    Right(usize),
    Location(NonZeroUsize, NonZeroUsize),
    ByteOffset(usize),
    StartOfFile,
    EndOfFile,
    StartOfLine,
    EndOfLine,
    NextWordBoundaryLeft,
    NextWordBoundaryRight,
    MatchingPair,
}

/// Quotes strings with spaces, quotes, or control characters in them
/// Only intended to provide visual clarity, does NOT make the path shell-safe!
pub fn quote_path(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string()
    }
    let mut single_quote = false;
    let mut double_quote = false;
    let mut space = false;
    let mut special = false;
    for c in s.chars() {
        match c {
            '\'' => single_quote = true,
            '"' => double_quote = true,
            ' ' => space = true,
            _ => if c.is_whitespace() || c.is_control() { special = true }
        }
    }
    if !special {
        if !single_quote && !double_quote && !space {
            return s.to_string()
        }
        if !single_quote {
            return format!("'{s}'")
        }
        if !double_quote {
            return format!("\"{s}\"")
        }
    }
    format!("{s:?}")
}

/// Expands ~ to `$HOME` if `$HOME` is defined
pub fn expand_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        match std::env::var_os("HOME") {
            Some(homedir) => std::path::PathBuf::from(homedir).join(rest),
            None => path.into(),
        }
    } else if path == "~" {
        match std::env::var_os("HOME") {
            Some(homedir) => std::path::PathBuf::from(homedir),
            None => "~".into(),
        }
    } else {
        path.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string() {
        assert_eq!(quote_path(""), "''");
    }

    #[test]
    fn no_special_chars() {
        assert_eq!(quote_path("file.txt"), "file.txt");
    }

    #[test]
    fn with_space() {
        assert_eq!(quote_path("my file.txt"), "'my file.txt'");
    }

    #[test]
    fn with_special_char() {
        assert_eq!(quote_path("file\n.txt"), "\"file\\n.txt\"");
    }

    #[test]
    fn with_single_quote_only() {
        assert_eq!(quote_path("file's.txt"), "\"file's.txt\"");
    }

    #[test]
    fn with_double_quote_only() {
        assert_eq!(quote_path("file\"name.txt"), "'file\"name.txt'");
    }

    #[test]
    fn with_both_quotes() {
        assert_eq!(quote_path("he said: \"don't\""), "\"he said: \\\"don't\\\"\"");
    }
}
