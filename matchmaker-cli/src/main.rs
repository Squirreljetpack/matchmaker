use matchmaker_partial::Set;
use std::process::exit;

mod clap;
mod config;
mod crokey;
mod parse;
mod paths;
mod setup;
mod utils;

use anyhow::bail;
use clap::*;
use config::PartialConfig;
use paths::*;
use setup::*;
use utils::*;

use cli_boilerplate_automation::{bog::BogOkExt, text::split::split_nesting};
use matchmaker::{MatchError, preview::AppendOnly};

use crate::parse::{get_pairs, try_split_kv};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    #[cfg(debug_assertions)]
    let verbosity = 6;
    #[cfg(not(debug_assertions))]
    let verbosity = 3;

    init_logger(verbosity, &logs_dir().join(format!("{BINARY_SHORT}.log")));

    let (cli, config_args) = Cli::get_partitioned_args();
    log::debug!("{cli:?}, {config_args:?}");

    display_doc(&cli);

    // get config overrides
    let partial = get_partial(config_args).__ebog();
    log::trace!("{partial:?}");

    // get config
    let config = enter(cli, partial).__ebog();

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

fn get_partial(config_args: Vec<String>) -> anyhow::Result<PartialConfig> {
    let split = get_pairs(config_args)?;
    log::trace!("{split:?}");
    let mut partial = PartialConfig::default();
    for (path, val) in split {
        let parts = match split_nesting(&val, '[', ']') {
            Ok(mut parts) => {
                let is_binds = parts.len() == 1 && ["binds", "b"].contains(&parts[0].as_ref());
                try_split_kv(&mut parts, is_binds)?;
                parts
            }
            Err(n) => {
                if n > 0 {
                    bail!("Encountered {} unclosed parentheses", n)
                } else {
                    bail!("Extra closing parenthesis at index {}", -n)
                }
            }
        };

        log::trace!("{parts:?}");

        partial.set(path.as_slice(), &parts)?;
    }

    Ok(partial)
}

fn display_doc(cli: &Cli) {
    use termimad::MadSkin;
    use termimad::crossterm::style::Color;

    let mut md = String::new();
    if cli.options {
        md.push_str(include_str!("../assets/docs/options.md"));
    }
    if cli.binds {
        md.push_str(include_str!("../assets/docs/binds.md"));
    }

    if !md.is_empty() {
        let mut skin = MadSkin::default();
        skin.bold.set_fg(Color::Yellow);
        skin.print_text(&md);
        exit(0)
    }
}
