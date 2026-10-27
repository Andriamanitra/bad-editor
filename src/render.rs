use std::time::Instant;

use crossterm::QueueableCommand;
use crossterm::cursor::{MoveTo, MoveToNextLine};
use crossterm::style::{Color, ContentStyle, Print, PrintStyledContent, StyledContent, Stylize};
use crossterm::terminal::{
    BeginSynchronizedUpdate,
    Clear,
    ClearType,
    EndSynchronizedUpdate,
    WindowSize,
};
use ropey::RopeSlice;
use syntect::highlighting::{FontStyle as SyntectFontStyle, Style as SyntectStyle};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::completer::SuggestionMenu;
use crate::highlighter::BadHighlighter;
use crate::ropebuffer::RopeBuffer;
use crate::{App, ByteOffset};

fn to_crossterm_style(syntect_style: SyntectStyle) -> ContentStyle {
    let fg = {
        let syntect::highlighting::Color { r, g, b, .. } = syntect_style.foreground;
        Color::Rgb { r, g, b }
    };
    let bg = {
        let syntect::highlighting::Color { r, g, b, .. } = syntect_style.background;
        Color::Rgb { r, g, b }
    };
    let mut style = ContentStyle::new().with(fg).on(bg);
    if syntect_style.font_style.contains(SyntectFontStyle::UNDERLINE) {
        style = style.underlined();
    }
    if syntect_style.font_style.contains(SyntectFontStyle::ITALIC) {
        style = style.italic();
    }
    if syntect_style.font_style.contains(SyntectFontStyle::BOLD) {
        style = style.bold();
    }
    style
}

fn unicode_line_break_symbol(grapheme_cluster: &str) -> Option<&'static str> {
    // https://en.wikipedia.org/wiki/Newline#Unicode
    // https://docs.rs/ropey/1.6.1/ropey/index.html#a-note-about-line-breaks
    match grapheme_cluster {
        // LINE FEED (U+000A)
        "\n" => Some("⏎"),
        // CARRIAGE RETURN (U+000D)
        "\r" => Some("␍"),
        // CRLF (U+000A U+000D)
        "\r\n" => Some("␍␊"),
        // VERTICAL TAB (U+000B)
        "\u{000B}" => Some("␋"),
        // FORM FEED (U+000C)
        "\u{000C}" => Some("␌"),
        // NEXT LINE (U+0085)
        "\u{0085}" => Some("␤"),
        // unfortunately there are no glyphs to represent the last two
        // LINE SEPARATOR (U+2028)
        "\u{2028}" => Some("<U+2028>"),
        // PARAGRAPH SEPARATOR (U+2029)
        "\u{2029}" => Some("<U+2029>"),
        _ => None,
    }
}

fn replacement_symbol(g: &str) -> Option<String> {
    if UnicodeWidthStr::width(g) == 0 {
        return Some(g.chars().map(|c| format!("<U+{:X}>", c as u32)).collect());
    }
    if g.len() != 1 {
        return None
    }
    g.chars().next().and_then(|c|
        if c.is_control() {
            Some(format!("<{:02}>", c as u32))
        } else {
            None
        }
    )
}

struct RenderingContext {
    n_selections: usize,
    is_cursor: bool,
    current_column: usize,
    visible_from_column: usize,
    available_columns: usize,
    tab_width: usize,
    token_style: ContentStyle,
    queue: Vec<(usize, usize, StyledContent<String>)>,
}
impl RenderingContext {
    fn is_selection(&self) -> bool {
        self.n_selections > 0
    }

    fn push(&mut self, g: StyledContent<String>) {
        let width = UnicodeWidthStr::width(g.content().as_str());
        if self.current_column >= self.visible_from_column {
            self.queue.push((self.current_column, width, g));
        }
        self.current_column += width;
    }
}

fn grapheme_display_width(g: &str, current_column: usize, tab_width: usize) -> usize {
    if g.len() == 1 {
        let b = g.as_bytes()[0];
        if (b' '..0x7f).contains(&b) {
            return 1
        }
    }
    if g == "\t" {
        if tab_width > 0 {
            tab_width - (current_column % tab_width)
        } else {
            0
        }
    } else if let Some(glyph) = unicode_line_break_symbol(g) {
        UnicodeWidthStr::width(glyph)
    } else if let Some(disp) = replacement_symbol(g) {
        UnicodeWidthStr::width(disp.as_str())
    } else {
        UnicodeWidthStr::width(g)
    }
}

fn is_printable_ascii(b: u8) -> bool {
    (b' '..=b'~').contains(&b)
}

struct TextUnit<'a> {
    end: usize,
    text: &'a str,
    ascii: bool,
}

fn next_text_unit(line: &str, offset: usize) -> Option<TextUnit<'_>> {
    if offset >= line.len() {
        return None
    }
    let bytes = line.as_bytes();
    if is_printable_ascii(bytes[offset]) {
        let mut end = offset + 1;
        while end < bytes.len() && is_printable_ascii(bytes[end]) {
            end += 1;
        }
        if end < bytes.len() && !bytes[end].is_ascii() {
            // The last ASCII character may share a grapheme cluster with the
            // following non-ASCII character, e.g. a combining mark.
            if end - offset > 1 {
                return Some(TextUnit { end: end - 1, text: &line[offset..end - 1], ascii: true })
            }
        } else {
            return Some(TextUnit { end, text: &line[offset..end], ascii: true })
        }
    }
    let text = line[offset..].graphemes(true).next().expect("offset is in bounds");
    Some(TextUnit { end: offset + text.len(), text, ascii: text.bytes().all(is_printable_ascii) })
}

fn text_unit_display_width(unit: &TextUnit, current_column: usize, tab_width: usize) -> usize {
    if unit.ascii {
        unit.text.len()
    } else {
        grapheme_display_width(unit.text, current_column, tab_width)
    }
}

