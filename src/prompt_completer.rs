#[derive(Clone)]
pub struct CmdCompleter {
    cmds: Vec<Cmd>
}

impl CmdCompleter {
    pub fn make_completer(filetypes: &[&str]) -> CmdCompleter {
        macro_rules! argchoice {
            ($($x:expr),* $(,)?) => {
                Arg::OneOf(vec![$($x.into()),*])
            };
        }

        macro_rules! argseq {
            ($($x:expr),* $(,)?) => {
                Arg::Seq(vec![$($x.into()),*])
            };
        }

        let filetypes: Vec<Arg> = filetypes.iter().map(|s| Arg::Literal(s.to_string())).collect();

        CmdCompleter {
            cmds: vec![
                CmdBuilder::new("exec").alias("x")
                    .args(Arg::String)
                    .help("exec [TEMPLATE]")
                    .build(),
                CmdBuilder::new("find")
                    .args(Arg::String)
                    .help("find STR")
                    .build(),
                CmdBuilder::new("goto")
                    .args(Arg::String)
                    .help("goto LINE[:COL]")
                    .build(),
                CmdBuilder::new("insertchar").alias("c")
                    .args(Arg::String)
                    .help("insertchar CODEPOINT[, CODEPOINT]...")
                    .build(),
                CmdBuilder::new("lint")
                    .help("lint")
                    .build(),
                CmdBuilder::new("open")
                    .args(Arg::File)
                    .help("open FILE")
                    .build(),
                CmdBuilder::new("save")
                    .args(Arg::File)
                    .help("save [FILE]")
                    .build(),
                CmdBuilder::new("set")
                    .args(
                        argchoice![
                            argseq!["autoindent", argchoice!["off", "keep"]],
                            argseq!["eol", argchoice!["lf", "crlf", "cr"]],
                            argseq!["ftype", Arg::OneOf(filetypes)],
                            argseq!["debug", argchoice!["off", "scopes"]],
                        ]
                    )
                    .help("set KEY VALUE")
                    .build(),
                CmdBuilder::new("to")
                    .args(argchoice!["lower", "upper", "quoted", "list"])
                    .help("to (lower|upper|quoted|list)")
                    .build(),
                CmdBuilder::new("quit").alias(":q").alias("exit").alias("q")
                    .help("quit")
                    .build(),
            ]
        }
    }
}

impl reedline::Completer for CmdCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> Vec<reedline::Suggestion> {
        let input = &line[..pos];
        if let Some(cmd) = self.cmds.iter().find(|cmd| cmd.has_alias(input)) {
            // when completing a valid command with nothing after it we want to
            // a) turn alias into the primary name (eg. ":q" -> "quit")
            // b) insert a space if the command takes args, eg. "open" -> "open "
            return vec![
                reedline::Suggestion {
                    value: cmd.primary_name().to_string(),
                    span: reedline::Span { start: 0, end: pos },
                    append_whitespace: cmd.takes_args(),
                    ..Default::default()
                }
            ]
        }
    
        if let Some((first, rest)) = input.split_once(' ') {
            for cmd in &self.cmds {
                if cmd.has_alias(first) {
                    return cmd.arg_complete(rest, first.len() + 1)
                }
            }
            vec![]
        } else {
            self.cmds.iter()
                .filter(|cmd| cmd.primary_name().starts_with(input))
                .map(|cmd|
                    reedline::Suggestion {
                        value: cmd.primary_name().to_string(),
                        description: Some(cmd.help.to_string()),
                        extra: None,
                        style: None,
                        span: reedline::Span { start: 0, end: pos },
                        append_whitespace: cmd.takes_args(),
                    }
                )
                .collect()
        }
    }
}

#[derive(Clone)]
pub enum Arg {
    String,
    File,
    Literal(String),
    OneOf(Vec<Arg>),
    Seq(Vec<Arg>),
}

impl Default for Arg {
    fn default() -> Self {
        Arg::Seq(vec![])
    }
}

impl From<&'static str> for Arg {
    fn from(val: &'static str) -> Arg {
        Arg::Literal(val.into())
    }
}

enum ArgCompleteResult {
    NoMatch,
    SkipTo(usize),
    Suggest(Vec<reedline::Suggestion>),
}

