use matchmaker_partial::Set;
use std::process::exit;

mod action;
mod clap;
mod config;
mod crokey;
mod parse;
mod paths;
mod start;
mod utils;

use anyhow::bail;
use clap::*;
use config::PartialConfig;
use paths::*;
use start::*;
use utils::*;

use cli_boilerplate_automation::{
    bait::ResultExt, bog::BogOkExt, bring::split::split_nesting, ebog,
};
use matchmaker::MatchError;

use crate::parse::{get_pairs, try_split_kv};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let (cli, config_args) = Cli::get_partitioned_args();

    init_logger(
        [cli.quiet, cli.verbose],
        &state_dir().join(format!("{BINARY_SHORT}.log")),
    );
    log::debug!("{cli:?}, {config_args:?}");

    display_doc(&cli);

    // get config overrides
    let partial = get_partial(config_args).__ebog();
    log::trace!("{partial:?}");

    let no_read = cli.no_read;
    // get config
    let config = enter(cli, partial).__ebog();

    // begin
    match start(config, no_read).await {
        Ok(_) => {}
        Err(err) => match err {
            MatchError::Abort(i) => {
                exit(i);
            }
            MatchError::EventLoopClosed => {
                exit(127);
            }
            MatchError::TUIError(e) => {
                ebog!("TUI"; "{e}")
            }
            _ => unreachable!(),
        },
    };
}

fn get_partial(config_args: Vec<String>) -> anyhow::Result<PartialConfig> {
    let split = get_pairs(config_args)?;
    log::trace!("{split:?}");
    let mut partial = PartialConfig::default();
    for (path, val) in split {
        let parts = match split_nesting(&val, ['(', ')'], ['[', ']']) {
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

        partial
            .set(path.as_slice(), &parts)
            .prefix(format!("Invalid value for {}", path.join(".")))?;
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
