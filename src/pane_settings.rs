use std::path::Path;

use ec4rs::PropertiesSource;

use crate::IndentKind;

const DEFAULT_EDITOR_CONFIG: &str = include_str!("../default_config/editorconfig");

#[derive(Debug)]
pub enum AutoIndent {
    /// Do not automatically insert any indentation
    None,
    /// Keep the current indentation level when a newline is inserted
    Keep,
    // TODO: smart indent
}

#[derive(Debug)]
pub struct PaneSettings {
    pub indent_kind: IndentKind,
    pub indent_size: usize,
    pub tab_width: usize,
    pub end_of_line: &'static str,
    pub autoindent: AutoIndent,
    pub trim_trailing_whitespace: bool,
    pub normalize_end_of_line: bool,
    pub insert_final_newline: bool,
    pub debug_scopes: bool,
}

impl PaneSettings {
    pub(crate) fn indent_as_string(&self) -> String {
        match self.indent_kind {
            IndentKind::Spaces => " ".repeat(self.indent_size),
            IndentKind::Tabs => {
                let mut width = 0;
                let mut indent = String::new();
                if self.tab_width > 0 {
                    while width + self.tab_width <= self.indent_size {
                        indent.push('\t');
                        width += self.tab_width;
                    }
                }
                if width < self.indent_size {
                    indent.push_str(&" ".repeat(self.indent_size - width));
                }
                indent
            }
        }
    }

    pub(crate) fn from_editorconfig(path: impl AsRef<Path>) -> Self {
        use ec4rs::property::*;
        let mut settings = Self::default();

        let mut props = ec4rs::Properties::default();
        ec4rs::ConfigParser::new_buffered(DEFAULT_EDITOR_CONFIG.as_bytes())
            .expect("this should not fail because default editorconfig is checked in build.rs")
            .apply_to(&mut props, &path)
            .expect("this should not fail because default editorconfig is checked in build.rs");

        if let Ok(override_props) = ec4rs::properties_of(&path) {
            let _ = override_props.apply_to(&mut props, &path);
        }

        if let Ok(TabWidth::Value(n)) = props.get::<TabWidth>() {
            settings.tab_width = n;
        }
        if let Ok(indent_kind) = props.get::<IndentStyle>() {
            settings.indent_kind = match indent_kind {
                IndentStyle::Tabs => IndentKind::Tabs,
                IndentStyle::Spaces => IndentKind::Spaces,
            };
        }
        if let Ok(indent_width) = props.get::<IndentSize>() {
            settings.indent_size = match indent_width {
                IndentSize::UseTabWidth => settings.tab_width,
                IndentSize::Value(n) => n,
            };
        }

        if let Ok(eol) = props.get::<EndOfLine>() {
            settings.end_of_line = match eol {
                EndOfLine::Lf => "\n",
                EndOfLine::CrLf => "\r\n",
                EndOfLine::Cr => "\r",
            };
            settings.normalize_end_of_line = true;
        }

        if let Ok(FinalNewline::Value(val)) = props.get::<FinalNewline>() {
            settings.insert_final_newline = val;
        }

        if let Ok(TrimTrailingWs::Value(val)) = props.get::<TrimTrailingWs>() {
            settings.trim_trailing_whitespace = val;
        }

        settings
    }
}

impl std::default::Default for PaneSettings {
    fn default() -> Self {
        PaneSettings {
            tab_width: 4,
            indent_kind: IndentKind::Spaces,
            indent_size: 4,
            end_of_line: "\n",
            autoindent: AutoIndent::Keep,
            trim_trailing_whitespace: true,
            normalize_end_of_line: false,
            insert_final_newline: true,
            debug_scopes: false,
        }
    }
}
