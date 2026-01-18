use std::error::Error;
use std::fmt::Display;

pub trait Clipboard {
    fn new() -> Self;
    fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError>;
    fn update_from_external(&mut self) -> Result<(), ClipboardError>;
    fn content(&self) -> &[String];
}

#[derive(Debug)]
#[non_exhaustive]  // adding new error variants is not a breaking change
#[allow(dead_code)]  // some variants may never be constructed on certain platforms
pub enum ClipboardError {
    ContentNotAvailable,
    NotSupported,
    Occupied,
    ConversionFailure,
    TermuxApiError(&'static str),
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
                ClipboardError::TermuxApiError(msg) => msg,
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

    const TIMEOUT: std::time::Duration = std::time::Duration::from_millis(800);

    pub struct TermuxClipboard {
        clips: Vec<String>
    }

    impl Clipboard for TermuxClipboard {
        fn new() -> Self {
            Self { clips: vec![] }
        }

        fn copy(&mut self, content: Vec<String>) -> Result<(), ClipboardError> {
            self.clips = content;
            let termux_clipboard_set = duct::cmd!("termux-clipboard-set", self.clips.join("\n"))
                .start()
                .map_err(|_| ClipboardError::TermuxApiError("unable to run termux-clipboard-set (is termux-api installed?)"))?;
            match termux_clipboard_set.wait_timeout(TIMEOUT) {
                Ok(Some(_)) => Ok(()),
                Ok(None) => {
                    termux_clipboard_set.kill().expect("killing termux-clipboard-set should never fail");
                    Err(ClipboardError::TermuxApiError("termux-clipboard-set timed out (is Termux:API installed?)"))
                }
                Err(err) => Err(ClipboardError::Unknown { description: err.to_string() }),
            }
        }

        fn update_from_external(&mut self) -> Result<(), ClipboardError> {
            let termux_clipboard_get = duct::cmd!("termux-clipboard-get")
                .start()
                .map_err(|_| ClipboardError::TermuxApiError("unable to run termux-clipboard-get (is termux-api installed?)"))?;

            match termux_clipboard_get.wait_timeout(TIMEOUT) {
                Ok(Some(output)) => {
                    match String::from_utf8(output.stdout.to_owned()) {
                        Ok(txt) => {
                            self.clips = vec![txt];
                            Ok(())
                        }
                        Err(_) => Err(ClipboardError::ConversionFailure)
                    }
                }
                Ok(None) => {
                    termux_clipboard_get.kill().expect("killing termux-clipboard-get should never fail");
                    Err(ClipboardError::TermuxApiError("termux-clipboard-get timed out (is Termux:API installed?)"))
                }
                Err(err) => Err(ClipboardError::Unknown { description: err.to_string() })
            }
        }

        fn content(&self) -> &[String] {
            &self.clips
        }
    }
}
