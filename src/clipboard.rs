use crossterm::clipboard::CopyToClipboard;

pub struct Clipboard {
    /// stores one String per cursor to enable multicursor paste
    internal: Vec<String>,
}

impl Clipboard {
    pub fn new() -> Self {
        Self { internal: vec![] }
    }

    pub fn copy(&mut self, content: Vec<String>) {
        let s = content.join("\n");
        self.internal = content;

        // TODO: copying to external clipboard through crossterm only works
        // on terminals that support OSC52
        let _ = crossterm::execute!(
            std::io::stdout(),
            CopyToClipboard::to_clipboard_from(&s),
            CopyToClipboard::to_primary_from(&s),
        );
    }

    pub fn content(&self) -> &[String] {
        // TODO: paste from external clipboard?
        &self.internal
    }
}
