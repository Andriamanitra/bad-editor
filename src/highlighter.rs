use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use syntect::highlighting::{
    Color,
    HighlightIterator,
    HighlightState,
    Highlighter,
    ScopeSelectors,
    Style,
    StyleModifier,
    Theme,
    ThemeItem,
    ThemeSettings,
};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference, SyntaxSet};
use unicode_segmentation::UnicodeSegmentation;

use crate::ropebuffer::RopeBuffer;

pub struct BadHighlighterManager {
    theme: Theme,
    syntax_set: SyntaxSet,
}

impl BadHighlighterManager {
    pub fn new() -> Self {
        let syntax_set: SyntaxSet = syntect::dumps::from_uncompressed_data(
            include_bytes!(concat!(env!("OUT_DIR"), "/syntaxes.packdump"))
        ).expect("syntaxes.packdump should be valid");

        macro_rules! theme_scopes {
            ( $( $scope:literal = $fg:literal )* ) => {
                vec![
                    $(
                        ThemeItem {
                            scope: ScopeSelectors::from_str($scope).unwrap(),
                            style: StyleModifier {
                                foreground: Color::from_str($fg).ok(),
                                background: None,
                                font_style: None,
                            }
                        }
                    ),*
                ]
            }
        }

        let theme = Theme {
            name: Some("default".into()),
            author: Some("Andriamanitra".into()),
            settings: ThemeSettings {
                foreground: Color::from_str("#F8F8F2").ok(),
                background: Color::from_str("#1A1A1A").ok(),
                ..ThemeSettings::default()
            },
            scopes: theme_scopes![
                "string.quoted,punctuation.definition.string" = "#E6DB74"
                "comment,punctuation.definition.comment" = "#75715E"
                "keyword,storage,punctuation.separator,punctuation.terminator,punctuation.accessor,punctuation.definition.block" = "#D6006B"
                "constant" = "#AE81FF"
                "support.function,entity.name,meta.mapping.key.yaml" = "#66D9EF"
                "storage.type,support.class,entity.name.type,support.type,meta.type" =  "#569CD6"
                "storage.modifier.lifetime" = "#2AACAB"
                "diff.inserted" = "#30CF50"
                "diff.changed" = "#FFAF00"
                "diff.deleted" = "#DB0000"
                "string.regexp punctuation.definition.string.begin,string.regexp punctuation.definition.string.end" = "#D92682"
                "string.regexp" = "#FB7FA8"
                "support.macro,support.function.macro,variable.macro,entity.name.macro,punctuation.definition.macro" = "#A6E22E"
                "punctuation.definition.annotation,variable.annotation" = "#A6E22E"
                "meta.interpolation" = "#FFFFFF"
                "punctuation.section" = "#D8D8D2"
            ],
        };
        Self { theme, syntax_set }
    }

    pub fn new_with_syntaxes_from_dir<P: AsRef<std::path::Path>>(syntax_dir: P) -> (Self, Result<(), syntect::LoadingError>) {
        let mut new = Self::new();
        let mut builder = new.syntax_set.into_builder();
        let result = builder.add_from_folder(syntax_dir, true);
        new.syntax_set = builder.build();
        (new, result)
    }

    pub fn filetypes(&self) -> Vec<&str> {
        self.syntax_set.syntaxes().iter().filter(|syn| syn.name != "Plain Text").map(|syn| syn.name.as_str()).collect()
    }

    fn highlighter<'a>(&'a self) -> Highlighter<'a> {
        Highlighter::new(&self.theme)
    }
}

impl Default for BadHighlighterManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone)]
pub struct CachedState {
    parse_state: ParseState,
    highlight_state: HighlightState,
    line_number: usize,
}

#[derive(Clone)]
pub struct BadHighlighter {
    filetype: String,
    manager: Arc<BadHighlighterManager>,
    cache: BTreeMap<usize, CachedState>,
    initial_parse_state: ParseState,
    parse_state: ParseState,
    highlight_state: HighlightState,
    current_line: usize,
}

impl BadHighlighter {
    const MAX_LINE_LENGTH_FOR_HIGHLIGHTING: usize = 1024;

    pub fn for_file<P: AsRef<std::path::Path>>(file_path: P, manager: Arc<BadHighlighterManager>) -> Self {
        let syntax = match manager.syntax_set.find_syntax_for_file(file_path) {
            Ok(Some(s)) => s,
            _ => manager.syntax_set.find_syntax_plain_text(),
        };
        BadHighlighter::for_syntax(syntax, manager.clone())
    }

