use std::cmp::Ordering;
use std::ops::Range;

use ropey::Rope;

use crate::IndentKind;
use crate::MultiCursor;
use crate::ByteOffset;
use crate::ropebuffer::RopeBuffer;

#[derive(Debug)]
pub struct EditBatch {
    edits: Vec<Edit>
}

impl EditBatch {
    pub fn rev_iter(&self) -> impl Iterator<Item = &Edit> {
        self.edits.iter().rev()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Edit> {
        self.edits.iter()
    }

    pub fn from_edits(mut edits: Vec<Edit>) -> Self {
        edits.sort();
        let mut next_start_offset = ByteOffset::MAX;
        for edit in edits.iter_mut().rev() {
            match edit {
                Edit::Delete(range) => {
                    range.end = range.end.min(next_start_offset);
                    next_start_offset = range.start;
                }
                Edit::Insert(offset, _) => {
                    next_start_offset = *offset;
                }
            }
        }
        Self { edits }
    }

    pub fn insert_with_cursors(cursors: &MultiCursor, s: &str) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            edits.push(Edit::insert_str(cursor.offset, s));
            if let Some(selection) = cursor.selection() {
                edits.push(Edit::Delete(selection));
            }
        }
        Self::from_edits(edits)
    }

    pub fn insert_from_clipboard(cursors: &MultiCursor, clips: &[String]) -> Self {
        if clips.len() == cursors.cursor_count() {
            let mut edits = vec![];
            for (cursor, s) in cursors.iter().zip(clips) {
                edits.push(Edit::insert_str(cursor.offset, s));
                if let Some(selection) = cursor.selection() {
                    edits.push(Edit::Delete(selection));
                }
            }
            Self::from_edits(edits)
        } else {
            Self::insert_with_cursors(cursors, &clips.join(""))
        }
    }

    pub fn delete_backward_with_cursors(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            match cursor.selection() {
                Some(selection) => {
                    edits.push(Edit::Delete(selection));
                },
                None => {
                    let a = cursor.left(content, 1);
                    let b = cursor.offset;
                    if a != b {
                        edits.push(Edit::Delete(a..b));
                    }
                }
            }
        }
        Self::from_edits(edits)
    }

    pub fn delete_forward_with_cursors(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            match cursor.selection() {
                Some(selection) => {
                    edits.push(Edit::Delete(selection));
                },
                None => {
                    let a = cursor.offset;
                    let b = cursor.right(content, 1);
                    if a != b {
                        edits.push(Edit::Delete(a..b));
                    }
                }
            }
        }
        Self::from_edits(edits)
    }

    pub fn indent_with_cursors(cursors: &MultiCursor, content: &RopeBuffer, indent: IndentKind) -> Self {
        let indent = indent.string();
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(content) {
                let bpos = content.line_to_byte(lineno);
                edits.push(Edit::insert_str(bpos, &indent));
            }
        }

        Self::from_edits(edits)
    }

    pub fn dedent_with_cursors(cursors: &MultiCursor, content: &RopeBuffer, indent: IndentKind) -> Self {
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(content) {
                let bpos = content.line_to_byte(lineno);
                match indent {
                    IndentKind::Spaces(n) => {
                        let n = n as usize;
                        if bpos.0 + n < content.len_bytes()
                        && (0..n).all(|i| b' ' == content.byte(ByteOffset(bpos.0 + i))) {
                            let indent_range = bpos .. ByteOffset(bpos.0 + n);
                            edits.push(Edit::Delete(indent_range));
                        }
                    }
                    IndentKind::Tabs => {
                        if content.byte(bpos) == b'\t' {
                            let indent_range = bpos .. ByteOffset(bpos.0 + 1);
                            edits.push(Edit::Delete(indent_range));
                        }
                    }
                }
            }
        }

        Self::from_edits(edits)
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
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}

