use std::error::Error;
use std::fmt::Display;

pub trait Clipboard {
    fn new() -> Self;
    fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError>;
    fn update_from_external(&mut self) -> Result<(), ClipboardError>;
    fn content(&self) -> &[String];
}

#[derive(Debug)]
pub enum ClipboardError {
    ContentNotAvailable,
    NotSupported,
    Occupied,
    ConversionFailure,
    Unknown { description: String }
}

impl Error for ClipboardError {}

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

#[cfg(not(target_os = "android"))]
pub mod arboard {
    use crate::clipboard::Clipboard;
    use super::ClipboardError;

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

    pub struct ArboardClipboard {
        /// stores one String per cursor to enable multicursor paste
        clips: Vec<String>,
        external_clipboard: Result<arboard::Clipboard, arboard::Error>
    }

    impl ArboardClipboard {
        fn external(&mut self) -> Result<&mut arboard::Clipboard, ClipboardError> {
            self.external_clipboard.as_mut().map_err(|_| ClipboardError::NotSupported)
        }
    }

    impl Clipboard for ArboardClipboard {
        fn new() -> Self {
            Self {
                clips: vec![],
                external_clipboard: arboard::Clipboard::new(),
            }
        }

        fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError> {
            self.clips = content;
            let s = self.clips.join("\n");
            self.external()?.set_text(&s).map_err(ClipboardError::from)
        }

        fn update_from_external(&mut self) -> Result<(), ClipboardError> {
            let txt = self.external()?.get_text().map_err(ClipboardError::from)?;
            if txt != self.clips.join("\n") {
                self.clips = vec![txt];
            }
            Ok(())
        }

        fn content(&self) -> &[String] {
            &self.clips
        }
    }
}

// termux module is currently only used on Android builds
#[allow(dead_code)]
pub mod termux {
    use crate::clipboard::Clipboard;
    use crate::clipboard::ClipboardError;

    pub struct TermuxClipboard {
        clips: Vec<String>
    }

    impl Clipboard for TermuxClipboard {
        fn new() -> Self {
            Self { clips: vec![] }
        }

        fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError> {
            self.clips = content;
            let s = self.clips.join("\n");
            let status = std::process::Command::new("termux-clipboard-set")
                .arg(&s)
                .status()
                .map_err(|_| ClipboardError::Unknown {
                    description: "unable to run termux-clipboard-get (is termux-api installed?)".to_string()
                })?;
            match status.code() {
                Some(0) => Ok(()),
                Some(code) => Err(ClipboardError::Unknown {
                    description: format!("termux-clipboard-set exited with {code}")
                }),
                None => Err(ClipboardError::Unknown {
                    description: "termux-clipboard-set was terminated by a signal".to_string()
                }),
            }
        }

        fn update_from_external(&mut self) -> Result<(), ClipboardError> {
            let out = std::process::Command::new("termux-clipboard-get")
                .output()
                .map_err(|_| ClipboardError::Unknown {
                    description: "unable to run termux-clipboard-get (is termux-api installed?)".to_string()
                })?;
            match out.status.code() {
                Some(0) => {
                    match String::from_utf8(out.stdout) {
                        Ok(txt) => {
                            self.clips = vec![txt];
                            Ok(())
                        }
                        Err(_) => Err(ClipboardError::ConversionFailure),
                    }
                },
                Some(code) => Err(ClipboardError::Unknown {
                    description: format!("termux-clipboard-get exited with {code}")
                }),
                None => Err(ClipboardError::Unknown {
                    description: "termux-clipboard-set was terminated by a signal".to_string()
                }),
            }
        }

        fn content(&self) -> &[String] {
            &self.clips
        }
    }
}