    pub fn for_filetype(filetype: &str, manager: Arc<BadHighlighterManager>) -> Option<Self> {
        let syntax = manager.syntax_set.find_syntax_by_name(filetype)?;
        Some(BadHighlighter::for_syntax(syntax, manager.clone()))
    }

    fn for_syntax(syntax: &SyntaxReference, manager: Arc<BadHighlighterManager>) -> Self {
        let initial_parse_state = ParseState::new(syntax);
        let parse_state = initial_parse_state.clone();
        let highlight_state = HighlightState::new(&manager.highlighter(), ScopeStack::new());
        Self {
            filetype: syntax.name.clone(),
            manager,
            cache: BTreeMap::new(),
            initial_parse_state,
            parse_state,
            highlight_state,
            current_line: 0,
        }
    }

    pub fn ft(&self) -> &str {
        // "Plain Text" is hardcoded name for the fallback syntax in syntect but it
        // doesn't match our filetype naming conventions (short and all lowercase)
        if self.filetype == "Plain Text" {
            return "plain"
        }
        &self.filetype
    }

    fn reset_state(&mut self) {
        self.current_line = 0;
        self.parse_state.clone_from(&self.initial_parse_state);
        self.highlight_state = HighlightState::new(&self.manager.highlighter(), ScopeStack::new());
    }

    pub fn invalidate_cache_starting_from_line(&mut self, lineno: usize) {
        self.cache.split_off(&lineno);
        // If we're currently positioned after the invalidation point, reset
        if self.current_line >= lineno {
            self.reset_state();
        }
    }

    pub fn skip_to_line(&mut self, target_line: usize, text: &RopeBuffer) {
        if self.current_line == target_line {
            return
        }

        // Find the best cache entry to start from
        if let Some((_, cached_state)) = self.cache.range(..=target_line).next_back() {
            self.current_line = cached_state.line_number;
            self.highlight_state = cached_state.highlight_state.clone();
            self.parse_state = cached_state.parse_state.clone();
        } else if self.current_line > target_line {
            self.reset_state();
        }

        for line in text.lines_at(self.current_line) {
            if self.current_line == target_line {
                return
            }
            self.parse_line(&line.to_string());
        }
    }

    pub fn scope_stack_at(&self, target_line: usize, col_offset: usize, text: &RopeBuffer) -> ScopeStack {
        // TODO: make this less stupid, currently it doubles the render times
        // (but this is only called when debug scopes is active)
        let mut clone = self.clone();
        clone.skip_to_line(target_line, text);
        let line = text.lines_at(clone.current_line).next().unwrap().to_string();
        let ops: Vec<_> = clone.parse_state.parse_line(&line, &clone.manager.syntax_set).unwrap_or_default();
        let pp = ops.partition_point(|(i, _)| *i <= col_offset);
        for _ in HighlightIterator::new(&mut clone.highlight_state, &ops[..pp], &line, &clone.manager.highlighter()) {}
        clone.highlight_state.path
    }

    fn parse_line(&mut self, line: &str) {
        if line.len() <= Self::MAX_LINE_LENGTH_FOR_HIGHLIGHTING {
            let ops = self.parse_state.parse_line(line, &self.manager.syntax_set).unwrap_or_default();
            for _ in HighlightIterator::new(&mut self.highlight_state, &ops, line, &self.manager.highlighter()) {}
        }
        self.current_line += 1;
        self.memorize_current_state();
    }

    fn memorize_current_state(&mut self) {
        if self.current_line & 0x69 == 0x69 {
            self.cache.insert(self.current_line, CachedState {
                parse_state: self.parse_state.clone(),
                highlight_state: self.highlight_state.clone(),
                line_number: self.current_line,
            });
        }
    }

    pub fn highlight_line<'t>(&mut self, line: &'t str) -> impl Iterator<Item = (Style, &'t str)> {
        let highlights: Vec<(Style, &'t str)> = if line.len() <= Self::MAX_LINE_LENGTH_FOR_HIGHLIGHTING {
            let ops = self.parse_state.parse_line(line, &self.manager.syntax_set).unwrap_or_default();
            HighlightIterator::new(&mut self.highlight_state, &ops, line, &self.manager.highlighter()).collect()
        } else {
            let style = self.manager.highlighter().style_for_stack(self.highlight_state.path.as_slice());
            line.graphemes(true).map(|g| (style, g)).collect()
        };
        self.current_line += 1;
        self.memorize_current_state();
        highlights.into_iter()
    }
}
