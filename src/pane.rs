use std::io::{BufReader, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cli::FilePathWithOptionalLocation;
use crate::cursor::Cursor;
use crate::editing::EditBatch;
use crate::highlighter::{BadHighlighter, BadHighlighterManager};
use crate::linter::Lint;
use crate::ropebuffer::RopeBuffer;
use crate::{ByteOffset, IndentKind, MoveTarget, MultiCursor};

#[derive(Debug, Clone)]
pub enum PaneAction {
    MoveTo(MoveTarget),
    SpawnMultiCursorTo(MoveTarget),
    SelectTo(MoveTarget),
    SelectAll,
    Insert(String),
    InsertNewline,
    DeleteBackward,
    DeleteForward,
    DeleteWord,
    Indent,
    Dedent,
    MoveLinesUp,
    MoveLinesDown,
    Undo,
    Redo,
    Find(String),
    RepeatFind,
    RepeatFindBackward,
    QuickAddNext,
    Save,
    SaveAs(PathBuf),
    ScrollDown(usize),
    ScrollUp(usize),
}

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
    pub tab_width: usize,
    pub indent_kind: IndentKind,
    pub indent_width: usize,
    pub end_of_line: &'static str,
    pub autoindent: AutoIndent,
}

impl PaneSettings {
    fn indent_as_string(&self) -> String {
        match self.indent_kind {
            IndentKind::Spaces => " ".repeat(self.indent_width),
            IndentKind::Tabs => {
                let mut width = 0;
                let mut indent = String::new();
                if self.tab_width > 0 {
                    while width + self.tab_width <= self.indent_width {
                        indent.push('\t');
                        width += self.tab_width;
                    }
                }
                if width < self.indent_width {
                    indent.push_str(&" ".repeat(self.indent_width - width));
                }
                indent
            }
        }
    }

    fn from_editorconfig(path: impl AsRef<Path>) -> Self {
        use ec4rs::property::*;
        let mut settings = Self::default();
        if let Ok(props) = ec4rs::properties_of(path) {
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
                settings.indent_width = match indent_width {
                    IndentSize::UseTabWidth => settings.tab_width,
                    IndentSize::Value(n) => n,
                };
            }

            if let Ok(eol) = props.get::<EndOfLine>() {
                settings.end_of_line = match eol {
                    EndOfLine::Lf => "\n",
                    EndOfLine::CrLf => "\r\n",
                    EndOfLine::Cr => "\r",
                }
            }
        }
        settings
    }
}

impl std::default::Default for PaneSettings {
    fn default() -> Self {
        PaneSettings {
            tab_width: 4,
            indent_kind: IndentKind::Spaces,
            indent_width: 4,
            end_of_line: "\n",
            autoindent: AutoIndent::Keep,
        }
    }
}

pub struct Pane {
    pub(crate) title: String,
    pub(crate) path: Option<PathBuf>,
    pub(crate) content: RopeBuffer,
    pub(crate) viewport_position_row: usize,
    pub(crate) viewport_width: u16,
    pub(crate) viewport_height: u16,
    pub(crate) modified: bool,
    pub(crate) cursors: MultiCursor,
    pub(crate) settings: PaneSettings,
    pub(crate) highlighter: Option<BadHighlighter>,
    pub(crate) last_search: Option<String>,
    pub(crate) lints: Vec<Lint>,
    info: Option<String>,
}

impl Pane {
    pub fn empty() -> Self {
        Self {
            title: "untitled".to_string(),
            path: None,
            content: RopeBuffer::new(),
            cursors: MultiCursor::new(),
            viewport_position_row: 0,
            // these will be set during rendering
            viewport_height: 0,
            viewport_width: 0,

            settings: PaneSettings::default(),
            highlighter: None,
            last_search: None,
            lints: vec![],
            info: None,
            modified: false,
        }
    }

    pub fn esc(&mut self) {
        if self.cursors.cursor_count() > 1 || self.cursors.primary().has_selection() {
            self.cursors.esc();
        } else {
            self.lints.clear();
        }
        self.clear_status_msg();
    }

