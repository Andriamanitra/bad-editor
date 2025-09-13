use std::collections::VecDeque;
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
            panes: vec![],
            current_pane_index: 0,
            state: AppState::Idle,
            action_queue: VecDeque::new(),
            highlighting: Arc::new(highlighting),
            prompt_completer,
            clipboard: Clipboard::new(),
            dirs: None,
            info: None,
        }
    }

    pub fn set_project_dirs(&mut self) {
        self.dirs = directories::ProjectDirs::from("", "Bad", "bad");
    }

    pub(crate) fn switch_to_new_pane(&mut self, pane: Pane) {
        self.panes.push(pane);
        self.current_pane_index = self.panes.len() - 1;
    }

    fn create_pane_from_file(&mut self, file_loc: &FilePathWithOptionalLocation) -> Pane {
        let highlighting = self.highlighting.clone();
        Pane::new_from_file(file_loc, highlighting)
    }

    fn confirm_saved(&mut self) -> bool {
        if self.current_pane().modified && self.current_pane().path.is_some() {
            if let Ok(wsize) = crossterm::terminal::window_size() {
                let _ = crossterm::execute!(
                    std::io::stdout(),
                    crossterm::cursor::MoveTo(0, wsize.height - 1)
                );
            }
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::style::Print("save changes to file before closing? (y)es / (n)o / (a)bort")
            );
            use crossterm::event::{Event, KeyEvent, KeyCode};
            loop {
                let event = crossterm::event::read();
                if let Ok(Event::Key(KeyEvent { code, .. })) = event {
                    match code {
                        KeyCode::Char('Y' | 'y') => {
                            self.current_pane_mut().save();
                            return true
                        }
                        KeyCode::Char('N' | 'n') => return true,
                        KeyCode::Char('A' | 'a') => return false,
                        KeyCode::Esc => return false,
                        _ => {}
                    }
                }
            }
        } else {
            true
        }
    }

    pub fn open_file_in_new_pane(&mut self, file_loc: &FilePathWithOptionalLocation) -> &mut Pane {
        let pane = self.create_pane_from_file(file_loc);
        self.switch_to_new_pane(pane);
        let i = self.panes.len() - 1;
        &mut self.panes[i]
    }

    pub fn open_file_in_current_pane(&mut self, file_loc: &FilePathWithOptionalLocation) {
        if self.confirm_saved() {
            let pane = self.create_pane_from_file(file_loc);
            self.panes[self.current_pane_index] = pane;
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

    pub fn linter_script_file(&self) -> Option<std::path::PathBuf> {
        self.dirs.as_ref().map(|dirs| dirs.config_dir().join("linters.janet"))
    }

    pub fn set(&mut self, setting: &str, new_value: &str) {
        let new_value = new_value.trim();
        // TODO: we should make it impossible to have these not match prompt_completer
        match setting {
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
            "debug" => {
                match new_value {
                    "scopes" => self.current_pane_mut().settings.debug_scopes = true,
                    "off" => self.current_pane_mut().settings.debug_scopes = false,
                    _ => self.inform("set error: debug must be one of: scopes, off".into()),
                }
            }
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
            "ft" | "ftype" => {
                let manager = self.highlighting.clone();
                if let Err(()) = self.current_pane_mut().set_filetype(new_value, manager) {
                    self.inform(format!("set error: {setting} must be one of {}", &self.highlighting.filetypes().join(", ")));
                }
            },
            "indent_size" => {
                match new_value.parse() {
                    Ok(n) if n <= 32 => {
                        self.current_pane_mut().settings.indent_size = n;
                        self.current_pane_mut().settings.tab_width = n;
                    }
                    _ => {
                        self.inform("set error: indent_size must be a number between 0 and 32".into());
                    }
                }
            }
            "indent_style" => {
                self.current_pane_mut().settings.indent_kind = match new_value {
                    "spaces" => crate::IndentKind::Spaces,
                    "tabs" => crate::IndentKind::Tabs,
                    _ => {
                        self.inform("set error: indent_style must be one of: spaces, tabs".into());
                        return
                    }
                }
            }
            "insert_final_newline" => {
                self.current_pane_mut().settings.insert_final_newline = match new_value {
                    "on" => true,
                    "off" => false,
                    _ => {
                        self.inform("set error: insert_final_newline must be one of: on, off".into());
                        return
                    }
                }
            }
            "normalize_end_of_line" => {
                self.current_pane_mut().settings.normalize_end_of_line = match new_value {
                    "on" => true,
                    "off" => false,
                    _ => {
                        self.inform("set error: normalize_end_of_line must be one of: on, off".into());
                        return
                    }
                }
            }
            "trim_trailing_whitespace" => {
                self.current_pane_mut().settings.trim_trailing_whitespace = match new_value {
                    "on" => true,
                    "off" => false,
                    _ => {
                        self.inform("set error: trim_trailing_whitespace must be one of: on, off".into());
                        return
                    }
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
            self.prompt_completer = CmdCompleter::make_completer(self.highlighting.filetypes().as_slice());
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
                if let Err(err) = self.clipboard.copy(self.current_pane().selections()) {
                    self.inform(err.to_string());
                }
            }
            Action::Cut => {
                let cuts = self.current_pane_mut().cut();
                if let Err(err) = self.clipboard.copy(cuts) {
                    self.inform(err.to_string())
                }
            }
            Action::Paste => {
                if let Err(err) = self.clipboard.update_from_external() {
                    self.inform(err.to_string());
                }
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
                self.open_file_in_current_pane(&path);
            }
            Action::NewPane => {
                self.panes.push(Pane::empty());
                self.current_pane_index = self.panes.len() - 1;
            }
            Action::ClosePane => {
                if self.panes.len() > 1 {
                    if self.confirm_saved() {
                        self.panes.remove(self.current_pane_index);
                        self.current_pane_index = self.current_pane_index.saturating_sub(1);
                    }
                } else {
                    self.current_pane_mut().inform("the last pane can not be closed".into());
                }
            }
            Action::GoToPane(idx) => {
                if idx < self.panes.len() {
                    self.current_pane_index = idx;
                } else {
                    self.inform(format!("there is no pane {}", idx + 1));
                }
            }
            Action::NextPane => {
                if self.current_pane_index + 1 < self.panes.len() {
                    self.current_pane_index += 1;
                }
            }
            Action::PreviousPane => {
                if self.current_pane_index > 0 {
                    self.current_pane_index -= 1;
                }
            }
        }
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
