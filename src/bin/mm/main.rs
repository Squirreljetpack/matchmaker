use std::{env, process::exit};

use log::debug;
use matchmaker::{
    Matchmaker, MatchmakerError, Result,
    config::{
        Config,
        utils::{get_config, write_config},
    },
    message::Interrupt,
    nucleo::injector::Injector,
    spawn::AppendOnly,
    tui::{map_chunks, map_reader_lines, read_to_chunks, stdin_reader},
};

mod crokey;
mod parse;
mod types;
mod utils;

use parse::*;
use types::*;
use utils::*;

pub fn enter() -> Result<Config> {
    let args = env::args();
    let cli = parse(args.collect());
    log::debug!("{cli:?}");

    if cli.dump_config {
        write_config(&cli.config)?;
        exit(0);
    }
    if cli.test_keys {
        crokey::main();
        exit(0);
    }

    write_config(&cli.config)?;
    let mut config = get_config(&cli.config)?;
    cli.merge_config(&mut config);

    log::debug!("{config:?}");

    Ok(config)
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_logger(&logs_dir().join("picker.log"));

    let config = enter()?;
    let sync = config.matcher.exit.sync;
    let delimiter = config.matcher.delimiter;
    let (mut mm, injector, _) = Matchmaker::new_from_config(config);

    let print = AppendOnly::new();

    let _print = print.clone();
    let formatter = mm.worker.make_format_fn::<false>(|item| &item.inner.inner);
    mm.register_interrupt_handler(
        matchmaker::message::Interrupt::Print("".into()),
        move |state, i| {
            if let Interrupt::Print(template) = i {
                if let Some(t) = state.current_raw() {
                    let s = formatter(t, template);
                    _print.push(s);
                }
            }
        },
    );
    debug!("{mm:?}");

    if let Some(stdin) = stdin_reader() {
        let read_handle = if let Some(c) = delimiter {
            tokio::spawn(async move {
                map_chunks::<true>(read_to_chunks(stdin, c), |line| {
                    injector.push(line).map_err(|e| e.into())
                })
            })
        } else {
            tokio::spawn(async move {
                map_reader_lines::<true>(stdin, |line| injector.push(line).map_err(|e| e.into()))
            })
        };

        if sync {
            let _ = read_handle.await;
        }
    }

    match mm.pick().await {
        Ok(iter) => {
            print.map_to_vec(|s| println!("{s}"));
            for s in iter {
                println!("{s}");
            }
        }
        Err(err) => {
            if let Some(e) = err.downcast_ref::<MatchmakerError>() {
                match e {
                    MatchmakerError::Abort(i) => {
                        exit(*i);
                    }
                    MatchmakerError::EventLoopClosed => {
                        exit(127);
                    }
                    _ => {
                        unreachable!()
                    }
                }
            } else {
                eprintln!("Other error: {err}");
            }
        }
    }

    Ok(())
}
