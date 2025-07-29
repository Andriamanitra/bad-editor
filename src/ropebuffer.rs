use std::fmt::Display;
use std::ops::Range;

use ropey::Rope;
use ropey::RopeSlice;

use crate::cursor::Cursor;
use crate::RopeExt;
use crate::ByteOffset;
use crate::MultiCursor;
use crate::IndentKind;

#[derive(Debug)]
struct EditBatch {
    edits: Vec<Edit>,
    cursors: MultiCursor
}

impl EditBatch {
    fn new(edits: Vec<Edit>, cursors: MultiCursor) -> Self {
        Self { edits, cursors }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum Edit {
    Insert(ByteOffset, Rope),
    Delete(Range<ByteOffset>),
}

impl Edit {
    pub fn insert_str(offset: ByteOffset, s: &str) -> Self {
        Edit::Insert(offset, Rope::from(s))
    }

    pub fn delete(offset: ByteOffset, length: usize) -> Self {
        let range = offset .. ByteOffset(offset.0 + length);
        Edit::Delete(range)
    }

    pub fn pos(&self) -> ByteOffset {
        match self {
            Edit::Insert(offset, _) => *offset,
            Edit::Delete(range) => range.start,
        }
    }
}

impl PartialOrd for Edit {
    fn partial_cmp(&self, rhs: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(rhs))
    }
}

impl Ord for Edit {
    fn cmp(&self, rhs: &Self) -> std::cmp::Ordering {
        self.pos().cmp(&rhs.pos())
    }
}


#[derive(Debug, Default)]
pub struct RopeBuffer {
    rope: Rope,
    undo: Vec<EditBatch>,
    redo: Vec<EditBatch>,
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
        let line_up_to_offset = line_start .. offset;
        self.slice(&line_up_to_offset).count_grapheme_clusters()
    }

    fn byte_to_char(&self, offset: ByteOffset) -> usize {
        self.rope.byte_to_char(offset.0)
    }

    pub fn byte(&self, offset: ByteOffset) -> u8 {
        self.rope.byte(offset.0)
    }

    fn insert_rope(&mut self, offset: ByteOffset, rope: Rope) {
        let char_idx = self.byte_to_char(offset);
        let tail = self.rope.split_off(char_idx);
        self.rope.append(rope);
        self.rope.append(tail);
    }

    fn remove(&mut self, range: &Range<ByteOffset>) {
        let a = self.rope.byte_to_char(range.start.0);
        let b = self.rope.byte_to_char(range.end.0);
        self.rope.remove(a..b);
    }

