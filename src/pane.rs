use std::io::{BufReader, ErrorKind, Read};

use crate::cursor::Cursor;
use crate::editing::EditBatch;
use crate::ropebuffer::RopeBuffer;
use crate::ByteOffset;
use crate::IndentKind;
use crate::MoveTarget;
use crate::MultiCursor;

#[derive(Debug, Clone)]
pub enum PaneAction {
    MoveTo(MoveTarget),
    SelectTo(MoveTarget),
    SelectAll,
    Insert(String),
    DeleteBackward,
    DeleteForward,
    Indent,
    Dedent,
    Undo,
    Redo,
    Find(String),
    RepeatFind,
    RepeatFindBackward,
    QuickAddNext,
}

#[derive(Debug)]
pub struct PaneSettings {
    pub tab_width: usize,
    pub indent_kind: IndentKind
}

impl std::default::Default for PaneSettings {
    fn default() -> Self {
        PaneSettings {
            tab_width: 4,
            indent_kind: IndentKind::default()
        }
    }
}

pub struct Pane {
    pub(crate) title: String,
    pub(crate) content: RopeBuffer,
    pub(crate) viewport_position_row: usize,
    pub(crate) viewport_width: u16,
    pub(crate) viewport_height: u16,
    pub(crate) cursors: MultiCursor,
    pub(crate) settings: PaneSettings,
    pub(crate) last_search: Option<String>,
}

impl Pane {
    pub fn empty() -> Self {
        Self {
            title: "bad.txt".to_string(),
            content: RopeBuffer::new(),
            cursors: MultiCursor::new(),
            viewport_position_row: 0,
            // these will be set during rendering
            viewport_height: 0,
            viewport_width: 0,

            settings: PaneSettings::default(),
            last_search: None,
        }
    }

    pub fn open_file(&mut self, path: &str) -> std::io::Result<()> {
        let content = match std::fs::File::open(path) {
            Ok(file) => {
                // TODO: do something more efficient than this
                let mut s = String::new();
                BufReader::new(file).read_to_string(&mut s)?;
                RopeBuffer::from_str(&s)
            }
            Err(err) if err.kind() == ErrorKind::NotFound => RopeBuffer::new(),
            Err(err) => return Err(err)
        };
        self.title = path.to_string();
        self.content = content;
        self.cursors = MultiCursor::new();
        Ok(())
    }

    pub fn selections(&self) -> Vec<String> {
        self.cursors.iter()
            .filter_map(|cursor| cursor.selection())
            .map(|sel| self.content.slice(&sel).to_string())
            .collect()
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

    pub fn insert_from_clipboard(&mut self, clips: &[String]) {
        let edits = EditBatch::insert_from_clipboard(&self.cursors, clips);
        self.content.do_edits(&mut self.cursors, edits);
    }

    pub(crate) fn handle_event(&mut self, event: PaneAction) {
        match event {
            PaneAction::MoveTo(target) => {
                self.cursors.move_to(&self.content, target);
            }
            PaneAction::SelectTo(target) => {
                self.cursors.select_to(&self.content, target);
            }
            PaneAction::SelectAll => {
                self.cursors.esc();
                let cursor = self.cursors.primary_mut();
                cursor.offset = ByteOffset(0);
                cursor.select_to(&self.content, MoveTarget::End);
            }
            PaneAction::Insert(s) => {
                let edits = EditBatch::insert_with_cursors(&self.cursors, &s);
                self.content.do_edits(&mut self.cursors, edits);
            }
            PaneAction::DeleteBackward => {
                let edits = EditBatch::delete_backward_with_cursors(&self.cursors, &self.content);
                self.content.do_edits(&mut self.cursors, edits);
            }
            PaneAction::DeleteForward => {
                let edits = EditBatch::delete_forward_with_cursors(&self.cursors, &self.content);
                self.content.do_edits(&mut self.cursors, edits);
            }
            PaneAction::Indent => {
                let edits = EditBatch::indent_with_cursors(&self.cursors, &self.content, self.settings.indent_kind);
                self.content.do_edits(&mut self.cursors, edits);
            }
            PaneAction::Dedent => {
                let edits = EditBatch::dedent_with_cursors(&self.cursors, &self.content, self.settings.indent_kind);
                self.content.do_edits(&mut self.cursors, edits);
            }
            PaneAction::Undo => self.cursors = self.content.undo(self.cursors.clone()),
            PaneAction::Redo => self.cursors = self.content.redo(self.cursors.clone()),
            PaneAction::Find(needle) => {
                self.content.search_with_cursors(&mut self.cursors, &needle);
                self.last_search = Some(needle);
            }
            PaneAction::RepeatFind => {
                if let Some(last_search) = self.last_search.as_ref() {
                    self.content.search_with_cursors(&mut self.cursors, last_search);
                }
            }
            PaneAction::RepeatFindBackward => {
                if let Some(last_search) = self.last_search.as_ref() {
                    self.content.search_with_cursors_backward(&mut self.cursors, last_search);
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
                }
            }
        }
    }
}
