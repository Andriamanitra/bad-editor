use std::io::{BufReader, ErrorKind, Read};
use std::collections::VecDeque;

use crate::Action;
use crate::MultiCursor;
use crate::IndentKind;
use crate::PaneAction;
use crate::highlighter::BadHighlighterManager;
use crate::ropebuffer::RopeBuffer;


pub(crate) enum AppState {
    Idle,
    InPrompt,
}

#[derive(Debug)]
pub struct PaneSettings {
    pub tab_width: usize,
    pub indent_kind: IndentKind
}

impl std::default::Default for PaneSettings {
    fn default() -> Self {
        PaneSettings {
            tab_width: 4,
            indent_kind: IndentKind::default()
        }
    }
}

pub struct Pane {
    pub(crate) title: String,
    pub(crate) content: RopeBuffer,
    pub(crate) viewport_position_row: usize,
    pub(crate) viewport_width: u16,
    pub(crate) viewport_height: u16,
    pub(crate) cursors: MultiCursor,
    pub(crate) settings: PaneSettings
}

impl Pane {
    pub fn open_file(&mut self, path: &str) -> std::io::Result<()> {
        let content = match std::fs::File::open(path) {
            Ok(file) => {
                // TODO: do something more efficient than this
                let mut s = String::new();
                BufReader::new(file).read_to_string(&mut s)?;
                RopeBuffer::from_str(&s)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => RopeBuffer::new(),
            Err(err) => return Err(err)
        };
        self.title = path.to_string();
        self.content = content;
        self.cursors = MultiCursor::new();
        Ok(())
    }

    pub fn update_viewport_size(&mut self, columns: u16, rows: u16) {
        self.viewport_width = columns;
        self.viewport_height = rows;
    }

    pub fn adjust_viewport(&mut self) {
        let line_number = self.cursors.primary().current_line_number(&self.content);
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
                self.cursors.move_to(&self.content, target);
            }
            PaneAction::SelectTo(target) => {
                self.cursors.select_to(&self.content, target);
            }
            PaneAction::Insert(s) => {
                self.content.insert_with_cursors(&mut self.cursors, &s);
            }
            PaneAction::DeleteBackward => {
                self.content.delete_backward_with_cursors(&mut self.cursors);
            }
            PaneAction::DeleteForward => {
                self.content.delete_forward_with_cursors(&mut self.cursors);
            }
            PaneAction::Indent => {
                self.content.indent_with_cursors(&mut self.cursors, self.settings.indent_kind);
            }
            PaneAction::Dedent => {
                self.content.dedent_with_cursors(&mut self.cursors, self.settings.indent_kind);
            }
            PaneAction::Undo => self.cursors = self.content.undo(self.cursors.clone()),
            PaneAction::Redo => self.cursors = self.content.redo(self.cursors.clone()),
        }
    }
}

pub struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) current_pane_index: usize,
    pub(crate) info: Option<String>,
    pub(crate) state: AppState,
    pub(crate) action_queue: VecDeque<Action>,
    pub(crate) highlighting: BadHighlighterManager,
}

impl App {
    pub fn new() -> Self {
        let pane = Pane {
            title: "bad.txt".to_string(),
            content: RopeBuffer::new(),
            cursors: MultiCursor::new(),
            viewport_position_row: 0,
            // these will be set during rendering
            viewport_height: 0,
            viewport_width: 0,
            settings: PaneSettings::default(),
        };

        Self {
            panes: vec![pane],
            current_pane_index: 0,
            info: None,
            state: AppState::Idle,
            action_queue: VecDeque::new(),
            highlighting: BadHighlighterManager::new()
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
            Action::Esc => {
                self.current_pane_mut().cursors.esc();
                self.info.take();
            }
            Action::CommandPrompt => {
                self.info.take();
                self.command_prompt_with(None);
            }
            Action::CommandPromptEdit(stub) => {
                self.info.take();
                self.command_prompt_with(Some(stub));
            }
            Action::SetInfo(s) => self.inform(s),
            Action::HandledByPane(pa) => self.current_pane_mut().handle_event(pa),
        }
    }
}
