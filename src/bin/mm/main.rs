use std::{io::{self}, process::{Stdio, exit}};


mod crokey;
mod parse;
mod types;
mod utils;
mod setup;

use log::debug;
use matchmaker::{
    MatchError,
    Result,
    config::StartConfig,
    event::EventLoop,
    nucleo::injector::Injector,
    proc::{
        map_reader,
        spawn,
    },
};
use types::*;
use utils::*;
use setup::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_logger(&logs_dir().join(format!("{BINARY_SHORT}.log")));

    // get config
    let mut config = enter()?;

    let StartConfig { input_separator, default_command, sync, output_separator } = config.matcher.start.clone();

    let event_loop = EventLoop::with_binds(std::mem::take(&mut config.binds)).with_tick_rate(config.render.tick_rate());

    // make matcher and matchmaker with matchmaker-and-matcher-maker
    let (mm, injector, mut matcher, previewer, print) = make_mm(config, &event_loop);
    debug!("{mm:?}");

    // read stdin
    if !atty::is(atty::Stream::Stdin) {
        let stdin = io::stdin();
        let read_handle = map_reader(
            stdin,
            move |line| {
                injector.push(line).map_err(|e| e.into())
            },
            input_separator
        );

        if sync {
            let _ = read_handle.await;
        }
    } else if !default_command.is_empty() {
        if let Some(mut child) = spawn(&default_command, vec![], Stdio::null(), Stdio::piped(), Stdio::null())
        && let Some(stdout) = child.stdout.take() {
            let _handle = map_reader(
                stdout,
                move |line| {
                    injector.push(line).map_err(|e| e.into())
                },
                input_separator
            );
        } else {
            eprintln!("error: no stdout from default command.");
        }
    } else {
        eprintln!("error: no input detected.");
    }

    // begin
    tokio::spawn(async move {
        let _ = previewer.run().await;
    });

    match mm.pick_with(&mut matcher, event_loop).await {
        Ok(iter) => {
            print.map_to_vec(|s| println!("{s}"));

            let sep = output_separator.as_deref().unwrap_or("\n");
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
