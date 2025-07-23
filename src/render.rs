use crossterm;
use crossterm::{
    cursor::MoveTo,
    style::{Color, ContentStyle, Print, PrintStyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    QueueableCommand,
};
use syntect::highlighting::Style as SyntectStyle;
use syntect::highlighting::FontStyle as SyntectFontStyle;

use crate::bad::App;
use crate::ByteOffset;

fn to_crossterm_style(syntect_style: SyntectStyle, is_selection: bool) -> ContentStyle {
    let fg = {
        let syntect::highlighting::Color {r, g, b, ..} = syntect_style.foreground;
        Color::Rgb { r, g, b }
    };
    let bg = {
        let syntect::highlighting::Color {r, g, b, ..} = syntect_style.background;
        Color::Rgb { r, g, b }
    };
    let mut style = ContentStyle::new().with(fg).on(bg);
    if is_selection {
        style = style.negative()
    }
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

impl App {
    pub fn render(&mut self, mut writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let wsize = crossterm::terminal::window_size()?;
        {
            let pane = self.current_pane_mut();
            pane.update_viewport_size(wsize.columns, wsize.rows.saturating_sub(2));
            pane.adjust_viewport();
        }
        let current_pane = &self.current_pane();
        let content = &current_pane.content;
        let default_style = ContentStyle::new()
            .with(Color::White)
            .on(Color::Rgb { r: 0x1a, g: 0x1a, b: 0x1a });
        let lineno_style = ContentStyle::new()
            .with(Color::Rgb { r: 0xaa, g: 0xaa, b: 0xaa })
            .on(Color::Rgb { r: 0x24, g: 0x24, b: 0x24 });

        macro_rules! peek {
            ($it:expr) => {
                match $it.peek() {
                    Some(Cur::Start(b) | Cur::End(b)) => *b,
                    None => ByteOffset::MAX
                }
            }
        }

        #[derive(Copy, Clone, Debug)]
        enum Cur {
            Start(ByteOffset),
            End(ByteOffset),
        }

        let mut hl = self.highlighting.get_highlighter_for_file_ext("rb");
        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;

        writer.queue(crossterm::cursor::Hide)?;

        if wsize.rows < 3 {
            writer.queue(Print("window too smol"))?;
        } else {
            let mut curs = {
                let mut curs: Vec<Cur> = vec![];
                for cursor in self.current_pane().cursors.iter() {
                    curs.push(Cur::Start(cursor.visual_start_offset()));
                    curs.push(Cur::End(cursor.visual_end_offset(&content)));
                }
                curs.sort_unstable_by_key(|c| match c {
                    Cur::Start(b) | Cur::End(b) => *b
                });
                curs.into_iter().peekable()
            };

            let mut byte_offset = ByteOffset(0);
            let mut n_selections = 0;

            for (lineno, line) in content.lines().enumerate() {
                if lineno > current_pane.viewport_position_row + current_pane.viewport_height as usize {
                    break
                }
                let line = line.to_string();
                if lineno < current_pane.viewport_position_row {
                    byte_offset.0 += line.len();
                    for _ in hl.highlight(&line) {}
                    continue
                }

                let console_row = (lineno - current_pane.viewport_position_row) as u16;
                writer.queue(MoveTo(0, console_row as u16))?;
                writer.queue(PrintStyledContent(lineno_style.apply(format!("{:3} ", 1 + lineno))))?;

                while peek!(curs) <= byte_offset {
                    match curs.peek().unwrap() {
                        Cur::Start(_) => n_selections += 1,
                        Cur::End(_) => n_selections -= 1,
                    }
                    curs.next();
                }
                for (style, mut s) in hl.highlight(&line) {
                    macro_rules! print_fragment {
                        ($it:expr) => {
                            let xtyle = to_crossterm_style(style, n_selections > 0);
                            byte_offset.0 += $it.len();
                            if $it.ends_with('\n') {
                                writer.queue(PrintStyledContent(xtyle.apply($it.trim_end_matches('\n'))))?;
                                if n_selections > 0 {
                                    writer.queue(PrintStyledContent(xtyle.apply("⏎")))?;
                                }
                            } else {
                                writer.queue(PrintStyledContent(xtyle.apply($it)))?;
                            }
                        }
                    }

                    let token_end = ByteOffset(byte_offset.0 + s.len());
                    while peek!(curs) < token_end {
                        let length = peek!(curs).0 - byte_offset.0;
                        print_fragment!(&s[..length]);
                        s = &s[length..];

                        match curs.peek().unwrap() {
                            Cur::Start(_) => n_selections += 1,
                            Cur::End(_) => n_selections -= 1,
                        }
                        curs.next();
                    }
                    print_fragment!(&s);
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
            let status_line_title = format!("{:width$}", self.current_pane().title, width = width);
            writer.queue(PrintStyledContent(default_style.negative().apply(status_line_title)))?;
            let cursor = &self.current_pane().cursors[0];
            let status_line_right = format!(
                "col:{:<3} line:{:<3} {}/{}B",
                1 + cursor.column(&content),
                1 + content.byte_to_line(cursor.offset.0),
                cursor.offset.0,
                content.len_bytes()
            );
            writer.queue(MoveTo((width - status_line_right.len()) as u16, wsize.rows - 2))?;
            writer.queue(PrintStyledContent(default_style.negative().apply(status_line_right)))?;

            if let Some(info) = &self.info {
                writer.queue(MoveTo(0, wsize.rows - 1))?;
                writer.queue(Print(format!("{:.width$}", &info, width = wsize.columns as usize)))?;
            }
            // this ensures prompt is printed in the right place!
            writer.queue(MoveTo(0, wsize.rows - 1))?;
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }
}
