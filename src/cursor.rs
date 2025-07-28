use std::ops::Range;

use crate::ByteOffset;
use crate::MoveTarget;
use crate::ropebuffer::RopeBuffer;

#[derive(Debug, Clone)]
pub struct MultiCursor {
    cursors: Vec<Cursor>,
    primary_index: usize,
}

impl MultiCursor {
    pub fn new() -> Self {
        Self {
            cursors: vec![Cursor::default()],
            primary_index: 0,
        }
    }

    /// Returns an immutable reference to the primary cursor
    pub fn primary<'a>(&'a self) -> &'a Cursor {
        self.cursors.get(self.primary_index)
            .expect("primary cursor should always exist")
    }

    /// Called when Esc is pressed, removes selections and extra cursors
    pub fn esc(&mut self) {
        for cursor in self.iter_mut() {
            cursor.deselect();
        }
        self.cursors[0] = self.cursors[self.primary_index];
        self.primary_index = 0;
        self.cursors.truncate(1);
    }

    pub fn move_to(&mut self, content: &RopeBuffer, target: MoveTarget) {
        for cursor in self.iter_mut() {
            cursor.move_to(content, target);
        }
    }

    pub fn select_to(&mut self, content: &RopeBuffer, target: MoveTarget) {
        for cursor in self.iter_mut() {
            cursor.select_to(&content, target);
        }
    }

    pub fn update_positions_insertion(&mut self, offset: ByteOffset, length: usize) {
        for cursor in self.iter_mut() {
            cursor.update_pos_insertion(offset, length);
        }
    }

    pub fn update_positions_deletion(&mut self, range: &Range<ByteOffset>) {
        for cursor in self.iter_mut() {
            cursor.update_pos_deletion(range);
        }
    }

    pub fn iter<'a>(&'a self) -> impl Iterator<Item = &'a Cursor> {
        self.cursors.iter()
    }

    pub fn iter_mut<'a>(&'a mut self) -> impl Iterator<Item = &'a mut Cursor> {
        self.cursors.iter_mut()
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Cursor {
    pub(crate) offset: ByteOffset,
    pub(crate) selection_from: Option<ByteOffset>,
}

impl Cursor {
    pub fn current_line_number(&self, content: &RopeBuffer) -> usize {
        content.byte_to_line(self.offset)
    }

    pub fn column(&self, content: &RopeBuffer) -> usize {
        content.byte_to_column(self.offset)
    }

    pub fn has_selection(&self) -> bool {
        self.selection_from.is_some()
    }

    pub fn selection(&self) -> Option<Range<ByteOffset>> {
        match self.selection_from {
            Some(sel_from) if sel_from > self.offset => Some(self.offset .. sel_from),
            Some(sel_from) => Some(sel_from .. self.offset),
            None => None
        }
    }

    pub fn deselect(&mut self) {
        self.selection_from.take();
    }

    pub fn target_byte_offset(&self, content: &RopeBuffer, target: MoveTarget) -> ByteOffset {
        match target {
            MoveTarget::Up(n) => self.up(content, n),
            MoveTarget::Down(n) => self.down(content, n),
            MoveTarget::Left(n) => self.left(content, n),
            MoveTarget::Right(n) => self.right(content, n),
            MoveTarget::Start => ByteOffset(0),
            MoveTarget::End => ByteOffset(content.len_bytes()),
            MoveTarget::StartOfLine => self.line_start(content),
            MoveTarget::EndOfLine => self.line_end(content),
        }
    }

    pub fn move_to(&mut self, content: &RopeBuffer, target: MoveTarget) {
        match self.selection() {
            Some(range) if matches!(target, MoveTarget::Left(1)) => {
                self.move_to_byte(range.start);
                self.deselect();
            }
            Some(range) if matches!(target, MoveTarget::Right(1)) => {
                self.move_to_byte(range.end);
                self.deselect();
            }
            Some(_) => {
                self.deselect();
                self.move_to_byte(self.target_byte_offset(content, target))
            }
            None => self.move_to_byte(self.target_byte_offset(content, target))
        }
    }

    pub fn select_to(&mut self, content: &RopeBuffer, target: MoveTarget) {
        self.select_to_byte(self.target_byte_offset(content, target))
    }


    fn move_to_byte(&mut self, new_offset: ByteOffset) {
        self.offset = new_offset;
        if self.selection_from == Some(self.offset) {
            self.deselect();
        }
    }

    fn select_to_byte(&mut self, new_offset: ByteOffset) {
        self.selection_from.get_or_insert(self.offset);
        self.move_to_byte(new_offset);
    }

    // TODO: handle column offset using unicode_segmentation

