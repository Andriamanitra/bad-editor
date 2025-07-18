use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ropey::Rope;
use crate::cursor::Cursor;

pub(crate) enum AppState {
    Idle,
    InPrompt,
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
                    cursor.insert(&mut self.content, &s);
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
