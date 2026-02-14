use std::process::exit;

mod clap;
mod config;
mod crokey;
mod parse;
mod paths;
mod setup;
mod utils;

use ::clap::Parser;
use clap::*;
use cli_boilerplate_automation::bog::BogOkExt;
use matchmaker::{MatchError, preview::AppendOnly};
use paths::*;
use setup::*;
use utils::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    #[cfg(debug_assertions)]
    let verbosity = 6;
    #[cfg(not(debug_assertions))]
    let verbosity = 3;

    init_logger(verbosity, &logs_dir().join(format!("{BINARY_SHORT}.log")));

    let cli = Cli::parse();
    log::debug!("{cli:?}");

    // get config
    let config = enter(cli).__ebog();
    let delimiter = config.matcher.start.output_separator.clone();
    let print = AppendOnly::new();

    // begin
    match start(config, print.clone()).await {
        Ok(iter) => {
            print.map_to_vec(|s| println!("{s}"));

            let sep = delimiter.as_deref().unwrap_or("\n");
            let output = iter
                .into_iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(sep);
            println!("{output}");
        }
        Err(err) => match err {
            MatchError::Abort(i) => {
                exit(i);
            }
            MatchError::EventLoopClosed => {
                exit(127);
            }
            _ => {
                unreachable!()
            }
        },
    };
}