    pub fn status_msg(&self) -> Option<&str> {
        self.info.as_ref().map(|s| s.as_ref())
    }

    pub fn clear_status_msg(&mut self) {
        self.info.take();
    }

    pub fn inform(&mut self, msg: String) {
        self.info.replace(msg);
    }

    /// Returns the current filetype as a string, eg. "plain" or "c++"
    pub fn filetype(&self) -> &str {
        // Note that the render function temporarily takes ownership of the highlighter
        // so this function always returns "plain" when rendering a frame is in progress!
        match &self.highlighter {
            Some(hl) => hl.ft(),
            None => "plain",
        }
    }

    pub fn open_file(&mut self, fileloc: &FilePathWithOptionalLocation, hl: Arc<BadHighlighterManager>) -> std::io::Result<()> {
        let content = match std::fs::File::open(&fileloc.path) {
            Ok(file) => {
                // TODO: do something more efficient than this
                let mut s = String::new();
                BufReader::new(file).read_to_string(&mut s)?;
                RopeBuffer::from_str(&s)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => RopeBuffer::new(),
            Err(err) => return Err(err),
        };
        self.title = crate::quote_path(&fileloc.path);
        self.path = Some(PathBuf::from(&fileloc.path));
        self.content = content;
        self.cursors = MultiCursor::new();
        self.lints.clear();
        self.highlighter = Some(BadHighlighter::for_file(&fileloc.path, hl));
        self.settings = PaneSettings::from_editorconfig(&fileloc.path);
        self.modified = false;
        if let Some(line_no) = fileloc.line {
            let column_no = fileloc.column.unwrap_or(NonZeroUsize::new(1).unwrap());
            self.cursors.primary_mut().move_to(&self.content, MoveTarget::Location(line_no, column_no));
            self.viewport_position_row = usize::from(line_no).saturating_sub(3);
        } else {
            self.viewport_position_row = 0;
        }
        Ok(())
    }

    fn save_as(&mut self, path: impl AsRef<Path>) {
        let file = match std::fs::OpenOptions::new().read(false).write(true).create(true).truncate(true).open(&path) {
            Ok(file) => file,
            Err(err) => {
                self.inform(format!("Unable to save: {err}"));
                return
            }
        };

        self.title = crate::quote_path(&path.as_ref().to_string_lossy());
        self.path.replace(path.as_ref().into());
        match self.content.write_to(file) {
            Ok(n) => {
                self.modified = false;
                let quoted_path = crate::quote_path(&path.as_ref().to_string_lossy());
                self.inform(format!("Saved {quoted_path} ({n} bytes)"));
            }
            Err(err) => {
                self.inform(format!("Unable to save: {err}"));
            }
        }
    }

    pub fn selections(&self) -> Vec<String> {
        self.cursors
            .iter()
            .filter_map(|cursor| cursor.selection())
            .map(|sel| self.content.slice(&sel).to_string())
            .collect()
    }

    pub(crate) fn set_filetype(&mut self, ftype: &str, manager: Arc<BadHighlighterManager>) -> Result<(), ()> {
        if let Some(hl) = BadHighlighter::for_filetype(ftype, manager) {
            self.highlighter.replace(hl);
            Ok(())
        } else {
            Err(())
        }
    }

    pub fn update_viewport_size(&mut self, columns: u16, rows: u16) {
        self.viewport_width = columns;
        self.viewport_height = rows;
    }

    pub fn adjust_viewport(&mut self) {
        let line_number = self.cursors.primary().current_line_number(&self.content);
        self.adjust_viewport_to_show_line(line_number);
    }

    fn adjust_viewport_to_show_line(&mut self, line_number: usize) {
        let pad = 2;
        let vh = self.viewport_height as usize;
        let last_visible_line_number = self.viewport_position_row + vh;
        if line_number < self.viewport_position_row + pad {
            self.viewport_position_row = line_number.saturating_sub(pad);
        } else if line_number >= last_visible_line_number.saturating_sub(pad) {
            let desired_last_visible_line_number = (line_number + pad + 1).min(self.content.len_lines());
            self.viewport_position_row = desired_last_visible_line_number.saturating_sub(vh);
        }
    }

    fn apply_editbatch(&mut self, edits: EditBatch) {
        if edits.is_empty() {
            return
        }
        if let Some(offset) = edits.first_edit_offset() {
            for hl in self.highlighter.iter_mut() {
                let lineno = self.content.byte_to_line(offset);
                hl.invalidate_cache_starting_from_line(lineno);
            }
        }
        self.content.do_edits(&mut self.cursors, edits);
        self.modified = true;
        self.adjust_viewport();
    }

    pub fn insert_from_clipboard(&mut self, clips: &[String]) {
        let edits = EditBatch::insert_from_clipboard(&self.cursors, clips);
        self.apply_editbatch(edits);
    }

    pub fn cut(&mut self) -> Vec<String> {
        let edits = EditBatch::cut(&self.cursors, &self.content);
        let clips = edits.iter().filter_map(|edit| {
            if let crate::editing::Edit::Delete(range) = edit {
                Some(self.content.slice(range).to_string())
            } else {
                None
            }
        }).collect();
        self.apply_editbatch(edits);
        for cursor in self.cursors.iter_mut() {
            cursor.deselect();
        }
        clips
    }

    pub(crate) fn transform_selections<F>(&mut self, transform: F)
        where F: Fn(String) -> Option<String>
    {
        let edits = EditBatch::transform_selections(&self.cursors, &self.content, transform);
        self.apply_editbatch(edits);
        for cursor in self.cursors.iter_mut() {
            cursor.deselect();
        }
    }

    pub(crate) fn pipe_through_shell_command(&mut self, command_str: &str) {
        fn run_shell(cmd: &str, input: &str) -> Option<String> {
            let mut child_process = std::process::Command::new("sh");
            let mut run = child_process
                .args(["-c", cmd])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::piped())
                .spawn()
                .ok()?;
            run.stdin.as_mut()?.write_all(input.as_bytes()).ok()?;
            let output = run.wait_with_output().ok()?;
            Some(String::from_utf8_lossy(&output.stdout).to_string())
        }

        // insert output of the command if there is only one cursor without selection,
        // otherwise pipe each selection through the command
        if !self.cursors.primary().has_selection() && self.cursors.cursor_count() == 1 {
            let output = run_shell(command_str, "").unwrap_or_default();
            let edits = EditBatch::insert_with_cursors(&self.cursors, &output);
            self.apply_editbatch(edits);
        } else {
            self.transform_selections(|sel| run_shell(command_str, &sel));
        }
    }

