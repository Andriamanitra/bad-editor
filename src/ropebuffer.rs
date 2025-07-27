use std::ops::Range;
use ropey::Rope;

use crate::RopeExt;
use crate::ByteOffset;

#[derive(Debug, Clone, Default)]
pub struct RopeBuffer {
    rope: Rope,
    // TODO: undo, redo
}

impl RopeBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_str(text: &str) -> Self {
        let rope = Rope::from_str(text);
        Self { rope, ..Default::default() }
    }

    pub fn len_bytes(&self) -> usize {
        self.rope.len_bytes()
    }

    pub fn len_lines(&self) -> usize {
        self.rope.len_lines()
    }

    pub fn line_to_byte(&self, line: usize) -> ByteOffset {
        ByteOffset(self.rope.line_to_byte(line))
    }

    pub fn byte_to_line(&self, offset: ByteOffset) -> usize {
        self.rope.byte_to_line(offset.0)
    }

    pub fn byte_to_column(&self, offset: ByteOffset) -> usize {
        let line_start = self.line_to_byte(self.byte_to_line(offset));
        self.rope.byte_slice(line_start.0 .. offset.0).count_grapheme_clusters()
    }

    pub fn byte(&self, offset: ByteOffset) -> u8 {
        self.rope.byte(offset.0)
    }

    pub fn replace_range<T: AsRef<str>>(&mut self, range: &Range<ByteOffset>, s: T) {
        let insert_offset = range.start;
        self.remove(range);
        self.insert(insert_offset, s);
    }

    pub fn insert<T: AsRef<str>>(&mut self, offset: ByteOffset, s: T) {
        let char_idx = self.rope.byte_to_char(offset.0);
        self.rope.insert(char_idx, s.as_ref());
    }

    pub fn remove(&mut self, range: &Range<ByteOffset>) {
        let a = self.rope.byte_to_char(range.start.0);
        let b = self.rope.byte_to_char(range.end.0);
        self.rope.remove(a..b);
    }

    pub fn next_boundary_from(&self, start: ByteOffset) -> Option<ByteOffset> {
        self.rope.next_boundary_from(start)
    }

    pub fn previous_boundary_from(&self, start: ByteOffset) -> Option<ByteOffset> {
        self.rope.previous_boundary_from(start)
    }

    pub fn lines(&self) -> ropey::iter::Lines<'_> {
        self.rope.lines()
    }
}

impl ToString for RopeBuffer {
    fn to_string(&self) -> String {
        self.rope.to_string()
    }
}
