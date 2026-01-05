use crate::{Result, config::TerminalConfig};
use cli_boilerplate_automation::bait::ResultExt;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode},
};
use log::{debug, error};
use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect, prelude::CrosstermBackend};
use serde::{Deserialize, Serialize};
use std::{
    io::{self, Write},
    thread::sleep,
    time::Duration,
};
pub struct Tui<W>
where
    W: Write,
{
    pub terminal: ratatui::Terminal<CrosstermBackend<W>>,
    pub area: Rect,
    pub config: TerminalConfig,
}

impl<W> Tui<W>
where
    W: Write,
{
    // waiting on https://github.com/ratatui/ratatui/issues/984 to implement growable inline, currently just tries to request max
    // if max > than remainder, then scrolls up a bit
    pub fn new_with_writer(writer: W, mut config: TerminalConfig) -> Result<Self> {
        let mut backend = CrosstermBackend::new(writer);
        let mut options = TerminalOptions::default();
        if config.sleep_ms.is_zero() {
            config.sleep_ms = Duration::from_millis(100)
        };

        // important for getting cursor
        crossterm::terminal::enable_raw_mode()?;

        let (width, height) = Self::full_size().unwrap_or_default();
        let area = if let Some(ref layout) = config.layout {
            let request = layout
                .percentage
                .compute_with_max(height, layout.max)
                .min(height);

            let cursor_y = Self::get_cursor_y(config.sleep_ms).unwrap_or_else(|e| {
                error!("Failed to read cursor: {e}");
                height - 1 // overestimate
            });

            let initial_height = height.saturating_sub(cursor_y);

            let scroll = request.saturating_sub(initial_height);
            debug!("TUI dimensions: {width}, {height}. Cursor_y: {cursor_y}.",);

            // ensure available by scrolling
            let cursor_y = match Self::scroll_up(&mut backend, scroll) {
                Ok(_) => {
                    cursor_y.saturating_sub(scroll) // the requested cursor doesn't seem updated so we assume it succeeded
                }
                Err(_) => cursor_y,
            };
            let available_height = height.saturating_sub(cursor_y);

            debug!(
                "TUI quantities: min: {}, initial_available: {initial_height}, requested: {request}, available: {available_height}, requested scroll: {scroll}",
                layout.min
            );

            if available_height < layout.min {
                error!("Failed to allocate minimum height, falling back to fullscreen");
                Rect::new(0, 0, width, height)
            } else {
                let area = Rect::new(
                    0,
                    cursor_y,
                    width,
                    available_height.min(request).max(layout.min),
                );

                // options.viewport = Viewport::Inline(available_height.min(request));
                options.viewport = Viewport::Fixed(area);

                area
            }
        } else {
            Rect::new(0, 0, width, height)
        };

        debug!("TUI area: {area}");

        let terminal = Terminal::with_options(backend, options)?;
        Ok(Self {
            terminal,
            config,
            area,
        })
    }

    pub fn enter(&mut self) -> Result<()> {
        let fullscreen = self.is_fullscreen();
        let backend = self.terminal.backend_mut();
        crossterm::terminal::enable_raw_mode()?; // duplicate but crossterm checks this
        execute!(backend, EnableMouseCapture)?;
        #[cfg(feature = "bracketed-paste")]
        execute!(backend, crossterm::event::EnableBracketedPaste)?;

        if fullscreen {
            self.enter_alternate()?;
        }
        Ok(())
    }

    pub fn enter_alternate(&mut self) -> Result<()> {
        let backend = self.terminal.backend_mut();
        execute!(backend, EnterAlternateScreen)?;
        execute!(backend, crossterm::terminal::Clear(ClearType::All))?;
        self.terminal.clear()?;
        debug!("Entered alternate screen");
        Ok(())
    }

    pub fn enter_execute(&mut self) {
        self.exit();
        sleep(self.config.sleep_ms); // necessary to give resize some time
        debug!("state: {:?}", crossterm::terminal::is_raw_mode_enabled());

        // do we ever need to scroll up?
    }

    pub fn resize(&mut self, area: Rect) {
        self.terminal.resize(area)._elog();
        self.area = area
    }

    pub fn redraw(&mut self) {
        self.terminal.resize(self.area)._elog();
    }

    pub fn return_execute(&mut self) -> Result<()> {
        self.enter()?;
        if !self.is_fullscreen() {
            // altho we cannot resize the viewport, this is the best we can do
            self.enter_alternate()._elog();
        }
        sleep(self.config.sleep_ms);
        log::debug!("During return, slept {}", self.config.sleep_ms.as_millis());

        execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::Clear(ClearType::All)
        )
        ._wlog();

        if self.is_fullscreen() || self.config.restore_fullscreen {
            if let Some((width, height)) = Self::full_size() {
                self.resize(Rect::new(0, 0, width, height));
            } else {
                error!("Failed to get terminal size");
                self.resize(self.area);
            }
        } else {
            self.resize(self.area);
        }

        Ok(())
    }

    pub fn exit(&mut self) {
        let backend = self.terminal.backend_mut();

        // if !fullscreen {
        execute!(
            backend,
            crossterm::cursor::MoveTo(0, self.area.y),
            crossterm::terminal::Clear(ClearType::FromCursorDown)
        )
        ._elog();
        // } else {
        //     if let Err(e) = execute!(backend, cursor::MoveTo(0, 0)) {
        //         warn!("Failed to move cursor: {:?}", e);
        //     }
        // }

        execute!(backend, LeaveAlternateScreen, DisableMouseCapture)._wlog();

        self.terminal.show_cursor()._wlog();

        disable_raw_mode()._wlog();

        debug!("Terminal exited");
    }

    // note: do not start before event stream
    pub fn get_cursor_y(timeout: Duration) -> io::Result<u16> {
        // crossterm uses stdout to determine cursor position
        // todo: workarounds?
        // #[cfg(not(target_os = "windows"))]
        Ok(if !atty::is(atty::Stream::Stdout) {
            utils::query_cursor_position(timeout)
                .map_err(io::Error::other)?
                .1
        } else {
            crossterm::cursor::position()?.1
        })
    }

    pub fn scroll_up(backend: &mut CrosstermBackend<W>, lines: u16) -> io::Result<u16> {
        execute!(backend, crossterm::terminal::ScrollUp(lines))?;
        Ok(0) // not used
        // Self::get_cursor_y() // note: do we want to skip this for speed
    }
    pub fn size() -> io::Result<(u16, u16)> {
        crossterm::terminal::size()
    }
    pub fn full_size() -> Option<(u16, u16)> {
        if let Ok((width, height)) = Self::size() {
            Some((width, height))
        } else {
            error!("Failed to read terminal size");
            None
        }
    }
    pub fn is_fullscreen(&self) -> bool {
        self.config.layout.is_none()
    }
    pub fn set_fullscreen(&mut self) {
        self.config.layout = None;
    }
}

