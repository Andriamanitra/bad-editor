use ropey::Rope;

use crate::RopeExt;
use crate::ByteOffset;

#[derive(Debug, Clone, Copy)]
pub enum MoveTarget {
    Up(usize),
    Down(usize),
    Left(usize),
    Right(usize),
    Start,
    End,
    StartOfLine,
    EndOfLine,
}

#[derive(Default)]
pub struct Cursor {
    pub(crate) offset: ByteOffset,
    pub(crate) selection_from: Option<ByteOffset>,
}

impl Cursor {
    pub fn current_line_number(&self, content: &Rope) -> usize {
        content.byte_to_line(self.offset.0)
    }

    pub fn column(&self, content: &Rope) -> usize {
        let a = content.line_to_byte(self.current_line_number(content));
        let b = self.offset.0;
        content.byte_slice(a..b).count_grapheme_clusters()
    }

    pub fn has_selection(&self) -> bool {
        self.selection_from.is_some()
    }

    pub fn deselect(&mut self) {
        self.selection_from = None;
    }

    pub fn target_byte_offset(&self, content: &Rope, target: MoveTarget) -> ByteOffset {
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

    pub fn move_to(&mut self, content: &Rope, target: MoveTarget) {
        self.deselect();
        self.move_to_byte(self.target_byte_offset(content, target))
    }

    pub fn select_to(&mut self, content: &Rope, target: MoveTarget) {
        self.select_to_byte(self.target_byte_offset(content, target))
    }


    fn move_to_byte(&mut self, new_offset: ByteOffset) {
        self.offset = new_offset;
    }

    fn select_to_byte(&mut self, new_offset: ByteOffset) {
        self.selection_from.get_or_insert(self.offset);
        self.move_to_byte(new_offset);
    }

    // TODO: handle column offset using unicode_segmentation

    pub fn up(&self, content: &Rope, n: usize) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line < n {
            ByteOffset(0)
        } else {
            let line_start = content.line_to_byte(current_line - n);
            ByteOffset(line_start)
        }
    }

