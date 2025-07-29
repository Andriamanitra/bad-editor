use std::io::ErrorKind;

use nu_ansi_term::{Color, Style};
use reedline::DefaultPrompt;
use reedline::DefaultPromptSegment;
use reedline::Reedline;
use reedline::ReedlineEvent;
use reedline::EditCommand;
use reedline::KeyCode;
use reedline::KeyModifiers;
use reedline::MenuBuilder;

use crate::Action;
use crate::PaneAction;

/// Quotes strings with spaces, quotes, or control characters in them
/// Only intended to provide visual clarity, does NOT make the path shell-safe!
fn quote_path(s: &str) -> String {
    if s.is_empty() {
        return "''".to_string()
    }
    let mut single_quote = false;
    let mut double_quote = false;
    let mut space = false;
    let mut special = false;
    for c in s.chars() {
        match c {
            '\'' => single_quote = true,
            '"' => double_quote = true,
            ' ' => space = true,
            _ => if c.is_whitespace() || c.is_control() { special = true }
        }
    }
    if !special {
        if !single_quote && !double_quote && !space {
            return s.to_string()
        }
        if !single_quote {
            return format!("'{s}'")
        }
        if !double_quote {
            return format!("\"{s}\"")
        }
    }
    format!("{s:?}")
}

fn parse_insertchar(s: &str) -> Option<char> {
    if let Some(s_hexadecimal) = s.strip_prefix("U+") {
        u32::from_str_radix(s_hexadecimal, 16)
            .ok()
            .and_then(char::from_u32)
    } else if s.starts_with(|c: char| c.is_ascii_digit()) {
        s.parse::<u32>()
            .ok()
            .and_then(char::from_u32)
    } else if s.eq_ignore_ascii_case("zwj") {
        Some('\u{200d}')
    } else {
        unicode_names2::character(s)
    }
}

impl crate::bad::App {
    pub fn command_prompt_with(&mut self, stub: Option<String>) {
        self.state = crate::bad::AppState::InPrompt;
        if let Some((command, arg)) = get_command(stub) {
            match command.as_str() {
                "exit" | "quit" | "q" | ":q"  => self.enqueue(Action::Quit),
                "find" => self.enqueue(Action::HandledByPane(PaneAction::Find(arg))),
                "insertchar" | "c" => {
                    let mut out = String::new();
                    let mut success = true;
                    for req in arg.split(',') {
                        if let Some(c) = parse_insertchar(req.trim()) {
                            out.push(c);
                        } else {
                            success = false;
                            self.inform(format!("No character with name {:?}", req));
                            break
                        }
                    }
                    if success {
                        self.enqueue(Action::HandledByPane(PaneAction::Insert(out)))
                    }
                }
                "open" => match self.current_pane_mut().open_file(&arg) {
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
                _ => self.inform(format!("Unknown command '{command}'")),
            }
        }
        self.state = crate::bad::AppState::Idle;
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
        ])
    );


    let commands = vec![
        "exit".into(),
        "find".into(),
        "insertchar".into(),
        "open".into(),
        "quit".into(),
    ];

    let completer =
        reedline::DefaultCompleter::new_with_wordlen(commands, 1);

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
