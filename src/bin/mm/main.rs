use std::process::exit;


mod crokey;
mod parse;
mod types;
mod utils;
mod setup;

use matchmaker::{
    MatchError, Result, proc::AppendOnly
};
use types::*;
use utils::*;
use setup::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_logger(&logs_dir().join(format!("{BINARY_SHORT}.log")));

    // get config
    let config = enter()?;
    let delimiter = config.matcher.start.output_separator.clone();
    let print = AppendOnly::new();

    // begin
    match pick(config, print.clone()).await {
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
        Err(err) => {
            match err {
                MatchError::Abort(i) => {
                    exit(i);
                }
                MatchError::EventLoopClosed => {
                    exit(127);
                }
                _ => {
                    unreachable!()
                }
            }
        }
    }

    Ok(())
}
