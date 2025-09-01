use std::io::ErrorKind;
use std::process::Command;

use nu_ansi_term::{Color, Style};
use reedline::{
    DefaultPrompt,
    DefaultPromptSegment,
    EditCommand,
    KeyCode,
    KeyModifiers,
    MenuBuilder,
    Reedline,
    ReedlineEvent,
};

use crate::app::AppState;
use crate::cli::FilePathWithOptionalLocation;
use crate::{Action, App, MoveTarget, PaneAction, quote_path};

pub enum Arg {
    String,
    File,
    OneOf(Vec<&'static str>),
}

#[derive(Default)]
pub struct Cmd {
    prefixes: Vec<&'static str>,
    args: Vec<Arg>,
    help: &'static str,
}

impl Cmd {
    fn has_alias(&self, alias: &str) -> bool {
        self.prefixes.contains(&alias)
    }

    fn n_args(&self) -> usize{
        self.args.len()
    }

    fn primary_name(&self) -> &'static str {
        self.prefixes[0]
    }
}

pub struct CmdBuilder {
    cmd: Cmd,
}

impl CmdBuilder {
    pub fn new(prefix: &'static str) -> Self {
        Self { cmd: Cmd { prefixes: vec![prefix], ..Default::default() } }
    }

    pub fn alias(mut self, prefix: &'static str) -> Self {
        self.cmd.prefixes.push(prefix);
        self
    }

    pub fn arg(mut self, a: Arg) -> Self {
        self.cmd.args.push(a);
        self
    }

    pub fn help(mut self, help: &'static str) -> Self {
        self.cmd.help = help;
        self
    }

    pub fn build(self) -> Cmd {
        self.cmd
    }
}

struct CmdCompleter {
    cmds: Vec<Cmd>
}

impl CmdCompleter {
    fn new(cmds: Vec<Cmd>) -> CmdCompleter {
        CmdCompleter { cmds }
    }
}

impl reedline::Completer for CmdCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<reedline::Suggestion> {
        let input = &line[..pos];
        let mut parts: Vec<&str> = input.split_ascii_whitespace().collect();
        if parts.is_empty() || input.ends_with(|c: char| c.is_ascii_whitespace()) {
            parts.push("");
        }

        let first = parts[0];
        let Some(cmd) = self.cmds.iter().find(|cmd| cmd.has_alias(first)) else {
            return self.cmds.iter()
                .filter(|cmd| cmd.primary_name().starts_with(first))
                .map(|cmd|
                    reedline::Suggestion {
                        value: cmd.primary_name().to_string(),
                        description: Some(cmd.help.to_string()),
                        extra: None,
                        style: None,
                        span: reedline::Span { start: 0, end: pos },
                        append_whitespace: cmd.n_args() > 0,
                    }
                )
                .collect()
        };

        let Some(arg_index) = parts.len().checked_sub(2) else {
            // when completing a valid command with nothing after it we want to
            // a) turn alias into the primary name (eg. ":q" -> "quit")
            // b) insert a space if the command takes args, eg. "open" -> "open "
            return vec![reedline::Suggestion {
                value: cmd.primary_name().to_string(),
                span: reedline::Span { start: 0, end: pos },
                append_whitespace: cmd.n_args() > 0,
                ..Default::default()
            }]
        };

        let Some(arg) = cmd.args.get(arg_index) else {
            return vec![]
        };

        let arg_prefix = parts.last().unwrap_or(&"");

