use crate::Result;
use log::{debug, error, warn};
use std::{io::{self, BufRead, Read, Write}, process::Stdio};

pub fn stdin_reader() -> Option<io::Stdin> {
    if atty::is(atty::Stream::Stdin) {
        error!("stdin is a TTY: picker requires piped input.");
        return None
    }

    Some(io::stdin())
}

pub fn read_to_chunks<R: Read>(reader: R, delim: char) -> std::io::Split<std::io::BufReader<R>> {
    io::BufReader::new(reader).split(delim as u8)
}

// do not use for newlines as it doesn't handle \r!
// todo: warn about this in config
pub fn map_chunks<const INVALID_FAIL: bool>(iter: impl Iterator<Item = std::io::Result<Vec<u8>>>, mut f: impl FnMut(String) -> Result<()>)
{
    for (i, chunk_result) in iter.enumerate() {
        if i == u32::MAX as usize {
            warn!("Reached maximum segment limit, stopping input read");
            break;
        }

        let chunk = match chunk_result {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error reading from stdin: {e}");
                break;
            }
        };

        match String::from_utf8(chunk) {
            Ok(s) => {
                debug!("Read: {s}");
                if f(s).is_err() {
                    break;
                }
            }
            Err(e) => {
                error!("Invalid UTF-8 in stdin at byte {}: {}", e.utf8_error().valid_up_to(), e);
                // Skip but continue reading
                if INVALID_FAIL {
                    break
                } else {
                    continue
                }
            }
        }
    }
}

pub fn map_reader_lines<const INVALID_FAIL: bool>(reader: impl Read, mut f: impl FnMut(String) -> Result<()>) {
    let buf_reader = io::BufReader::new(reader);

    for (i, line) in buf_reader.lines().enumerate() {
        if i == u32::MAX as usize {
            eprintln!("Reached maximum line limit, stopping input read");
            break;
        }
        match line {
            Ok(l) => {
                if f(l).is_err() {
                    break;
                }
            }
            Err(e) => {
                eprintln!("Error reading line: {}", e);
                if INVALID_FAIL {
                    break
                } else {
                    continue
                }
            }
        }
    }
}

pub fn tty_or_null() -> Stdio {
    if let Ok(mut tty) = std::fs::File::open("/dev/tty") {
        let _ = tty.flush(); // does nothing but seems logical
        Stdio::from(tty)
    } else {
        error!("Failed to open /dev/tty");
        Stdio::inherit()
    }
}