    pub fn slice(&self, range: &Range<ByteOffset>) -> RopeSlice<'_> {
        self.rope.byte_slice(range.start.0 .. range.end.0)
    }

    fn edit_rope(&mut self, edits: &[Edit]) {
        for edit in edits.iter().rev() {
            match edit {
                Edit::Insert(offset, s) => self.insert_rope(*offset, s.clone()),
                Edit::Delete(range) => self.remove(range),
            }
        }
    }

    fn inverse_of(&self, edit: &Edit) -> Edit {
        match edit {
            Edit::Insert(offset, s) => Edit::delete(*offset, s.len_bytes()),
            Edit::Delete(range) => Edit::Insert(range.start, self.slice(range).into()),
        }
    }

    fn finalize_edits(&mut self, mut edits: Vec<Edit>, cursors: &mut MultiCursor, old_cursors: MultiCursor) {
        edits.sort_unstable();
        self.undo.push(EditBatch::new(
            edits.iter().map(|e| self.inverse_of(e)).collect(),
            old_cursors
        ));
        self.edit_rope(&edits);

        for edit in edits.iter().rev() {
            match edit {
                Edit::Insert(offset, s) => cursors.update_positions_insertion(*offset, s.len_bytes()),
                Edit::Delete(range) => cursors.update_positions_deletion(range),
            }
        }
    }

    pub fn insert_with_cursors(&mut self, cursors: &mut MultiCursor, s: &str) {
        self.redo.clear();
        let old_cursors = cursors.clone();
        let mut edits = vec![];
        for cursor in cursors.iter() {
            if let Some(selection) = cursor.selection() {
                edits.push(Edit::Delete(selection));
            }
            edits.push(Edit::insert_str(cursor.offset, s));
        }
        self.finalize_edits(edits, cursors, old_cursors);
    }

    pub fn delete_backward_with_cursors(&mut self, cursors: &mut MultiCursor) {
        self.redo.clear();
        let old_cursors = cursors.clone();
        let mut edits = vec![];
        for cursor in cursors.iter_mut() {
            match cursor.selection() {
                Some(selection) => {
                    cursor.offset = selection.start;
                    cursor.deselect();
                    edits.push(Edit::Delete(selection));
                },
                None => {
                    let b = cursor.offset;
                    let a = cursor.left(self, 1);
                    if a != b {
                        edits.push(Edit::Delete(a..b));
                    }
                }
            }
        }
        self.finalize_edits(edits, cursors, old_cursors);
    }

    pub fn delete_forward_with_cursors(&mut self, cursors: &mut MultiCursor) {
        self.redo.clear();
        let old_cursors = cursors.clone();
        let mut edits = vec![];
        for cursor in cursors.iter_mut() {
            match cursor.selection() {
                Some(selection) => {
                    cursor.offset = selection.start;
                    cursor.deselect();
                    edits.push(Edit::Delete(selection));
                }
                None => {
                    let a = cursor.offset;
                    let b = cursor.right(self, 1);
                    if a != b {
                        edits.push(Edit::Delete(a..b));
                    }
                }
            }
        }
        self.finalize_edits(edits, cursors, old_cursors);
    }

    pub fn indent_with_cursors(&mut self, cursors: &mut MultiCursor, indent: IndentKind) {
        self.redo.clear();
        let indent = indent.string();
        let old_cursors = cursors.clone();
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(self) {
                let bpos = self.line_to_byte(lineno);
                edits.push(Edit::insert_str(bpos, &indent));
            }
        }

        self.finalize_edits(edits, cursors, old_cursors);
    }

    pub fn dedent_with_cursors(&mut self, cursors: &mut MultiCursor, indent: IndentKind) {
        self.redo.clear();
        let old_cursors = cursors.clone();
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(self) {
                let bpos = self.line_to_byte(lineno);
                match indent {
                    IndentKind::Spaces(n) => {
                        let n = n as usize;
                        if bpos.0 + n < self.len_bytes()
                        && (0..n).all(|i| b' ' == self.byte(ByteOffset(bpos.0 + i))) {
                            let indent_range = bpos .. ByteOffset(bpos.0 + n);
                            edits.push(Edit::Delete(indent_range));
                        }
                    }
                    IndentKind::Tabs => {
                        if self.byte(bpos) == b'\t' {
                            let indent_range = bpos .. ByteOffset(bpos.0 + 1);
                            edits.push(Edit::Delete(indent_range));
                        }
                    }
                }
            }
        }

        self.finalize_edits(edits, cursors, old_cursors);
    }

    /// Restores the last state from the undo stack (if any).
    /// Returns the updated positions of cursors.
    #[must_use]
    pub fn undo(&mut self, cursors: MultiCursor) -> MultiCursor {
        if let Some(EditBatch { edits, cursors: memorized_cursors }) = self.undo.pop() {
            let inverted = edits.iter().map(|e| self.inverse_of(e)).collect();
            self.edit_rope(&edits);
            self.redo.push(EditBatch::new(inverted, cursors));
            memorized_cursors
        } else {
            cursors
        }
    }

    /// Restores the next state from the redo stack (if any).
    /// Returns the updated positions of cursors.
    #[must_use]
    pub fn redo(&mut self, cursors: MultiCursor) -> MultiCursor {
        if let Some(EditBatch { edits, cursors: memorized_cursors }) = self.redo.pop() {
            let inverted = edits.iter().map(|e| self.inverse_of(e)).collect();
            self.edit_rope(&edits);
            self.undo.push(EditBatch::new(inverted, cursors));
            memorized_cursors
        } else {
            cursors
        }
    }

    pub fn search_with_cursors_backward(&self, cursors: &mut MultiCursor, s: &str) {
        let mut prev_found: Option<ByteOffset> = None;
        let mut new_cursors = vec![];
        for cursor in cursors.rev_iter() {
            let start = match cursor.selection_from {
                Some(sel_from) => cursor.offset.max(sel_from),
                None => cursor.offset,
            };
            if prev_found.is_none_or(|p| start < p) {
                if let Some(offset) = self.find_prev(start, s) {
                    prev_found.replace(offset);
                    let match_end = ByteOffset(offset.0 + s.len());
                    new_cursors.push(Cursor::new_with_selection(offset, Some(match_end)))
                }
            }
            if prev_found.is_none() {
                return
            }
        }
        let mut new_primary = 0;
        for (i, cursor) in new_cursors.iter().enumerate() {
            if cursor.offset > cursors.primary().offset {
                new_primary = i;
                break
            }
        }
        cursors.set_cursors(new_primary, new_cursors);
    }

    pub fn search_with_cursors(&self, cursors: &mut MultiCursor, s: &str) {
        let mut prev_found: Option<ByteOffset> = None;
        let mut new_cursors = vec![];
        for cursor in cursors.iter() {
            let start = match cursor.selection_from {
                Some(sel_from) => cursor.offset.max(sel_from),
                None => cursor.offset,
            };
            if prev_found.is_none_or(|p| start > p) {
                if let Some(offset) = self.find_next(start, s) {
                    prev_found.replace(offset);
                    let match_end = ByteOffset(offset.0 + s.len());
                    new_cursors.push(Cursor::new_with_selection(offset, Some(match_end)))
                }
            }
            if prev_found.is_none() {
                return
            }
        }
        let mut new_primary = 0;
        for (i, cursor) in new_cursors.iter().enumerate() {
            if cursor.offset > cursors.primary().offset {
                new_primary = i;
                break
            }
        }
        cursors.set_cursors(new_primary, new_cursors);
    }

    pub fn find_prev(&self, start: ByteOffset, s: &str) -> Option<ByteOffset> {
        let c = s.bytes().next()?;
        let first_possible_start = ByteOffset(start.0.checked_sub(s.len() - 1)?);
        self.find_byte_positions_backwards_from(first_possible_start, c)
            .find(|pos| s.bytes().eq(self.rope.bytes_at(pos.0).take(s.len())))
    }

    pub fn find_next(&self, start: ByteOffset, s: &str) -> Option<ByteOffset> {
        let c = s.bytes().next()?;
        self.find_byte_positions_from(start, c)
            .find(|pos| s.bytes().eq(self.rope.bytes_at(pos.0).take(s.len())))
    }

    pub fn find_next_cycle(&self, start: ByteOffset, s: &str) -> Option<ByteOffset> {
        self.find_next(start, s).or_else(|| self.find_next(ByteOffset(0), s))
    }

    fn find_byte_positions_backwards_from(&self, from: ByteOffset, c: u8) -> impl Iterator<Item = ByteOffset> {
        // note that .reversed() is different than .rev():
        // it iterates backwards from the *CURRENT* position of the iterator
        self.rope.bytes_at(from.0)
            .reversed()
            .enumerate()
            .filter(move |(_, x)| *x == c)
            .map(move |(i, _)| ByteOffset(from.0 - i - 1))
    }

    fn find_byte_positions_from(&self, from: ByteOffset, c: u8) -> impl Iterator<Item = ByteOffset> {
        self.rope.bytes_at(from.0)
            .enumerate()
            .filter(move |(_, x)| *x == c)
            .map(move |(i, _)| ByteOffset(from.0 + i))
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

impl Display for RopeBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rope.to_string())
    }
}

