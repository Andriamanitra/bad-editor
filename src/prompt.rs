use nu_ansi_term::{Color, Style};
use reedline::{
    DefaultPrompt,
    DefaultPromptSegment,
    EditCommand,
    FileBackedHistory,
    KeyCode,
    KeyModifiers,
    MenuBuilder,
    Reedline,
    ReedlineEvent,
};

use crate::app::AppState;
use crate::cli::FilePathWithOptionalLocation;
use crate::exec::execute_interactive_command_from_template;
use crate::prompt_completer::CmdCompleter;
use crate::{Action, App, MoveTarget, PaneAction};


fn parse_insertchar(s: &str) -> Option<char> {
    if let Some(s_hexadecimal) = s.strip_prefix("U+") {
        u32::from_str_radix(s_hexadecimal, 16).ok().and_then(char::from_u32)
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.parse::<u32>().ok().and_then(char::from_u32)
    } else {
        unicode_names2::character(s)
    }
}

fn parse_target(s: &str) -> Option<MoveTarget> {
    if let Some(s) = s.strip_prefix("B") {
        let offset = s.parse().ok()?;
        Some(MoveTarget::ByteOffset(offset))
    } else if let Some((line, col)) = s.split_once(":") {
        let line = line.parse().ok()?;
        let col = col.parse().ok()?;
        Some(MoveTarget::Location(line, col))
    } else {
        let line = s.parse().ok()?;
        Some(MoveTarget::Location(line, std::num::NonZero::<usize>::MIN))
    }
}

impl App {
    pub fn handle_command(&mut self, s: &str) {
        self.clear_status_msg();
        if let Some(shell_command) = s.strip_prefix("|") {
            self.current_pane_mut().pipe_through_shell_command(shell_command);
            return
        }
        let (command, arg) = s.split_once(' ').unwrap_or((s, ""));
        match command {
            "exit" | "quit" | "q" | ":q" => self.enqueue(Action::Quit),
            "find" => self.enqueue(Action::HandledByPane(PaneAction::Find(arg.to_string()))),
            "goto" => {
                if let Some(target) = parse_target(arg) {
                    self.enqueue(Action::HandledByPane(PaneAction::MoveTo(target)));
                } else {
                    self.inform(format!("goto error: {arg:?} is not a valid target"));
                }
            }
            "to" => {
                if let Some(reps) = arg.strip_prefix('*').and_then(|n| n.parse::<usize>().ok()) {
                    self.current_pane_mut().transform_selections(|s| Some(s.repeat(reps)));
                } else if arg == "upper" {
                    self.current_pane_mut().transform_selections(|s| Some(s.to_uppercase()));
                } else if arg == "lower" {
                    self.current_pane_mut().transform_selections(|s| Some(s.to_lowercase()));
                } else if arg == "list" {
                    self.current_pane_mut().transform_selections(|s| {
                        let v = s.split_ascii_whitespace().collect::<Vec<_>>();
                        Some(format!("[{}]", v.join(", ")))
                    });
                } else if arg == "quoted" {
                    self.current_pane_mut().transform_selections(|s| {
                        let mut transformed = String::new();
                        let mut in_word = false;
                        for c in s.chars() {
                            if c.is_ascii_whitespace() {
                                if in_word {
                                    transformed.push('"');
                                }
                                transformed.push(c);
                                in_word = false;
                            } else {
                                if !in_word {
                                    transformed.push('"');
                                }
                                if c == '"' || c == '\\' {
                                    transformed.push('\\');
                                }
                                transformed.push(c);
                                in_word = true;
                            }
                        }
                        if in_word {
                            transformed.push('"');
                        }
                        Some(transformed)
                    });
                } else {
                    self.inform(format!("to error: {arg:?} is not a valid transformation"));
                }
            }
            "exec" | "x" => {
                let arg = arg.trim();

                let template = if !arg.is_empty() {
                    arg
                } else {
                    let ft = self.current_pane().filetype();
                    // TODO: these should come from a config file
                    match ft {
                        "bash" => "bash %f",
                        "c" => "zig run -lc %f",
                        "c#" => "dotnet run %f",
                        "haskell" => "runhaskell %f",
                        "html" => "xdg-open %f",
                        "janet" => "janet %f",
                        "js" => "node %f",
                        "lua" => "lua %f",
                        "python" => "uv run %f",
                        "ruby" => "ruby %f",
                        "rust" => "cargo run",
                        _ => {
                            self.inform(format!("exec error: no exec command for ft:{ft}"));
                            return
                        }
                    }
                };

                let fpath = match &self.current_pane().path {
                    None if template.contains("%f") => {
                        self.inform("exec error: file needs to be saved".into());
                        return
                    }
                    Some(path) => path,
                    None => std::path::Path::new(""),
                };

                match execute_interactive_command_from_template(template, fpath) {
                    Ok(()) => {}
                    Err(err) => self.inform(format!("{err}"))
                }
            }
            "lint" => {
                if self.current_pane().modified {
                    self.inform("lint error: save your changes before linting".into());
                    return
                }
                self.current_pane_mut().lints.clear();
                // TODO: run the linter asynchronously in the background
                let fname = self.current_pane().path.as_ref().and_then(|p| p.to_str());
                let ft = self.current_pane().filetype();
                match crate::linter::run_linter_command(fname, ft) {
                    Ok(mut lints_by_filename) => {
                        for pane in self.panes.iter_mut() {
                            if let Some(path) = &pane.path {
                                if let Some(lints) = lints_by_filename.remove(path) {
                                    if let Some(first_error_loc) = lints
                                        .iter()
                                        .find_map(|lint| if lint.is_error() { lint.location() } else { None })
                                    {
                                        pane.cursors.esc();
                                        pane.cursors.primary_mut().move_to(&pane.content, first_error_loc);
                                        pane.adjust_viewport();
                                    }
                                    pane.inform(format!("linted - {} lint(s) in current file", lints.len()));
                                    pane.lints = lints;
                                }
                            }
                        }
                        self.inform("linted".into());
                    }
                    Err(err) => {
                        self.inform(err.to_string());
                    }
                }
            }
            "insertchar" | "c" => {
                let mut out = String::new();
                let mut success = true;
                for req in arg.split(',') {
                    if let Some(c) = parse_insertchar(req.trim()) {
                        out.push(c);
                    } else {
                        success = false;
                        self.inform(format!("No character with name {req:?}"));
                        break
                    }
                }
                if success {
                    self.enqueue(Action::HandledByPane(PaneAction::Insert(out)))
                }
            }
            "open" => {
                let path = FilePathWithOptionalLocation::parse_from_str(arg, true);
                self.enqueue(Action::Open(path));
            }
            "set" => {
                if let Some((key, value)) = arg.trim_start().split_once(' ') {
                    self.set(key, value);
                } else {
                    self.inform("set error: correct usage is 'set KEY VALUE'".into());
                }
            }
            "save" => {
                if arg.is_empty() {
                    self.enqueue(Action::Save);
                } else {
                    self.enqueue(Action::SaveAs(crate::expand_path(arg)));
                }
            }
            _ => self.inform(format!("Unknown command '{command}'")),
        }
    }

