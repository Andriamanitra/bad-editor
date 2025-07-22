pub mod bad;
mod prompt;
mod render;
mod cursor;
mod rope_ext;

pub use rope_ext::RopeExt;
pub use cursor::Cursor;

#[derive(Debug, Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}
