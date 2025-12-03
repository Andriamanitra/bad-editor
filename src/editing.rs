use std::cmp::Ordering;
use std::ops::Range;

use ropey::Rope;

use crate::ropebuffer::RopeBuffer;
use crate::{ByteOffset, MultiCursor};

#[derive(Debug)]
pub struct EditBatch {
    edits: Vec<Edit>,
}

impl EditBatch {
    pub fn rev_iter(&self) -> impl Iterator<Item = &Edit> {
        self.edits.iter().rev()
    }

    pub fn iter(&self) -> impl Iterator<Item = &Edit> {
        self.edits.iter()
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    pub fn first_edit_offset(&self) -> Option<ByteOffset> {
        self.edits.first().map(|e| e.pos())
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

    pub fn insert_newline_keep_indent(cursors: &MultiCursor, content: &RopeBuffer, eol: &str) -> EditBatch {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            let indent = cursor.current_line_indentation(content);
            let ins = format!("{eol}{indent}");
            edits.push(Edit::insert_str(cursor.offset, &ins));
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

    pub fn transform_selections<F>(cursors: &MultiCursor, content: &RopeBuffer, transform: F) -> Self
        where F: Fn(String) -> Option<String>
    {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            if let Some(selection) = cursor.selection() {
                if let Some(replacement) = transform(content.slice(&selection).to_string()) {
                    edits.push(Edit::insert_str(selection.start, &replacement));
                }
                edits.push(Edit::Delete(selection));
            }
        }
        Self::from_edits(edits)
    }

    pub fn cut(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            if let Some(selection) = cursor.selection() {
                edits.push(Edit::Delete(selection));
            } else {
                let a = cursor.line_start(content);
                let b = cursor.line_end(content);
                edits.push(Edit::Delete(a..b));
            }
        }
        Self::from_edits(edits)
    }

    /// Typically deletes one grapheme cluster backward. If the current line up
    /// to the cursor only contains spaces then delete backwards to the nearest
    /// tab stop.
    pub fn delete_backward_with_cursors(cursors: &MultiCursor, content: &RopeBuffer, indent_width: usize) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            match cursor.selection() {
                Some(selection) => {
                    edits.push(Edit::Delete(selection));
                }
                None => {
                    let mut deleted_count = 1;
                    if cursor.is_at_start_of_line(content) {
                        let indent_str = cursor.current_line_indentation(content);
                        if !indent_str.is_empty() && indent_str.bytes().all(|b| b == b' ') {
                            deleted_count = match indent_str.len() % indent_width {
                                0 => indent_width,
                                n => n,
                            }
                        }
                    }
                    let a = cursor.left(content, deleted_count);
                    let b = cursor.offset;
                    if a != b {
                        edits.push(Edit::Delete(a..b));
                    }
                }
            }
        }
        Self::from_edits(edits)
    }

    pub fn delete_word_with_cursors(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        let mut edits = vec![];
        for cursor in cursors.iter() {
            match cursor.selection() {
                Some(selection) => {
                    edits.push(Edit::Delete(selection));
                }
                None => {
                    let a = cursor.word_boundary_left(content);
                    let b = cursor.offset;
                    // if there is only a single space between cursor and previous word boundary
                    // we also want to delete the previous word
                    if a.0 + 1 == b.0 && content.byte(a) == b' ' {
                        let cursor = crate::cursor::Cursor::new_with_offset(a);
                        let a = cursor.word_boundary_left(content);
                        edits.push(Edit::Delete(a..b));
                    } else {
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
                }
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

    pub fn indent_with_cursors(cursors: &MultiCursor, content: &RopeBuffer, indent: &str) -> Self {
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(content) {
                let bpos = content.line_to_byte(lineno);
                edits.push(Edit::insert_str(bpos, indent));
            }
        }

        Self::from_edits(edits)
    }

    pub fn dedent_with_cursors(cursors: &MultiCursor, content: &RopeBuffer, indent_width: usize, tab_width: usize) -> Self {
        let mut edits = vec![];

        for cursor in cursors.iter() {
            for lineno in cursor.line_span(content) {
                let start_of_line = content.line_to_byte(lineno);
                let mut end_of_dedent = start_of_line;
                let mut removed_width = 0;
                let mut bytes_iter = content.bytes_at(start_of_line);
                while removed_width < indent_width {
                    match bytes_iter.next() {
                        Some(b' ') => {
                            removed_width += 1;
                        }
                        Some(b'\t') => {
                            removed_width += tab_width;
                        }
                        _ => break
                    }
                    end_of_dedent = ByteOffset(end_of_dedent.0 + 1);
                }
                edits.push(Edit::Delete(start_of_line..end_of_dedent));
            }
        }

        Self::from_edits(edits)
    }

    pub fn move_lines_up(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        // FIXME: moving line without a trailing newline
        // eg. "A\nB" should become "B\nA\n" instead of "BA\n"
        let mut edits = vec![];

        for span in cursors.line_ranges(content).iter().rev() {
            // moving line span up is equivalent to moving the *previous line down*,
            // and if we do it that way it's easier to keep the cursors in right places
            if span.start > 0 {
                let prev_line = content.line_to_byte(span.start - 1) .. content.line_to_byte(span.start);
                let end = content.line_to_byte(span.end);
                edits.push(Edit::Insert(end, content.slice(&prev_line).into()));
                edits.push(Edit::Delete(prev_line));
            }
        }

        Self::from_edits(edits)
    }

    pub fn move_lines_down(cursors: &MultiCursor, content: &RopeBuffer) -> Self {
        // FIXME: moving line without a trailing newline
        // eg. "A\nB" should become "B\nA\n" instead of "BA\n"
        let mut edits = vec![];

        for span in cursors.line_ranges(content).iter().rev() {
            // moving line span down is equivalent to moving the *next line *up*,
            // and if we do it that way it's easier to keep the cursors in right places
            let next_line = if span.end < content.len_lines() {
                let next_line_start = content.line_to_byte(span.end);
                let next_line_end = content.line_to_byte(span.end + 1);
                if next_line_start < next_line_end {
                    edits.push(Edit::Delete(next_line_start..next_line_end));
                    content.slice(&(next_line_start..next_line_end)).into()
                } else {
                    Rope::from("\n")
                }
            } else {
                Rope::from("\n")
            };
            let start = content.line_to_byte(span.start);
            edits.push(Edit::Insert(start, next_line));
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
        let range = offset..ByteOffset(offset.0 + length);
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
    use rstest::*;

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
        let edits = EditBatch::insert_with_cursors(&cursors, "x");
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.to_string(), "xx");
    }

    #[test]
    fn delete_word() {
        let mut r = RopeBuffer::from_str("hello xxxxxworld");
        let mut cursors = MultiCursor::new();
        cursors.move_to(&r, crate::MoveTarget::Right(11));
        let edits = EditBatch::delete_word_with_cursors(&cursors, &r);
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.to_string(), "hello world")
    }

    #[test]
    fn delete_word_and_space() {
        let mut r = RopeBuffer::from_str("hello xxxxx world");
        let mut cursors = MultiCursor::new();
        cursors.move_to(&r, crate::MoveTarget::Right(12));
        let edits = EditBatch::delete_word_with_cursors(&cursors, &r);
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.to_string(), "hello world")
    }

    #[test]
    fn insert_newline_keep_indent_mid_indent() {
        let mut r = RopeBuffer::from_str("    abc");
        let mut cursors = MultiCursor::new();
        cursors.move_to(&r, crate::MoveTarget::Right(2));
        let edits = EditBatch::insert_newline_keep_indent(&cursors, &r, "\n");
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.to_string(), "  \n    abc")
    }

    #[rstest]
    #[case(4, 4, 0)]
    #[case(5, 4, 4)]
    #[case(7, 4, 4)]
    #[case(8, 4, 4)]
    #[case(2, 2, 0)]
    #[case(7, 2, 6)]
    #[case(7, 8, 0)]
    #[case(15, 8, 8)]
    fn test_delete_to_tabstop_spaces(
        #[case] n_spaces: usize,
        #[case] indent_width: usize,
        #[case] expected_length_after: usize,
    ) {
        assert_eq!(expected_length_after % indent_width, 0);
        let mut r = RopeBuffer::from_str(&" ".repeat(n_spaces));
        let mut cursors = MultiCursor::new();
        cursors.move_to(&r, crate::MoveTarget::End);
        let edits = EditBatch::delete_backward_with_cursors(&cursors, &r, indent_width);
        r.do_edits(&mut cursors, edits);
        assert_eq!(r.len_bytes(), expected_length_after);
    }

    #[rstest]
    #[case("", "")]
    #[case("\t", "")]
    #[case("\t\t", "\t")]
    #[case("\t\t ", "\t\t")]
    #[case("\t\t  ", "\t\t ")]
    fn test_delete_to_tabstop(#[case] before: &str, #[case] after: &str) {
        let mut r = RopeBuffer::from_str(before);
        let mut cursors = MultiCursor::new();
        cursors.move_to(&r, crate::MoveTarget::End);
        let edits = EditBatch::delete_backward_with_cursors(&cursors, &r, 4);
        r.do_edits(&mut cursors, edits);
        assert_eq!(&r.to_string(), after);
    }
}
