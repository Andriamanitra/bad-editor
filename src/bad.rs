use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ropey::Rope;
use unicode_segmentation::GraphemeCursor;
use unicode_segmentation::GraphemeIncomplete;

pub(crate) enum AppState {
    Idle,
    InPrompt,
}

#[derive(Default, Clone, Copy, Ord, PartialOrd, PartialEq, Eq)]
pub struct ByteOffset(pub usize);
impl ByteOffset {
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);
}

#[derive(Default)]
pub struct Cursor {
    pub(crate) offset: ByteOffset,
    pub(crate) visual_column: usize,
    pub(crate) selection_from: Option<ByteOffset>,
}

impl Cursor {
    pub fn deselect(&mut self) {
        self.selection_from = None;
    }

    // TODO: handle column offset using unicode_segmentation

    pub fn move_up(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line < n {
            self.offset = ByteOffset(0);
        } else {
            let line_start = content.line_to_byte(current_line - n);
            self.offset = ByteOffset(line_start);
        }
    }

    pub fn move_down(&mut self, content: &Rope, n: usize) {
        let current_line = content.byte_to_line(self.offset.0);
        if current_line + n > content.len_lines() {
            self.offset = ByteOffset(content.len_bytes());
        } else {
            let line_start = content.line_to_byte(current_line + n);
            self.offset = ByteOffset(line_start);
        }
    }

    pub fn move_left(&mut self, content: &Rope, n: usize) {
        for _ in 0..n {
            let b = self.current_grapheme_cluster_len_bytes(content);
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

    pub fn current_grapheme_cluster_len_bytes(&self, content: &Rope) -> usize {
        let mut gr = GraphemeCursor::new(self.offset.0, content.len_bytes(), true);
        let (mut chunk, mut chunk_byte_idx, _, _) = content.chunk_at_byte(self.offset.0);
        loop {
            match gr.next_boundary(chunk, chunk_byte_idx) {
                Ok(Some(n)) => return n - self.offset.0,
                Ok(None) => return 0,
                Err(GraphemeIncomplete::NextChunk) => {
                    (chunk, chunk_byte_idx, _, _) =
                        content.chunk_at_byte(chunk_byte_idx + chunk.len());
                }
                Err(GraphemeIncomplete::PreContext(idx)) => {
                    let (ctx_chunk, ctx_chunk_byte_idx, _, _) =
                        content.chunk_at_byte(idx.saturating_sub(1));
                    gr.provide_context(ctx_chunk, ctx_chunk_byte_idx);
                }
                Err(err) => unreachable!("{err:?} should never happen!"),
            }
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

pub struct Pane {
    pub(crate) title: String,
    pub(crate) content: Rope,
    pub(crate) cursors: Vec<Cursor>,
}

impl Pane {
    pub fn open_file(&mut self, path: &str) -> std::io::Result<()> {
        let file = std::fs::File::open(path)?;
        let content = Rope::from_reader(std::io::BufReader::new(file))?;
        self.title = path.to_string();
        self.content = content;
        self.cursors = vec![Cursor::default()];
        Ok(())
    }

    fn handle_event(&mut self, event: PaneAction) {
        match event {
            PaneAction::MoveCursorUp(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_up(&self.content, n);
                }
            }
            PaneAction::MoveCursorDown(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_down(&self.content, n);
                }
            }
            PaneAction::MoveCursorLeft(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_left(&self.content, n);
                }
            }
            PaneAction::MoveCursorRight(n) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_right(&self.content, n);
                }
            }
            PaneAction::Insert(s) => {
                self.cursors.sort_by_key(|cur| cur.offset);
                for cursor in self.cursors.iter_mut().rev() {
                    let char_idx = self.content.byte_to_char(cursor.offset.0);
                    self.content.insert(char_idx, &s);
                    cursor.offset = ByteOffset(cursor.offset.0 + s.len());
                }
            }
            PaneAction::DeletePreviousChar => {}
            PaneAction::DeleteNextChar => {}
        }
    }
}

pub struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) current_pane_index: usize,
    pub(crate) viewport_position_row: usize,
    // pub(crate) viewport_position_col: usize,
    pub(crate) info: Option<String>,
    pub(crate) state: AppState,
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
            state: AppState::Idle,
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
        if matches!(self.state, AppState::InPrompt) {
            return
        }
        match action {
            Action::None => (),
            Action::Quit => (),
            Action::CommandPrompt => {
                self.info.take();
                self.command_prompt();
            }
            // TODO: this shouldn't go to current pane
            Action::SetInfo(s) => self.info = Some(s),
            Action::HandledByPane(pa) => self.current_pane_mut().handle_event(pa),
        }
    }
}

pub enum Action {
    None,
    Quit,
    CommandPrompt,
    SetInfo(String),
    HandledByPane(PaneAction),
}
pub enum PaneAction {
    MoveCursorUp(usize),
    MoveCursorDown(usize),
    MoveCursorLeft(usize),
    MoveCursorRight(usize),
    Insert(String),
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
                KeyCode::Char('e') if ctrl => Action::CommandPrompt,
                KeyCode::Char(c) if only_shift => Action::HandledByPane(PaneAction::Insert(c.to_string())),
                KeyCode::Up => Action::HandledByPane(PaneAction::MoveCursorUp(1)),
                KeyCode::Down => Action::HandledByPane(PaneAction::MoveCursorDown(1)),
                KeyCode::Left => Action::HandledByPane(PaneAction::MoveCursorLeft(1)),
                KeyCode::Right => Action::HandledByPane(PaneAction::MoveCursorRight(1)),
                KeyCode::Enter => Action::HandledByPane(PaneAction::Insert("\n".into())),
                KeyCode::Backspace => Action::HandledByPane(PaneAction::DeletePreviousChar),
                KeyCode::Delete => Action::HandledByPane(PaneAction::DeleteNextChar),
                _ => Action::SetInfo(format!("{kevent:?}")),
            }
        }
    }
}
