use super::clap::Cli;
use clap::{Parser, error::ErrorKind};

pub fn parse(args: Vec<String>) -> Cli {
    match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => match err.kind() {
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => {
                err.print().expect("Failed to print help/version");
                std::process::exit(0);
            }
            _ => err.exit(),
        },
    }
}
