use crossterm::{
    cursor::{self, MoveTo},
    event::{self, KeyCode, KeyEvent, KeyModifiers},
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal::{BeginSynchronizedUpdate, Clear, ClearType, EndSynchronizedUpdate},
    QueueableCommand,
};
use ropey::Rope;
use unicode_segmentation::GraphemeCursor;
use unicode_segmentation::GraphemeIncomplete;

#[derive(Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
struct ByteOffset(usize);
impl ByteOffset {
    const MAX: ByteOffset = ByteOffset(usize::MAX);
}

#[derive(Default)]
pub struct Cursor {
    offset: ByteOffset,
    visual_column: usize,
    selection_from: Option<ByteOffset>,
}
impl Cursor {
    fn deselect(&mut self) {
        self.selection_from = None;
    }

    // TODO: handle column offset using unicode_segmentation

    fn move_up(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line < n {
            self.offset = ByteOffset(0);
        } else {
            let line_start = content.line_to_byte(current_line - n);
            self.offset = ByteOffset(line_start);
        }
    }

    fn move_down(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line + n > content.len_lines() {
            self.offset = ByteOffset(content.len_bytes());
        } else {
            let line_start = content.line_to_byte(current_line + n);
            self.offset = ByteOffset(line_start);
        }
    }

    fn move_left(&mut self, content: &Rope, n: usize) {
        for _ in 0..n {
            let b = self.current_grapheme_cluster_len_bytes(content);
            self.offset = ByteOffset(self.offset.0.saturating_sub(b));
        }
    }

    fn move_right(&mut self, content: &Rope, n: usize) {
        for _ in 0..n {
            if self.offset < ByteOffset(content.len_bytes()) {
                let b = self.current_grapheme_cluster_len_bytes(content);
                self.offset = ByteOffset(self.offset.0 + b);
            }
        }
    }

    fn current_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        let mut gr = GraphemeCursor::new(self.offset.0, content.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = content.chunk_at_byte(self.offset.0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return n - self.offset.0,
                Ok(None) => return 0,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) = content.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) = content.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!")
            }
        }
    }

    fn visual_start_offset(&self) -> ByteOffset {
        match self.selection_from {
            None => self.offset,
            Some(selection_from) => self.offset.min(selection_from)
        }
    }

    fn visual_end_offset(&self, content: &Rope) -> ByteOffset {
        match self.selection_from {
            None => ByteOffset(self.offset.0 + self.current_grapheme_cluster_len_bytes(content)),
            Some(selection_from) => self.offset.max(selection_from)
        }
    }
}

pub struct Pane {
    title: String,
    content: Rope,
    cursors: Vec<Cursor>,
}

impl Pane {
    fn handle_event(&mut self, event: PaneAction) {
        match event {
            PaneAction::MoveCursorUp(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_up(&self.content, n);
                }
            },
            PaneAction::MoveCursorDown(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_down(&self.content, n);
                }
            },
            PaneAction::MoveCursorLeft(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_left(&self.content, n);
                }
            },
            PaneAction::MoveCursorRight(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_right(&self.content, n);
                }
            },
            PaneAction::Insert(s) => {
                self.cursors.sort_by_key(|cur| cur.offset);
                for cursor in self.cursors.iter_mut().rev() {
                    let char_idx = self.content.byte_to_char(cursor.offset.0);
                    self.content.insert(char_idx, &s);
                    cursor.offset = ByteOffset(cursor.offset.0 + s.len());
                }
            },
            PaneAction::InsertNewLine => {},
            PaneAction::DeletePreviousChar => {},
            PaneAction::DeleteNextChar => {},
        }
    }
}

pub struct App {
    panes: Vec<Pane>,
    current_pane_index: usize,
    viewport_position_row: usize,
    // viewport_position_col: usize,
    info: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let pane = Pane {
            title: "bad.txt".to_string(),
            content: Rope::from("bad is the best text editor\n\n".repeat(100)),
            cursors: vec![Cursor::default()],
        };

