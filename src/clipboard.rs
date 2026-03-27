use std::default::Default;
use crossterm::clipboard::CopyToClipboard;

#[derive(Default)]
pub struct InternalClipboard {
    clips: Vec<String>
}

impl InternalClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn copy(&mut self, content: Vec<String>) {
        self.clips = content;
        let content = self.content().join("\n");
        let _ = crossterm::execute!(std::io::stdout(), CopyToClipboard::to_clipboard_from(&content));
        let _ = crossterm::execute!(std::io::stdout(), CopyToClipboard::to_primary_from(&content));
    }

    pub fn content(&self) -> &[String] {
        &self.clips
    }
}
