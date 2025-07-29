use unicode_segmentation::GraphemeCursor;
use unicode_segmentation::GraphemeIncomplete;

use crate::ByteOffset;

pub trait RopeExt<'a> {
    fn count_grapheme_clusters(&'a self) -> usize;
    fn next_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset>;
    fn previous_boundary_from(&'a self, start: ByteOffset) -> Option<ByteOffset>;
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
                    (chunk, chunk_byte_idx, _, _) =
                        self.chunk_at_byte(chunk_byte_idx - 1);
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
                    (chunk, chunk_byte_idx, _, _) =
                        self.chunk_at_byte(chunk_byte_idx - 1);
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
}
