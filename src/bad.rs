use std::io::{BufReader, ErrorKind};

use crossterm::event::{self, KeyCode, KeyEvent, KeyModifiers};
use ropey::Rope;

use crate::Cursor;
use crate::cursor::MoveTarget;

pub(crate) enum AppState {
    Idle,
    InPrompt,
}

pub struct Pane {
    pub(crate) title: String,
    pub(crate) content: Rope,
    pub(crate) viewport_position_row: usize,
    pub(crate) viewport_width: u16,
    pub(crate) viewport_height: u16,
    pub(crate) cursors: Vec<Cursor>,
}

impl Pane {
    pub fn open_file(&mut self, path: &str) -> std::io::Result<()> {
        let content = match std::fs::File::open(path) {
            Ok(file) => Rope::from_reader(BufReader::new(file))?,
            Err(err) if err.kind() == ErrorKind::NotFound => Rope::new(),
            Err(err) => return Err(err)
        };
        self.title = path.to_string();
        self.content = content;
        self.cursors = vec![Cursor::default()];
        Ok(())
    }

    pub fn update_viewport_size(&mut self, columns: u16, rows: u16) {
        self.viewport_width = columns;
        self.viewport_height = rows;
    }

    pub fn adjust_viewport(&mut self) {
        // assume the first cursor is the primary one for now
        let mut line_number = 0;
        for cursor in self.cursors.iter().take(1) {
            line_number = cursor.current_line_number(&self.content);
        }
        self.adjust_viewport_to_show_line(line_number);
    }

    fn adjust_viewport_to_show_line(&mut self, line_number: usize) {
        let pad = 2;
        let vh = self.viewport_height as usize;
        let last_visible_line_number = self.viewport_position_row + vh;
        if line_number < self.viewport_position_row + pad {
            self.viewport_position_row = line_number.saturating_sub(pad);
        } else if line_number >= last_visible_line_number.saturating_sub(pad) {
            let desired_last_visible_line_number = (line_number + pad + 1).min(self.content.len_lines());
            self.viewport_position_row = desired_last_visible_line_number.saturating_sub(vh);
        }
    }

    fn handle_event(&mut self, event: PaneAction) {
        match event {
            PaneAction::MoveTo(target) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                    cursor.move_to(&self.content, target);
                }
            }
            PaneAction::SelectTo(target) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.select_to(&self.content, target);
                }
            }
            PaneAction::Insert(s) => {
                self.cursors.sort_by_key(|cur| cur.offset);
                for cursor in self.cursors.iter_mut().rev() {
                    cursor.insert(&mut self.content, &s);
                }
            }
            PaneAction::DeleteBackward => {
                self.cursors.sort_by_key(|cur| cur.offset);
                for cursor in self.cursors.iter_mut().rev() {
                    cursor.delete_backward(&mut self.content);
                }
            }
            PaneAction::DeleteForward => {
                self.cursors.sort_by_key(|cur| cur.offset);
                for cursor in self.cursors.iter_mut().rev() {
                    cursor.delete_forward(&mut self.content);
                }
            }
        }
    }
}

pub struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) current_pane_index: usize,
    pub(crate) info: Option<String>,
    pub(crate) state: AppState,
}

impl App {
    pub fn new() -> Self {
        let pane = Pane {
            title: "bad.txt".to_string(),
            content: Rope::from("bad is the bäst text editor\n\n".repeat(15) + "ääää"),
            cursors: vec![Cursor::default()],
            viewport_position_row: 0,
            // these will be set during rendering
            viewport_height: 0,
            viewport_width: 0,
        };

        Self {
            panes: vec![pane],
            current_pane_index: 0,
            info: None,
            state: AppState::Idle,
        }
    }

    pub fn inform(&mut self, msg: String) {
        self.info.replace(msg);
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
            Action::SetInfo(s) => self.inform(s),
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
    MoveTo(MoveTarget),
    SelectTo(MoveTarget),
    Insert(String),
    DeleteBackward,
    DeleteForward,
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
            let shift = modifiers.contains(KeyModifiers::SHIFT);
            let only_shift = (*modifiers - KeyModifiers::SHIFT).is_empty();
            // TODO: no hard coding, read keybindings from a config file
            match code {
                KeyCode::Char('q') if ctrl => Action::Quit,
                KeyCode::Char('e') if ctrl => Action::CommandPrompt,
                KeyCode::Char(c) if only_shift => Action::HandledByPane(PaneAction::Insert(c.to_string())),
                KeyCode::Up =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Up(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Up(1))) },
                KeyCode::Down =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Down(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Down(1))) },
                KeyCode::Left =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Left(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Left(1))) },
                KeyCode::Right =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::Right(1))) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::Right(1))) },
                KeyCode::Home =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::LineStart)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::LineStart)) },
                KeyCode::End =>
                    if shift { Action::HandledByPane(PaneAction::SelectTo(MoveTarget::LineEnd)) }
                    else     { Action::HandledByPane(PaneAction::MoveTo(MoveTarget::LineEnd)) },
                KeyCode::Enter => Action::HandledByPane(PaneAction::Insert("\n".into())),
                KeyCode::Backspace => Action::HandledByPane(PaneAction::DeleteBackward),
                KeyCode::Delete => Action::HandledByPane(PaneAction::DeleteForward),
                _ => Action::SetInfo(format!("{kevent:?}")),
            }
        }
    }
}