    pub(crate) fn handle_event(&mut self, event: PaneAction) {
        match event {
            PaneAction::MoveTo(target) => {
                self.cursors.move_to(&self.content, target);
                self.adjust_viewport();
            }
            PaneAction::SpawnMultiCursorTo(target) => {
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
                let new_cursors: Vec<Cursor> = self.cursors.iter().map(|cursor| {
                    let mut new = *cursor;
                    new.move_to(&self.content, target);
                    new
                }).collect();
                for cursor in new_cursors {
                    if self.cursors.spawn_new(cursor) {
                        self.adjust_viewport_to_show_line(cursor.current_line_number(&self.content));
                    }
                }
            }
            PaneAction::SelectTo(target) => {
                self.cursors.select_to(&self.content, target);
                self.adjust_viewport();
            }
            PaneAction::SelectAll => {
                self.cursors.esc();
                let cursor = self.cursors.primary_mut();
                cursor.offset = ByteOffset(0);
                cursor.select_to(&self.content, MoveTarget::End);
            }
            PaneAction::Insert(s) => {
                let edits = EditBatch::insert_with_cursors(&self.cursors, &s);
                self.apply_editbatch(edits);
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
            }
            PaneAction::InsertNewline => {
                let eol = self.settings.end_of_line;
                let edits = match self.settings.autoindent {
                    AutoIndent::None => EditBatch::insert_with_cursors(&self.cursors, eol),
                    AutoIndent::Keep => EditBatch::insert_newline_keep_indent(&self.cursors, &self.content, eol),
                };
                self.apply_editbatch(edits);
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
            }
            PaneAction::DeleteBackward => {
                let edits = EditBatch::delete_backward_with_cursors(&self.cursors, &self.content);
                self.apply_editbatch(edits);
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
            }
            PaneAction::DeleteForward => {
                let edits = EditBatch::delete_forward_with_cursors(&self.cursors, &self.content);
                self.apply_editbatch(edits);
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
            }
            PaneAction::DeleteWord => {
                let edits = EditBatch::delete_word_with_cursors(&self.cursors, &self.content);
                self.apply_editbatch(edits);
                for cursor in self.cursors.iter_mut() {
                    cursor.deselect();
                }
            }
            PaneAction::Indent => {
                let indent = self.settings.indent_as_string();
                let edits = EditBatch::indent_with_cursors(&self.cursors, &self.content, &indent);
                self.apply_editbatch(edits);
            }
            PaneAction::Dedent => {
                let edits = EditBatch::dedent_with_cursors(&self.cursors, &self.content, self.settings.indent_width, self.settings.tab_width);
                self.apply_editbatch(edits);
            }
            PaneAction::MoveLinesUp => {
                let edits = EditBatch::move_lines_up(&self.cursors, &self.content);
                self.apply_editbatch(edits);
            }
            PaneAction::MoveLinesDown => {
                let edits = EditBatch::move_lines_down(&self.cursors, &self.content);
                self.apply_editbatch(edits);
            }
            PaneAction::Undo => {
                self.cursors = self.content.undo(self.cursors.clone());
                self.modified = true;
                self.adjust_viewport();
            }
            PaneAction::Redo => {
                self.cursors = self.content.redo(self.cursors.clone());
                self.modified = true;
                self.adjust_viewport();
            }
            PaneAction::Find(needle) => {
                self.content.search_with_cursors(&mut self.cursors, &needle);
                self.last_search = Some(needle);
                self.adjust_viewport();
            }
            PaneAction::RepeatFind => {
                if let Some(last_search) = self.last_search.as_ref() {
                    self.content.search_with_cursors(&mut self.cursors, last_search);
                    self.adjust_viewport();
                }
            }
            PaneAction::RepeatFindBackward => {
                if let Some(last_search) = self.last_search.as_ref() {
                    self.content.search_with_cursors_backward(&mut self.cursors, last_search);
                    self.adjust_viewport();
                }
            }
            PaneAction::QuickAddNext => {
                if let Some(selection) = self.cursors.primary().selection() {
                    let selection_str = self.content.slice(&selection).to_string();
                    if let Some(offset) = self.content.find_next_cycle(selection.end, &selection_str) {
                        if offset != selection.start {
                            let sel_end = ByteOffset(offset.0 + selection.end.0 - selection.start.0);
                            let new_cursor = Cursor::new_with_selection(offset, Some(sel_end));
                            self.cursors.spawn_new_primary(new_cursor);
                        }
                    }
                    self.adjust_viewport();
                }
            }
            PaneAction::Save => {
                if let Some(path) = self.path.take() {
                    self.save_as(&path);
                    self.path.replace(path);
                } else {
                    self.inform("Unable to save: no path specified".into());
                }
            }
            PaneAction::SaveAs(path) => {
                self.save_as(path);
            }
            PaneAction::ScrollDown(n) => {
                let new_pos = self.viewport_position_row + n;
                self.viewport_position_row = new_pos.min(self.content.len_lines().saturating_sub(1));
            }
            PaneAction::ScrollUp(n) => {
                self.viewport_position_row = self.viewport_position_row.saturating_sub(n);
            }
        }
    }
}