impl Tui<Box<dyn Write + Send>> {
    pub fn new(config: TerminalConfig) -> Result<Self> {
        let writer = config.stream.to_stream();
        let tui = Self::new_with_writer(writer, config)?;
        Ok(tui)
    }
}

impl<W> Drop for Tui<W>
where
    W: Write,
{
    fn drop(&mut self) {
        self.exit();
    }
}

// ---------- IO ---------------

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub enum IoStream {
    Stdout,
    #[default]
    BufferedStderr,
}

impl IoStream {
    pub fn to_stream(&self) -> Box<dyn std::io::Write + Send> {
        match self {
            IoStream::Stdout => Box::new(io::stdout()),
            IoStream::BufferedStderr => Box::new(io::LineWriter::new(io::stderr())),
        }
    }
}

// ------------------------------------------------------------

mod utils {
    use anyhow::{Result, bail};
    use cli_boilerplate_automation::bait::{OptionExt, ResultExt};
    use std::{
        fs::OpenOptions,
        io::{Read, Write},
        time::Duration,
    };

    /// Query the terminal for the current cursor position (col, row)
    /// Needed because crossterm implementation fails when stdout is not connected.
    /// Requires raw mode
    pub fn query_cursor_position(timeout: Duration) -> Result<(u16, u16)> {
        use nix::sys::{
            select::{FdSet, select},
            time::{TimeVal, TimeValLike},
        };
        use std::os::fd::AsFd;

        let mut tty = OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
            .context("Failed to open /dev/tty")?;

        // Send the ANSI cursor position report query
        tty.write_all(b"\x1b[6n")?;
        tty.flush()?;

        // Wait for input using select()
        let fd = tty.as_fd();
        let mut fds = FdSet::new();
        fds.insert(fd);

        let mut timeout = TimeVal::milliseconds(timeout.as_millis() as i64);

        let ready =
            select(None, &mut fds, None, None, Some(&mut timeout)).context("select() failed")?;

        if ready == 0 {
            bail!("Timed out waiting for cursor position response: {timeout:?}");
        }

        // Read the response
        let mut buf = [0u8; 64];
        let n = tty.read(&mut buf)?;
        let s = String::from_utf8_lossy(&buf[..n]);

        parse_cursor_response(&s).context(format!("Failed to parse terminal response: {s}"))
    }

    /// Parse the terminal response with format ESC [ row ; col R
    /// and return (col, row) as 0-based coordinates.
    fn parse_cursor_response(s: &str) -> Result<(u16, u16)> {
        let coords = s
            .strip_prefix("\x1b[")
            .context("Missing ESC]")?
            .strip_suffix('R')
            .context("Missing R")?;

        let mut parts = coords.split(';');

        let row: u16 = parts.next().context("Missing row")?.parse()?;

        let col: u16 = parts.next().context("Missing column")?.parse()?;

        Ok((col - 1, row - 1)) // convert to 0-based
    }
}