impl Arg {
    fn complete(&self, s: &str, s_offset: usize, is_last: bool) -> ArgCompleteResult {
        let input = s.trim_start();
        let end = s_offset + s.len();
        let start = end - input.len();
        match self {
            Arg::String => ArgCompleteResult::Suggest(vec![]),
            Arg::Literal(lit) => {
                if input.len() > lit.len() && input.starts_with(lit) {
                    ArgCompleteResult::SkipTo(s_offset + lit.len())
                } else if lit.starts_with(input) {
                    ArgCompleteResult::Suggest(vec![
                        reedline::Suggestion {
                            value: lit.to_string(),
                            description: None,
                            extra: None,
                            style: None,
                            span: reedline::Span { start, end },
                            append_whitespace: !is_last,
                        }
                    ])
                } else {
                    ArgCompleteResult::NoMatch
                }
            }
            Arg::File => {
                if let Some(i) = input.find(' ') {
                    return ArgCompleteResult::SkipTo(start + i)
                }
                let mut suggestions = vec![];

                let (dir, file_prefix) = match input.rsplit_once('/') {
                    Some((dir, file_prefix)) => (dir, file_prefix),
                    None if input == "~" => ("~", ""),
                    None => (".", input),
                };

                let dir = crate::expand_path(dir);

                if let Ok(entries) = std::fs::read_dir(dir) {
                    for entry in entries.flatten() {
                        if let Some(name) = entry.file_name().to_str() {
                            if name.starts_with(file_prefix) {
                                let mut val = if input == "~" {
                                    format!("/{name}")
                                } else {
                                    name.to_string()
                                };
                                if entry.file_type().is_ok_and(|e| e.is_dir()) {
                                    val.push('/');
                                }
                                suggestions.push(reedline::Suggestion {
                                    value: val,
                                    description: None,
                                    extra: None,
                                    style: None,
                                    span: reedline::Span {
                                        start: end - file_prefix.len(),
                                        end,
                                    },
                                    append_whitespace: !is_last,
                                });
                            }
                        }
                    }
                }

                ArgCompleteResult::Suggest(suggestions)
            }
            Arg::Seq(args) => {
                let mut s = s;
                let mut s_offset = s_offset;
                let last_index = args.len() - 1;
                for (i, arg) in args.iter().enumerate() {
                    match arg.complete(s, s_offset, i == last_index) {
                        ArgCompleteResult::SkipTo(i) => {
                            s = &s[i - s_offset..];
                            s_offset = i;
                        }
                        sugg => return sugg
                    }
                }
                ArgCompleteResult::NoMatch
            }
            Arg::OneOf(choices) => {
                let mut suggestions = vec![];
                for choice in choices {
                    if let ArgCompleteResult::Suggest(sugg) = choice.complete(s, s_offset, is_last) {
                        suggestions.extend_from_slice(&sugg);
                    }
                }
                ArgCompleteResult::Suggest(suggestions)
            }
        }
    }
}

#[derive(Default, Clone)]
pub struct Cmd {
    prefixes: Vec<&'static str>,
    args: Arg,
    help: &'static str,
}

impl Cmd {
    fn has_alias(&self, alias: &str) -> bool {
        self.prefixes.contains(&alias)
    }

    fn takes_args(&self) -> bool {
        match &self.args {
            Arg::Seq(args) => !args.is_empty(),
            _ => true,
        }
    }

    fn primary_name(&self) -> &'static str {
        self.prefixes[0]
    }
    
    fn arg_complete(&self, s: &str, s_offset: usize) -> Vec<reedline::Suggestion> {
        match self.args.complete(s, s_offset, true) {
            ArgCompleteResult::SkipTo(_) => vec![],
            ArgCompleteResult::NoMatch => vec![],
            ArgCompleteResult::Suggest(suggestions) => suggestions,
        }
    }
}

struct CmdBuilder {
    cmd: Cmd,
}

impl CmdBuilder {
    fn new(prefix: &'static str) -> Self {
        Self { cmd: Cmd { prefixes: vec![prefix], ..Default::default() } }
    }

    fn alias(mut self, prefix: &'static str) -> Self {
        self.cmd.prefixes.push(prefix);
        self
    }

    fn args(mut self, args: Arg) -> Self {
        self.cmd.args = args;
        self
    }

    fn help(mut self, help: &'static str) -> Self {
        self.cmd.help = help;
        self
    }

    fn build(self) -> Cmd {
        self.cmd
    }
}
