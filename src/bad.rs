use std::collections::VecDeque;

use crate::Action;
use crate::Pane;
use crate::clipboard::Clipboard;
use crate::highlighter::BadHighlighterManager;


pub(crate) enum AppState {
    Idle,
    InPrompt,
}

pub struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) current_pane_index: usize,
    pub(crate) info: Option<String>,
    pub(crate) state: AppState,
    pub(crate) action_queue: VecDeque<Action>,
    pub(crate) highlighting: BadHighlighterManager,
    pub(crate) clipboard: Clipboard,
}

impl App {
    pub fn new() -> Self {
        let pane = Pane::empty();

        Self {
            panes: vec![pane],
            current_pane_index: 0,
            info: None,
            state: AppState::Idle,
            action_queue: VecDeque::new(),
            highlighting: BadHighlighterManager::new(),
            clipboard: Clipboard::new(),
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
            Action::Copy => {
                self.clipboard.copy(self.current_pane().selections());
            }
            Action::Paste => {
                let clips = self.clipboard.content().to_vec();
                self.current_pane_mut().insert_from_clipboard(&clips);
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
