use std::num::NonZeroUsize;
use std::path::PathBuf;

use clap::builder::{StringValueParser, TypedValueParser};
use clap::{Arg, Command};

#[derive(Debug, Clone)]
pub struct FilePathWithOptionalLocation {
    pub path: PathBuf,
    pub line: Option<NonZeroUsize>,
    pub column: Option<NonZeroUsize>,
}

impl FilePathWithOptionalLocation {
    pub fn parse_from_str(arg: &str, expand_path: bool) -> Self {
        let to_path = if expand_path {
            |s: &str| crate::expand_path(s)
        } else {
            |s: &str| PathBuf::from(s)
        };

        if to_path(arg).exists() {
            return FilePathWithOptionalLocation {
                path: to_path(arg),
                line: None,
                column: None,
            }
        }
        if let Some((pre1, num)) = arg.rsplit_once(':') {
            if let Ok(num_last) = num.parse() {
                if let Some((pre2, num)) = pre1.rsplit_once(':') {
                    if let Ok(num_second_last) = num.parse() {
                        return FilePathWithOptionalLocation {
                            path: to_path(pre2),
                            line: Some(num_second_last),
                            column: Some(num_last),
                        }
                    }
                }
                return FilePathWithOptionalLocation {
                    path: to_path(pre1),
                    line: Some(num_last),
                    column: None,
                }
            }
        }
        FilePathWithOptionalLocation {
            path: to_path(arg),
            line: None,
            column: None,
        }
    }
}

impl From<PathBuf> for FilePathWithOptionalLocation {
    fn from(value: PathBuf) -> Self {
        Self { path: value, line: None, column: None }
    }
}

pub fn parse_cli_args() -> clap::ArgMatches {
    let open_file_at_loc_parser =
        StringValueParser::new().map(|p| FilePathWithOptionalLocation::parse_from_str(&p, false));

    Command::new("bad")
        .version("0.1")
        .arg(
            Arg::new("file")
                .value_parser(open_file_at_loc_parser)
                .action(clap::ArgAction::Append)
                .help("File to open, position can be specified via file[:row[:col]]"),
        )
        .get_matches()
}
