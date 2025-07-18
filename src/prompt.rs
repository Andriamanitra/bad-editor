use reedline::DefaultPrompt;
use reedline::DefaultPromptSegment;
use reedline::Reedline;

impl crate::bad::App {
    pub fn command_prompt(&mut self) {
        self.state = crate::bad::AppState::InPrompt;
        match get_command() {
            Some((command, args)) if command == "open" => {
                if let Err(err) = self.current_pane_mut().open_file(&args) {
                    self.info = Some(format!("{err}"));
                }
            }
            Some((command, _args)) => {
                self.info = Some(format!("Unknown command '{command}'"));
            }
            _ => {}
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
