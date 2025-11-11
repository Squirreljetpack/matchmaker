use std::{process::{Stdio, exit}};


mod crokey;
mod parse;
mod types;
mod utils;
mod setup;

use log::debug;
use matchmaker::{
    MatchError, Result, config::{StartConfig}, nucleo::injector::
    Injector, proc::
    {spawn, map_chunks, map_reader_lines, read_to_chunks, stdin_reader}
};
use std::io::{stdout, Write};
use types::*;
use utils::*;
use setup::*;

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    init_logger(&logs_dir().join(format!("{BINARY_SHORT}.log")));
    
    // get config
    let config = enter()?;
    
    let StartConfig { input_separator, default_command, sync, output_separator } = config.matcher.run.clone();
    
    // make matcher and matchmaker with matchmaker and matcher maker
    let (mm, injector, mut matcher, previewer, print) = make_mm(config);
    debug!("{mm:?}");
    
    // read stdin
    if let Some(stdin) = stdin_reader() {
        let read_handle = if let Some(c) = input_separator {
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
    } else if !default_command.is_empty() {
        if let Some(mut child) = spawn(&default_command, vec![], Stdio::null(), Stdio::piped(), Stdio::null()) {
            if let Some(stdout) = child.stdout.take() {
                let _handle = if let Some(delim) = input_separator {
                    tokio::spawn(async move {
                        map_chunks::<true>(read_to_chunks(stdout, delim), |line| injector.push(line).map_err(|e| e.into()))
                    })
                } else {
                    tokio::spawn(async move {
                        map_reader_lines::<true>(stdout, |line| injector.push(line).map_err(|e| e.into()))
                    })
                };
            } else {
                eprintln!("error: no stdout detected from default command.");
            }
        }
    } else {
        eprintln!("error: no input detected.");
    }
    
    // spawn previewer
    tokio::spawn(async move {
        let _ = previewer.run().await;
    });
    
    // begin
    match mm.pick_with_matcher(&mut matcher).await {
        Ok(iter) => {
            print.map_to_vec(|s| println!("{s}"));

            let sep = output_separator.as_deref().unwrap_or("\n");
            let output = iter
            .into_iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .join(sep);
            writeln!(stdout(), "{output}")?;
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
