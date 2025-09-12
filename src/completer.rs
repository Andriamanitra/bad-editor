use std::collections::VecDeque;
use std::sync::Arc;

use radix_trie::Trie;
use radix_trie::TrieCommon;
use unicode_width::UnicodeWidthStr;

pub struct SuggestionMenu {
    current_idx: usize,
    suggestions: Vec<Arc<str>>,
}

impl SuggestionMenu {
    pub fn current(&self) -> &str {
        &self.suggestions[self.current_idx]
    }

    pub fn cycle_next(&mut self) {
        if self.current_idx + 1 < self.suggestions.len() {
            self.current_idx += 1;
        } else {
            self.current_idx = 0;
        }
    }

    pub fn cycle_previous(&mut self) {
        if self.current_idx > 0 {
            self.current_idx -= 1;
        } else {
            self.current_idx = self.suggestions.len().saturating_sub(1);
        }
    }

    // TODO: Renderable trait instead of this nonsense
    pub fn render(&self, max_width: usize) -> String {
        let max_width = max_width - 2;
        let mut suggs = VecDeque::new();
        let mut width = 0;
        width += self.current().width() + 2;
        suggs.push_back(format!("[{}]", self.current()));

        let mut right = self.suggestions[self.current_idx + 1 ..].iter().map(|s| (s, s.width() + 2));
        if let Some((sugg, w)) = right.next() {
            if width + w < max_width {
                width += w;
                suggs.push_back(format!(" {sugg} "));
            }
        }
        let left = self.suggestions[0..self.current_idx].iter().rev().map(|s| (s, s.width() + 2));
        for (sugg, w) in left {
            if width + w > max_width {
                break
            }
            width += w;
            suggs.push_front(format!(" {sugg} "));
        }
        for (sugg, w) in right {
            if width + w > max_width {
                break
            }
            width += w;
            suggs.push_back(format!(" {sugg} "));
        }

        suggs.into_iter().collect()
    }
}

pub struct Completer {
    trie: Trie<String, String>,
}

pub enum CompletionResult<'a> {
    NoResults,
    ReplaceWith(&'a str),
    Menu(SuggestionMenu),
}

impl Completer {
    pub fn new() -> Self {
        let mut trie = Trie::new();
        let s = include_str!("../default_config/completions").trim();

        for mut kv in s.lines().map(|line| line.split_ascii_whitespace()) {
            let k = kv.next().unwrap();
            let v = kv.next().unwrap();
            trie.insert(k.into(), v.into());
        }

        Self {
            trie
        }
    }

    pub fn accept<'a>(&'a self, stem: &str) -> CompletionResult<'a> {
        match self.trie.get_raw_descendant(stem).and_then(|sub| sub.values().next()) {
            Some(val) => CompletionResult::ReplaceWith(val.as_str()),
            None => CompletionResult::NoResults,
        }
    }

    pub fn complete<'a>(&'a self, stem: &str) -> CompletionResult<'a> {
        let Some(sub) = self.trie.get_raw_descendant(stem) else {
            return CompletionResult::NoResults
        };

        if sub.is_leaf() {
            return match sub.key() {
                Some(key) if key == stem => CompletionResult::ReplaceWith(sub.value().unwrap().as_str()),
                Some(key) => CompletionResult::ReplaceWith(key),
                None => CompletionResult::NoResults,
            }
        }
        let suggestions: Vec<Arc<str>> = sub.keys().map(|k| Arc::from(k.as_str())).collect();
        CompletionResult::Menu(SuggestionMenu { current_idx: 0, suggestions })
    }
}