    pub fn command_prompt_with(&mut self, stub: Option<String>, completer: CmdCompleter) {
        self.state = AppState::InPrompt;
        let history = self.prompt_history_file()
            .and_then(|hist_file| FileBackedHistory::with_file(100, hist_file).ok())
            .unwrap_or_else(|| FileBackedHistory::new(100).expect("creating in-memory history should never fail"));
        if let Some(s) = get_command(stub, completer, history) {
            self.handle_command(&s);
        }
        self.state = AppState::Idle;
    }
}

struct BadHinter {
    style: Style,
    hinter: reedline::DefaultHinter
}

impl BadHinter {
    fn with_style(style: Style) -> Self {
        Self {
            style,
            hinter: reedline::DefaultHinter::default().with_style(style)
        }
    }
}

impl reedline::Hinter for BadHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        history: &dyn reedline::History,
        use_ansi_coloring: bool,
        cwd: &str,
    ) -> String {
        let hint = self.hinter.handle(line, pos, history, use_ansi_coloring, cwd);
        if line.is_empty() {
            let hint = "  Press Tab to browse commands";
            if use_ansi_coloring {
                self.style.paint(hint).to_string()
            } else {
                hint.to_string()
            }
        } else {
            hint
        }
    }

    fn complete_hint(&self) -> String {
        self.hinter.complete_hint()
    }

    fn next_hint_token(&self) -> String {
        self.hinter.next_hint_token()
    }
}

fn get_command(stub: Option<String>, completer: CmdCompleter, history: FileBackedHistory) -> Option<String> {
    macro_rules! edits {
        ( $( $x:expr ),* $(,)? ) => {
            ReedlineEvent::Edit(vec![ $( $x ),* ])
        };
    }

    let mut keybindings = reedline::default_emacs_keybindings();

    let cancel = ReedlineEvent::Multiple(vec![edits![EditCommand::Clear], ReedlineEvent::Submit]);
    keybindings.add_binding(KeyModifiers::NONE, KeyCode::Esc, cancel.clone());
    keybindings.add_binding(KeyModifiers::NONE, KeyCode::Enter, ReedlineEvent::Submit);
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('e'), cancel.clone());
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('q'), cancel.clone());
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('d'), cancel.clone());
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('c'), edits![EditCommand::CopySelection]);
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('x'), edits![EditCommand::CutSelection]);
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('y'), edits![EditCommand::Redo]);
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('v'), edits![EditCommand::Paste]);
    keybindings.add_binding(KeyModifiers::CONTROL, KeyCode::Char('a'), edits![EditCommand::SelectAll]);
    keybindings.add_binding(KeyModifiers::ALT, KeyCode::Char('t'), edits![EditCommand::SwapWords]);
    keybindings.add_binding(KeyModifiers::SHIFT, KeyCode::BackTab, ReedlineEvent::MenuPrevious);
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".into()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let completion_menu =
        reedline::ReedlineMenu::EngineCompleter(
            Box::new(reedline::ColumnarMenu::default().with_name("completion_menu"))
        );

    let hinter = BadHinter::with_style(Style::new().fg(Color::Rgb(75, 75, 75)));

    let mut ed = Reedline::create()
        .with_completer(Box::new(completer))
        .with_partial_completions(true)
        .with_quick_completions(true)
        .with_menu(completion_menu)
        .with_history(Box::new(history))
        .with_edit_mode(Box::new(reedline::Emacs::new(keybindings)))
        .with_hinter(Box::new(hinter))
        .use_kitty_keyboard_enhancement(true);
    if let Some(stub) = stub {
        ed.run_edit_commands(&[EditCommand::InsertString(stub)]);
    }

    let prompt = DefaultPrompt {
        left_prompt: DefaultPromptSegment::Empty,
        right_prompt: DefaultPromptSegment::WorkingDirectory,
    };
    if let Ok(reedline::Signal::Success(cmd)) = ed.read_line(&prompt) {
        if cmd.is_empty() {
            return None
        }
        Some(cmd)
    } else {
        None
    }
}
