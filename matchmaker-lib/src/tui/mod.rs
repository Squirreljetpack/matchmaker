use crate::config::TerminalConfig;
use anyhow::{Context, Result};
use cba::{_info, _trace, bait::ResultExt};
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode},
};
use log::{debug, error};
use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect, prelude::CrosstermBackend};
use std::{
    io::{self, Write},
    thread::sleep,
    time::Duration,
};

mod layout;
mod query;
mod stream;

pub use stream::IoStream;

/// Ownership of bracketed paste mode.
#[cfg(feature = "bracketed-paste")]
#[derive(Debug, Clone, Copy, PartialEq)]
enum PasteMode {
    /// Disabled when we started; we may enable it and must then disable it on
    /// exit.
    Off,
    /// Enabled by us; must be disabled on exit.
    Ours,
    /// Already enabled before we started (e.g. by a shell line editor); leave
    /// it untouched so pasting keeps working after we exit.
    External,
}
pub struct Tui<W>
where
    W: Write,
{
    pub terminal: ratatui::Terminal<CrosstermBackend<W>>,
    pub area: Rect,
    pub config: TerminalConfig,

    /// Ownership of bracketed paste mode; see [`PasteMode`].
    #[cfg(feature = "bracketed-paste")]
    paste_mode: PasteMode,

    in_execute: bool,
}

