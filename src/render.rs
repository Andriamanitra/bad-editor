use std::time::Instant;

use crossterm::{
    cursor::MoveTo,
    style::{Color, ContentStyle, Print, PrintStyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    QueueableCommand,
};
use syntect::highlighting::Style as SyntectStyle;
use syntect::highlighting::FontStyle as SyntectFontStyle;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::bad::App;
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

const BLUEISH: Color = Color::Rgb {  r: 0x4a, g: 0x54, b: 0x6e };
const DEFAULT_FG: Color = Color::White;
const DEFAULT_BG: Color = Color::Rgb { r: 0x1a, g: 0x1a, b: 0x1a };
const SELECTION_FG: Color = Color::Black;
const SELECTION_BG: Color = Color::Rgb { r: 0x88, g: 0xff, b: 0xc5 };
const LIGHT_GREY: Color = Color::Rgb { r: 0xaa, g: 0xaa, b: 0xaa };
const LIGHTER_BG: Color = Color::Rgb { r: 0x24, g: 0x24, b: 0x24 };

impl App {
    fn status_line_text_left(&self) -> &str {
        &self.current_pane().title
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

    pub fn render(&mut self, mut writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let wsize = crossterm::terminal::window_size()?;
        {
            let pane = self.current_pane_mut();
            pane.update_viewport_size(wsize.columns, wsize.rows.saturating_sub(2));
            pane.adjust_viewport();
        }

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

    fn render_content(&self, writer: &mut dyn std::io::Write, wsize: crossterm::terminal::WindowSize) -> std::io::Result<()> {
        let current_pane = &self.current_pane();
        let now = Instant::now();
        let content = &current_pane.content;
        let tab_width = current_pane.settings.tab_width;
        let primary_cursor_line = current_pane.cursors.primary().current_line_number(content);
        let default_style = ContentStyle::new().with(DEFAULT_FG).on(DEFAULT_BG);
        let sel_style = ContentStyle::new().with(SELECTION_FG).on(SELECTION_BG);
        let escaped_style = ContentStyle::new().with(DEFAULT_FG).on(BLUEISH);
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
        let mut n_selections = 0;

        let last_visible_lineno = current_pane.viewport_position_row + current_pane.viewport_height as usize;
        let max_lineno_width = {
            let mut n = content.len_lines();
            let mut w = 1;
            while n > 9 {
                n /= 10;
                w += 1;
            }
            w
        };
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

            let (visible_from, visible_to) = {
                let available_columns = (wsize.columns as usize).saturating_sub(max_lineno_width + 2);
                if UnicodeWidthStr::width(line.as_str()) > available_columns {
                    let primary_cursor = current_pane.cursors.primary();
                    if lineno == primary_cursor_line {
                        let visible_upto_global_offset = content.next_boundary_from(primary_cursor.offset).unwrap_or_else(|| ByteOffset(content.len_bytes()));
                        let visible_upto = visible_upto_global_offset.0 - byte_offset.0;
                        let mut required_columns = UnicodeWidthStr::width(&line[0..visible_upto]);
                        if required_columns > available_columns {
                            let mut visible_from = byte_offset;
                            for g in line.graphemes(true) {
                                if required_columns < available_columns {
                                    break
                                }
                                required_columns -= UnicodeWidthStr::width(g);
                                visible_from.0 += g.len();
                            }
                            (visible_from, visible_upto_global_offset)
                        } else {
                            let visible_upto = {
                                let mut width = 0;
                                let mut len_bytes = 0;
                                for g in line.graphemes(true) {
                                    width += UnicodeWidthStr::width(g);
                                    if width <= available_columns {
                                        len_bytes += g.len();
                                    } else {
                                        break
                                    }
                                }
                                ByteOffset(byte_offset.0 + len_bytes)
                            };
                            (ByteOffset(0), visible_upto)
                        }
                    } else {
                        let visible_upto = {
                            let mut width = 0;
                            let mut len_bytes = 0;
                            for g in line.graphemes(true) {
                                width += UnicodeWidthStr::width(g);
                                if width <= available_columns {
                                    len_bytes += g.len();
                                } else {
                                    break
                                }
                            }
                            ByteOffset(byte_offset.0 + len_bytes)
                        };
                        (ByteOffset(0), visible_upto)
                    }
                } else {
                    (ByteOffset(0), ByteOffset::MAX)
                }
            };

            let console_row = (lineno - current_pane.viewport_position_row) as u16;
            writer.queue(MoveTo(0, console_row))?;
            let left_scroll_indicator = if visible_from > byte_offset { '<' } else { ' ' };
            let sidebar = format!(" {:width$}{}", 1 + lineno, left_scroll_indicator, width=max_lineno_width);
            writer.queue(PrintStyledContent(lineno_style.apply(&sidebar)))?;

            for (style, s) in hl.highlight(&line) {
                let xtyle = to_crossterm_style(style);
                // visual_column = None means it's currently unknown
                let mut visual_column = Some(0);
                for g in s.graphemes(true) {
                    let mut is_cursor = false;
                    while peek!(curs) <= byte_offset {
                        match curs.peek() {
                            Some(Cur::Start(_)) => n_selections += 1,
                            Some(Cur::End(_)) => n_selections -= 1,
                            Some(Cur::NoSelection(b)) if b == &byte_offset => {
                                is_cursor = true;
                            }
                            _ => {}
                        }
                        curs.next();
                    }
                    if byte_offset < visible_from {
                        byte_offset.0 += g.len();
                        continue
                    }
                    if byte_offset >= visible_to {
                        byte_offset.0 += g.len();
                        continue
                    }
                    if g == "\t" {
                        // '\t' is variable width depending on current column!
                        let tab_width = match visual_column {
                            Some(n) => tab_width - (n % tab_width),
                            None => match crossterm::cursor::position() {
                                Ok((col, _row)) => {
                                    let cur_col = (col as usize).saturating_sub(sidebar.len());
                                    visual_column.replace(cur_col);
                                    tab_width - (cur_col % tab_width)
                                }
                                Err(_) => tab_width
                            }
                        };
                        if n_selections > 0 {
                            writer.queue(PrintStyledContent(sel_style.apply(" ".repeat(tab_width))))?;
                        } else if is_cursor {
                            // when cursor is placed before '\t' only show one space as cursor
                            // rather than the full width of the tab
                            writer.queue(PrintStyledContent(xtyle.reverse().apply(" ")))?;
                            writer.queue(PrintStyledContent(xtyle.apply(" ".repeat(tab_width - 1))))?;
                        } else {
                            writer.queue(PrintStyledContent(xtyle.apply(" ".repeat(tab_width))))?;
                        }
                        visual_column = visual_column.map(|offset| offset + tab_width);
                    } else if let Some(glyph) = unicode_line_break_symbol(g) {
                        if n_selections > 0 {
                            writer.queue(PrintStyledContent(sel_style.with(BLUEISH).apply(glyph)))?;
                        } else if is_cursor {
                            writer.queue(PrintStyledContent(xtyle.reverse().apply(" ")))?;
                        }
                    } else if g.len() == 1 && g.chars().next().is_some_and(|c| c.is_control()) {
                        let c = g.chars().next().unwrap();
                        let disp = format!("<{:02}>", c as u32);
                        let style =
                            if n_selections > 0 {
                                sel_style.with(BLUEISH)
                            } else if is_cursor {
                                escaped_style.reverse()
                            } else {
                                escaped_style
                            };
                        writer.queue(PrintStyledContent(style.apply(disp)))?;
                        visual_column = visual_column.map(|offset| offset + 4);
                    } else {
                        let styled =
                            if n_selections > 0 {
                                sel_style.apply(g)
                            } else if is_cursor {
                                xtyle.reverse().apply(g)
                            } else {
                                xtyle.apply(g)
                            };
                        writer.queue(PrintStyledContent(styled))?;
                        if g.len() == 1 {
                            visual_column = visual_column.map(|offset| offset + 1);
                        } else {
                            visual_column = None;
                        }
                    }

                    byte_offset.0 += g.len();
                }
                writer.queue(crossterm::style::SetStyle(default_style))?;
                writer.queue(Clear(ClearType::UntilNewLine))?;
            }
        }

        // render cursor at the end of the file
        if curs.peek().is_some() {
            writer.queue(PrintStyledContent(default_style.negative().apply(" ")))?;
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
            match &self.info {
                Some(info) => format!("{:.width$}", &info, width = wsize.columns as usize),
                None => format!("render took {:.3?}", now.elapsed()),
            }
        ))?;
        // this ensures prompt is printed in the right place!
        writer.queue(MoveTo(0, wsize.rows - 1))?;
        Ok(())
    }
}
