use reedline::DefaultPrompt;
use reedline::DefaultPromptSegment;
use reedline::Reedline;
use std::io::ErrorKind;

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

impl crate::bad::App {
    pub fn command_prompt(&mut self) {
        self.state = crate::bad::AppState::InPrompt;
        let Some((command, arg)) = get_command() else { return };
        match command.as_str() {
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
        self.state = crate::bad::AppState::Idle;
    }
}

pub fn get_command() -> Option<(String, String)> {
    let mut ed = Reedline::create();
    let prompt = DefaultPrompt {
        left_prompt: DefaultPromptSegment::Empty,
        right_prompt: DefaultPromptSegment::WorkingDirectory,
    };
    if let Ok(reedline::Signal::Success(cmd)) = ed.read_line(&prompt) {
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
