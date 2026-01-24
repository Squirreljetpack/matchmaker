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
}
