use std::path::PathBuf;
use std::num::NonZeroUsize;

use clap::builder::StringValueParser;
use clap::Arg;
use clap::Command;
use clap::builder::TypedValueParser;

#[derive(Debug, Clone)]
pub struct FilePathWithOptionalLocation {
    pub path: String,
    pub line: Option<NonZeroUsize>,
    pub column: Option<NonZeroUsize>,
}

impl FilePathWithOptionalLocation {
    pub fn parse_from_str(arg: &str) -> Self {
        if PathBuf::from(arg).exists() {
            return FilePathWithOptionalLocation {
                path: String::from(arg),
                line: None,
                column: None,
            }
        }
        if let Some((pre1, num)) = arg.rsplit_once(':') {
            if let Ok(num_last) = num.parse() {
                if let Some((pre2, num)) = pre1.rsplit_once(':') {
                    if let Ok(num_second_last) = num.parse() {
                        return FilePathWithOptionalLocation {
                            path: String::from(pre2),
                            line: Some(num_second_last),
                            column: Some(num_last),
                        }
                    }
                }
                return FilePathWithOptionalLocation {
                    path: String::from(pre1),
                    line: Some(num_last),
                    column: None,
                }
            }
        }
        FilePathWithOptionalLocation {
            path: String::from(arg),
            line: None,
            column: None,
        }
    }
}

pub fn parse_cli_args() -> clap::ArgMatches {
    let open_file_at_loc_parser =
        StringValueParser::new().map(|p| FilePathWithOptionalLocation::parse_from_str(&p));

    Command::new("bad")
        .version("0.1")
        .arg(
            Arg::new("file").value_parser(open_file_at_loc_parser)
                .help("File to open, position can be specified via file[:row[:col]]")
        )
        .get_matches()
}
