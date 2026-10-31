use unicode_segmentation::{GraphemeCursor, GraphemeIncomplete};

use crate::ByteOffset;

pub trait RopeExt<'a> {
    fn count_grapheme_clusters(&'a self) -> usize;
    fn next_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset>;
    fn previous_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset>;
    fn is_boundary(&'a self, offset: ByteOffset) -> bool;
    fn graphemes(&'a self) -> RopeGraphemes<'a>;
}

impl<'a> RopeExt<'a> for ropey::RopeSlice<'a> {
    fn count_grapheme_clusters(&'a self) -> usize {
        let mut gr = GraphemeCursor::new(0, self.len_bytes(), true);
        let mut count = 0;
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(_)) => count += 1,
                Ok(None) => return count,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn next_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset> {
        let mut gr = GraphemeCursor::new(start.0, self.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(start.0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return Some(ByteOffset(n)),
                Ok(None) => return None,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) =
                        self.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn previous_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset> {
        let mut gr = GraphemeCursor::new(start.0, self.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(start.0);
        loop {
            match gr.prev_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return Some(ByteOffset(n)),
                Ok(None) => return None,
                Err(GraphemeIncomplete::PrevChunk) => {
                    (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(chunk_byte_idx - 1);
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn is_boundary(&self, offset: ByteOffset) -> bool {
        if offset.0 > self.len_bytes() {
            return false
        }
        if offset.0 != self.char_to_byte(self.byte_to_char(offset.0)) {
            return false
        }
        let mut gr = GraphemeCursor::new(offset.0, self.len_bytes(), true);
        let (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(offset.0);
        for _ in 0..5 {
            match gr.is_boundary(chunk, chunk_byte_idx) {
                Ok(is_boundary) => return is_boundary,
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
        false
    }

    fn graphemes(&self) -> RopeGraphemes<'_> {
        let mut chunks = self.chunks();
        let first_chunk = chunks.next().unwrap_or("");
        RopeGraphemes {
            text: *self,
            chunks,
            cur_chunk: first_chunk,
            cur_chunk_start: 0,
            cursor: GraphemeCursor::new(0, self.len_bytes(), true)
        }
    }
}

impl RopeExt<'_> for ropey::Rope {
    fn count_grapheme_clusters(&self) -> usize {
        let mut gr = GraphemeCursor::new(0, self.len_bytes(), true);
        let mut count = 0;
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(_)) => count += 1,
                Ok(None) => return count,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn next_boundary_from(&self, start: ByteOffset) -> Option<ByteOffset> {
        let mut gr = GraphemeCursor::new(start.0, self.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(start.0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return Some(ByteOffset(n)),
                Ok(None) => return None,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) =
                        self.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn previous_boundary_from(&self, start: ByteOffset) -> Option<ByteOffset> {
        let mut gr = GraphemeCursor::new(start.0, self.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = self.chunk_at_byte(start.0);
        loop {
            match gr.prev_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return Some(ByteOffset(n)),
                Ok(None) => return None,
                Err(GraphemeIncomplete::PrevChunk) => {
                    (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(chunk_byte_idx - 1);
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
    }

    fn is_boundary(&self, offset: ByteOffset) -> bool {
        if offset.0 > self.len_bytes() {
            return false
        }
        if offset.0 != self.char_to_byte(self.byte_to_char(offset.0)) {
            return false
        }
        let mut gr = GraphemeCursor::new(offset.0, self.len_bytes(), true);
        let (chunk, chunk_byte_idx, _, _) = self.chunk_at_byte(offset.0);
        for _ in 0..5 {
            match gr.is_boundary(chunk, chunk_byte_idx) {
                Ok(is_boundary) => return is_boundary,
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        self.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
        }
        false
    }

    fn graphemes(&self) -> RopeGraphemes<'_> {
        let mut chunks = self.chunks();
        let first_chunk = chunks.next().unwrap_or("");
        RopeGraphemes {
            text: self.slice(0..),
            chunks,
            cur_chunk: first_chunk,
            cur_chunk_start: 0,
            cursor: GraphemeCursor::new(0, self.len_bytes(), true)
        }
    }
}

/// Graphemes iterator for Rope and RopeSlice
/// https://github.com/cessen/ropey/blob/master/examples/graphemes_iter.rs
pub struct RopeGraphemes<'a> {
    text: ropey::RopeSlice<'a>,
    chunks: ropey::iter::Chunks<'a>,
    cur_chunk: &'a str,
    cur_chunk_start: usize,
    cursor: GraphemeCursor,
}

impl<'a> Iterator for RopeGraphemes<'a> {
    type Item = ropey::RopeSlice<'a>;

    fn next(&mut self) -> Option<ropey::RopeSlice<'a>> {
        let a = self.cursor.cur_cursor();
        let b;
        loop {
            match self
            .cursor
            .next_boundary(self.cur_chunk, self.cur_chunk_start)
            {
                Ok(None) => {
                    return None;
                }
                Ok(Some(n)) => {
                    b = n;
                    break;
                }
                Err(GraphemeIncomplete::NextChunk) => {
                    self.cur_chunk_start += self.cur_chunk.len();
                    self.cur_chunk = self.chunks.next().unwrap_or("");
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (chunk, byte_idx, _, _) = self.text.chunk_at_byte(idx.saturating_sub(1));
                    self.cursor.provide_context(chunk, byte_idx);
                }
                _ => unreachable!(),
            }
        }

        if a < self.cur_chunk_start {
            let a_char = self.text.byte_to_char(a);
            let b_char = self.text.byte_to_char(b);

            Some(self.text.slice(a_char..b_char))
        } else {
            let a2 = a - self.cur_chunk_start;
            let b2 = b - self.cur_chunk_start;
            Some((&self.cur_chunk[a2..b2]).into())
        }
    }
}
