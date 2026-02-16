use clap::Parser;
use std::ffi::OsString;

pub static BINARY_FULL: &str = "matchmaker";
pub static BINARY_SHORT: &str = "mm";

#[derive(Debug, Parser, Default, Clone)]
#[command(trailing_var_arg = true)]
pub struct Cli {
    #[arg(long, value_name = "PATH_OR_STRING")]
    pub config: Option<OsString>,
    #[arg(long)]
    pub dump_config: bool,
    #[arg(short = 'F')]
    pub fullscreen: bool,
    #[arg(long)]
    pub test_keys: bool,
    #[arg(long)]
    pub header_lines: Option<usize>,
    #[arg(long, default_value_t = 3)]
    pub verbosity: u8, // todo: implement

    // docs
    /// Display options doc
    #[arg(long)]
    pub options: bool,
    #[arg(long)]
    pub binds: bool,
}

impl Cli {
    /// All words parsed by clap need to be repeated here to be extracted.
    fn partition_clap_args(args: Vec<OsString>) -> (Vec<OsString>, Vec<OsString>) {
        let mut clap_args = Vec::new();
        let mut rest = Vec::new();

        let mut iter = args.into_iter();
        while let Some(arg) = iter.next() {
            let s = arg.to_string_lossy();

            // Check end of options
            if s == "--" {
                rest.extend(iter);
                break;
            }

            macro_rules! try_parse {
                ($name:literal) => {{
                    let eq_opt = concat!("--", $name, "=");
                    let long_opt = concat!("--", $name);

                    if s == long_opt || s.starts_with(eq_opt) {
                        let needs_next = s == long_opt;
                        clap_args.push(arg.clone());
                        if needs_next {
                            if let Some(next) = iter.next() {
                                clap_args.push(next);
                            }
                        }
                        continue;
                    }
                }};
            }

            // Long options with optional or required values
            try_parse!("config");
            try_parse!("header-lines");
            try_parse!("verbosity");
            try_parse!("options");
            try_parse!("binds");

            // Flags
            if ["--dump-config", "--test-keys", "--fullscreen", "-F"].contains(&s.as_ref()) {
                clap_args.push(arg);
                continue;
            }

            // Anything else
            rest.push(arg);
        }

        (clap_args, rest)
    }

    pub fn get_partitioned_args() -> (Self, Vec<String>) {
        use std::env;

        // Grab all args from the environment
        let args: Vec<std::ffi::OsString> = env::args_os().collect();
        let prog_name = args.get(0).cloned().unwrap_or_else(|| "prog".into());

        // Partition the args, skipping the program name
        let (mut clap_args, rest_os) =
            Self::partition_clap_args(args.into_iter().skip(1).collect());

        clap_args.insert(0, prog_name);

        // Parse the Clap args
        let cli = Cli::parse_from(clap_args);

        // Convert the rest to Strings
        let rest: Vec<String> = rest_os
            .into_iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();

        (cli, rest)
    }
}
