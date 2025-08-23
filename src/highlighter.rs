use std::collections::BTreeMap;
use std::str::FromStr;

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
                "string,punctuation.definition.string" = "#E6DB74"
                "comment" = "#75715E"
                "keyword,storage" = "#D6006B"
                "constant" = "#AE81FF"
                "entity.name" = "#66D9EF"
                "storage.type,entity.name.type,support.type,meta.type" = "#4A9CAB"
                "diff.inserted" = "#30CF50"
                "diff.changed" = "#FFAF00"
                "diff.deleted" = "#DB0000"
                "support.function.builtin" = "#66D9EF"
                "string.regexp" = "#D92682"
                "support.macro,entity.name.macro,keyword.declaration.macro" = "#A6E22E"
                "meta.interpolation" = "#FFFFFF"
                "punctuation.section" = "#D8D8D2"
            ],
        };
        Self { theme, syntax_set }
    }

    pub fn highlighter2_for_file(&self, file_path: &str) -> BadHighlighter {
        let syntax = match self.syntax_set.find_syntax_for_file(file_path) {
            Ok(Some(s)) => s,
            _ => self.syntax_set.find_syntax_plain_text(),
        };
        BadHighlighter::new(syntax, &self.theme, &self.syntax_set)
    }
}

#[derive(Clone)]
pub struct CachedState {
    parse_state: ParseState,
    highlight_state: HighlightState,
    line_number: usize,
}

pub struct BadHighlighter<'a> {
    syntax_set: &'a SyntaxSet,
    highlighter: Highlighter<'a>,
    pub cache: BTreeMap<usize, CachedState>,
    initial_parse_state: ParseState,
    parse_state: ParseState,
    highlight_state: HighlightState,
    current_line: usize,
}

impl<'a> BadHighlighter<'a> {
    pub fn new(syntax: &'a SyntaxReference, theme: &'a Theme, syntax_set: &'a SyntaxSet) -> Self {
        let highlighter = Highlighter::new(theme);
        let initial_parse_state = ParseState::new(syntax);
        let parse_state = initial_parse_state.clone();
        let highlight_state = HighlightState::new(&highlighter, ScopeStack::new());
        Self {
            highlighter,
            syntax_set,
            cache: BTreeMap::new(),
            initial_parse_state,
            parse_state,
            highlight_state,
            current_line: 0,
        }
    }

    fn reset_state(&mut self) {
        self.current_line = 0;
        self.parse_state.clone_from(&self.initial_parse_state);
        self.highlight_state = HighlightState::new(&self.highlighter, ScopeStack::new());
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

    fn parse_line(&mut self, line: &str) {
        let ops = self.parse_state.parse_line(line, self.syntax_set).unwrap_or_default();
        for _ in HighlightIterator::new(&mut self.highlight_state, &ops, line, &self.highlighter) {}
        self.current_line += 1;
        self.memorize_current_state();
    }

    fn memorize_current_state(&mut self) {
        if self.current_line & 1023 == 1023 {
            self.cache.insert(self.current_line, CachedState {
                parse_state: self.parse_state.clone(),
                highlight_state: self.highlight_state.clone(),
                line_number: self.current_line,
            });
        }
    }

    pub fn highlight_line<'t>(&mut self, line: &'t str) -> impl Iterator<Item = (Style, &'t str)> {
        let ops = self.parse_state.parse_line(line, self.syntax_set).unwrap_or_default();
        let highlights = HighlightIterator::new(&mut self.highlight_state, &ops, line, &self.highlighter).collect::<Vec<_>>();
        self.current_line += 1;
        self.memorize_current_state();
        highlights.into_iter()
    }
}