mod tests {
    use super::*;

    #[test]
    fn find_bytes() {
        let s = "aaaba".to_string();
        let r = RopeBuffer::from_str(&s);
        let positions = r.find_byte_positions_from(ByteOffset(0), b'a')
            .collect::<Vec<_>>();
        assert_eq!(positions, vec![ByteOffset(0), ByteOffset(1), ByteOffset(2), ByteOffset(4)]);
    }

    #[test]
    fn find_bytes_backwards() {
        let s = "aaaba".to_string();
        let r = RopeBuffer::from_str(&s);
        let positions = r.find_byte_positions_backwards_from(ByteOffset(5), b'a')
            .collect::<Vec<_>>();
        assert_eq!(positions, vec![ByteOffset(4), ByteOffset(2), ByteOffset(1), ByteOffset(0)]);
    }

    #[test]
    fn search_backwards_from_inside_needle() {
        let r = RopeBuffer::from_str("abcabc");
        assert_eq!(r.find_prev(ByteOffset(1), "abc"), None);
        assert_eq!(r.find_prev(ByteOffset(3), "abc"), Some(ByteOffset(0)));
        assert_eq!(r.find_prev(ByteOffset(4), "abc"), Some(ByteOffset(0)));
    }

    #[test]
    fn search_forwards_from_inside_needle() {
        let r = RopeBuffer::from_str("abcabc");
        assert_eq!(r.find_next(ByteOffset(1), "abc"), Some(ByteOffset(3)));
        assert_eq!(r.find_next(ByteOffset(3), "abc"), Some(ByteOffset(3)));
        assert_eq!(r.find_next(ByteOffset(4), "abc"), None);
    }

    #[test]
    fn delete_at_eof() {
        let mut r = RopeBuffer::from_str("abc");
        let mut cursors = MultiCursor::new();
        let cursor = cursors.primary_mut();
        cursor.move_to(&r, crate::MoveTarget::Right(2));
        r.delete_forward_with_cursors(&mut cursors);
        assert_eq!(r.to_string(), "ab");
        r.delete_forward_with_cursors(&mut cursors);
        assert_eq!(r.to_string(), "ab");
    }
}