    pub fn down(&self, content: &Rope, n: usize) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line + n > content.len_lines() {
            ByteOffset(content.len_bytes())
        } else {
            let line_start = content.line_to_byte(current_line + n);
            ByteOffset(line_start)
        }
    }

    pub fn left(&self, content: &Rope, n: usize) -> ByteOffset {
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

    pub fn right(&self, content: &Rope, n: usize) -> ByteOffset {
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

    pub fn line_start(&self, content: &Rope) -> ByteOffset {
        ByteOffset(content.line_to_byte(self.current_line_number(content)))
    }

    pub fn line_end(&self, content: &Rope) -> ByteOffset {
        let current_line = self.current_line_number(content);
        if current_line == content.len_lines() - 1 {
            ByteOffset(content.len_bytes())
        } else {
            let next_line_start = content.line_to_byte(current_line + 1);
            ByteOffset(next_line_start - 1)
        }
    }

    pub fn insert(&mut self, content: &mut Rope, s: &str) {
        if self.has_selection() {
            self.delete_selection(content);
        }
        let char_idx = content.byte_to_char(self.offset.0);
        content.insert(char_idx, &s);
        self.offset = ByteOffset(self.offset.0 + s.len());
    }

    fn delete_selection(&mut self, content: &mut Rope) {
        if let Some(offset) = self.selection_from {
            let a = content.byte_to_char(self.offset.0);
            let b = content.byte_to_char(offset.0);
            self.selection_from.take();
            if a < b {
                content.remove(a..b)
            } else {
                self.offset = offset;
                content.remove(b..a)
            }
        }
    }

    pub fn delete_backward(&mut self, content: &mut Rope) {
        if self.has_selection() {
            self.delete_selection(content);
        } else {
            let b = content.byte_to_char(self.offset.0);
            self.offset = self.left(&content, 1);
            let a = content.byte_to_char(self.offset.0);
            content.remove(a..b);
        }
    }

    pub fn delete_forward(&mut self, content: &mut Rope) {
        if self.has_selection() {
            self.delete_selection(content);
        } else {
            let a = content.byte_to_char(self.offset.0);
            let b = content.byte_to_char(self.right(content, 1).0);
            content.remove(a..b);
        }
    }

    pub fn previous_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        match content.previous_boundary_from(self.offset) {
            Some(boundary) => self.offset.0 - boundary.0,
            None => 0
        }
    }

    pub fn current_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        match content.next_boundary_from(self.offset) {
            Some(boundary) => boundary.0 - self.offset.0,
            None => 0
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
            None => ByteOffset(self.offset.0 + self.current_grapheme_cluster_len_bytes(content).max(1)),
            Some(selection_from) => self.offset.max(selection_from),
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use ropey::Rope;
    const SIMPLE_EMOJI: &'static str = "\u{1f60a}";
    const THUMBS_UP_WITH_MODIFIER: &'static str = "\u{1f44d}\u{1f3fb}";
    const FAMILY: &'static str = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f466}";

    #[test]
    fn test_move_right() {
        let s = format!("a{SIMPLE_EMOJI}ä{THUMBS_UP_WITH_MODIFIER}b{FAMILY}");
        let r = Rope::from_str(&s);
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
    fn test_move_left() {
        let s = format!("a{SIMPLE_EMOJI}ä{THUMBS_UP_WITH_MODIFIER}b{FAMILY}x");
        let r = Rope::from_str(&s);
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
            assert_eq!(cursor.offset.0, expected, "{}", r.len_bytes());
        }
    }

    #[test]
    fn test_insert_multibyte() {
        let mut r = Rope::from_str("abc");
        let mut cursor = Cursor { offset: ByteOffset(1), selection_from: None };
        cursor.insert(&mut r, FAMILY);
        let expected = format!("a{FAMILY}bc");
        assert_eq!(r.to_string(), expected);
        assert_eq!(cursor.offset, ByteOffset(1 + FAMILY.len()));
    }

    #[test]
    fn test_delete_backward_multibyte() {
        let mut r = Rope::from_str("a😊b");
        let mut cursor = Cursor { offset: ByteOffset(5), selection_from: None };
        cursor.delete_backward(&mut r);
        assert_eq!(&r.to_string(), "ab");
        assert_eq!(cursor.offset, ByteOffset(1));
    }

    #[test]
    fn test_delete_forward_multibyte() {
        let mut r = Rope::from_str("a😊b");
        let mut cursor = Cursor { offset: ByteOffset(1), selection_from: None };
        cursor.delete_forward(&mut r);
        assert_eq!(&r.to_string(), "ab");
        assert_eq!(cursor.offset, ByteOffset(1));
    }

    #[test]
    fn test_move_home_end() {
        let r = Rope::from_str("abc\ndef");
        let mut cursor = Cursor { offset: ByteOffset(1), selection_from: None };
        cursor.move_to(&r, MoveTarget::EndOfLine);
        assert_eq!(cursor.offset, ByteOffset(3));
        cursor.move_to(&r, MoveTarget::StartOfLine);
        assert_eq!(cursor.offset, ByteOffset(0));
    }

    #[test]
    fn test_move_home_end_last_line() {
        let r = Rope::from_str("abc\ndef");
        let mut cursor = Cursor { offset: ByteOffset(5), selection_from: None };
        cursor.move_to(&r, MoveTarget::StartOfLine);
        assert_eq!(cursor.offset, ByteOffset(4));
        cursor.move_to(&r, MoveTarget::EndOfLine);
        assert_eq!(cursor.offset, ByteOffset(7));
    }

    #[test]
    fn test_move_up_down() {
        let r = Rope::from_str("abc\ndef\n\nghi");
        let mut cursor = Cursor { offset: ByteOffset(2), selection_from: None };

        // cursor should move to between e|f
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset.0), 1);
        assert_eq!(cursor.offset, ByteOffset(6));

        // cursor should move to the empty line between f and g
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset.0), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // cursor should move to between h|i
        // (remember horizontal position from before entering the empty line)
        cursor.move_to(&r, MoveTarget::Down(1));
        assert_eq!(r.byte_to_line(cursor.offset.0), 3);
        assert_eq!(cursor.offset, ByteOffset(11));

        // back up to the empty line
        cursor.move_to(&r, MoveTarget::Up(1));
        assert_eq!(r.byte_to_line(cursor.offset.0), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // back up to between e|f
        // (remember horizontal position from before entering the empty line)
        cursor.move_to(&r, MoveTarget::Up(1));
        assert_eq!(r.byte_to_line(cursor.offset.0), 1);
        assert_eq!(cursor.offset, ByteOffset(6));
    }

    #[test]
    fn test_grapheme_cluster_len_bytes_on_umlaut() {
        // "bäst" — 'ä' is at byte offset 1 and is 2 bytes long
        let rope = Rope::from_str("bäst");

        let cursor = Cursor {
            offset: ByteOffset(1),
            selection_from: None,
        };

        let len = cursor.current_grapheme_cluster_len_bytes(&rope);
        assert_eq!(len, "ä".len()); // 2 bytes
    }
}
