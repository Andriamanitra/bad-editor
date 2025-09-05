use std::collections::VecDeque;
use std::io::ErrorKind;
use std::sync::Arc;

use crate::cli::FilePathWithOptionalLocation;
use crate::clipboard::Clipboard;
use crate::highlighter::BadHighlighterManager;
use crate::prompt_completer::CmdCompleter;
use crate::{Action, Pane};

pub(crate) enum AppState {
    Idle,
    InPrompt,
}

pub struct App {
    pub(crate) panes: Vec<Pane>,
    pub(crate) current_pane_index: usize,
    pub(crate) state: AppState,
    pub(crate) action_queue: VecDeque<Action>,
    pub(crate) highlighting: Arc<BadHighlighterManager>,
    pub(crate) prompt_completer: CmdCompleter,
    pub(crate) clipboard: Clipboard,
    pub(crate) dirs: Option<directories::ProjectDirs>,
    info: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let highlighting = BadHighlighterManager::new();
        let prompt_completer = CmdCompleter::make_completer(highlighting.filetypes().as_slice());
        Self {
            panes: vec![Pane::empty()],
            current_pane_index: 0,
            state: AppState::Idle,
            action_queue: VecDeque::new(),
            highlighting: Arc::new(highlighting),
            prompt_completer,
            clipboard: Clipboard::new(),
            dirs: directories::ProjectDirs::from("", "Bad", "bad"),
            info: None,
        }
    }

    pub fn open_file_pane(&mut self, file_loc: &FilePathWithOptionalLocation) {
        let highlighting = self.highlighting.clone();
        if let Err(err) = self.current_pane_mut().open_file(file_loc, highlighting) {
            let fpath = crate::quote_path(file_loc.path.to_string_lossy().as_ref());
            self.current_pane_mut().inform(match err.kind() {
                ErrorKind::PermissionDenied => format!("Permission denied: {fpath}"),
                ErrorKind::IsADirectory => format!("Can not open a directory: {fpath}"),
                _ => format!("{err}: {fpath}"),
            });
        }
    }

    pub fn status_msg(&self) -> Option<&str> {
        match self.current_pane().status_msg() {
            Some(msg) => Some(msg),
            None => match self.info.as_ref() {
                Some(msg) => Some(msg),
                None => None,
            },
        }
    }

    pub fn clear_status_msg(&mut self) {
        self.info.take();
        for pane in self.panes.iter_mut() {
            pane.clear_status_msg();
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

    pub fn syntax_dir(&self) -> Option<std::path::PathBuf> {
        self.dirs.as_ref().map(|dirs| dirs.config_dir().join("syntaxes"))
    }

    pub fn prompt_history_file(&self) -> Option<std::path::PathBuf> {
        self.dirs.as_ref().map(|dirs| dirs.state_dir().unwrap_or_else(|| dirs.cache_dir()).join("history"))
    }

    pub fn set(&mut self, setting: &str, new_value: &str) {
        let new_value = new_value.trim();
        // TODO: we should make it impossible to have these not match prompt_completer
        match setting {
            "ft" | "ftype" => {
                let manager = self.highlighting.clone();
                if let Err(()) = self.current_pane_mut().set_filetype(new_value, manager) {
                    self.inform(format!("set error: {setting} must be one of {}", &self.highlighting.filetypes().join(", ")));
                }
            },
            "autoindent" => {
                self.current_pane_mut().settings.autoindent = match new_value {
                    "off" => crate::pane::AutoIndent::None,
                    "keep" => crate::pane::AutoIndent::Keep,
                    _ => {
                        self.inform("set error: autoindent must be one of: off, keep".into());
                        return
                    }
                }
            },
            "eol" => {
                self.current_pane_mut().settings.end_of_line = match new_value {
                    "lf" => "\n",
                    "crlf" => "\r\n",
                    "cr" => "\r",
                    _ => {
                        self.inform("set error: eol must be one of: lf, crlf, cr".into());
                        return
                    }
                }
            },
            "debug" => {
                match new_value {
                    "scopes" => self.current_pane_mut().settings.debug_scopes = true,
                    "off" => self.current_pane_mut().settings.debug_scopes = false,
                    _ => self.inform("set error: debug must be one of: scopes, off".into()),
                }
            }
            _ => {
                self.info.replace(format!("set error: '{setting}' is not a valid setting"));
            },
        }
    }

    pub fn load_runtime_syntaxes(&mut self) -> Option<()> {
        let syntax_dir = self.syntax_dir()?;
        if !syntax_dir.exists() {
            std::fs::DirBuilder::new().recursive(true).create(&syntax_dir).ok()?;
        }
        let (hl, result) = BadHighlighterManager::new_with_syntaxes_from_dir(&syntax_dir);
        if let Err(err) = result {
            self.inform(format!("{err}"));
            None
        } else {
            self.highlighting = Arc::new(hl);
            Some(())
        }
    }

    pub fn handle_action(&mut self, action: Action) {
        if matches!(self.state, AppState::InPrompt) {
            return
        }
        match action {
            Action::None => (),
            Action::Quit => (),
            Action::Esc => {
                self.current_pane_mut().esc();
                self.info.take();
            }
            Action::Resize(_columns, _rows) => {
                // this event is handled in App::run
            }
            Action::Command(cmd) => {
                self.handle_command(&cmd);
            }
            Action::CommandPrompt => {
                self.info.take();
                self.command_prompt_with(None, self.prompt_completer.clone());
            }
            Action::CommandPromptEdit(stub) => {
                self.info.take();
                self.command_prompt_with(Some(stub), self.prompt_completer.clone());
            }
            Action::SetInfo(s) => self.inform(s),
            Action::HandledByPane(pa) => self.current_pane_mut().handle_event(pa),
            Action::Copy => {
                self.clipboard.copy(self.current_pane().selections());
            }
            Action::Cut => {
                let cuts = self.current_pane_mut().cut();
                self.clipboard.copy(cuts);
            }
            Action::Paste => {
                let clips = self.clipboard.content().to_vec();
                self.current_pane_mut().insert_from_clipboard(&clips);
            }
            Action::Save => {
                self.current_pane_mut().save();
            }
            Action::SaveAs(path) => {
                let hl = self.highlighting.clone();
                self.current_pane_mut().save_as(&path, hl);
            }
            Action::Open(path) => {
                self.open_file_pane(&path);
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
