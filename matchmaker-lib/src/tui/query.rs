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
    /// Graphics capabilities, queried only when images are enabled.
    #[cfg(feature = "image")]
    pub image: ImageQuery,
}

/// Graphics capabilities reported by the terminal, and the decisions each one
/// feeds when [`Tui::image_picker`](super::Tui::image_picker) builds the preview
/// pane's picker:
///
/// An image is a rectangle of pixels and the pane it is shown in is a rectangle of
/// character cells, so scaling one onto the other needs the pixel size of a cell.
/// ratatui-image calls that rectangle a font size and keeps it in the picker, which
/// multiplies it by a cell count to get the pixels it may draw in, and divides it
/// by an image's size to get the cells the image needs.
///
/// - [`font_size`](Self::font_size) is that rectangle. When the terminal reported
///   none, it is derived from the window size instead, and when that fails too there
///   is no cell size to work with at all: only halfblocks can render without one,
///   through the picker's built-in 10x20 placeholder cell.
/// - [`kitty`](Self::kitty) and [`sixel`](Self::sixel) name the protocol through
///   [`protocol`](Self::protocol), kitty first because it is the only protocol
///   with rectangular operations and compression. That protocol applies only to
///   [`ImageProtocol::Auto`](crate::config::ImageProtocol::Auto); any other
///   configured protocol is forced over it, so images are switched off by
///   `ImageProtocol::None` alone.
/// - Neither flag set means no protocol was reported, not that none is
///   supported: a terminal can leave the probes unanswered (a tmux pane does
///   not answer the kitty query in a form the parser matches). The picker then
///   keeps the protocol its constructor inferred from the environment (tmux and
///   iTerm2 hints), or halfblocks.
///
/// A terminal that answers nothing at all leaves every field at its default,
/// which still renders through halfblocks.
#[cfg(feature = "image")]
#[derive(Debug, Clone, Copy, Default)]
pub struct ImageQuery {
    /// Pixel size of one character cell, as `(width, height)`, from the terminal's
    /// answer to the cell size query (`CSI 16 t`, answered with `height;width`).
    pub font_size: Option<(u16, u16)>,
    /// Whether the terminal answered the kitty graphics probe.
    pub kitty: bool,
    /// Whether the terminal advertised sixel in its DA1 attributes.
    pub sixel: bool,
}

#[cfg(feature = "image")]
impl ImageQuery {
    /// The protocol the terminal reported, most capable first, or `None` when
    /// it reported no graphics protocol. Applied before the configured one, so
    /// it is what [`ImageProtocol::Auto`](crate::config::ImageProtocol::Auto)
    /// resolves to.
    pub fn protocol(&self) -> Option<ratatui_image::picker::ProtocolType> {
        // Kitty takes precedence: it is the only protocol with
        // rectangular operations and compression.
        if self.kitty {
            Some(ratatui_image::picker::ProtocolType::Kitty)
        } else if self.sixel {
            Some(ratatui_image::picker::ProtocolType::Sixel)
        } else {
            None
        }
    }

    fn record(&mut self, response: ratatui_image::picker::cap_parser::Response) {
        use ratatui_image::picker::cap_parser::Response;
        match response {
            Response::Kitty => self.kitty = true,
            Response::Sixel => self.sixel = true,
            Response::CellSize(Some(size)) => self.font_size = Some(size),
            _ => {}
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::Startup;
    use crate::config::ImageProtocol;
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
    pub fn startup(timeout: Duration, image_protocol: ImageProtocol) -> Result<Startup> {
        let mut tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("Failed to open /dev/tty")?;

        tty.write_all(PASTE_QUERY.as_bytes())?;
        #[cfg(feature = "image")]
        if image_protocol != ImageProtocol::None {
            tty.write_all(image_query().as_bytes())?;
        }
        #[cfg(not(feature = "image"))]
        let _ = image_protocol;
        tty.write_all(CURSOR_QUERY.as_bytes())?;
        tty.flush()?;

        let deadline = Instant::now() + timeout;
        let mut out = Startup::default();
        let mut buf = String::new();
        #[cfg(feature = "image")]
        let mut image_parser = ratatui_image::picker::cap_parser::Parser::new();

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
            let text = String::from_utf8_lossy(&chunk[..n]);

            #[cfg(feature = "image")]
            for response in text.chars().flat_map(|c| image_parser.push(c)) {
                out.image.record(response);
            }

            buf.push_str(&text);

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

    /// The graphics capability query, mirroring what ratatui-image's picker
    /// asks for when it queries stdio itself, minus the probes whose answers
    /// matchmaker has no use for (background color, text sizing).
    #[cfg(feature = "image")]
    fn image_query() -> String {
        use ratatui_image::picker::{
            ProtocolType,
            cap_parser::{Parser, QueryStdioOptions},
        };

        // Mirror ratatui-image's own tmux detection: inside tmux the query must
        // be wrapped in a passthrough sequence.
        let is_tmux = std::env::var("TERM").is_ok_and(|t| t.starts_with("tmux"))
            || std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "tmux");

        let mut options = QueryStdioOptions::default();
        // Terminals that claim kitty/sixel support but render it incorrectly.
        if std::env::var("WEZTERM_EXECUTABLE").is_ok_and(|v| !v.is_empty())
            || std::env::var("KONSOLE_VERSION").is_ok_and(|v| !v.is_empty())
        {
            options.blacklist_protocols = vec![ProtocolType::Kitty, ProtocolType::Sixel];
        }

        Parser::query(is_tmux, options)
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
    ///
    /// The buffer may hold other query replies before it, so every `ESC [`
    /// sequence is considered; only the cursor report is parsed as digits,
    /// separator, digits, `R`.
    fn take_cursor_position(buf: &mut String) -> Option<(u16, u16)> {
        let mut from = 0;
        while let Some(pos) = buf[from..].find("\x1b[") {
            let start = from + pos + 2;
            if !buf[start..].starts_with(|c: char| c.is_ascii_digit()) {
                from = start;
                continue;
            }

            let bytes = buf.as_bytes();
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_digit() || bytes[end] == b';') {
                end += 1;
            }
            if end >= bytes.len() {
                // Incomplete report, wait for the rest.
                return None;
            }
            if bytes[end] != b'R' {
                from = start;
                continue;
            }

            let mut parts = buf[start..end].split(';');
            let row: u16 = parts.next()?.parse().ok()?;
            let col: u16 = parts.next()?.parse().ok()?;

            buf.drain(..=end);
            return Some((col.saturating_sub(1), row.saturating_sub(1)));
        }
        None
    }
}

#[cfg(windows)]
mod imp {
    use super::Startup;
    use crate::config::ImageProtocol;
    use anyhow::Result;
    use std::time::Duration;

    /// The console API answers the cursor query directly; there is no reply to
    /// parse for bracketed paste or for graphics capabilities, so their state
    /// stays unknown.
    pub fn startup(_timeout: Duration, _image_protocol: ImageProtocol) -> Result<Startup> {
        Ok(Startup {
            cursor: Some(crossterm::cursor::position()?),
            ..Startup::default()
        })
    }
}

pub use imp::startup;
