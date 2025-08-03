pub mod bad;
pub mod cli;
mod prompt;
mod render;
mod run;
mod editing;
mod cursor;
mod rope_ext;
mod highlighter;
mod ropebuffer;
mod pane;
mod clipboard;

use std::num::NonZeroUsize;

pub use pane::Pane;
pub use pane::PaneAction;
pub use rope_ext::RopeExt;
pub use cursor::MultiCursor;

#[derive(Debug, Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}

#[derive(Debug, Clone, Copy)]
pub enum IndentKind {
    Spaces,
    Tabs
}

#[derive(Debug, Clone)]
pub enum Action {
    None,
    Quit,
    Esc,
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
    Start,
    End,
    StartOfLine,
    EndOfLine,
    NextWordBoundaryLeft,
    NextWordBoundaryRight,
    MatchingPair,
}