        match arg {
            Arg::String => vec![],
            Arg::OneOf(choices) => choices.iter()
                .filter(|s| s.starts_with(arg_prefix))
                .map(|s| reedline::Suggestion {
                    value: s.to_string(),
                    description: None,
                    extra: None,
                    style: None,
                    span: reedline::Span { start: pos - arg_prefix.len(), end: pos },
                    append_whitespace: cmd.n_args() > arg_index + 1,
                })
                .collect(),
            Arg::File => {
                let mut suggestions = Vec::new();

                let (dir, file_prefix) = match arg_prefix.rsplit_once('/') {
                    Some((dir, file_prefix)) => (dir, file_prefix),
                    None if arg_prefix == &"~" => ("~", ""),
                    None => (".", *arg_prefix),
                };

                let dir = crate::expand_path(dir);

                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.starts_with(file_prefix) {
                                let mut val = if arg_prefix == &"~" {
                                    format!("/{name}")
                                } else {
                                    name.to_string()
                                };
                                if entry.file_type().is_ok_and(|e| e.is_dir()) {
                                    val.push('/');
                                }
                                suggestions.push(reedline::Suggestion {
                                    value: val,
                                    description: None,
                                    extra: None,
                                    style: None,
                                    span: reedline::Span {
                                        start: pos - file_prefix.len(),
                                        end: pos,
                                    },
                                    append_whitespace: false,
                                });
                            }
                        }
                    }
                }

                suggestions
            }
        }
    }
}


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
            "ex" | "exec" | "execute" => {
                // TODO: support args
                fn get_command_for_file(fpath: &std::path::Path, filetype: &str) -> Option<Command> {
                    // TODO: these should come from a config file
                    match filetype {
                        "bash" => {
                            let mut command = Command::new("bash");
                            command.arg(fpath);
                            Some(command)
                        }
                        "haskell" => {
                            let mut command = Command::new("runhaskell");
                            command.arg(fpath);
                            Some(command)
                        }
                        "python" => {
                            let mut command = Command::new("uv");
                            command.arg("run");
                            command.arg(fpath);
                            Some(command)
                        }
                        "ruby" => {
                            let mut command = Command::new("ruby");
                            command.arg(fpath);
                            Some(command)
                        }
                        "rust" => {
                            let mut command = Command::new("cargo");
                            command.arg("run");
                            Some(command)
                        }
                        _ => None,
                    }
                }
                if let Some(fpath) = &self.current_pane().path {
                    let ft = self.current_pane().filetype();
                    if let Some(command) = get_command_for_file(fpath, ft) {
                        let _ = crate::exec::execute_interactive_command(command);
                    } else {
                        self.inform(format!("exec error: no exec command for ft:{ft}"));
                    }
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
                                    pane.inform(format!("linted ({} lint(s) in current file)", lints.len()));
                                    pane.lints = lints;
                                }
                            }
                        }
                        self.inform("linted".into());
                    }
                    Err(err) => {
                        self.inform(format!("linter error: {err:?}"));
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
                let hl = self.highlighting.clone();
                match self.current_pane_mut().open_file(&FilePathWithOptionalLocation::parse_from_str(arg, true), hl) {
                    Ok(()) => {},
                    Err(err) => {
                        let fpath = quote_path(arg);
                        self.inform(match err.kind() {
                            ErrorKind::PermissionDenied => format!("Permission denied: {fpath}"),
                            ErrorKind::IsADirectory => format!("Can not open a directory: {fpath}"),
                            _ => format!("{err}: {fpath}"),
                        });
                    }
                }
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
                    self.enqueue(Action::HandledByPane(PaneAction::Save));
                } else {
                    self.enqueue(Action::HandledByPane(PaneAction::SaveAs(crate::expand_path(arg))));
                }
            }
            _ => self.inform(format!("Unknown command '{command}'")),
        }
    }

    pub fn command_prompt_with(&mut self, stub: Option<String>) {
        self.state = AppState::InPrompt;
        if let Some(s) = get_command(stub) {
            self.handle_command(&s);
        }
        self.state = AppState::Idle;
    }
}

pub fn get_command(stub: Option<String>) -> Option<String> {
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

    let completer = CmdCompleter::new(vec![
        CmdBuilder::new("exec").alias("execute")
            .help("exec")
            .build(),
        CmdBuilder::new("find").arg(Arg::String)
            .help("find STR")
            .build(),
        CmdBuilder::new("goto").arg(Arg::String)
            .help("goto LINE[:COL]")
            .build(),
        CmdBuilder::new("insertchar").alias("c").arg(Arg::String)
            .help("insertchar CODEPOINT[, CODEPOINT]...")
            .build(),
        CmdBuilder::new("lint")
            .help("lint")
            .build(),
        CmdBuilder::new("open").arg(Arg::File)
            .help("open FILE")
            .build(),
        CmdBuilder::new("save").arg(Arg::File)
            .help("save [FILE]")
            .build(),
        CmdBuilder::new("set").arg(Arg::OneOf(vec!["ftype", "autoindent", "eol"])).arg(Arg::String)
            .help("set KEY VALUE")
            .build(),
        CmdBuilder::new("quit").alias(":q").alias("exit").alias("q")
            .help("quit")
            .build(),
        CmdBuilder::new("to").arg(Arg::OneOf(vec!["lower", "upper", "quoted", "list"]))
            .help("to (lower|upper|quoted|list)")
            .build(),
    ]);

    let completion_menu =
        reedline::ReedlineMenu::EngineCompleter(
            Box::new(reedline::ColumnarMenu::default().with_name("completion_menu"))
        );

    // TODO: follow XDG spec
    let history = {
        let home = std::env::var("HOME").expect("$HOME should always be defined");
        let hist_path = format!("{home}/.local/state/bad/history");
        reedline::FileBackedHistory::with_file(100, hist_path.into())
            .expect("configuring history should be fine")
    };

    let hinter =
        reedline::DefaultHinter::default()
            .with_style(Style::new().fg(Color::Rgb(75, 75, 75)));

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_string() {
        assert_eq!(quote_path(""), "''");
    }

    #[test]
    fn test_no_special_chars() {
        assert_eq!(quote_path("file.txt"), "file.txt");
    }

    #[test]
    fn test_with_space() {
        assert_eq!(quote_path("my file.txt"), "'my file.txt'");
    }

    #[test]
    fn test_with_special_char() {
        assert_eq!(quote_path("file\n.txt"), "\"file\\n.txt\"");
    }

    #[test]
    fn test_with_single_quote_only() {
        assert_eq!(quote_path("file's.txt"), "\"file's.txt\"");
    }

    #[test]
    fn test_with_double_quote_only() {
        assert_eq!(quote_path("file\"name.txt"), "'file\"name.txt'");
    }

    #[test]
    fn test_with_both_quotes() {
        assert_eq!(quote_path("he said: \"don't\""), "\"he said: \\\"don't\\\"\"");
    }
}
