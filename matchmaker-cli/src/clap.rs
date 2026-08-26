use clap::{ArgAction, Parser};
use std::{ffi::OsString, path::PathBuf};

pub static LIBRARY_FULL: &str = "matchmaker";
pub static BINARY_SHORT: &str = "mm";

#[derive(Debug, Parser, Default, Clone)]
#[command(
    disable_help_flag = true,
    arg(
        clap::Arg::new("help")
            .long("help")
            .action(ArgAction::Help)
            .global(true)
    )
)]
#[command(name = "mm", version)]
pub struct Cli {
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,

    /// Paths without a toml extension refer
    /// to a preset.
    #[arg(long, short, value_name = "PATH")]
    pub r#override: Vec<PathBuf>,
    /// Write the default configuration to the default location.
    /// If piped, writes the current configuration to stdout.
    #[arg(long)]
    pub dump_config: bool,
    /// Run in fullscreen
    #[arg(short = 'F')]
    pub fullscreen: bool,

    #[arg(long)]
    pub test_keys: bool,
    /// Print the last key pressed in the last `mm` run.
    #[arg(long)]
    pub last_key: bool,

    /// Force the default command to run.
    #[arg(long)]
    pub no_read: bool,

    /// Context lines rolled into each item.
    #[arg(short = 'C', value_name = "N", default_value_t = 0)]
    pub context: u16,

    /// args passed to the populating command.
    #[arg(last = true)]
    pub args: Vec<OsString>,

    /// Reduce the verbosity level.
    #[clap(short, conflicts_with("verbose"), action = ArgAction::Count)]
    pub quiet: u8,
    /// Increase the verbosity level.
    #[clap(short, conflicts_with("quiet"), action = ArgAction::Count)]
    pub verbose: u8,

    /// Download all presets from GitHub, or a specific subfolder / preset file.
    #[arg(long, value_name = "FOLDER", num_args = 0..=1, default_missing_value = "")]
    pub download: Option<String>,

    /// List installed presets.
    #[arg(long)]
    pub presets: bool,

    /// Test a preset (see `mm --doc other`).
    #[arg(long, value_name = "N@ALIAS | N-M | N:TEMPLATE | TEMPLATE", num_args = 0..=1, default_missing_value = "")]
    pub list: Option<String>,

    /// Display documentation
    #[arg(long, short, value_enum)]
    pub doc: Option<Doc>,
}

#[derive(Debug, Clone, clap::ValueEnum, PartialEq)]
pub enum Doc {
    Options,
    Binds,
    Template,
    #[value(alias = "lua")]
    Execute,
    Other,
    Pager,
}

impl Cli {
    /// All words parsed by clap need to be repeated here to be extracted.
    fn partition_clap_args(args: Vec<OsString>) -> (Vec<OsString>, Vec<OsString>) {
        let mut clap_args = Vec::new();
        let mut rest = Vec::new();

        let mut iter = args.iter();
        while let Some(arg) = iter.next() {
            let s = arg.to_string_lossy();

            // Flags that exit without running the matcher: when one of them
            // appears anywhere before `--`, clap parses the entire command line.
            macro_rules! skips_matcher {
                ($name:literal) => {{
                    let eq_opt = concat!("--", $name, "=");
                    let long_opt = concat!("--", $name);
                    s == long_opt || s.starts_with(eq_opt)
                }};
            }

            // Check end of options
            if s == "--" {
                clap_args.push(arg.clone());
                clap_args.extend(iter.cloned());
                break;
            }

            if skips_matcher!("doc") || skips_matcher!("list") || skips_matcher!("download") {
                return (args, Vec::new());
            }

            macro_rules! try_parse {
                ($name:literal, $prefix:expr) => {{
                    let eq_opt = concat!($prefix, $name, "=");
                    let long_opt = concat!($prefix, $name);

                    if s == long_opt || s.starts_with(eq_opt) {
                        let needs_next = s == long_opt;
                        clap_args.push(arg.clone());
                        if needs_next {
                            if let Some(next) = iter.next() {
                                clap_args.push(next.clone());
                            } else {
                                // clap will handle
                            }
                        }
                        continue;
                    }
                }};
            }

            // Long options with optional or required values
            try_parse!("config", "--");
            try_parse!("verbosity", "--");
            try_parse!("d", "-");
            try_parse!("override", "--");
            try_parse!("o", "-");
            try_parse!("C", "-");

            // Flags
            if [
                "--dump-config",
                "--presets",
                "--test-keys",
                "--last-key",
                "--no-read",
                "--help",
                "--version",
                "-V",
                "-F",
            ]
            .contains(&s.as_ref())
                || s.strip_prefix('-')
                    .is_some_and(|x| x.chars().all(|c| c == 'v') || x.chars().all(|c| c == 'q'))
            {
                clap_args.push(arg.clone());
                continue;
            }

            // Anything else
            rest.push(arg.clone());
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

#[cfg(test)]
mod tests {
    use super::*;

    fn partition(args: &[&str]) -> (Vec<String>, Vec<String>) {
        // `partition_clap_args` receives argv without the program name.
        let args: Vec<OsString> = args.iter().map(Into::into).collect();
        let (clap_args, rest) = Cli::partition_clap_args(args);
        (
            clap_args
                .into_iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect(),
            rest.into_iter()
                .map(|s| s.to_string_lossy().into_owned())
                .collect(),
        )
    }

    #[test]
    fn ui_less_flags_hand_everything_to_clap() {
        for flag in [
            "--download",
            "--download=x.toml",
            "--list",
            "--list=N@alias",
            "--doc",
            "--doc=options",
        ] {
            let (clap_args, rest) = partition(&[flag, "x.toml"]);
            assert_eq!(clap_args, vec![flag, "x.toml"], "flag {flag}");
            assert!(rest.is_empty(), "flag {flag}");
        }
    }

    #[test]
    fn matcher_args_still_partition() {
        let (clap_args, rest) = partition(&["--config", "c.toml", "binds.quit=q", "echo", "hi"]);
        assert_eq!(clap_args, vec!["--config", "c.toml"]);
        assert_eq!(rest, vec!["binds.quit=q", "echo", "hi"]);
    }

    #[test]
    fn end_of_options_is_respected() {
        let (clap_args, rest) = partition(&["--", "echo", "--download"]);
        assert_eq!(clap_args, vec!["--", "echo", "--download"]);
        assert!(rest.is_empty());
    }
}
