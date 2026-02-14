use std::ffi::OsString;

use clap::Parser;

pub static BINARY_FULL: &str = "matchmaker";
pub static BINARY_SHORT: &str = "mm";

#[derive(Debug, Parser, Default, Clone)]
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
}
