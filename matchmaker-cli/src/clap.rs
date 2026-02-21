use clap::{ArgAction, Parser};
use std::{ffi::OsString, path::PathBuf};

pub static BINARY_FULL: &str = "matchmaker";
pub static BINARY_SHORT: &str = "mm";

#[derive(Debug, Parser, Default, Clone)]
#[command(trailing_var_arg = true)]
pub struct Cli {
    #[arg(long)]
    pub config: Option<PathBuf>,
    #[arg(long)]
    pub dump_config: bool,
    #[arg(short = 'F')]
    pub fullscreen: bool,
    #[arg(long)]
    pub test_keys: bool,

    /// Force the default command to run.
    #[arg(long)]
    pub no_read: bool,

    /// Reduce the verbosity level.
    #[clap(short, conflicts_with("verbose"), action = ArgAction::Count)]
    pub quiet: u8,
    /// Increase the verbosity level.
    #[clap(short, conflicts_with("quiet"), action = ArgAction::Count)]
    pub verbose: u8,

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
                            } else {
                                // clap will handle
                            }
                        }
                        continue;
                    }
                }};
            }

            // Long options with optional or required values
            try_parse!("config");
            try_parse!("verbosity");
            try_parse!("options");
            try_parse!("binds");

            // Flags
            if ["--dump-config", "--test-keys", "--no-read", "--help", "-F"].contains(&s.as_ref())
                || s.strip_prefix('-')
                    .is_some_and(|x| x.chars().all(|c| c == 'v') || x.chars().all(|c| c == 'q'))
            {
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
        let prog_name = args.first().cloned().unwrap_or_else(|| "prog".into());

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