impl Ord for Edit {
    fn cmp(&self, rhs: &Self) -> Ordering {
        let pos_cmp = self.pos().cmp(&rhs.pos());
        if pos_cmp == Ordering::Equal {
            return match (self, rhs) {
                (Edit::Insert(_, _), Edit::Delete(_)) => Ordering::Less,
                (Edit::Delete(_), Edit::Insert(_, _)) => Ordering::Greater,
                (Edit::Delete(left_range), Edit::Delete(right_range)) => left_range.end.cmp(&right_range.end),
                _ => Ordering::Equal,
            }
        }
        pos_cmp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_overlapping_deletes() {
        let edits = vec![
            Edit::Delete(ByteOffset(15)..ByteOffset(20)),
            Edit::Delete(ByteOffset(5)..ByteOffset(10)),
            Edit::Delete(ByteOffset(25)..ByteOffset(30)),
        ];
        
        let batch = EditBatch::from_edits(edits);
        
        // Should be sorted but otherwise remain unchanged
        assert_eq!(batch.edits.len(), 3);
        assert_eq!(batch.edits[0], Edit::Delete(ByteOffset(5)..ByteOffset(10)));
        assert_eq!(batch.edits[1], Edit::Delete(ByteOffset(15)..ByteOffset(20)));
        assert_eq!(batch.edits[2], Edit::Delete(ByteOffset(25)..ByteOffset(30)));
    }

    #[test]
    fn overlapping_deletes() {
        let edits = vec![
            Edit::Delete(ByteOffset(10)..ByteOffset(20)),
            Edit::Delete(ByteOffset(5)..ByteOffset(15)),
        ];
        
        let batch = EditBatch::from_edits(edits);
        
        assert_eq!(batch.edits.len(), 2);
        assert_eq!(batch.edits[0], Edit::Delete(ByteOffset(5)..ByteOffset(10)));
        assert_eq!(batch.edits[1], Edit::Delete(ByteOffset(10)..ByteOffset(20)));
    }

    #[test]
    fn insert_and_delete_cmp() {
        assert!(
            Edit::insert_str(ByteOffset(10), "text") < Edit::Delete(ByteOffset(10)..ByteOffset(20)),
            "Insert should always be before Delete if same offset"
        )
    }

    #[test]
    fn mixed_inserts_and_deletes() {
        let edits = vec![
            Edit::Delete(ByteOffset(5)..ByteOffset(15)),
            Edit::insert_str(ByteOffset(12), "hello"),
            Edit::Delete(ByteOffset(20)..ByteOffset(30)),
            Edit::insert_str(ByteOffset(25), "world"),
        ];
        
        let batch = EditBatch::from_edits(edits);
        
        assert_eq!(batch.edits.len(), 4);
        
        assert_eq!(batch.edits[0], Edit::Delete(ByteOffset(5)..ByteOffset(12)));  // Truncated
        assert_eq!(batch.edits[1], Edit::insert_str(ByteOffset(12), "hello"));
        assert_eq!(batch.edits[2], Edit::Delete(ByteOffset(20)..ByteOffset(25))); // Truncated  
        assert_eq!(batch.edits[3], Edit::insert_str(ByteOffset(25), "world"));
    }

    #[test]
    fn sorting_and_processing() {
        // Test that edits are properly sorted before processing
        let edits = vec![
            Edit::Delete(ByteOffset(20)..ByteOffset(30)),  // This should be processed first (rightmost)
            Edit::Delete(ByteOffset(5)..ByteOffset(25)),   // This should be truncated
            Edit::insert_str(ByteOffset(15), "mid"),
        ];

        let batch = EditBatch::from_edits(edits);

        assert_eq!(batch.edits.len(), 3);
        assert_eq!(batch.edits[0], Edit::Delete(ByteOffset(5)..ByteOffset(15)));  // Truncated by insert
        assert_eq!(batch.edits[1], Edit::insert_str(ByteOffset(15), "mid"));
        assert_eq!(batch.edits[2], Edit::Delete(ByteOffset(20)..ByteOffset(30))); // Unchanged (rightmost)
    }

    #[test]
    fn insert_with_multicursor_same_offset() {
        let mut r = RopeBuffer::from_str("abab");
        let mut cursors = MultiCursor::new();
        cursors.select_to(&r, crate::MoveTarget::Right(2));
        cursors.spawn_new_primary(crate::cursor::Cursor::new_with_selection(ByteOffset(2), Some(ByteOffset(4))));
        assert_eq!(cursors.cursor_count(), 2);
        let edits = crate::editing::EditBatch::insert_with_cursors(&cursors, "x");
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.to_string(), "xx");
    }
}
