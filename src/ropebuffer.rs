use std::fmt::Display;
use std::ops::Range;

use ropey::Rope;
use ropey::RopeSlice;

use crate::cursor::Cursor;
use crate::RopeExt;
use crate::ByteOffset;
use crate::MultiCursor;
use crate::editing::{EditBatch, Edit};


#[derive(Debug, Default)]
pub struct RopeBuffer {
    rope: Rope,
    undo: Vec<(EditBatch, MultiCursor)>,
    redo: Vec<(EditBatch, MultiCursor)>,
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

    pub fn try_line_to_byte(&self, line: usize) -> Option<ByteOffset> {
        self.rope.try_line_to_byte(line).ok().map(ByteOffset)
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

    pub fn get_byte(&self, offset: ByteOffset) -> Option<u8> {
        self.rope.get_byte(offset.0)
    }

    pub fn byte(&self, offset: ByteOffset) -> u8 {
        self.rope.byte(offset.0)
    }

    pub fn bytes_at(&self, offset: ByteOffset) -> ropey::iter::Bytes<'_> {
        self.rope.bytes_at(offset.0)
    }

    pub fn is_word_boundary(&self, offset: ByteOffset) -> bool {
        // The unicode segmentation crates don't currently (as of August 2025) provide
        // an API for word boundaries that would be usable with Rope so we will use a
        // simple implementation that should be reasonable for the simplest of cases.
        // Unicode defines more thorough word boundary rules that might be worth
        // implementing: https://www.unicode.org/reports/tr29/#Word_Boundaries

        fn is_midletter(c: char) -> bool {
            matches!(c, '\u{003A}' | '\u{00B7}' | '\u{0387}' | '\u{055F}' | '\u{05F4}' | '\u{2027}' | '\u{FE13}' | '\u{FE55}' | '\u{FF1A}')
        }

        fn is_midnumletq(c: char) -> bool {
            matches!(c, '\u{002E}' | '\u{2018}' | '\u{2019}' | '\u{2024}' | '\u{FE52}' | '\u{FF07}' | '\u{FF0E}' | '\u{0027}')
        }

        fn is_midnum(c: char) -> bool {
            matches!(c, '\u{066C}' | '\u{FE50}' | '\u{FE54}' | '\u{FF0C}' |  '\u{FF1B}')
        }

        let char_offset = self.byte_to_char(offset);
        let mut prevs = self.rope.chars_at(char_offset);
        let mut nexts = self.rope.chars_at(char_offset);
        let Some(prev) = prevs.prev() else { return true };
        let Some(next) = nexts.next() else { return true };
        if prev.is_whitespace() && next.is_whitespace() {
            return false
        }
        if (prev.is_alphanumeric() || prev == '_') && (next.is_alphanumeric() || next == '_') {
            return false
        }
        if prev.is_ascii_punctuation() && next.is_ascii_punctuation() {
            return false
        }
        let prevprev = prevs.prev();
        let nextnext = nexts.next();
        if prev.is_alphabetic() && (is_midletter(next) || is_midnumletq(next)) && nextnext.is_some_and(|c| c.is_alphabetic()) {
            return false
        }
        if next.is_alphabetic() && (is_midletter(prev) || is_midnumletq(prev)) && prevprev.is_some_and(|c| c.is_alphabetic()) {
            return false
        }
        if prev.is_numeric() && (is_midnum(next) || is_midnumletq(next)) && nextnext.is_some_and(|c| c.is_numeric()) {
            return false
        }
        if next.is_numeric() && (is_midnum(prev) || is_midnumletq(prev)) && prevprev.is_some_and(|c| c.is_numeric()) {
            return false
        }
        true
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

    fn edit_rope(&mut self, edits: &EditBatch) {
        for edit in edits.rev_iter() {
            match edit {
                Edit::Insert(offset, s) => self.insert_rope(*offset, s.clone()),
                Edit::Delete(range) => self.remove(range),
            }
        }
    }

    fn inverse_of(&self, edits: &EditBatch) -> EditBatch {
        let mut inverted_edits = vec![];
        let mut n_deleted: usize = 0;
        let mut n_inserted: usize = 0;
        for edit in edits.iter() {
            inverted_edits.push(
                match edit {
                    Edit::Insert(offset, s) => {
                        let mut offset = *offset;
                        offset.0 += n_inserted;
                        offset.0 -= n_deleted;
                        n_inserted += s.len_bytes();
                        Edit::delete(offset, s.len_bytes())
                    }
                    Edit::Delete(range) => {
                        let mut range = range.clone();
                        let rope = self.slice(&range).into();
                        range.start.0 += n_inserted;
                        range.start.0 -= n_deleted;
                        range.end.0 += n_inserted;
                        range.end.0 -= n_deleted;
                        n_deleted += range.end.0 - range.start.0;
                        Edit::Insert(range.start, rope)
                    }
                }
            );
        }
        EditBatch::from_edits(inverted_edits)
    }

    pub fn do_edits(&mut self, cursors: &mut MultiCursor, edits: EditBatch) {
        let cursors_before_edits = cursors.clone();
        let inverted = self.inverse_of(&edits);
        self.undo.push((inverted, cursors_before_edits));
        for cursor in cursors.iter_mut() {
            let original_offset = cursor.offset;
            let original_sel = cursor.selection_from;
            for edit in edits.iter() {
                match edit {
                    Edit::Insert(offset, rope) => {
                        if offset <= &original_offset {
                            cursor.offset.0 += rope.len_bytes();
                        }
                        if let Some(sel) = original_sel {
                            if offset <= &sel {
                                for sel_offset in cursor.selection_from.iter_mut() {
                                    sel_offset.0 += rope.len_bytes();
                                }
                            }
                        }
                    }
                    Edit::Delete(range) => {
                        if range.start <= original_offset {
                            cursor.offset.0 -= range.end.0.min(original_offset.0) - range.start.0;
                        }
                        if let Some(sel) = original_sel {
                            if range.start <= sel {
                                for sel_offset in cursor.selection_from.iter_mut() {
                                    sel_offset.0 -= range.end.0.min(sel.0) - range.start.0;
                                }
                            }
                        }
                    }
                }
            }
        }
        self.edit_rope(&edits);
    }

    /// Restores the last state from the undo stack (if any).
    /// Returns the updated positions of cursors.
    #[must_use]
    pub fn undo(&mut self, cursors: MultiCursor) -> MultiCursor {
        if let Some((edits, old_cursors)) = self.undo.pop() {
            self.redo.push((self.inverse_of(&edits), cursors));
            self.edit_rope(&edits);
            old_cursors
        } else {
            cursors
        }
    }

    /// Restores the next state from the redo stack (if any).
    /// Returns the updated positions of cursors.
    #[must_use]
    pub fn redo(&mut self, cursors: MultiCursor) -> MultiCursor {
        if let Some((edits, old_cursors)) = self.redo.pop() {
            self.undo.push((self.inverse_of(&edits), cursors));
            self.edit_rope(&edits);
            old_cursors
        } else {
            cursors
        }
    }

    pub fn search_with_cursors_backward(&self, cursors: &mut MultiCursor, s: &str) {
        let mut prev_found: Option<ByteOffset> = None;
        let mut new_cursors = vec![];
        for cursor in cursors.rev_iter() {
            let start = match cursor.selection_from {
                Some(sel_from) => cursor.offset.min(sel_from),
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

    pub fn write_to<W: std::io::Write>(&self, mut writer: W) -> std::io::Result<usize> {
        let mut bytes_written = 0;
        for chunk in self.rope.chunks() {
            writer.write_all(chunk.as_bytes())?;
            bytes_written += chunk.len();
        }
        Ok(bytes_written)
    }
}

impl Display for RopeBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.rope.to_string())
    }
}

#[cfg(test)]
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
        cursors.move_to(&r, crate::MoveTarget::Right(2));
        let del = EditBatch::delete_forward_with_cursors(&cursors, &r);
        r.do_edits(&mut cursors, del);
        assert_eq!(r.to_string(), "ab");
        let del = EditBatch::delete_forward_with_cursors(&cursors, &r);
        r.do_edits(&mut cursors, del);
        assert_eq!(r.to_string(), "ab");
    }

    #[test]
    fn word_boundary_hello_world() {
        let r = RopeBuffer::from_str("hello world");
        assert!(r.is_word_boundary(ByteOffset(0)));
        assert!(!r.is_word_boundary(ByteOffset(1)));
        assert!(r.is_word_boundary(ByteOffset(5)));
        assert!(r.is_word_boundary(ByteOffset(6)));
        assert!(r.is_word_boundary(ByteOffset(11)));
    }

    #[test]
    fn word_boundary_decimal_number() {
        let r = RopeBuffer::from_str(" 1_002.34");
        assert!(r.is_word_boundary(ByteOffset(0)));
        assert!(r.is_word_boundary(ByteOffset(1)));
        assert!(!r.is_word_boundary(ByteOffset(2)));
        assert!(!r.is_word_boundary(ByteOffset(3)));
        assert!(!r.is_word_boundary(ByteOffset(4)));
        assert!(!r.is_word_boundary(ByteOffset(5)));
        assert!(!r.is_word_boundary(ByteOffset(6)));
        assert!(!r.is_word_boundary(ByteOffset(7)));
        assert!(!r.is_word_boundary(ByteOffset(8)));
        assert!(r.is_word_boundary(ByteOffset(9)));
    }
}
