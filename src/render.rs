use crate::bad::App;
use crate::cursor::ByteOffset;

use crossterm;
use crossterm::{
    cursor::MoveTo,
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    QueueableCommand,
};

impl App {
    pub fn render(&self, mut writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let wsize = crossterm::terminal::window_size()?;

        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;

        writer.queue(Clear(ClearType::All))?;
        writer.queue(crossterm::cursor::Hide)?;

        if wsize.rows < 3 {
            writer.queue(Print("window too smol"))?;
        } else {
            let content = &self.current_pane().content;
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

            let mut byte_offset = ByteOffset(content.line_to_byte(self.viewport_position_row));
            let mut starts_idx = 0;
            let mut ends_idx = 0;
            let mut n_selections = 0;

            let last_visible_lineno = content
                .len_lines()
                .min(self.viewport_position_row + wsize.rows as usize - 2);
            for lineno in self.viewport_position_row..last_visible_lineno {
                let console_row = (lineno - self.viewport_position_row) as u16;
                writer.queue(MoveTo(0, console_row as u16))?;
                writer.queue(PrintStyledContent(
                    format!("{:3} ", 1 + lineno)
                        .with(Color::DarkGrey)
                        .on(Color::Black),
                ))?;
                if n_selections == 0 {
                    writer.queue(crossterm::style::SetForegroundColor(Color::White))?;
                    writer.queue(crossterm::style::SetBackgroundColor(Color::Black))?;
                } else {
                    writer.queue(crossterm::style::SetForegroundColor(Color::Black))?;
                    writer.queue(crossterm::style::SetBackgroundColor(Color::White))?;
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
                    let s = content.slice(byte_offset.0..cur_start.min(cur_end).0);
                    writer.queue(Print(s.to_string().trim_end_matches('\n')))?;
                    if cur_start < cur_end {
                        byte_offset = cur_start;
                        starts_idx += 1;
                        cur_start = match cursor_starts.get(starts_idx) {
                            Some(x) => *x,
                            None => ByteOffset::MAX,
                        };
                        n_selections += 1;
                        if n_selections == 1 {
                            writer.queue(crossterm::style::SetForegroundColor(Color::Black))?;
                            writer.queue(crossterm::style::SetBackgroundColor(Color::White))?;
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
                            writer.queue(crossterm::style::SetForegroundColor(Color::White))?;
                            writer.queue(crossterm::style::SetBackgroundColor(Color::Black))?;
                        }
                    }
                }
                if byte_offset < line_end {
                    let s = content.slice(byte_offset.0..line_end.0);
                    writer.queue(Print(s.to_string().trim_end_matches('\n')))?;
                    byte_offset = line_end;
                }
                if n_selections > 0 {
                    writer.queue(Print("⏎"))?;
                }
            }
            writer.queue(MoveTo(0, wsize.rows - 2))?;
            let status_line = format!(
                "{:width$}",
                self.current_pane().title,
                width = wsize.columns as usize
            );
            writer.queue(PrintStyledContent(
                status_line.with(Color::Black).on(Color::White),
            ))?;
            if let Some(info) = &self.info {
                writer.queue(MoveTo(0, wsize.rows - 1))?;
                writer.queue(Print(info))?;
            }
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }
}
