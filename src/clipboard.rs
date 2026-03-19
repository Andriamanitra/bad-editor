use std::default::Default;
use crossterm::clipboard::{ClipboardSelection, CopyToClipboard, ClipboardType};

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
        let destination = ClipboardSelection(vec![ClipboardType::Clipboard, ClipboardType::Primary]);
        let _ = crossterm::execute!(std::io::stdout(), CopyToClipboard { content, destination });
    }

    pub fn content(&self) -> &[String] {
        &self.clips
    }
}
