pub mod bad;
mod prompt;
mod render;
mod run;
mod cursor;
mod rope_ext;
mod highlighter;

pub use rope_ext::RopeExt;
pub use cursor::Cursor;

#[derive(Debug, Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}

#[derive(Debug, Clone, Copy)]
pub enum IndentKind {
    Spaces(u8),
    Tabs
}
impl std::default::Default for IndentKind {
    fn default() -> Self {
        IndentKind::Spaces(4)
    }
}
impl IndentKind {
    fn string(&self) -> String {
        match self {
            IndentKind::Spaces(n) => " ".repeat(*n as usize),
            IndentKind::Tabs => "\t".to_string(),
        }
    }
}
