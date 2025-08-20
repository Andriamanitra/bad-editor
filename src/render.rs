use std::time::Instant;

use crossterm::{
    cursor::MoveTo,
    cursor::MoveToNextLine,
    style::{Color, ContentStyle, Print, PrintStyledContent, StyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate, WindowSize},
    QueueableCommand,
};
use syntect::highlighting::Style as SyntectStyle;
use syntect::highlighting::FontStyle as SyntectFontStyle;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::App;
use crate::ByteOffset;

fn to_crossterm_style(syntect_style: SyntectStyle) -> ContentStyle {
    let fg = {
        let syntect::highlighting::Color {r, g, b, ..} = syntect_style.foreground;
        Color::Rgb { r, g, b }
    };
    let bg = {
        let syntect::highlighting::Color {r, g, b, ..} = syntect_style.background;
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
        _ => None
    }
}

fn replacement_symbol(g: &str) -> Option<String> {
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
    queue: Vec<(usize, usize, StyledContent<String>)>
}
impl RenderingContext {
    fn is_selection(&self) -> bool {
        self.n_selections > 0
    }

    fn push(&mut self, g: StyledContent<String>) {
        let width = UnicodeWidthStr::width(g.content().as_str());
        self.queue.push((self.current_column, width, g));
        self.current_column += width;
    }
}

fn grapheme_representation(g: &str, ctx: &mut RenderingContext) {
    let sel_style = ContentStyle::new().with(SELECTION_FG).on(SELECTION_BG);
    let escaped_style = ContentStyle::new().with(DEFAULT_FG).on(BLUEISH);

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

const BLUEISH: Color = Color::Rgb {  r: 0x4a, g: 0x54, b: 0x6e };
const DEFAULT_FG: Color = Color::White;
const DEFAULT_BG: Color = Color::Rgb { r: 0x1a, g: 0x1a, b: 0x1a };
const SELECTION_FG: Color = Color::Black;
const SELECTION_BG: Color = Color::Rgb { r: 0x88, g: 0xff, b: 0xc5 };
const LIGHT_GREY: Color = Color::Rgb { r: 0xaa, g: 0xaa, b: 0xaa };
const LIGHTER_BG: Color = Color::Rgb { r: 0x24, g: 0x24, b: 0x24 };

impl App {
    fn status_line_text_left(&self) -> String {
        match self.current_pane().modified {
            true => format!("{} [+]", &self.current_pane().title),
            false => self.current_pane().title.to_string(),
        }
    }

    fn status_line_text_right(&self) -> String {
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
        format!(
            " col:{:<3} line:{:<3} {}",
            1 + cursor.column(content),
            1 + content.byte_to_line(cursor.offset),
            fsize_indicator
        )
    }

    pub fn render(&self, mut writer: &mut dyn std::io::Write, wsize: &WindowSize) -> std::io::Result<()> {
        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;
        writer.queue(crossterm::cursor::Hide)?;

        if wsize.rows < 3 {
            writer.queue(Clear(ClearType::All))?;
            writer.queue(MoveTo(0, 0))?;
            writer.queue(Print("window too smol"))?;
        } else {
            self.render_content(writer, wsize)?;
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }

    fn render_content(&self, writer: &mut dyn std::io::Write, wsize: &WindowSize) -> std::io::Result<()> {
        let current_pane = &self.current_pane();
        let now = Instant::now();
        let content = &current_pane.content;
        let primary_cursor_offset = current_pane.cursors.primary().offset;
        let primary_cursor_span = current_pane.cursors.primary().line_span(content);
        let default_style = ContentStyle::new().with(DEFAULT_FG).on(DEFAULT_BG);
        let lineno_style = ContentStyle::new().with(LIGHT_GREY).on(LIGHTER_BG);

        macro_rules! peek {
            ($it:expr) => {
                match $it.peek() {
                    Some(Cur::Start(b) | Cur::End(b) | Cur::NoSelection(b)) => *b,
                    None => ByteOffset::MAX
                }
            }
        }

        #[derive(Copy, Clone, Debug)]
        enum Cur {
            Start(ByteOffset),
            End(ByteOffset),
            NoSelection(ByteOffset),
        }

        let mut hl = self.highlighting.highlighter_for_file(&current_pane.title);

        let mut curs = {
            let mut curs: Vec<Cur> = vec![];
            for cursor in self.current_pane().cursors.iter() {
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

        let mut byte_offset = ByteOffset(0);

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
            tab_width: self.current_pane().settings.tab_width,
            token_style: default_style,
            queue: vec![],
        };

        let mut console_row: u16 = 0;
        writer.queue(MoveTo(0, 0))?;
        for (lineno, line) in content.lines().enumerate() {
            if lineno > last_visible_lineno {
                break
            }
            let line = line.to_string();
            if lineno < current_pane.viewport_position_row {
                byte_offset.0 += line.len();
                for _ in hl.highlight(&line) {}
                continue
            }
            ctx.visible_from_column = 0;
            ctx.current_column = 0;

            for (style, s) in hl.highlight(&line) {
                ctx.token_style = to_crossterm_style(style);
                for g in s.graphemes(true) {
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
                    grapheme_representation(g, &mut ctx);
                    if byte_offset == primary_cursor_offset {
                        let required_columns = ctx.current_column;
                        ctx.visible_from_column = required_columns.saturating_sub(ctx.available_columns.saturating_sub(1));
                    }
                    byte_offset.0 += g.len();
                }
            }

            // render cursor at the end of the file
            if 1 + lineno >= content.len_lines() && {
                let content_end_offset = ByteOffset(content.len_bytes());
                self.current_pane().cursors.iter().any(|cur| !cur.has_selection() && cur.offset == content_end_offset)
            } {
                ctx.is_cursor = true;
                let required_columns = ctx.current_column + 1;
                ctx.visible_from_column = required_columns.saturating_sub(ctx.available_columns.saturating_sub(1));
                grapheme_representation(" ", &mut ctx);
            }
            // render line numbers
            {
                let left_scroll_indicator = if ctx.visible_from_column > 0 { '<' } else { ' ' };
                let sidebar = format!(" {:width$}{}", 1 + lineno, left_scroll_indicator, width=max_lineno_width);
                let mut lineno_style = lineno_style;
                if current_pane.lints.iter().any(|lint| lint.lineno == lineno) {
                    let lints = current_pane.lints.iter().filter(|l| l.lineno == lineno);
                    for lint in lints {
                        lineno_style = lineno_style.with(lint.color());
                    }
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

            // render possible lints
            if primary_cursor_span.contains(&lineno) && current_pane.lints.iter().any(|lint| lint.lineno == lineno) {
                let lints = current_pane.lints.iter().filter(|l| l.lineno == lineno);
                for lint in lints {
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
        let status_line_left = format!("{:width$}", self.status_line_text_left(), width = width);
        writer.queue(PrintStyledContent(default_style.negative().apply(status_line_left)))?;
        let status_line_right = self.status_line_text_right();
        writer.queue(MoveTo(width.saturating_sub(status_line_right.len()) as u16, wsize.rows - 2))?;
        writer.queue(PrintStyledContent(default_style.negative().apply(status_line_right)))?;

        writer.queue(MoveTo(0, wsize.rows - 1))?;
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
