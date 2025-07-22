use ropey::Rope;
use crate::rope_ext::RopeExt;

#[derive(Debug, Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
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

    pub fn deselect(&mut self) {
        self.selection_from = None;
    }

    // TODO: handle column offset using unicode_segmentation

    pub fn move_up(&mut self, content: &Rope, n: usize) {
        let current_line = self.current_line_number(content);
        if current_line < n {
            self.offset = ByteOffset(0);
        } else {
            let line_start = content.line_to_byte(current_line - n);
            self.offset = ByteOffset(line_start);
        }
    }

    pub fn move_down(&mut self, content: &Rope, n: usize) {
        let current_line = self.current_line_number(content);
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

    pub fn move_line_start(&mut self, content: &Rope) {
        let line_start = content.line_to_byte(self.current_line_number(content));
        self.offset = ByteOffset(line_start);
    }

    pub fn move_line_end(&mut self, content: &Rope) {
        let current_line = self.current_line_number(content);
        if current_line == content.len_lines() - 1 {
            self.offset = ByteOffset(content.len_bytes());
        } else {
            let next_line_start = content.line_to_byte(current_line + 1);
            self.offset = ByteOffset(next_line_start - 1);
        }
    }

    pub fn insert(&mut self, content: &mut Rope, s: &str) {
        let char_idx = content.byte_to_char(self.offset.0);
        content.insert(char_idx, &s);
        self.offset = ByteOffset(self.offset.0 + s.len());
    }

    pub fn delete_backward(&mut self, content: &mut Rope) {
        let char_range = match self.selection_from {
            Some(offset) => {
                let a = content.byte_to_char(self.offset.0);
                let b = content.byte_to_char(offset.0);
                if a < b {
                    a..b
                } else {
                    b..a
                }
            },
            None => {
                let b = content.byte_to_char(self.offset.0);
                self.move_left(&content, 1);
                let a = content.byte_to_char(self.offset.0);
                a..b
            }
        };
        content.remove(char_range);
    }

    pub fn delete_forward(&mut self, content: &mut Rope) {
        let char_range = match self.selection_from {
            Some(offset) => {
                let a = content.byte_to_char(self.offset.0);
                let b = content.byte_to_char(offset.0);
                if a < b {
                    a..b
                } else {
                    b..a
                }
            },
            None => {
                let old_offset = self.offset.0;
                self.move_right(&content, 1);
                let a = content.byte_to_char(old_offset);
                let b = content.byte_to_char(self.offset.0);
                self.offset = ByteOffset(old_offset);
                a..b
            }
        };
        content.remove(char_range);
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
            None => ByteOffset(self.offset.0 + self.current_grapheme_cluster_len_bytes(content)),
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
            cursor.move_right(&r, 1);
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
            cursor.move_left(&r, 1);
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
        cursor.move_line_end(&r);
        assert_eq!(cursor.offset, ByteOffset(3));
        cursor.move_line_start(&r);
        assert_eq!(cursor.offset, ByteOffset(0));
    }

    #[test]
    fn test_move_home_end_last_line() {
        let r = Rope::from_str("abc\ndef");
        let mut cursor = Cursor { offset: ByteOffset(5), selection_from: None };
        cursor.move_line_start(&r);
        assert_eq!(cursor.offset, ByteOffset(4));
        cursor.move_line_end(&r);
        assert_eq!(cursor.offset, ByteOffset(7));
    }

    #[test]
    fn test_move_up_down() {
        let r = Rope::from_str("abc\ndef\n\nghi");
        let mut cursor = Cursor { offset: ByteOffset(2), selection_from: None };

        // cursor should move to between e|f
        cursor.move_down(&r, 1);
        assert_eq!(r.byte_to_line(cursor.offset.0), 1);
        assert_eq!(cursor.offset, ByteOffset(6));

        // cursor should move to the empty line between f and g
        cursor.move_down(&r, 1);
        assert_eq!(r.byte_to_line(cursor.offset.0), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // cursor should move to between h|i
        // (remember horizontal position from before entering the empty line)
        cursor.move_down(&r, 1);
        assert_eq!(r.byte_to_line(cursor.offset.0), 3);
        assert_eq!(cursor.offset, ByteOffset(11));

        // back up to the empty line
        cursor.move_up(&r, 1);
        assert_eq!(r.byte_to_line(cursor.offset.0), 2);
        assert_eq!(cursor.offset, ByteOffset(8));

        // back up to between e|f
        // (remember horizontal position from before entering the empty line)
        cursor.move_up(&r, 1);
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
