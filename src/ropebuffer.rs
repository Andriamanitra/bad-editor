use std::ops::Range;

use ropey::Rope;
use ropey::RopeSlice;

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
        self.pos().partial_cmp(&rhs.pos())
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

    fn slice(&self, range: &Range<ByteOffset>) -> RopeSlice<'_> {
        self.rope.byte_slice(range.start.0 .. range.end.0)
    }

    fn edit_rope(&mut self, edits: &[Edit]) {
        for edit in edits.iter().rev() {
            match edit {
                Edit::Insert(offset, s) => self.insert_rope(*offset, s.clone()),
                Edit::Delete(range) => self.remove(&range),
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
                    let a = cursor.left(&self, 1);
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
                    let b = cursor.offset;
                    let a = cursor.right(&self, 1);
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
            for lineno in cursor.line_span(&self) {
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
            for lineno in cursor.line_span(&self) {
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
