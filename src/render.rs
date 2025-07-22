use crossterm;
use crossterm::{
    cursor::MoveTo,
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    QueueableCommand,
};

use crate::bad::App;
use crate::ByteOffset;

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

        macro_rules! slice {
            ($range:expr) => {
                content.byte_slice($range.start.0 .. $range.end.0)
            };
        }

        macro_rules! set_colors {
            ($fg:expr, $bg:expr) => {
                writer.queue(crossterm::style::SetForegroundColor($fg))?;
                writer.queue(crossterm::style::SetBackgroundColor($bg))?;
            }
        }

        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;

        writer.queue(crossterm::cursor::Hide)?;

        if wsize.rows < 3 {
            writer.queue(Print("window too smol"))?;
        } else {
            let (cursor_starts, cursor_ends) = {
                let mut starts: Vec<ByteOffset> = vec![];
                let mut ends: Vec<ByteOffset> = vec![];
                for cursor in self.current_pane().cursors.iter() {
                    starts.push(cursor.visual_start_offset());
                    ends.push(cursor.visual_end_offset(&content));
                }
                starts.sort_unstable();
                ends.sort_unstable();
                (starts, ends)
            };

            let mut byte_offset = ByteOffset(content.line_to_byte(self.current_pane().viewport_position_row));
            let mut starts_idx = 0;
            let mut ends_idx = 0;
            let mut n_selections = 0;

            let last_visible_lineno = content
                .len_lines()
                .min(current_pane.viewport_position_row + wsize.rows as usize - 2);
            for lineno in current_pane.viewport_position_row..last_visible_lineno {
                let console_row = (lineno - current_pane.viewport_position_row) as u16;
                writer.queue(MoveTo(0, console_row as u16))?;
                writer.queue(PrintStyledContent(
                    format!("{:3} ", 1 + lineno)
                        .with(Color::DarkGrey)
                        .on(Color::Black),
                ))?;
                if n_selections == 0 {
                    set_colors!(Color::White, Color::Black);
                } else {
                    set_colors!(Color::Black, Color::White);
                }

                let line_end = ByteOffset(content.line_to_byte(lineno + 1));
                let mut cur_start = match cursor_starts.get(starts_idx) {
                    Some(x) => *x,
                    None => ByteOffset::MAX,
                };
                let mut cur_end = match cursor_ends.get(ends_idx) {
                    Some(x) => *x,
                    None => ByteOffset::MAX,
                };

                while cur_start < line_end || cur_end < line_end {
                    let s = slice!(byte_offset..cur_start.min(cur_end));
                    for c in s.chars().take_while(|&c| c != '\n') {
                        writer.queue(Print(c))?;
                    }
                    if cur_start < cur_end {
                        byte_offset = cur_start;
                        starts_idx += 1;
                        cur_start = match cursor_starts.get(starts_idx) {
                            Some(x) => *x,
                            None => ByteOffset::MAX,
                        };
                        n_selections += 1;
                        if n_selections == 1 {
                            set_colors!(Color::Black, Color::White);
                        }
                    } else {
                        byte_offset = cur_end;
                        ends_idx += 1;
                        cur_end = match cursor_ends.get(ends_idx) {
                            Some(x) => *x,
                            None => ByteOffset::MAX,
                        };
                        n_selections -= 1;
                        if n_selections == 0 {
                            set_colors!(Color::White, Color::Black);
                        }
                    }
                }
                if byte_offset < line_end {
                    let s = slice!(byte_offset..line_end);
                    for c in s.chars().take_while(|&c| c != '\n') {
                        writer.queue(Print(c))?;
                    }
                    byte_offset = line_end;
                }
                if n_selections > 0 && lineno + 1 < content.len_lines() {
                    writer.queue(Print("⏎"))?;
                }
                set_colors!(Color::White, Color::Black);
                writer.queue(Clear(ClearType::UntilNewLine))?;
            }
            // render cursor at the end of the file
            if starts_idx < cursor_starts.len() {
                set_colors!(Color::Black, Color::White);
                writer.queue(Print(" "))?;
            }
            set_colors!(Color::White, Color::Black);
            writer.queue(Clear(ClearType::FromCursorDown))?;

            writer.queue(MoveTo(0, wsize.rows - 2))?;
            set_colors!(Color::Black, Color::White);
            let width = wsize.columns as usize;
            let status_line_title = format!("{:width$}", self.current_pane().title, width = width);
            writer.queue(Print(status_line_title))?;
            let cursor = &self.current_pane().cursors[0];
            let status_line_right = format!(
                "col:{:<3} line:{:<3} {}/{}B",
                1 + cursor.column(&content),
                1 + content.byte_to_line(cursor.offset.0),
                cursor.offset.0,
                content.len_bytes()
            );
            writer.queue(MoveTo((width - status_line_right.len()) as u16, wsize.rows - 2))?;
            writer.queue(Print(status_line_right))?;

            set_colors!(Color::White, Color::Black);
            if let Some(info) = &self.info {
                writer.queue(MoveTo(0, wsize.rows - 1))?;
                writer.queue(Print(info))?;
            }
            // this ensures prompt is printed in the right place!
            writer.queue(MoveTo(0, wsize.rows - 1))?;
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }
}