    pub fn up(&self, content: &RopeBuffer, n: usize) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line < n {
            ByteOffset(0)
        } else {
            content.line_to_byte(current_line - n)
        }
    }

    pub fn down(&self, content: &RopeBuffer, n: usize) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line + n > content.len_lines() {
            ByteOffset(content.len_bytes())
        } else {
            content.line_to_byte(current_line + n)
        }
    }

    pub fn left(&self, content: &RopeBuffer, n: usize) -> ByteOffset {
        let mut p = self.offset;
        for _ in 0..n {
            if let Some(prev) = content.previous_boundary_from(p) {
                p = prev;
            } else {
                break
            }
        }
        p
    }

    pub fn right(&self, content: &RopeBuffer, n: usize) -> ByteOffset {
        let mut p = self.offset;
        for _ in 0..n {
            if let Some(next) = content.next_boundary_from(p) {
                p = next;
            } else {
                break
            }
        }
        p
    }

    pub fn line_start(&self, content: &RopeBuffer) -> ByteOffset {
        content.line_to_byte(self.current_line_number(content))
    }

    pub fn line_end(&self, content: &RopeBuffer) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line == content.len_lines() - 1 {
            ByteOffset(content.len_bytes())
        } else {
            let next_line_start = content.line_to_byte(current_line + 1);
            ByteOffset(next_line_start.0 - 1)
        }
    }

    pub fn update_pos_deletion(&mut self, del: &std::ops::Range<ByteOffset>) {
        if self.offset > del.end {
            self.offset.0 -= del.end.0 - del.start.0;
        } else if self.offset > del.start {
            self.offset.0 = del.start.0;
        }
        if let Some(sel) = self.selection_from {
            if sel > del.end {
                let length = del.end.0 - del.start.0;
                self.selection_from.replace(ByteOffset(sel.0 - length));
            } else if sel > del.start {
                self.selection_from.replace(ByteOffset(del.start.0));
            }
        }
        if self.selection_from == Some(self.offset) {
            self.selection_from.take();
        }
    }

    pub fn update_pos_insertion(&mut self, pos: ByteOffset, length: usize) {
        if pos <= self.offset {
            self.offset.0 += length;
        }
        if let Some(mut sel) = self.selection_from {
            if pos <= sel {
                sel.0 += length;
            }
        }
    }

    pub fn line_span(&self, content: &RopeBuffer) -> Range<usize> {
        match self.selection_from {
            Some(sel) if sel < self.offset => {
                let lineno_start = content.byte_to_line(sel);
                let lineno_end = content.byte_to_line(self.offset);
                lineno_start..lineno_end+1
            }
            Some(sel) => {
                let lineno_start = content.byte_to_line(self.offset);
                let lineno_end = content.byte_to_line(sel);
                lineno_start..lineno_end+1
            }
            None => {
                let lineno = content.byte_to_line(self.offset);
                lineno..lineno+1
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rstest::*;

    const SIMPLE_EMOJI: &'static str = "\u{1f60a}";
    const THUMBS_UP_WITH_MODIFIER: &'static str = "\u{1f44d}\u{1f3fb}";
    const FAMILY: &'static str = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f466}";

    #[test]
    fn move_right() {
        let s = format!("a{SIMPLE_EMOJI}ä{THUMBS_UP_WITH_MODIFIER}b{FAMILY}");
        let r = RopeBuffer::from_str(&s);
        let mut cursor = Cursor::default();

        let expected_offsets = vec![
            1,              // a (1 byte)
            5,              // SIMPLE_EMOJI (4 bytes)
            7,              // ä (2 bytes)
            15,             // THUMBS_UP_WITH_MODIFIER (8 bytes: thumbs up sign + skin tone modifier)
            16,             // b (1 byte)
            34,             // FAMILY (18 bytes: man + zwj + woman + zwj + boy)
            34,
            34,
        ];

        for &expected in &expected_offsets {
            cursor.move_to(&r, MoveTarget::Right(1));
            assert_eq!(cursor.offset.0, expected);
        }
    }

    #[test]
    fn move_left() {
        let s = format!("a{SIMPLE_EMOJI}ä{THUMBS_UP_WITH_MODIFIER}b{FAMILY}x");
        let r = RopeBuffer::from_str(&s);
        let mut cursor = Cursor { offset: ByteOffset(r.len_bytes()), selection_from: None };

        let expected_offsets = vec![
            0,
            0,
            1,              // a (1 byte)
            5,              // SIMPLE_EMOJI (4 bytes)
            7,              // ä (2 bytes)
            15,             // THUMBS_UP_WITH_MODIFIER (8 bytes: thumbs up sign + skin tone modifier)
            16,             // b (1 byte)
            34,             // FAMILY (18 bytes: man + zwj + woman + zwj + boy)
        ];

        for &expected in expected_offsets.iter().rev() {
            cursor.move_to(&r, MoveTarget::Left(1));
            assert_eq!(cursor.offset.0, expected);
        }
    }

    #[test]
    fn move_home_end() {
        let r = RopeBuffer::from_str("abc\ndef");
        let mut cursor = Cursor { offset: ByteOffset(1), selection_from: None };
        cursor.move_to(&r, MoveTarget::EndOfLine);
        assert_eq!(cursor.offset, ByteOffset(3));
        cursor.move_to(&r, MoveTarget::StartOfLine);
        assert_eq!(cursor.offset, ByteOffset(0));
    }

    #[test]
    fn move_home_end_last_line() {
        let r = RopeBuffer::from_str("abc\ndef");
        let mut cursor = Cursor { offset: ByteOffset(5), selection_from: None };
        cursor.move_to(&r, MoveTarget::StartOfLine);
        assert_eq!(cursor.offset, ByteOffset(4));
        cursor.move_to(&r, MoveTarget::EndOfLine);
        assert_eq!(cursor.offset, ByteOffset(7));
    }

    #[test]
    fn move_up_down() {
        let r = RopeBuffer::from_str("abc\ndef\n\nghi");
        let mut cursor = Cursor { offset: ByteOffset(2), selection_from: None };

        // cursor should move to between e|f
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset), 1);
        assert_eq!(cursor.offset, ByteOffset(6));

        // cursor should move to the empty line between f and g
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // cursor should move to between h|i
        // (remember horizontal position from before entering the empty line)
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset), 3);
        assert_eq!(cursor.offset, ByteOffset(11));

        // back up to the empty line
        cursor.move_to(&r, MoveTarget::Up(1));
        assert_eq!(r.byte_to_line(cursor.offset), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // back up to between e|f
        // (remember horizontal position from before entering the empty line)
        cursor.move_to(&r, MoveTarget::Up(1));
        assert_eq!(r.byte_to_line(cursor.offset), 1);
        assert_eq!(cursor.offset, ByteOffset(6));
    }

    #[rstest]
    #[case(Cursor { offset: ByteOffset(1), selection_from: Some(ByteOffset(5)) }, ByteOffset(1))]
    #[case(Cursor { offset: ByteOffset(4), selection_from: Some(ByteOffset(1)) }, ByteOffset(1))]
    #[case(Cursor { offset: ByteOffset(6), selection_from: Some(ByteOffset(7)) }, ByteOffset(6))]
    fn move_1_left_with_selection(
        #[case] mut cursor: Cursor,
        #[case] offset_after_move: ByteOffset,
    ) {
        let r = RopeBuffer::from_str("abcde\nfghij");
        cursor.move_to(&r, MoveTarget::Left(1));
        assert_eq!(cursor.offset, offset_after_move);
        assert!(!cursor.has_selection());
    }

    #[rstest]
    #[case(Cursor { offset: ByteOffset(1), selection_from: Some(ByteOffset(5)) }, ByteOffset(5))]
    #[case(Cursor { offset: ByteOffset(4), selection_from: Some(ByteOffset(1)) }, ByteOffset(4))]
    #[case(Cursor { offset: ByteOffset(5), selection_from: Some(ByteOffset(6)) }, ByteOffset(6))]
    fn move_1_right_with_selection(
        #[case] mut cursor: Cursor,
        #[case] offset_after_move: ByteOffset,
    ) {
        let r = RopeBuffer::from_str("abcde\nfghij");
        cursor.move_to(&r, MoveTarget::Right(1));
        assert_eq!(cursor.offset, offset_after_move);
        assert!(!cursor.has_selection());
    }

    #[rstest]
    #[case(MoveTarget::Left(100), ByteOffset(0))]
    #[case(MoveTarget::Up(100), ByteOffset(0))]
    #[case(MoveTarget::Right(100), ByteOffset(10))]
    #[case(MoveTarget::Down(100), ByteOffset(10))]
    fn move_out_of_bounds(
        #[case] target: MoveTarget,
        #[case] offset_after_move: ByteOffset,
    ) {
        let r = RopeBuffer::from_str("0\n234\n67\n9");
        let mut cursor = Cursor { offset: ByteOffset(5), selection_from: None };
        cursor.move_to(&r, target);
        assert_eq!(cursor.offset, offset_after_move);
    }

    #[rstest]
    #[case(Cursor { offset: ByteOffset(0), selection_from: None }, 0..1)]
    #[case(Cursor { offset: ByteOffset(1), selection_from: None }, 0..1)]
    #[case(Cursor { offset: ByteOffset(2), selection_from: None }, 1..2)]
    #[case(Cursor { offset: ByteOffset(0), selection_from: Some(ByteOffset(10)) }, 0..4)]
    #[case(Cursor { offset: ByteOffset(10), selection_from: Some(ByteOffset(0)) }, 0..4)]
    #[case(Cursor { offset: ByteOffset(5), selection_from: Some(ByteOffset(6)) }, 1..3)]
    #[case(Cursor { offset: ByteOffset(6), selection_from: Some(ByteOffset(5)) }, 1..3)]
    fn cursor_line_span(
        #[case] cursor: Cursor,
        #[case] expected_line_span: Range<usize>
    ) {
        let r = RopeBuffer::from_str("0\n234\n67\n9");
        assert_eq!(cursor.line_span(&r), expected_line_span);
    }

    #[rstest]
    #[case("ab", ByteOffset(2))]
    #[case("abc\nxyz\n", ByteOffset(3))]
    #[case("abcd\r\nxyz\n", ByteOffset(4))]
    fn line_end(
        #[case] s: &'static str,
        #[case] expected: ByteOffset
    ) {
        let r = RopeBuffer::from_str(s);
        let cursor = Cursor::default();
        assert_eq!(cursor.line_end(&r), expected, "expected {expected:?} for {s:?}");
    }
}