fn next_grapheme_width(
    slice: &RopeSlice<'_>,
    offset: usize,
    current_column: usize,
    tab_width: usize,
) -> Option<usize> {
    if offset >= slice.len_bytes() {
        return None
    }
    let remaining = slice.len_bytes() - offset;
    let mut buffer = String::new();
    for chunk in slice.byte_slice(offset..).chunks() {
        buffer.push_str(chunk);
        let mut graphemes = buffer.graphemes(true);
        let first = graphemes.next()?;
        if graphemes.next().is_some() || buffer.len() >= remaining {
            return Some(grapheme_display_width(first, current_column, tab_width))
        }
    }
    buffer.graphemes(true).next().map(|g| grapheme_display_width(g, current_column, tab_width))
}

fn line_content_len(line_slice: &RopeSlice<'_>) -> usize {
    let line_len = line_slice.len_bytes();
    if line_len == 0 {
        return 0;
    }
    match line_slice.get_byte(line_len - 1) {
        Some(b'\n') if line_len > 1 && line_slice.get_byte(line_len - 2) == Some(b'\r') => line_len - 2,
        Some(b'\n' | b'\r') => line_len - 1,
        _ => line_len,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct TextCheckpoint {
    byte: usize,
    column: usize,
    graphemes: usize,
}

pub(crate) struct LongLineLayout {
    revision: u64,
    line: usize,
    tab_width: usize,
    content_len: usize,
    checkpoints: Vec<TextCheckpoint>,
}

impl LongLineLayout {
    fn new(slice: &RopeSlice<'_>, tab_width: usize, revision: u64, line: usize) -> Self {
        Self {
            revision,
            line,
            tab_width,
            content_len: slice.len_bytes(),
            checkpoints: vec![TextCheckpoint { byte: 0, column: 0, graphemes: 0 }],
        }
    }

    fn matches(&self, revision: u64, line: usize, tab_width: usize, content_len: usize) -> bool {
        self.revision == revision
            && self.line == line
            && self.tab_width == tab_width
            && self.content_len == content_len
    }

    pub(crate) fn try_rebase_after_edit(
        &mut self,
        content: &RopeBuffer,
        first_edit_offset: ByteOffset,
        old_line_count: usize,
    ) -> bool {
        if content.len_lines() != old_line_count {
            return false
        }
        let Some(line_start) = content.try_line_to_byte(self.line) else {
            return false
        };
        if first_edit_offset.0 < line_start.0 {
            return false
        }
        let edit_offset = first_edit_offset.0 - line_start.0;
        let Some(last) = self.checkpoints.last() else {
            return false
        };
        if edit_offset < last.byte {
            return false
        }
        let Some(line) = content.lines_at(self.line).next() else {
            return false
        };
        self.revision = content.revision();
        self.content_len = line_content_len(&line);
        true
    }

    fn ensure_through(&mut self, slice: &RopeSlice<'_>, target_byte: usize) {
        let last = *self.checkpoints.last().expect("layout always has an initial checkpoint");
        if target_byte <= last.byte {
            return
        }
        let mut local_checkpoints = vec![];
        text_width(
            &slice.byte_slice(last.byte..target_byte),
            self.tab_width,
            last.column,
            Some(&mut local_checkpoints),
        );
        for checkpoint in local_checkpoints.into_iter().skip(1) {
            self.checkpoints.push(TextCheckpoint {
                byte: last.byte + checkpoint.byte,
                column: checkpoint.column,
                graphemes: last.graphemes + checkpoint.graphemes,
            });
        }
    }

    fn checkpoint_for_byte(&self, byte: usize) -> TextCheckpoint {
        let index = self.checkpoints.partition_point(|checkpoint| checkpoint.byte <= byte).saturating_sub(1);
        self.checkpoints[index]
    }

    fn checkpoint_for_column(&self, column: usize) -> TextCheckpoint {
        let index = self.checkpoints.partition_point(|checkpoint| checkpoint.column <= column).saturating_sub(1);
        self.checkpoints[index]
    }
}

const CHECKPOINT_INTERVAL: usize = 16 * 1024;

fn ascii_tab_advance(b: u8, column: usize, tab_width: usize) -> Option<usize> {
    if b == b'\t' {
        if tab_width > 0 {
            Some(column + tab_width - (column % tab_width))
        } else {
            Some(column)
        }
    } else if is_printable_ascii(b) {
        Some(column + 1)
    } else {
        None
    }
}

fn ascii_tabs_advance_str(s: &str, mut column: usize, tab_width: usize) -> Option<usize> {
    const ONES: u64 = u64::MAX / 255;
    const HIGHS: u64 = ONES * 0x80;
    const LESS_THAN_SPACE: u64 = ONES * 0x20;
    const ABOVE_TILDE: u64 = ONES;
    const NINES: u64 = ONES * 0x09;
    let bytes = s.as_bytes();
    let mut i = 0;
    while i + 8 <= bytes.len() {
        let word = u64::from_ne_bytes(bytes[i..i + 8].try_into().expect("chunk is 8 bytes"));
        let tab_bits = word ^ NINES;
        let has_tab = (tab_bits.wrapping_sub(ONES) & !tab_bits & HIGHS) != 0;
        let printable = (word.wrapping_sub(LESS_THAN_SPACE) & !word & HIGHS) == 0
            && ((word.wrapping_add(ABOVE_TILDE) | word) & HIGHS) == 0;
        if !has_tab && printable {
            column += 8;
            i += 8;
            continue
        }
        for _ in 0..8 {
            column = ascii_tab_advance(bytes[i], column, tab_width)?;
            i += 1;
        }
    }
    while i < bytes.len() {
        column = ascii_tab_advance(bytes[i], column, tab_width)?;
        i += 1;
    }
    Some(column)
}

fn chunk_may_extend_next(chunk: &str, next: Option<&str>) -> bool {
    match next.and_then(|next| next.chars().next()) {
        Some(next) if !next.is_ascii() => true,
        Some('\n') => chunk.ends_with('\r'),
        _ => false,
    }
}

fn can_extend_carry(carry: &str, next: &str) -> bool {
    match (carry.chars().last(), next.chars().next()) {
        (Some('\r'), Some('\n')) => true,
        (_, Some(next)) if !next.is_ascii() => true,
        _ => false,
    }
}

fn text_width(
    slice: &RopeSlice<'_>,
    tab_width: usize,
    start_column: usize,
    mut checkpoints: Option<&mut Vec<TextCheckpoint>>,
) -> (usize, usize) {
    let mut column = start_column;
    let mut graphemes = 0;
    let mut processed = 0;
    let mut last_checkpoint = 0;
    let mut carry = String::new();
    if let Some(checkpoints) = checkpoints.as_deref_mut() {
        checkpoints.push(TextCheckpoint { byte: 0, column: start_column, graphemes: 0 });
    }
    let mut chunks = slice.chunks().peekable();
    while let Some(chunk) = chunks.next() {
        if !carry.is_empty() && !can_extend_carry(&carry, chunk) {
            column += grapheme_display_width(&carry, column, tab_width);
            graphemes += 1;
            processed += carry.len();
            carry.clear();
        }
        if carry.is_empty() && chunk.is_ascii() && !chunk_may_extend_next(chunk, chunks.peek().copied()) {
            if let Some(next_column) = ascii_tabs_advance_str(chunk, column, tab_width) {
                column = next_column;
                graphemes += chunk.len();
                processed += chunk.len();
                if let Some(checkpoints) = checkpoints.as_deref_mut() {
                    if processed - last_checkpoint >= CHECKPOINT_INTERVAL {
                        checkpoints.push(TextCheckpoint { byte: processed, column, graphemes });
                        last_checkpoint = processed;
                    }
                }
                continue
            }
        }
        let mut buffer = std::mem::take(&mut carry);
        buffer.push_str(chunk);
        let mut iter = buffer.graphemes(true).peekable();
        while let Some(grapheme) = iter.next() {
            if iter.peek().is_none() {
                carry = grapheme.to_string();
            } else {
                column += grapheme_display_width(grapheme, column, tab_width);
                graphemes += 1;
                processed += grapheme.len();
            }
        }
        if let Some(checkpoints) = checkpoints.as_deref_mut() {
            if processed - last_checkpoint >= CHECKPOINT_INTERVAL {
                checkpoints.push(TextCheckpoint { byte: processed, column, graphemes });
                last_checkpoint = processed;
            }
        }
    }
    if !carry.is_empty() {
        column += grapheme_display_width(&carry, column, tab_width);
        graphemes += 1;
    }
    (column, graphemes)
}

fn advance_to_column(
    slice: &RopeSlice<'_>,
    start_byte: usize,
    start_column: usize,
    target_column: usize,
    tab_width: usize,
    inclusive: bool,
) -> (usize, usize) {
    let prefix = slice.byte_slice(start_byte..);
    let mut byte = start_byte;
    let mut column = start_column;
    let mut carry = String::new();
    let mut chunks = prefix.chunks().peekable();
    while let Some(chunk) = chunks.next() {
        if !carry.is_empty() && !can_extend_carry(&carry, chunk) {
            let width = grapheme_display_width(&carry, column, tab_width);
            if column + width >= target_column {
                if inclusive {
                    return (byte + carry.len(), column + width)
                }
                return (byte, column)
            }
            column += width;
            byte += carry.len();
            carry.clear();
        }
        if carry.is_empty() && chunk.is_ascii() && !chunk_may_extend_next(chunk, chunks.peek().copied()) {
            if let Some(next_column) = ascii_tabs_advance_str(chunk, column, tab_width) {
                if next_column < target_column {
                    column = next_column;
                    byte += chunk.len();
                    continue
                }
                if inclusive && next_column == target_column {
                    return (byte + chunk.len(), next_column)
                }
                for &b in chunk.as_bytes() {
                    if column >= target_column {
                        return (byte, column)
                    }
                    let next_column = ascii_tab_advance(b, column, tab_width).expect("slice was validated");
                    if next_column >= target_column {
                        if inclusive {
                            return (byte + 1, next_column)
                        }
                        return (byte, column)
                    }
                    column = next_column;
                    byte += 1;
                }
                continue
            }
        }
        let mut buffer = std::mem::take(&mut carry);
        buffer.push_str(chunk);
        let mut iter = buffer.graphemes(true).peekable();
        while let Some(grapheme) = iter.next() {
            let width = grapheme_display_width(grapheme, column, tab_width);
            if column + width >= target_column {
                if inclusive {
                    return (byte + grapheme.len(), column + width)
                }
                return (byte, column)
            }
            if iter.peek().is_none() {
                carry = grapheme.to_string();
            } else {
                column += width;
                byte += grapheme.len();
            }
        }
    }
    if !carry.is_empty() {
        let width = grapheme_display_width(&carry, column, tab_width);
        if inclusive || column + width < target_column {
            column += width;
            byte += carry.len();
        }
    }
    (byte, column)
}

fn grapheme_representation(g: &str, ctx: &mut RenderingContext) {
    let sel_style = ContentStyle::new().with(SELECTION_FG).on(SELECTION_BG);
    let escaped_style = ContentStyle::new().with(DEFAULT_FG).on(BLUEISH);

    // Skip building styled strings for everything left of the visible region.
    // Tabs are handled below because they may need to be split into individual
    // spaces when the visible region starts in the middle of a tab.
    if g != "\t" && ctx.current_column < ctx.visible_from_column {
        ctx.current_column += grapheme_display_width(g, ctx.current_column, ctx.tab_width);
        return;
    }

    if g == "\t" {
        if ctx.tab_width > 0 {
            let w = ctx.tab_width - (ctx.current_column % ctx.tab_width);
            // push the spaces as separate tokens in case the line is horizontally scrolled such
            // that we need to cut the line in the middle of a tab
            if ctx.is_selection() {
                for _ in 0..w {
                    ctx.push(sel_style.apply(" ".into()));
                }
            } else if ctx.is_cursor {
                ctx.push(ctx.token_style.reverse().apply(" ".to_string()));
                for _ in 1..w {
                    ctx.push(ctx.token_style.apply(" ".into()));
                }
            } else {
                for _ in 0..w {
                    ctx.push(ctx.token_style.apply(" ".into()));
                }
            }
        }
    } else if let Some(glyph) = unicode_line_break_symbol(g) {
        if ctx.is_selection() {
            ctx.push(sel_style.with(BLUEISH).apply(glyph.into()));
        } else if ctx.is_cursor {
            ctx.push(ctx.token_style.reverse().apply(" ".into()));
        }
    } else if let Some(disp) = replacement_symbol(g) {
        if ctx.is_selection() {
            ctx.push(sel_style.with(BLUEISH).apply(disp));
        } else if ctx.is_cursor {
            ctx.push(escaped_style.reverse().apply(disp));
        } else {
            ctx.push(escaped_style.apply(disp));
        }
    } else if ctx.is_selection() {
        ctx.push(sel_style.apply(g.into()));
    } else if ctx.is_cursor {
        ctx.push(ctx.token_style.reverse().apply(g.into()));
    } else {
        ctx.push(ctx.token_style.apply(g.into()));
    }
}

const BLUEISH: Color = Color::Rgb { r: 0x4a, g: 0x54, b: 0x6e };
const DEFAULT_FG: Color = Color::White;
const DEFAULT_BG: Color = Color::Rgb { r: 0x1a, g: 0x1a, b: 0x1a };
const SELECTION_FG: Color = Color::Black;
const SELECTION_BG: Color = Color::Rgb { r: 0x88, g: 0xff, b: 0xc5 };
const LIGHT_GREY: Color = Color::Rgb { r: 0xaa, g: 0xaa, b: 0xaa };
const SLIGHTLY_LIGHTER_BG: Color = Color::Rgb { r: 0x1e, g: 0x1e, b: 0x1e };
const LIGHTER_BG: Color = Color::Rgb { r: 0x24, g: 0x24, b: 0x24 };

impl SuggestionMenu {
    // TODO: Renderable trait instead of this nonsense
    pub fn render(&self, writer: &mut dyn std::io::Write, max_width: usize, style: ContentStyle) -> std::io::Result<()> {
        let usable_width = max_width - 4;
        let mut width = 0;
        width += self.current().width();
        let mut pre = Vec::new();
        let mut post = String::new();

        let mut skipped_start = false;
        let mut skipped_end = false;

        let mut right = self.suggestions[self.current_idx + 1 ..].iter().map(|s| (s, s.width() + 1));
        if let Some((sugg, w)) = right.next() {
            if width + w < usable_width {
                width += w;
                post.push(' ');
                post.push_str(sugg);
            } else {
                skipped_end = true;
            }
        }
        let left = self.suggestions[0..self.current_idx].iter().rev().map(|s| (s, s.width() + 1));
        for (sugg, w) in left {
            if width + w > usable_width {
                skipped_start = true;
                break
            }
            width += w;
            pre.push(" ");
            pre.push(sugg);
        }
        for (sugg, w) in right {
            if width + w > usable_width {
                skipped_end = true;
                break
            }
            width += w;
            post.push(' ');
            post.push_str(sugg);
        }

        let pre: String = pre.into_iter().rev().collect();
        writer.queue(crossterm::style::SetStyle(style))?;
        if skipped_start {
            writer.queue(Print("< "))?;
        } else {
            writer.queue(Print("  "))?;
        }
        writer.queue(Print(pre))?;
        writer.queue(PrintStyledContent(style.reverse().apply(self.current())))?;
        writer.queue(crossterm::style::SetStyle(style))?;
        writer.queue(Print(post))?;
        writer.queue(Clear(ClearType::UntilNewLine))?;
        if skipped_end {
            writer.queue(crossterm::cursor::MoveToColumn(max_width as u16 - 1))?;
            writer.queue(Print(">"))?;
        }
        Ok(())
    }
}

impl App {
    fn status_line_text_left(&self, ft: &str) -> String {
        let title = &self.current_pane().title;
        let modified = match self.current_pane().modified {
            true => "[+] ",
            false => "",
        };
        format!("{title} {modified}| ft:{ft}")
    }

    fn status_line_text_right(&self, cursor_column: Option<usize>) -> String {
        let pane = self.current_pane();
        let content = &pane.content;
        let cursor = self.current_pane().cursors.primary();
        let filesize = content.len_bytes();
        let fsize_indicator = if filesize < 10_000 {
            format!("{}/{}B", cursor.offset.0, filesize)
        } else {
            const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
            let mut unit = 0;
            let mut filesize = filesize as f32;
            while filesize >= 1024.0 {
                filesize /= 1024.0;
                unit += 1;
            }
            format!("{:.decimal_places$}{}", filesize, UNITS[unit], decimal_places=if filesize < 10.0 { 2 } else { 1 })
        };
        let pane_indicator = match (self.panes.len(), 1 + self.current_pane_index) {
            (1, _) => "".to_string(),
            (n_panes, curr @ 1..12) => {
                let mut s = String::new();
                for i in 1..=n_panes.min(12) {
                    if i == curr {
                        s.push('[');
                    } else if i == 1 + curr {
                        s.push(']');
                    } else {
                        s.push(' ');
                    }
                    s.push_str(&format!("{i}"));
                }
                if curr == n_panes {
                    s.push(']');
                } else {
                    s.push(' ');
                }
                s
            }
            (n_panes, curr) => {
                if curr == n_panes {
                    format!("..{}[{}]", curr - 1, curr)
                } else if curr + 1 == n_panes {
                    format!("..{}[{}]{}", curr - 1, curr, curr + 1)
                } else {
                    format!("..{}[{}]{}..", curr - 1, curr, curr + 1)
                }
            }
        };
        format!(
            "{} {:>3}:{:<3} {}",
            pane_indicator,
            1 + content.byte_to_line(cursor.offset),
            1 + cursor_column.unwrap_or_else(|| cursor.column(content)),
            fsize_indicator
        )
    }

    pub fn render(&mut self, mut writer: &mut dyn std::io::Write, wsize: &WindowSize) -> std::io::Result<()> {
        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;
        writer.queue(crossterm::cursor::Hide)?;

        if wsize.rows < 3 {
            writer.queue(Clear(ClearType::All))?;
            writer.queue(MoveTo(0, 0))?;
            writer.queue(Print("window too smol"))?;
        } else {
            let mut hl = self.current_pane_mut().highlighter.take().unwrap_or_else(|| {
                BadHighlighter::for_file("", self.highlighting.clone())
            });
            self.render_content(writer, wsize, &mut hl)?;
            self.current_pane_mut().highlighter.replace(hl);
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }

    fn render_content(&self, writer: &mut dyn std::io::Write, wsize: &WindowSize, hl: &mut BadHighlighter) -> std::io::Result<()> {
        let current_pane = &self.current_pane();
        let now = Instant::now();
        let content = &current_pane.content;
        let primary_cursor_offset = current_pane.cursors.primary().offset;
        let primary_cursor_span = current_pane.cursors.primary().line_span(content);
        let primary_cursor_line = current_pane.cursors.primary().current_line_number(content);
        let mut primary_cursor_column = None;
        let primary_cursor_is_selected = current_pane.cursors.iter().any(|cur| {
            cur.selection_from.is_some_and(|sel_from| {
                let start = cur.offset.min(sel_from);
                let end = cur.offset.max(sel_from);
                start <= primary_cursor_offset && primary_cursor_offset < end
            })
        });
        let default_style = ContentStyle::new().with(DEFAULT_FG).on(DEFAULT_BG);
        let completions_style = ContentStyle::new().with(LIGHT_GREY).on(SLIGHTLY_LIGHTER_BG);
        let lineno_style = ContentStyle::new().with(LIGHT_GREY).on(LIGHTER_BG);

        macro_rules! peek {
            ($it:expr) => {
                match $it.peek() {
                    Some(Cur::Start(b) | Cur::End(b) | Cur::NoSelection(b)) => *b,
                    None => ByteOffset::MAX,
                }
            }
        }

        #[derive(Copy, Clone, Debug)]
        enum Cur {
            Start(ByteOffset),
            End(ByteOffset),
            NoSelection(ByteOffset),
        }

        let mut curs = {
            let mut curs: Vec<Cur> = vec![];
            for cursor in current_pane.cursors.iter() {
                match cursor.selection_from {
                    Some(sel_from) => {
                        let a = cursor.offset.min(sel_from);
                        let b = cursor.offset.max(sel_from);
                        curs.push(Cur::Start(a));
                        curs.push(Cur::End(b));
                    }
                    None => {
                        curs.push(Cur::NoSelection(cursor.offset));
                    }
                }
            }
            curs.sort_unstable_by_key(|c| match c {
                Cur::Start(b) | Cur::End(b) | Cur::NoSelection(b) => *b
            });
            curs.into_iter().peekable()
        };

        let mut last_visible_lineno = current_pane.viewport_position_row + current_pane.viewport_height as usize;
        let max_lineno_width = {
            let mut n = content.len_lines();
            let mut w = 1;
            while n > 9 {
                n /= 10;
                w += 1;
            }
            w
        };

        let mut ctx = RenderingContext {
            is_cursor: false,
            n_selections: 0,
            current_column: 0,
            visible_from_column: 0,
            available_columns: (wsize.columns as usize).saturating_sub(max_lineno_width + 2),
            tab_width: current_pane.settings.tab_width,
            token_style: default_style,
            queue: vec![],
        };

        let mut console_row: u16 = 0;
        writer.queue(MoveTo(0, 0))?;
        let first_visible_lineno = current_pane.viewport_position_row;
        let mut byte_offset;

        macro_rules! consume_cursor_markers {
            () => {
                ctx.is_cursor = false;
                while peek!(curs) <= byte_offset {
                    match curs.peek() {
                        Some(Cur::Start(_)) => ctx.n_selections += 1,
                        Some(Cur::End(_)) => ctx.n_selections -= 1,
                        Some(Cur::NoSelection(b)) if b == &byte_offset => {
                            ctx.is_cursor = true;
                        }
                        _ => {}
                    }
                    curs.next();
                }
            };
        }

        macro_rules! skip_cursor_markers_before {
            ($target:expr) => {
                while peek!(curs) < $target {
                    match curs.peek() {
                        Some(Cur::Start(_)) => ctx.n_selections += 1,
                        Some(Cur::End(_)) => ctx.n_selections -= 1,
                        _ => {}
                    }
                    curs.next();
                }
            };
        }

        hl.skip_to_line(first_visible_lineno, content);

        for (line, lineno) in content.lines_at(current_pane.viewport_position_row).zip(first_visible_lineno..) {
            if lineno > last_visible_lineno {
                break
            }
            let one_based_lineno = 1 + lineno;
            let line_slice = line;
            let line_start_offset = content.line_to_byte(lineno);
            ctx.current_column = 0;
            ctx.visible_from_column = 0;
            byte_offset = line_start_offset;

            let mut rendered_long_line = false;
            if line_slice.len_bytes() > BadHighlighter::MAX_LINE_LENGTH_FOR_HIGHLIGHTING {
                let line_len = line_slice.len_bytes();
                let line_content_len = line_content_len(&line_slice);
                let content = line_slice.byte_slice(0..line_content_len);
                let cursor_on_line = lineno == primary_cursor_line;
                let cursor_rel = primary_cursor_offset.0.saturating_sub(line_start_offset.0).min(line_len);
                let mut visible_from_col = 0;
                let mut window_start_byte = 0;
                let mut window_start_col = 0;
                let mut prefix_ok = !cursor_on_line;
                if cursor_on_line && cursor_rel <= line_content_len {
                    let revision = current_pane.content.revision();
                    let mut cached_layout = current_pane.long_line_layout.borrow_mut();
                    if cached_layout
                        .as_ref()
                        .is_none_or(|layout| !layout.matches(revision, lineno, ctx.tab_width, line_content_len))
                    {
                        *cached_layout =
                            Some(LongLineLayout::new(&content, ctx.tab_width, revision, lineno));
                    }
                    let layout = cached_layout.as_mut().expect("layout was just initialized");
                    layout.ensure_through(&content, cursor_rel);
                    let prefix_checkpoint = layout.checkpoint_for_byte(cursor_rel);
                    let prefix = content.byte_slice(prefix_checkpoint.byte..cursor_rel);
                    let (prefix_width, local_graphemes) =
                        text_width(&prefix, ctx.tab_width, prefix_checkpoint.column, None);
                    let prefix_graphemes = prefix_checkpoint.graphemes + local_graphemes;
                    let cursor_unit_width = if cursor_rel < line_content_len {
                        next_grapheme_width(&content, cursor_rel, prefix_width, ctx.tab_width).unwrap_or(0)
                    } else if line_content_len < line_len {
                        let line_break = line_slice.byte_slice(line_content_len..line_len).to_string();
                        let width = unicode_line_break_symbol(&line_break)
                            .map(UnicodeWidthStr::width)
                            .unwrap_or(1);
                        if primary_cursor_is_selected { width } else { 1 }
                    } else {
                        0
                    };
                    let cursor_col = prefix_width + cursor_unit_width;
                    visible_from_col = cursor_col.saturating_sub(ctx.available_columns.saturating_sub(1));
                    let window_checkpoint = layout.checkpoint_for_column(visible_from_col);
                    if window_checkpoint.column == visible_from_col {
                        window_start_byte = window_checkpoint.byte;
                        window_start_col = window_checkpoint.column;
                    } else {
                        (window_start_byte, window_start_col) = advance_to_column(
                            &content,
                            window_checkpoint.byte,
                            window_checkpoint.column,
                            visible_from_col,
                            ctx.tab_width,
                            false,
                        );
                    }
                    primary_cursor_column = Some(prefix_graphemes);
                    prefix_ok = true;
                }
                if prefix_ok {
                    let window_end_col = visible_from_col
                        .saturating_add(ctx.available_columns)
                        .saturating_add(2);
                    let (window_end_byte, _) = advance_to_column(
                        &content,
                        window_start_byte,
                        window_start_col,
                        window_end_col,
                        ctx.tab_width,
                        true,
                    );
                    let style = hl.highlight_long_line();
                    ctx.token_style = to_crossterm_style(style);
                    ctx.visible_from_column = visible_from_col;
                    ctx.current_column = window_start_col;
                    byte_offset = ByteOffset(line_start_offset.0 + window_start_byte);
                    skip_cursor_markers_before!(byte_offset);

                    let window = content.byte_slice(window_start_byte..window_end_byte).to_string();
                    let mut offset = 0;
                    'long: while let Some(unit) = next_text_unit(&window, offset) {
                        if unit.ascii {
                            for (i, _) in unit.text.bytes().enumerate() {
                                consume_cursor_markers!();
                                grapheme_representation(&unit.text[i..i + 1], &mut ctx);
                                byte_offset.0 += 1;
                                if ctx.current_column > ctx.visible_from_column + ctx.available_columns {
                                    break 'long
                                }
                            }
                        } else {
                            consume_cursor_markers!();
                            grapheme_representation(unit.text, &mut ctx);
                            byte_offset.0 += unit.text.len();
                            if ctx.current_column > ctx.visible_from_column + ctx.available_columns {
                                break 'long
                            }
                        }
                        offset = unit.end;
                    }
                    if line_content_len < line_len
                        && byte_offset.0 == line_start_offset.0 + line_content_len
                        && window_end_byte == line_content_len
                    {
                        let line_break = line_slice.byte_slice(line_content_len..line_len).to_string();
                        consume_cursor_markers!();
                        grapheme_representation(&line_break, &mut ctx);
                        byte_offset.0 += line_len - line_content_len;
                    }
                    rendered_long_line = true;
                }
            }

            if !rendered_long_line {
                let line = line_slice.to_string();
                if lineno == primary_cursor_line {
                    // Figure out how far the line needs to be scrolled horizontally before
                    // iterating the text so we can skip everything left of the viewport.
                    // ASCII spans are one byte per column, so they can be counted in bulk;
                    // grapheme clustering is only needed around non-ASCII text and tabs.
                    let mut column = 0;
                    let mut grapheme_column = 0;
                    let mut offset = 0;
                    while let Some(unit) = next_text_unit(&line, offset) {
                        let unit_start = line_start_offset.0 + offset;
                        let unit_end = line_start_offset.0 + unit.end;
                        let width = text_unit_display_width(&unit, column, ctx.tab_width);
                        if unit_start >= primary_cursor_offset.0 {
                            primary_cursor_column = Some(grapheme_column);
                            // Include the cursor unit so the cursor doesn't end up in the
                            // last visible column where the overflow indicator overwrites it.
                            // Cursor line breaks render as a single space instead of the
                            // full line break glyph unless they are selected.
                            let renders_as_single_space =
                                !primary_cursor_is_selected && unicode_line_break_symbol(unit.text).is_some();
                            column += if unit.ascii || renders_as_single_space { 1 } else { width };
                            break
                        }
                        if unit_end > primary_cursor_offset.0 {
                            if unit.ascii {
                                grapheme_column += primary_cursor_offset.0 - unit_start;
                            }
                            primary_cursor_column = Some(grapheme_column);
                            column += if unit.ascii {
                                primary_cursor_offset.0 - unit_start + 1
                            } else {
                                width
                            };
                            break
                        }
                        grapheme_column += if unit.ascii { unit.text.len() } else { 1 };
                        column += width;
                        offset = unit.end;
                    }
                    if primary_cursor_column.is_none() {
                        primary_cursor_column = Some(grapheme_column);
                    }
                    ctx.visible_from_column = column.saturating_sub(ctx.available_columns.saturating_sub(1));
                }

                'line: for (style, s) in hl.highlight_line(&line) {
                    ctx.token_style = to_crossterm_style(style);
                    let mut offset = 0;
                    while offset < s.len() {
                        let unit = next_text_unit(s, offset).expect("offset is in bounds");
                        if unit.ascii {
                            if ctx.current_column + unit.text.len() <= ctx.visible_from_column {
                                // Skip the whole span without building any styled strings.
                                let target = ByteOffset(byte_offset.0 + unit.text.len());
                                skip_cursor_markers_before!(target);
                                ctx.current_column += unit.text.len();
                                byte_offset = target;
                                offset = unit.end;
                                continue
                            }
                            if ctx.current_column < ctx.visible_from_column {
                                // The visible region starts in the middle of this span.
                                let skip = ctx.visible_from_column - ctx.current_column;
                                let target = ByteOffset(byte_offset.0 + skip);
                                skip_cursor_markers_before!(target);
                                ctx.current_column += skip;
                                byte_offset = target;
                                offset += skip;
                                continue
                            }
                            for (i, _) in unit.text.bytes().enumerate() {
                                consume_cursor_markers!();
                                grapheme_representation(&unit.text[i..i + 1], &mut ctx);
                                byte_offset.0 += 1;
                                // Any grapheme starting beyond this column would only be
                                // rendered as the overflow indicator, so there is no need
                                // to process the rest of the line.
                                if ctx.current_column > ctx.visible_from_column + ctx.available_columns {
                                    break 'line
                                }
                            }
                            offset = unit.end;
                        } else {
                            consume_cursor_markers!();
                            grapheme_representation(unit.text, &mut ctx);
                            byte_offset.0 += unit.text.len();
                            if ctx.current_column > ctx.visible_from_column + ctx.available_columns {
                                break 'line
                            }
                            offset = unit.end;
                        }
                    }
                }
            }

            // render cursor at the end of the file
            if one_based_lineno >= content.len_lines() && {
                let content_end_offset = ByteOffset(content.len_bytes());
                current_pane.cursors.iter().any(|cur| !cur.has_selection() && cur.offset == content_end_offset)
            } {
                ctx.is_cursor = true;
                let required_columns = ctx.current_column + 1;
                ctx.visible_from_column = required_columns.saturating_sub(ctx.available_columns.saturating_sub(1));
                grapheme_representation(" ", &mut ctx);
            }
            // render line numbers
            {
                let left_scroll_indicator = if ctx.visible_from_column > 0 { '<' } else { ' ' };
                let sidebar = format!(" {one_based_lineno:max_lineno_width$}{left_scroll_indicator}");
                let mut lineno_style = lineno_style;
                if let Some(lint) = current_pane.lints.iter().find(|lint| lint.lineno() == one_based_lineno) {
                    lineno_style = lineno_style.with(lint.color());
                }
                writer.queue(PrintStyledContent(lineno_style.apply(&sidebar)))?;
            }

            // render visible segment of the current line
            let mut current_column = 0;
            for (s_start, width, s) in ctx.queue.drain(..) {
                if s_start < ctx.visible_from_column {
                    continue
                }
                if current_column + width <= ctx.available_columns {
                    writer.queue(PrintStyledContent(s))?;
                    current_column += width;
                } else {
                    writer.queue(MoveTo(wsize.columns.saturating_sub(1), console_row))?;
                    writer.queue(PrintStyledContent(lineno_style.apply(">")))?;
                    break
                }
            }

            // clear rest
            writer.queue(crossterm::style::SetStyle(default_style))?;
            writer.queue(Clear(ClearType::UntilNewLine))?;
            writer.queue(MoveToNextLine(1))?;
            console_row += 1;

            // render suggestions
            if primary_cursor_line == lineno {
                if let Some(suggs) = current_pane.suggestions.as_ref() {
                    suggs.render(writer, wsize.columns as usize, completions_style)?;
                    writer.queue(MoveToNextLine(1))?;
                    console_row += 1;
                }
            }

            // render debug scopes
            if current_pane.settings.debug_scopes && primary_cursor_line == lineno {
                let line_start = current_pane.cursors.primary().line_start(content);
                let primary_cursor_offset_within_line = primary_cursor_offset.0 - line_start.0;
                let ss = hl.scope_stack_at(primary_cursor_line, primary_cursor_offset_within_line, content);
                for scope in ss.as_slice().iter() {
                    writer.queue(crossterm::style::SetStyle(lineno_style))?;
                    writer.queue(Print(format!("{}· {scope}", " ".repeat(max_lineno_width))))?;
                    writer.queue(Clear(ClearType::UntilNewLine))?;
                    writer.queue(MoveToNextLine(1))?;
                    console_row += 1;
                }
            }

            // render possible lints
            if primary_cursor_span.contains(&lineno) {
                for lint in current_pane.lints.iter().filter(|lint| lint.lineno() == one_based_lineno) {
                    writer.queue(PrintStyledContent(ContentStyle::new().on(lint.color()).apply(" ".repeat(max_lineno_width + 2))))?;
                    writer.queue(PrintStyledContent(default_style.on(LIGHTER_BG).apply(&lint.message)))?;
                    writer.queue(crossterm::style::SetStyle(default_style.on(LIGHTER_BG)))?;
                    writer.queue(Clear(ClearType::UntilNewLine))?;
                    writer.queue(MoveToNextLine(1))?;
                    console_row += 1;
                    last_visible_lineno = last_visible_lineno.saturating_sub(1);
                }
            }
        }

        writer.queue(crossterm::style::SetStyle(default_style))?;
        writer.queue(Clear(ClearType::FromCursorDown))?;

        writer.queue(MoveTo(0, wsize.rows - 2))?;
        writer.queue(crossterm::style::SetStyle(default_style.negative()))?;
        let width = wsize.columns as usize;
        let status_line_left = format!("{:width$}", self.status_line_text_left(hl.ft()), width = width);
        writer.queue(PrintStyledContent(default_style.negative().apply(status_line_left)))?;
        let status_line_right = self.status_line_text_right(primary_cursor_column);
        writer.queue(MoveTo(width.saturating_sub(status_line_right.len()) as u16, wsize.rows - 2))?;
        writer.queue(PrintStyledContent(default_style.negative().apply(status_line_right)))?;

        writer.queue(MoveTo(0, wsize.rows - 1))?;
        writer.queue(crossterm::style::SetStyle(default_style))?;
        writer.queue(Print(
            match self.status_msg() {
                Some(info) => format!("{:.width$}", &info, width = wsize.columns as usize),
                None => format!("render took {:.3?}", now.elapsed()),
            }
        ))?;
        // this ensures prompt is printed in the right place!
        writer.queue(MoveTo(0, wsize.rows - 1))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_long_line_layout_rebase_after_edit() {
        let text = "a\t".repeat(24_000) + "tail";
        let content = crate::ropebuffer::RopeBuffer::from_str(&text);
        let slice = content.lines_at(0).next().unwrap();
        let cursor = slice.len_bytes() - 4;
        let mut layout = LongLineLayout::new(&slice, 4, content.revision(), 0);
        layout.ensure_through(&slice, cursor);
        let old_line_count = content.len_lines();

        let mut edited = text.clone();
        edited.insert_str(cursor, "XY");
        let edited_content = crate::ropebuffer::RopeBuffer::from_str(&edited);
        assert!(layout.try_rebase_after_edit(&edited_content, ByteOffset(cursor), old_line_count));
        assert!(layout.matches(edited_content.revision(), 0, 4, edited.len()));

        let edited_slice = edited_content.lines_at(0).next().unwrap();
        layout.ensure_through(&edited_slice, edited_slice.len_bytes());
        for byte in [
            0,
            123,
            CHECKPOINT_INTERVAL,
            CHECKPOINT_INTERVAL + 7,
            cursor,
            cursor + 1,
            cursor + 2,
            edited_slice.len_bytes(),
        ] {
            let checkpoint = layout.checkpoint_for_byte(byte);
            let (width, graphemes) =
                text_width(&edited_slice.byte_slice(checkpoint.byte..byte), 4, checkpoint.column, None);
            let (expected_width, expected_graphemes) = text_width(&edited_slice.byte_slice(..byte), 4, 0, None);
            assert_eq!(width, expected_width);
            assert_eq!(checkpoint.graphemes + graphemes, expected_graphemes);
        }

        let mut stale = LongLineLayout::new(&slice, 4, content.revision(), 0);
        stale.ensure_through(&slice, cursor);
        let mut early_edit = text;
        early_edit.insert(1, 'Z');
        let early_content = crate::ropebuffer::RopeBuffer::from_str(&early_edit);
        assert!(!stale.try_rebase_after_edit(&early_content, ByteOffset(1), old_line_count));
    }

    #[test]
    fn test_replacement_symbols() {
        assert_eq!(replacement_symbol("\u{200C}"), Some("<U+200C>".into()));
        assert_eq!(replacement_symbol("\u{0}"), Some("<00>".into()));
    }

    fn text_units(line: &str) -> Vec<(&str, bool)> {
        let mut units = vec![];
        let mut offset = 0;
        while let Some(unit) = next_text_unit(line, offset) {
            units.push((unit.text, unit.ascii));
            offset = unit.end;
        }
        units
    }

    #[test]
    fn test_text_width_tabs_and_graphemes() {
        let rope = ropey::Rope::from_str("ab\tc e\u{301}😀");
        let slice = rope.slice(..);
        assert_eq!(text_width(&slice, 4, 0, None), (9, 7));
        assert_eq!(next_grapheme_width(&slice, 2, 2, 4), Some(2));
        assert_eq!(next_grapheme_width(&slice, 5, 5, 4), Some(1));
        assert_eq!(next_grapheme_width(&slice, 8, 7, 4), Some(2));
        assert_eq!(next_grapheme_width(&slice, slice.len_bytes(), 9, 4), None);
    }

    #[test]
    fn test_long_line_layout_checkpoints() {
        let text = format!(
            "{}\t{}é",
            "a".repeat(CHECKPOINT_INTERVAL * 2),
            "b".repeat(CHECKPOINT_INTERVAL + 23),
        );
        let rope = ropey::Rope::from_str(&text);
        let slice = rope.slice(..);
        let mut layout = LongLineLayout::new(&slice, 4, 7, 3);

        assert!(layout.matches(7, 3, 4, slice.len_bytes()));
        assert!(!layout.matches(8, 3, 4, slice.len_bytes()));
        assert!(!layout.matches(7, 4, 4, slice.len_bytes()));
        assert!(!layout.matches(7, 3, 8, slice.len_bytes()));

        for byte in [
            0,
            1,
            CHECKPOINT_INTERVAL,
            CHECKPOINT_INTERVAL * 2,
            CHECKPOINT_INTERVAL * 2 + 1,
            slice.len_bytes(),
        ] {
            layout.ensure_through(&slice, byte);
            let checkpoint = layout.checkpoint_for_byte(byte);
            let (local_width, local_graphemes) =
                text_width(&slice.byte_slice(checkpoint.byte..byte), 4, checkpoint.column, None);
            let (expected_width, expected_graphemes) =
                text_width(&slice.byte_slice(..byte), 4, 0, None);
            assert_eq!(local_width, expected_width);
            assert_eq!(checkpoint.graphemes + local_graphemes, expected_graphemes);
        }

        layout.ensure_through(&slice, slice.len_bytes());
        for column in [0, 1, CHECKPOINT_INTERVAL + 1, CHECKPOINT_INTERVAL * 2 + 5, slice.len_bytes()] {
            let checkpoint = layout.checkpoint_for_column(column);
            assert!(checkpoint.column <= column);
            assert_eq!(
                advance_to_column(&slice, checkpoint.byte, checkpoint.column, column, 4, false),
                advance_to_column(&slice, 0, 0, column, 4, false),
            );
        }
    }

    #[test]
    fn test_advance_to_column() {
        let rope = ropey::Rope::from_str("ab\tc");
        let slice = rope.slice(..);
        assert_eq!(advance_to_column(&slice, 0, 0, 2, 4, true), (2, 2));
        assert_eq!(advance_to_column(&slice, 0, 0, 3, 4, false), (2, 2));
        assert_eq!(advance_to_column(&slice, 0, 0, 3, 4, true), (3, 4));
        assert_eq!(advance_to_column(&slice, 0, 0, 5, 4, true), (4, 5));

        let rope = ropey::Rope::from_str("a😀b");
        let slice = rope.slice(..);
        assert_eq!(advance_to_column(&slice, 0, 0, 2, 4, false), (1, 1));
        assert_eq!(advance_to_column(&slice, 0, 0, 2, 4, true), (5, 3));
        assert_eq!(advance_to_column(&slice, 0, 0, 4, 4, true), (6, 4));
    }

    #[test]
    fn test_text_units_keep_combining_mark_with_ascii_base() {
        assert_eq!(text_units("ab\u{0301}c"), vec![("a", true), ("b\u{0301}", false), ("c", true)]);
    }

    #[test]
    fn test_text_units_split_ascii_and_grapheme_fallback() {
        assert_eq!(text_units("hi\né"), vec![("hi", true), ("\n", false), ("é", false)]);
        assert_eq!(text_units("1\u{FE0F}\u{20E3}x"), vec![("1\u{FE0F}\u{20E3}", false), ("x", true)]);
    }
}
