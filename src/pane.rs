use std::io::{BufReader, ErrorKind, Read, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cli::FilePathWithOptionalLocation;
use crate::completer::{Completer, CompletionResult, SuggestionMenu};
use crate::cursor::Cursor;
use crate::editing::{Edit, EditBatch};
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
    ScrollDown(usize),
    ScrollUp(usize),
    Tab,
    BackTab,
    Autocomplete,
    AutocompleteCyclePrevious,
    AutocompleteCycleNext,
    AutocompleteAcceptSuggestion,
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
    fn indent_as_string(&self) -> String {
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
    completer: Completer,
    pub(crate) suggestions: Option<SuggestionMenu>,
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
            completer: Completer::new(),
            suggestions: None,
            last_search: None,
            lints: vec![],
            info: None,
            modified: false,
        }
    }

    pub fn new_from_file(fileloc: &FilePathWithOptionalLocation, hl: Arc<BadHighlighterManager>) -> Self {
        let mut pane = Pane::empty();
        match std::fs::File::open(&fileloc.path) {
            Ok(file) => {
                // TODO: do something more efficient than this
                let mut s = String::new();
                if BufReader::new(file).read_to_string(&mut s).is_ok() {
                    pane.content = RopeBuffer::from_str(&s);
                    pane.path = Some(PathBuf::from(&fileloc.path));
                } else {
                    pane.inform("Error reading file".into());
                }
            }
            Err(err) => {
                let fpath = crate::quote_path(fileloc.path.to_string_lossy().as_ref());
                match err.kind() {
                    ErrorKind::NotFound => {
                        pane.path = Some(PathBuf::from(&fileloc.path));
                    },
                    ErrorKind::PermissionDenied => pane.inform(format!("Permission denied: {fpath}")),
                    ErrorKind::IsADirectory => pane.inform(format!("Can not open a directory: {fpath}")),
                    _ => pane.inform(format!("{err}: {fpath}")),
                }
            }
        };
        
        if let Some(path) = pane.path.as_ref() {
            pane.title = crate::quote_path(&path.to_string_lossy());
            pane.highlighter = Some(BadHighlighter::for_file(path, hl));
            pane.settings = PaneSettings::from_editorconfig(path);
        }
        if let Some(line_no) = fileloc.line {
            let column_no = fileloc.column.unwrap_or(NonZeroUsize::new(1).unwrap());
            pane.cursors.primary_mut().move_to(&pane.content, MoveTarget::Location(line_no, column_no));
            let cursor_line_no = pane.cursors.primary().current_line_number(&pane.content);
            pane.viewport_position_row = cursor_line_no.saturating_sub(3);
        }
        pane
    }

    pub fn esc(&mut self) {
        if self.cursors.cursor_count() > 1 || self.cursors.primary().has_selection() {
            self.cursors.esc();
        } else {
            self.lints.clear();
        }
        self.suggestions.take();
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

    fn set_path(&mut self, path: impl AsRef<Path>, hl: Arc<BadHighlighterManager>) -> std::io::Result<()> {
        if let Err(err) = std::fs::OpenOptions::new().read(false).write(true).create(true).truncate(false).open(&path) {
            self.inform(format!("Unable to save: {err}"));
            return Err(err)
        }
        if self.path.as_ref().is_none_or(|old_path| old_path != path.as_ref()) {
            self.path.replace(path.as_ref().into());
            self.highlighter.replace(BadHighlighter::for_file(&path, hl));
            self.title = crate::quote_path(&path.as_ref().to_string_lossy());
        }
        Ok(())
    }

    fn write_to_file(&self, mut file: std::fs::File, rope: &RopeBuffer) -> std::io::Result<()> {
        // TODO: atomic file write

        // https://docs.rs/ropey/1.6.1/ropey/index.html#a-note-about-line-breaks
        const UNICODE_LINE_END_CHARS: [char; 7] = [
            '\u{000A}', '\u{000D}', '\u{000B}', '\u{000C}', '\u{0085}', '\u{2028}', '\u{2029}'
        ];

        for line in rope.lines() {
            // TODO: iterate over line.chunks() instead to avoid building temporary strings
            let full_line = line.to_string();

            if let Some(line) = full_line.strip_suffix("\r\n") {
                if self.settings.trim_trailing_whitespace {
                    file.write_all(line.trim_end().as_bytes())?;
                } else {
                    file.write_all(line.as_bytes())?;
                }
                if self.settings.normalize_end_of_line {
                    file.write_all(self.settings.end_of_line.as_bytes())?;
                } else {
                    file.write_all(b"\r\n")?;
                }
            } else if let Some(line) = full_line.strip_suffix(UNICODE_LINE_END_CHARS) {
                if self.settings.trim_trailing_whitespace {
                    file.write_all(line.trim_end().as_bytes())?;
                } else {
                    file.write_all(line.as_bytes())?;
                }
                if self.settings.normalize_end_of_line {
                    file.write_all(self.settings.end_of_line.as_bytes())?;
                } else {
                    let line_end = full_line.chars().last().unwrap();
                    file.write_all(line_end.to_string().as_bytes())?;
                }
            } else if !full_line.is_empty() {
                file.write_all(full_line.as_bytes())?;
                if self.settings.insert_final_newline {
                    file.write_all(self.settings.end_of_line.as_bytes())?;
                }
            }
        }
        file.flush()?;
        Ok(())
    }

    pub(crate) fn save(&mut self) {
        if let Some(path) = self.path.as_ref() {
            let file = match std::fs::OpenOptions::new().read(false).write(true).create(true).truncate(true).open(path) {
                Ok(file) => file,
                Err(err) => {
                    self.inform(format!("Unable to save: {err}"));
                    return
                }
            };
            // FIXME: saving can modify the contents (eg. modifying line endings)
            // and the editor should react to that
            match self.write_to_file(file, &self.content) {
                Ok(()) => {
                    self.modified = false;
                    let quoted_path = crate::quote_path(path.to_string_lossy().as_ref());
                    self.inform(format!("Saved {quoted_path}"));
                }
                Err(err) => {
                    self.inform(format!("Failed to save: {err}"));
                }
            }
        } else {
            self.inform("Unable to save: no file specified".into());
        }
    }

    pub(crate) fn save_as(&mut self, path: impl AsRef<Path>, hl: Arc<BadHighlighterManager>) {
        if self.set_path(path, hl).is_ok() {
            self.save();
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
            PaneAction::ScrollDown(_) => (),
            PaneAction::ScrollUp(_) => (),
            PaneAction::Tab => (),
            PaneAction::BackTab => (),
            PaneAction::AutocompleteCyclePrevious => (),
            PaneAction::AutocompleteCycleNext => (),
            _ => {
                self.suggestions.take();
            }
        }

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
                let edits = EditBatch::dedent_with_cursors(&self.cursors, &self.content, self.settings.indent_size, self.settings.tab_width);
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
            PaneAction::ScrollDown(n) => {
                let new_pos = self.viewport_position_row + n;
                self.viewport_position_row = new_pos.min(self.content.len_lines().saturating_sub(1));
            }
            PaneAction::ScrollUp(n) => {
                self.viewport_position_row = self.viewport_position_row.saturating_sub(n);
            }
            PaneAction::Tab => {
                if self.suggestions.is_some() {
                    self.handle_event(PaneAction::AutocompleteCycleNext);
                } else if self.cursors.iter().any(|c| c.has_selection()) || self.cursors.primary().is_at_start_of_line(&self.content) {
                    self.handle_event(PaneAction::Indent);
                } else {
                    self.handle_event(PaneAction::Autocomplete);
                }
            }
            PaneAction::BackTab => {
                if self.suggestions.is_some() {
                    self.handle_event(PaneAction::AutocompleteCyclePrevious);
                } else {
                    self.handle_event(PaneAction::Dedent);
                }
            }
            PaneAction::Autocomplete => {
                if self.cursors.cursor_count() == 1 && !self.cursors.primary().has_selection() {
                    let stem = self.cursors.primary().stem(&self.content);
                    match self.completer.complete(&stem) {
                        CompletionResult::NoResults => self.inform("no completions".into()),
                        CompletionResult::ReplaceWith(ins) => {
                            let stem_start = ByteOffset(self.cursors.primary().offset.0 - stem.len());
                            let edits = vec![Edit::delete(stem_start, stem.len()), Edit::insert_str(stem_start, ins)];
                            let edits = EditBatch::from_edits(edits);
                            self.apply_editbatch(edits);
                        }
                        CompletionResult::Menu(suggestion_menu) => {
                            let ins = suggestion_menu.current();
                            let stem_start = ByteOffset(self.cursors.primary().offset.0 - stem.len());
                            let edits = vec![Edit::delete(stem_start, stem.len()), Edit::insert_str(stem_start, ins)];
                            let edits = EditBatch::from_edits(edits);
                            self.suggestions = Some(suggestion_menu);
                            self.apply_editbatch(edits);
                        }
                    };
                }
            }
            PaneAction::AutocompleteAcceptSuggestion => {
                let stem = self.cursors.primary().stem(&self.content);
                match self.completer.accept(&stem) {
                    CompletionResult::NoResults => self.inform("no completions".into()),
                    CompletionResult::ReplaceWith(ins) => {
                        let stem_start = ByteOffset(self.cursors.primary().offset.0 - stem.len());
                        let edits = vec![Edit::delete(stem_start, stem.len()), Edit::insert_str(stem_start, ins)];
                        let edits = EditBatch::from_edits(edits);
                        self.apply_editbatch(edits);
                    }
                    CompletionResult::Menu(_) => {}
                }
            }
            PaneAction::AutocompleteCycleNext => {
                let edits = match self.suggestions.as_mut() {
                    Some(menu) => {
                        let stem_length = menu.current().len();
                        let stem_start = ByteOffset(self.cursors.primary().offset.0 - stem_length);
                        menu.cycle_next();
                        let edits = vec![Edit::delete(stem_start, stem_length), Edit::insert_str(stem_start, menu.current())];
                        EditBatch::from_edits(edits)
                    }
                    None => return
                };
                self.apply_editbatch(edits);
            }
            PaneAction::AutocompleteCyclePrevious => {
                let edits = match self.suggestions.as_mut() {
                    Some(menu) => {
                        let stem_length = menu.current().len();
                        let stem_start = ByteOffset(self.cursors.primary().offset.0 - stem_length);
                        menu.cycle_previous();
                        let edits = vec![Edit::delete(stem_start, stem_length), Edit::insert_str(stem_start, menu.current())];
                        EditBatch::from_edits(edits)
                    }
                    None => return
                };
                self.apply_editbatch(edits);
            }
        }
    }
}
