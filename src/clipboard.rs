use std::error::Error;
use std::fmt::Display;

use crossterm::clipboard::CopyToClipboard;

#[derive(Debug)]
pub enum ClipboardError {
    ContentNotAvailable,
    NotSupported,
    Occupied,
    ConversionFailure,
    Unknown { description: String }
}
impl Display for ClipboardError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "clipboard error: {}",
            match self {
                ClipboardError::ContentNotAvailable => "content from external clipboard not available",
                ClipboardError::NotSupported => "external clipboard not supported",
                ClipboardError::Occupied => "external clipboard is occupied by another program",
                ClipboardError::ConversionFailure => "invalid format for external clipboard",
                ClipboardError::Unknown { description } => description,
            }
        )
    }
}

impl From<arboard::Error> for ClipboardError {
    fn from(value: arboard::Error) -> Self {
        match value {
            arboard::Error::ContentNotAvailable => ClipboardError::ContentNotAvailable,
            arboard::Error::ClipboardNotSupported => ClipboardError::NotSupported,
            arboard::Error::ClipboardOccupied => ClipboardError::Occupied,
            arboard::Error::ConversionFailure => ClipboardError::ConversionFailure,
            arboard::Error::Unknown { description } => ClipboardError::Unknown { description },
            _ => ClipboardError::Unknown { description: "not sure what happened, sorry :(".into() },
        }
    }
}

impl Error for ClipboardError {}

pub struct Clipboard {
    /// stores one String per cursor to enable multicursor paste
    clips: Vec<String>,
    external_clipboard: Result<arboard::Clipboard, arboard::Error>
}

impl Clipboard {
    pub fn new() -> Self {
        Self {
            clips: vec![],
            external_clipboard: arboard::Clipboard::new(),
        }
    }

    fn external(&mut self) -> Result<&mut arboard::Clipboard, ClipboardError> {
        self.external_clipboard.as_mut().map_err(|_| ClipboardError::NotSupported)
    }

    pub fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError> {
        self.clips = content;
        let s = self.clips.join("\n");
        // also copy to OSC52 clipboard to give the clip a better chance
        // of surviving the application exiting under wayland
        // https://whynothugo.nl/journal/2022/10/21/how-the-clipboard-works/
        let _ = crossterm::execute!(
            std::io::stdout(),
            CopyToClipboard::to_clipboard_from(&s),
            CopyToClipboard::to_primary_from(&s),
        );
        self.external()?.set_text(&s).map_err(ClipboardError::from)
    }

    pub fn update_from_external(&mut self) -> Result<(), ClipboardError> {
        let txt = self.external()?.get_text().map_err(ClipboardError::from)?;
        if txt != self.clips.join("\n") {
            self.clips = vec![txt];
        }
        Ok(())
    }

    pub fn content(&self) -> &[String] {
        &self.clips
    }
}
