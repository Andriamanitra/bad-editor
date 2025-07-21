use ropey::Rope;
use unicode_segmentation::GraphemeCursor;
use unicode_segmentation::GraphemeIncomplete;

#[derive(Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}


#[derive(Default)]
pub struct Cursor {
    pub(crate) offset: ByteOffset,
    pub(crate) visual_column: usize,
    pub(crate) selection_from: Option<ByteOffset>,
}

impl Cursor {
    pub fn column(&self, content: &Rope) -> usize {
        let current_line = content.byte_to_line(self.offset.0);
        let line_start = content.line_to_byte(current_line);
        // FIXME: column should be offset in grapheme clusters not bytes
        self.offset.0 - line_start
    }

    pub fn deselect(&mut self) {
        self.selection_from = None;
    }

    // TODO: handle column offset using unicode_segmentation

    pub fn move_up(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line < n {
            self.offset = ByteOffset(0);
        } else {
            let line_start = content.line_to_byte(current_line - n);
            self.offset = ByteOffset(line_start);
        }
    }

    pub fn move_down(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line + n > content.len_lines() {
            self.offset = ByteOffset(content.len_bytes());
        } else {
            let line_start = content.line_to_byte(current_line + n);
            self.offset = ByteOffset(line_start);
        }
    }

    pub fn move_left(&mut self, content: &Rope, n: usize) {
        for _ in 0..n {
            let b = self.previous_grapheme_cluster_len_bytes(content);
            self.offset = ByteOffset(self.offset.0.saturating_sub(b));
        }
    }

    pub fn move_right(&mut self, content: &Rope, n: usize) {
        for _ in 0..n {
            if self.offset < ByteOffset(content.len_bytes()) {
                let b = self.current_grapheme_cluster_len_bytes(content);
                self.offset = ByteOffset(self.offset.0 + b);
            }
        }
    }

    pub fn insert(&mut self, content: &mut Rope, s: &str) {
        let char_idx = content.byte_to_char(self.offset.0);
        content.insert(char_idx, &s);
        self.offset = ByteOffset(self.offset.0 + s.len());
    }

    pub fn previous_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        let mut gr = GraphemeCursor::new(self.offset.0, content.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = content.chunk_at_byte(self.offset.0);
        loop {
            match gr.prev_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return self.offset.0 - n,
                Ok(None) => return 0,
                Err(GraphemeIncomplete::PrevChunk) => {
                    (chunk, chunk_byte_idx, _, _) =
                        content.chunk_at_byte(chunk_byte_idx - 1);
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        content.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    pub fn current_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        let mut gr = GraphemeCursor::new(self.offset.0, content.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = content.chunk_at_byte(self.offset.0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return n - self.offset.0,
                Ok(None) => return 0,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) =
                        content.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        content.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    pub fn visual_start_offset(&self) -> ByteOffset {
        match self.selection_from {
            None => self.offset,
            Some(selection_from) => self.offset.min(selection_from),
        }
    }

    pub fn visual_end_offset(&self, content: &Rope) -> ByteOffset {
        match self.selection_from {
            None => ByteOffset(self.offset.0 + self.current_grapheme_cluster_len_bytes(content)),
            Some(selection_from) => self.offset.max(selection_from),
        }
    }
}