        Self {
            panes: vec![pane],
            current_pane_index: 0,
            viewport_position_row: 0,
            // viewport_position_col: 0,
            info: None,
        }
    }

    pub fn current_pane_mut(&mut self) -> &mut Pane {
        self.panes
            .get_mut(self.current_pane_index)
            .expect("there should always be a pane at current_pane_index")
    }

    pub fn current_pane(&self) -> &Pane {
        self.panes
            .get(self.current_pane_index)
            .expect("there should always be a pane at current_pane_index")
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::None => (),
            Action::Quit => (),
            // TODO: this shouldn't go to current pane
            Action::SetInfo(s) => self.info = Some(s),
            Action::HandledByPane(pa) => self.current_pane_mut().handle_event(pa),
        }
    }

    pub fn render(&self, mut writer: &mut dyn std::io::Write) -> std::io::Result<()> {
        let wsize = crossterm::terminal::window_size()?;

        crossterm::execute!(&mut writer, BeginSynchronizedUpdate)?;

        writer.queue(Clear(ClearType::All))?;
        writer.queue(cursor::Hide)?;

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

            let last_visible_lineno = content.len_lines().min(self.viewport_position_row + wsize.rows as usize - 2);
            for lineno in self.viewport_position_row .. last_visible_lineno {
                let console_row = (lineno - self.viewport_position_row) as u16;
                writer.queue(MoveTo(0, console_row as u16))?;
                writer.queue(PrintStyledContent(format!("{:3} ", 1 + lineno).with(Color::DarkGrey).on(Color::Black)))?;
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
                    let s = content.slice(byte_offset.0 .. cur_start.min(cur_end).0);
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
                    let s = content.slice(byte_offset.0 .. line_end.0);
                    writer.queue(Print(s.to_string().trim_end_matches('\n')))?;
                    byte_offset = line_end;
                }
                if n_selections > 0 {
                    writer.queue(Print("⏎"))?;
                }
            }
            writer.queue(MoveTo(0, wsize.rows - 2))?;
            let status_line = format!("{:width$}", self.current_pane().title, width = wsize.columns as usize);
            writer.queue(PrintStyledContent(status_line.with(Color::Black).on(Color::White)))?;
            writer.queue(MoveTo(0, wsize.rows - 1))?;
            writer.queue(Print(">"))?;
        }
        writer.flush()?;

        crossterm::execute!(&mut writer, EndSynchronizedUpdate)?;
        Ok(())
    }
}

pub enum Action {
    None,
    Quit,
    SetInfo(String),
    HandledByPane(PaneAction),
}
pub enum PaneAction {
    MoveCursorUp(usize),
    MoveCursorDown(usize),
    MoveCursorLeft(usize),
    MoveCursorRight(usize),
    Insert(String),
    InsertNewLine,
    DeletePreviousChar,
    DeleteNextChar,
}

pub fn get_action(ev: &event::Event) -> Action {
    use event::Event::*;
    match ev {
        FocusGained => Action::None,
        FocusLost => Action::None,
        Resize(_, _) => Action::None,
        Mouse(_) => todo!(),
        Paste(_) => todo!(),
        Key(
            kevent @ KeyEvent {
                code,
                modifiers,
                kind: _,
                state: _,
            },
        ) => {
            let ctrl = modifiers.contains(KeyModifiers::CONTROL);
            let only_shift = (*modifiers - KeyModifiers::SHIFT).is_empty();
            // TODO: no hard coding, read keybindings from a config file
            match code {
                KeyCode::Char('q') if ctrl => Action::Quit,
                KeyCode::Char(c) if only_shift => Action::HandledByPane(PaneAction::Insert(c.to_string())),
                KeyCode::Up => Action::HandledByPane(PaneAction::MoveCursorUp(1)),
                KeyCode::Down => Action::HandledByPane(PaneAction::MoveCursorDown(1)),
                KeyCode::Left => Action::HandledByPane(PaneAction::MoveCursorLeft(1)),
                KeyCode::Right => Action::HandledByPane(PaneAction::MoveCursorRight(1)),
                KeyCode::Enter => Action::HandledByPane(PaneAction::InsertNewLine),
                KeyCode::Backspace => Action::HandledByPane(PaneAction::DeletePreviousChar),
                KeyCode::Delete => Action::HandledByPane(PaneAction::DeleteNextChar),
                _ => Action::SetInfo(format!("{kevent:?}")),
            }
        }
    }
}