impl<W> Tui<W>
where
    W: Write,
{
    // waiting on https://github.com/ratatui/ratatui/issues/984 to implement growable inline, currently just tries to request max
    // if max > than remainder, then scrolls up a bit
    pub fn new_with_writer(writer: W, mut config: TerminalConfig) -> Result<Self> {
        if matches!(config.stream, IoStream::Auto) {
            config.stream = config
                .stream
                .resolve()
                .context("Failed to select a render stream")?;
            debug!("Resolved IoStream::Auto to {:?}", config.stream);
        }
        let mut backend = CrosstermBackend::new(writer);
        let mut options = TerminalOptions::default();

        // important for getting cursor
        if config.stream != IoStream::Test {
            crossterm::terminal::enable_raw_mode()?;
        }

        // Single query round trip against the terminal; see [`query::startup`].
        let startup = if config.stream != IoStream::Test {
            match query::startup(Duration::from_millis(config.sleep_ms)) {
                Ok(s) => s,
                Err(e) => {
                    debug!("Terminal startup queries failed: {e:#}");
                    query::Startup::default()
                }
            }
        } else {
            query::Startup::default()
        };

        // Bracketed paste that was already on stays untouched so exit does not
        // disable it out from under whoever turned it on.
        #[cfg(feature = "bracketed-paste")]
        let paste_mode = match startup.bracketed_paste {
            Some(true) => PasteMode::External,
            _ => PasteMode::Off,
        };

        // In headless environments (e.g. CI) there is no terminal to size;
        // the test backend falls back to a fixed size so rendering is possible.
        let (width, height) = Self::full_size().unwrap_or_else(|| {
            if config.stream == IoStream::Test {
                (80, 24)
            } else {
                (0, 0)
            }
        });
        let area = if let Some(ref layout) = config.layout {
            _info!(layout);

            let request = layout
                .percentage
                .compute_clamped(height, layout.min, layout.max);

            let cursor_y = startup.cursor.map(|(_, y)| y).unwrap_or_else(|| {
                error!("Failed to read cursor");
                height.saturating_sub(1) // overestimate
            });

            let initial_height = height.saturating_sub(cursor_y);

            let scroll = layout::scroll_amount(layout.scroll, request, layout.min, initial_height);
            debug!("TUI dimensions: {width}, {height}. Cursor_y: {cursor_y}.",);

            // ensure available by scrolling
            let cursor_y = match Self::scroll_up(&mut backend, scroll)._elog() {
                Some(_) => {
                    cursor_y.saturating_sub(scroll) // the requested cursor doesn't seem updated so we assume it succeeded
                    // todo: highpri: scroll doesn't actually seem happening tho, erasing buffer
                }
                None => cursor_y,
            };
            let available_height = height.saturating_sub(cursor_y);

            debug!(
                "TUI quantities: min: {}, initial_available: {initial_height}, requested_height: {request}, available_after_scroll: {available_height}, requested_scroll: {scroll}",
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
                    layout::viewport_height(
                        layout.scroll,
                        request,
                        layout.min,
                        layout.max,
                        available_height,
                    ),
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
            #[cfg(feature = "bracketed-paste")]
            paste_mode,
            in_execute: false,
        })
    }

    pub fn enter(&mut self) -> Result<()> {
        let fullscreen = self.is_fullscreen();
        _trace!("entering tui"; fullscreen);

        if self.config.stream != IoStream::Test {
            crossterm::terminal::enable_raw_mode()?;
        }

        if fullscreen {
            self.enter_alternate_screen(true)?;
        }

        let backend = self.terminal.backend_mut();
        execute!(backend, EnableMouseCapture)._elog();
        #[cfg(feature = "bracketed-paste")]
        if self.paste_mode == PasteMode::Off
            && execute!(backend, crossterm::event::EnableBracketedPaste)
                ._elog()
                .is_some()
        {
            self.paste_mode = PasteMode::Ours;
        }

        if self.config.extended_keys {
            execute!(
                backend,
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )
            ._elog();
            log::trace!("keyboard enhancement set");
        }

        Ok(())
    }

    // call iff self.is_fullscreen
    pub fn enter_alternate_screen(&mut self, clear: bool) -> Result<()> {
        let backend = self.terminal.backend_mut();
        execute!(backend, EnterAlternateScreen)
            .prefix("EnterAlternateScreen")
            ._elog();

        if clear {
            execute!(backend, crossterm::terminal::Clear(ClearType::All))
                .prefix("Clear Terminal")
                ._elog();
            // self.terminal.clear()._elog(); we can just do a full clear
        }

        debug!("Entered alternate screen");
        Ok(())
    }

    fn sleep(&self) -> Duration {
        std::time::Duration::from_millis(self.config.sleep_ms)
    }

    pub fn enter_execute(&mut self) {
        self.exit(None);
        sleep(self.sleep()); // necessary to give resize some time
        _trace!(crossterm::terminal::is_raw_mode_enabled());
        self.in_execute = true;

        // do we ever need to scroll up?
    }

    pub fn return_execute(&mut self, clear: bool) -> Result<()> {
        log::trace!("returning from execute");
        if self.config.restore_fullscreen {
            self.config.layout = None;
        }

        // somehow this scroll amount is wrong. Also previously rendered execute are not visible.
        // if let Some(y) = Self::get_cursor_y(self.sleep())._elog() {
        //     let lines = y.saturating_sub(self.area.y);
        //     if lines > 0 {
        //         Self::scroll_up(self.terminal.backend_mut(), lines)._elog();
        //     }
        // }
        self.enter()?;

        // not sure if clear does anything
        if clear {
            sleep(self.sleep());
            log::trace!("During return, slept {}", self.sleep().as_millis());

            execute!(
                self.terminal.backend_mut(),
                crossterm::terminal::Clear(ClearType::All)
            )
            ._wlog();
        }

        // resize
        if self.is_fullscreen() {
            if let Some((width, height)) = Self::full_size() {
                self.resize(Rect::new(0, 0, width, height));
            } else {
                error!("Failed to get terminal size");
                self.resize(self.area);
            }
        } else {
            self.resize(self.area);
        }
        self.in_execute = false;

        Ok(())
    }

    pub fn exit(&mut self, mut clear: Option<bool>) {
        if self.in_execute {
            // let backend = self.terminal.backend_mut();
            // execute!(backend, LeaveAlternateScreen, DisableMouseCapture)._wlog();
            // disable_raw_mode()._wlog();
            log::debug!("Skipped teardown after already having left");
            return;
        }
        let backend = self.terminal.backend_mut();

        if self.config.extended_keys {
            execute!(backend, PopKeyboardEnhancementFlags)._elog();
        }
        #[cfg(feature = "bracketed-paste")]
        if self.paste_mode == PasteMode::Ours
            && execute!(backend, crossterm::event::DisableBracketedPaste)
                ._elog()
                .is_some()
        {
            self.paste_mode = PasteMode::Off;
        }
        execute!(backend, LeaveAlternateScreen, DisableMouseCapture)._wlog();

        if clear.is_none() {
            if !self.config.clear_on_exit {
                clear = Some(false)
            } else
            // todo: condition allowing for None variant
            {
                clear = Some(true)
            }
        }

        if self.config.stream != IoStream::Test {
            match clear {
                Some(true) => {
                    execute!(
                        backend,
                        crossterm::cursor::MoveToRow(self.area.y),
                        crossterm::cursor::MoveToColumn(0),
                        crossterm::terminal::Clear(ClearType::FromCursorDown)
                    )
                    ._elog();
                }
                None => {
                    execute!(
                        backend,
                        crossterm::cursor::MoveUp(0), // todo
                        crossterm::cursor::MoveToColumn(0),
                        crossterm::terminal::Clear(ClearType::FromCursorDown)
                    )
                    ._elog();
                }
                _ => {}
            }
        }

        self.terminal.show_cursor()._wlog();

        if self.config.stream != IoStream::Test {
            disable_raw_mode()._wlog();
        }

        debug!("Terminal exited");
    }

    // todo: for some reason this leaves artifacts that initial tui.flush cannot remove after we implemented aggressive caching
    pub fn exit_lite(&mut self) {
        let backend = self.terminal.backend_mut();

        // execute!(backend, LeaveAlternateScreen, DisableMouseCapture)._wlog();

        if self.config.extended_keys {
            execute!(backend, PopKeyboardEnhancementFlags)._elog();
        }
        #[cfg(feature = "bracketed-paste")]
        if self.paste_mode == PasteMode::Ours
            && execute!(backend, crossterm::event::DisableBracketedPaste)
                ._elog()
                .is_some()
        {
            self.paste_mode = PasteMode::Off;
        }

        disable_raw_mode()._wlog();

        debug!("Terminal exited (lite)");
    }

    pub fn resize(&mut self, area: Rect) {
        self.terminal.resize(area)._elog();
        self.area = area
    }

    pub fn flush(&mut self) {
        self.terminal.resize(self.area)._elog();
    }

    // note: do not start before event stream
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
    pub fn new(mut config: TerminalConfig) -> Result<Self> {
        let stream = config
            .stream
            .resolve()
            .context("Failed to select a render stream")?;
        config.stream = stream.clone();
        debug!("Render stream: {:?}", config.stream);
        let writer = stream.to_stream()?;
        let tui = Self::new_with_writer(writer, config)?;
        Ok(tui)
    }
}

impl<W> Drop for Tui<W>
where
    W: Write,
{
    fn drop(&mut self) {
        self.exit(None);
    }
}
