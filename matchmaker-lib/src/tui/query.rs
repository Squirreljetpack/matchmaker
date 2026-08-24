//! Escape-sequence queries against the controlling terminal.
//!
//! This is the "terminal implementation" layer: raw request/reply exchanges
//! over the tty device, independent of crossterm's event handling.

/// Result of the startup query round trip.
#[derive(Debug, Clone, Copy, Default)]
pub struct Startup {
    /// Cursor position (col, row), if the terminal answered the DSR query.
    pub cursor: Option<(u16, u16)>,
    /// Whether bracketed paste (DECSET 2004) was enabled; `None` when the
    /// terminal did not answer the DECRQM query.
    pub bracketed_paste: Option<bool>,
}

#[cfg(unix)]
mod imp {
    use super::Startup;
    use anyhow::{Context, Result};
    use nix::sys::{
        select::{FdSet, select},
        time::{TimeVal, TimeValLike},
    };
    use std::{
        fs::OpenOptions,
        io::{Read, Write},
        os::fd::AsFd,
        time::{Duration, Instant},
    };

    const CURSOR_QUERY: &str = "\x1b[6n";
    const PASTE_QUERY: &str = "\x1b[?2004$p";

    /// Ask the terminal for everything we need in a single round trip: the
    /// queries go out in one write and the cursor position query is last,
    /// because every terminal answers it. Its reply therefore bounds the wait:
    /// a reply still missing by then belongs to a query the terminal does not
    /// know (`None` in [`Startup`]), rather than one we stopped waiting for
    /// too early.
    ///
    /// Requires raw mode so replies are not echoed or interpreted.
    pub fn startup(timeout: Duration) -> Result<Startup> {
        let mut tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("Failed to open /dev/tty")?;

        tty.write_all(PASTE_QUERY.as_bytes())?;
        tty.write_all(CURSOR_QUERY.as_bytes())?;
        tty.flush()?;

        let deadline = Instant::now() + timeout;
        let mut out = Startup::default();
        let mut buf = String::new();

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }

            let ready = {
                let mut fds = FdSet::new();
                fds.insert(tty.as_fd());
                let mut tv = TimeVal::milliseconds(remaining.as_millis() as i64);
                select(None, &mut fds, None, None, Some(&mut tv)).context("select() failed")?
            };
            if ready == 0 {
                break;
            }

            let mut chunk = [0u8; 64];
            let n = tty.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            buf.push_str(&String::from_utf8_lossy(&chunk[..n]));

            if out.bracketed_paste.is_none() {
                out.bracketed_paste = take_paste_report(&mut buf);
            }
            if out.cursor.is_none() {
                out.cursor = take_cursor_position(&mut buf);
            }
            if out.cursor.is_some() {
                break;
            }
        }

        Ok(out)
    }

    /// Cut the first DECRQM report (`ESC [ ? 2004 ; status $ y`) out of the
    /// buffer and interpret its status: 1 (set) and 3 (permanently set) mean
    /// enabled, 0/2/4 mean not enabled.
    fn take_paste_report(buf: &mut String) -> Option<bool> {
        const HEAD: &str = "\x1b[?2004;";
        let start = buf.find(HEAD)?;
        let end = start + buf[start..].find("$y")? + "$y".len();

        let status: u32 = buf[start + HEAD.len()..end - "$y".len()].parse().ok()?;
        buf.drain(..end);
        Some(matches!(status, 1 | 3))
    }

    /// Cut the first cursor position report (`ESC [ row ; col R`) out of the
    /// buffer and return (col, row) as 0-based coordinates.
    fn take_cursor_position(buf: &mut String) -> Option<(u16, u16)> {
        let start = buf.find("\x1b[")?;
        // the paste report starts with `ESC [ ?`, only the cursor report is
        // followed directly by a digit
        if !buf[start + 2..].starts_with(|c: char| c.is_ascii_digit()) {
            return None;
        }
        let r = start + buf[start..].find('R')?;
        let body = &buf[start + 2..r];

        let mut parts = body.split(';');
        let row: u16 = parts.next()?.parse().ok()?;
        let col: u16 = parts.next()?.parse().ok()?;

        buf.drain(..r + 1);
        Some((col - 1, row - 1))
    }
}

#[cfg(windows)]
mod imp {
    use super::Startup;
    use anyhow::Result;
    use std::time::Duration;

    /// The console API answers the cursor query directly; there is no reply to
    /// parse for bracketed paste, so its state stays unknown.
    pub fn startup(_timeout: Duration) -> Result<Startup> {
        Ok(Startup {
            cursor: Some(crossterm::cursor::position()?),
            ..Startup::default()
        })
    }
}

pub use imp::startup;
