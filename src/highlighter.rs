use std::str::FromStr;

use syntect;
use syntect::easy::HighlightLines;
use syntect::parsing::SyntaxSet;
//use syntect::parsing::SyntaxSetBuilder;
use syntect::highlighting::Color;
use syntect::highlighting::Style;
use syntect::highlighting::Theme;
use syntect::highlighting::ThemeItem;
use syntect::highlighting::ThemeSet;
use syntect::highlighting::ThemeSettings;
use syntect::highlighting::ScopeSelectors;
use syntect::highlighting::StyleModifier;

pub struct BadHighlighterManager {
    syntax_set: SyntaxSet,
    theme_set: ThemeSet,
}

impl BadHighlighterManager {
    pub fn new() -> Self {
        // TODO: read syntaxes from file, the built-in ones are outdated!
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let mut theme_set = ThemeSet::load_defaults();

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

        let default_theme = Theme {
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
                "keyword" = "#D6006B"
                "constant" = "#AE81FF"
                "entity.name" = "#66D9EF"
                "diff.inserted" = "#30CF50"
                "diff.changed" = "#FFAF00"
                "diff.deleted" = "#DB0000"
                "support.function.builtin" = "#66D9EF"
                "string.regexp" = "#D92682"
                "meta.interpolation" = "#FFFFFF"
                "punctuation.section" = "#D8D8D2"
            ]
        };
        theme_set.themes.insert("default".to_string(), default_theme);
        Self { syntax_set, theme_set }
    }

    pub fn get_highlighter_for_file_ext<'a>(&'a self, file_ext: &str) -> BadHighlighter<'a> {
        let syntax = self.syntax_set.find_syntax_by_extension(file_ext).unwrap();
        let theme = self.theme_set.themes.get("default").unwrap();
        let highlighter = HighlightLines::new(syntax, &theme);
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
