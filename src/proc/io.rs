use crate::{MMItem, MapReaderError, Result};
use log::{error, warn};
use std::{io::{self, BufRead, Read, Write}, process::Stdio};

pub fn read_to_chunks<R: Read>(reader: R, delim: char) -> std::io::Split<std::io::BufReader<R>> {
    io::BufReader::new(reader).split(delim as u8)
}

// do not use for newlines as it doesn't handle \r!
// todo: warn about this in config
// note: stream means wrapping with closure passed stream::unfold and returning f() inside

pub fn map_chunks<const INVALID_FAIL: bool, E>(iter: impl Iterator<Item = std::io::Result<Vec<u8>>>, mut f: impl FnMut(String) -> Result<(), E>) -> Result<(), MapReaderError<E>>
{
    for (i, chunk_result) in iter.enumerate() {
        if i == u32::MAX as usize {
            warn!("Reached maximum segment limit, stopping input read");
            return Err(MapReaderError::ChunkError(i));
        }

        let chunk = match chunk_result {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("Error reading chunk: {e}");
                return Err(MapReaderError::ChunkError(i));
            }
        };

        match String::from_utf8(chunk) {
            Ok(s) => {
                if let Err(e) = f(s) {
                    return Err(MapReaderError::Custom(e));
                }
            }
            Err(e) => {
                error!("Invalid UTF-8 in stdin at byte {}: {}", e.utf8_error().valid_up_to(), e);
                // Skip but continue reading
                if INVALID_FAIL {
                    return Err(MapReaderError::ChunkError(i));
                } else {
                    continue
                }
            }
        }
    }
    Ok(())
}


pub fn map_reader_lines<const INVALID_FAIL: bool, E>(reader: impl Read, mut f: impl FnMut(String) -> Result<(), E>) -> Result<(), MapReaderError<E>> {
    let buf_reader = io::BufReader::new(reader);

    for (i, line) in buf_reader.lines().enumerate() {
        if i == u32::MAX as usize {
            eprintln!("Reached maximum line limit, stopping input read");
            return Err(MapReaderError::ChunkError(i));
        }
        match line {
            Ok(l) => {
                if let Err(e) = f(l) {
                    return Err(MapReaderError::Custom(e));
                }
            }
            Err(e) => {
                eprintln!("Error reading line: {}", e);
                if INVALID_FAIL {
                    return Err(MapReaderError::ChunkError(i));
                } else {
                    continue
                }
            }
        }
    }
    Ok(())
}

/// Spawns a tokio task mapping f to reader segments.
/// Read aborts on error. Read errors are logged.
pub fn map_reader<E: MMItem>(reader: impl Read + MMItem, f: impl FnMut(String) -> Result<(), E> + MMItem, input_separator: Option<char>) -> tokio::task::JoinHandle<Result<(), MapReaderError<E>>> {
    if let Some(delim) = input_separator {
        tokio::spawn(async move {
            map_chunks::<true, E>(read_to_chunks(reader, delim), f)
        })
    } else {
        tokio::spawn(async move {
            map_reader_lines::<true, E>(reader, f)
        })
    }
}


// ---------------------------------------------------------------------

pub fn tty_or_null() -> Stdio {
    if let Ok(mut tty) = std::fs::File::open("/dev/tty") {
        let _ = tty.flush(); // does nothing but seems logical
        Stdio::from(tty)
    } else {
        error!("Failed to open /dev/tty");
        Stdio::inherit()
    }
}