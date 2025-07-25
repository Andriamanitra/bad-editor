use std::str::FromStr;

use syntect;
use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
//use syntect::parsing::SyntaxSetBuilder;
use syntect::highlighting::Color;
use syntect::highlighting::Style;
use syntect::highlighting::Theme;
use syntect::highlighting::ThemeItem;
use syntect::highlighting::ThemeSettings;
use syntect::highlighting::ScopeSelectors;
use syntect::highlighting::StyleModifier;

pub struct BadHighlighterManager {
    theme: Theme,
    syntax_set: SyntaxSet,
}

impl BadHighlighterManager {
    pub fn new() -> Self {
        // TODO: read syntaxes from file, the built-in ones are outdated!
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
            ]
        };
        Self { theme, syntax_set }
    }

    pub fn highlighter_for_file<'a>(&'a self, file_path: &str) -> BadHighlighter<'a> {
        let syntax = match self.syntax_set.find_syntax_for_file(file_path) {
            Ok(Some(s)) => s,
            _ => self.syntax_set.find_syntax_plain_text()
        };
        let highlighter = HighlightLines::new(syntax, &self.theme);
        BadHighlighter {
            ss: &self.syntax_set,
            highlighter
        }
    }
}

pub struct BadHighlighter<'a> {
    ss: &'a SyntaxSet,
    highlighter: HighlightLines<'a>,
}

impl<'a> BadHighlighter<'a> {
    pub fn highlight<'t>(&mut self, text: &'t str) -> impl Iterator<Item = (Style, &'t str)> {
        self.highlighter.highlight_line(text, &self.ss).unwrap().into_iter()
    }
}
