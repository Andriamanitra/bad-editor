use std::io::ErrorKind;

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
    pub fn command_prompt_with(&mut self, stub: Option<String>) {
        self.state = AppState::InPrompt;
        if let Some((command, arg)) = get_command(stub) {
            match command.as_str() {
                "exit" | "quit" | "q" | ":q" => self.enqueue(Action::Quit),
                "find" => self.enqueue(Action::HandledByPane(PaneAction::Find(arg))),
                "goto" => {
                    if let Some(target) = parse_target(&arg) {
                        self.enqueue(Action::HandledByPane(PaneAction::MoveTo(target)));
                    } else {
                        self.inform(format!("goto error: {arg:?} is not a valid target"));
                    }
                }
                "lint" => {
                    self.current_pane_mut().lints.clear();
                    // TODO: pick linter based on file type
                    match crate::linter::run_linter_command("rust") {
                        Ok(lints_by_filename) => {
                            // TODO: add lints for panes other than the current one
                            for (fname, lints) in lints_by_filename.into_iter() {
                                if self.current_pane().path.as_ref().is_some_and(|p| p == &fname) {
                                    self.current_pane_mut().lints = lints;
                                }
                            }
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
                    match self.current_pane_mut().open_file(&FilePathWithOptionalLocation::parse_from_str(&arg), hl) {
                        Ok(()) => {},
                        Err(err) => {
                            let fpath = quote_path(&arg);
                            self.inform(match err.kind() {
                                ErrorKind::PermissionDenied => format!("Permission denied: {fpath}"),
                                ErrorKind::IsADirectory => format!("Can not open a directory: {fpath}"),
                                _ => format!("{err}: {fpath}"),
                            });
                        }
                    }
                },
                "save" => {
                    if arg.is_empty() {
                        self.enqueue(Action::HandledByPane(PaneAction::Save));
                    } else {
                        self.enqueue(Action::HandledByPane(PaneAction::SaveAs(arg.into())));
                    }
                }
                _ => self.inform(format!("Unknown command '{command}'")),
            }
        }
        self.state = AppState::Idle;
    }
}

pub fn get_command(stub: Option<String>) -> Option<(String, String)> {
    // TODO: add completions, and maybe get rid of Reedline dependency
    // once our own editing capabilities are up to the task?

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
    keybindings.add_binding(
        KeyModifiers::NONE,
        KeyCode::Tab,
        ReedlineEvent::UntilFound(vec![
            ReedlineEvent::Menu("completion_menu".into()),
            ReedlineEvent::MenuNext,
        ]),
    );

    let commands = vec![
        "exit".into(),
        "find".into(),
        "goto".into(),
        "insertchar".into(),
        "open".into(),
        "save".into(),
        "quit".into(),
    ];

    let completer = reedline::DefaultCompleter::new_with_wordlen(commands, 1);

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
        let (command, args) = cmd.split_once(' ').unwrap_or((&cmd, ""));
        Some((command.to_string(), args.to_string()))
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
