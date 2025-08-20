mod app;
pub mod cli;
mod clipboard;
mod cursor;
mod editing;
mod highlighter;
mod linter;
mod pane;
mod prompt;
mod render;
mod rope_ext;
mod ropebuffer;
mod run;

use std::num::NonZeroUsize;

pub use app::App;
pub use cursor::MultiCursor;
pub use pane::{Pane, PaneAction};
pub use rope_ext::RopeExt;

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
    CommandPrompt,
    CommandPromptEdit(String),
    SetInfo(String),
    HandledByPane(PaneAction),
    Copy,
    Paste,
}

#[derive(Debug, Clone, Copy)]
pub enum MoveTarget {
    Up(usize),
    Down(usize),
    Left(usize),
    Right(usize),
    Location(NonZeroUsize, NonZeroUsize),
    ByteOffset(usize),
    Start,
    End,
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
