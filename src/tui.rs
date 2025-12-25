use crate::{Result, config::{TerminalConfig, TerminalLayoutSettings}};
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture}, execute, terminal::{ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode}
};
use log::{debug, error, warn};
use ratatui::{Terminal, TerminalOptions, Viewport, layout::Rect, prelude::CrosstermBackend};
use serde::{Deserialize, Serialize};
use std::{io::{self, Write}, thread::sleep, time::Duration};

pub struct Tui<W>
where
W: Write,
{
    pub terminal: ratatui::Terminal<CrosstermBackend<W>>,
    layout: Option<TerminalLayoutSettings>,
    pub area: Rect,
    pub sleep : u64,
    restore_fullscreen: bool
}

impl<W> Tui<W>
where
W: Write,
{
    // waiting on https://github.com/ratatui/ratatui/issues/984 to implement growable inline, currently just tries to request max
    // if max > than remainder, then scrolls up a bit
    pub fn new_with_writer(writer: W, config: TerminalConfig) -> Result<Self> {
        let mut backend = CrosstermBackend::new(writer);
        let mut options = TerminalOptions::default();

        let (width, height) = Self::full_size().unwrap_or_default();
        let area = if let Some(ref layout) = config.layout {
            let request = layout.percentage.get_max(height, layout.max).min(height);

            let cursor_y= Self::get_cursor_y().unwrap_or_else(|e| {
                warn!("Failed to read cursor: {e}");
                height // overestimate
            });

            let initial_height = height
            .saturating_sub(cursor_y);

            let scroll = request.saturating_sub(initial_height);
            debug!("TUI dimensions: {width}, {height}. Cursor: {cursor_y}.", );

            // ensure available by scrolling
            let cursor_y = match Self::scroll_up(&mut backend, scroll) {
                Ok(_) => {
                    cursor_y.saturating_sub(scroll) // the requested cursor doesn't seem updated so we assume it succeeded
                }
                Err(_) => {
                    cursor_y
                }
            };
            let available_height = height
            .saturating_sub(cursor_y);

            debug!("TUI quantities: min: {}, initial: {initial_height}, requested: {request}, available: {available_height}, requested scroll: {scroll}", layout.min);

            if available_height < layout.min {
                error!("Failed to allocate minimum height, falling back to fullscreen");
                Rect::new(0, 0, width, height)
            } else {
                let area = Rect::new(
                    0,
                    cursor_y,
                    width,
                    available_height.min(request),
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
            layout: config.layout,
            restore_fullscreen: config.restore_fullscreen,
            area,
            sleep: if config.sleep == 0 { 100 } else { config.sleep as u64 }
        })
    }



    pub fn enter(&mut self) -> Result<()> {
        let fullscreen = self.is_fullscreen();
        let backend = self.terminal.backend_mut();
        enable_raw_mode()?;
        execute!(backend, EnableMouseCapture)?;

        if fullscreen {
            self.enter_alternate()?;
        }
        Ok(())
    }

    pub fn enter_alternate(&mut self) -> Result<()> {
        let backend = self.terminal.backend_mut();
        execute!(backend, EnterAlternateScreen)?;
        execute!(
            backend,
            crossterm::terminal::Clear(ClearType::All)
        )?;
        self.terminal.clear()?;
        debug!("Entered alternate screen");
        Ok(())
    }

    pub fn enter_execute(&mut self) {
        self.exit();
        sleep(Duration::from_millis(self.sleep)); // necessary to give resize some time
        debug!("state: {:?}", crossterm::terminal::is_raw_mode_enabled());

        // do we ever need to scroll up?
    }

    pub fn resize(&mut self, area: Rect) {
        let _ = self
        .terminal
        .resize(area)
        .map_err(|e| error!("{e}"));
        self.area = area
    }

    pub fn redraw(&mut self) {
        let _ = self
        .terminal
        .resize(self.area)
        .map_err(|e| error!("{e}"));
    }

    pub fn return_execute(&mut self) -> Result<()> {
        self.enter()?;
        if !self.is_fullscreen() {
            // altho we cannot resize the viewport, this is the best we can do
            let _ = self.enter_alternate();
        }

        sleep(Duration::from_millis(self.sleep));

        let _ = execute!(
            self.terminal.backend_mut(),
            crossterm::terminal::Clear(ClearType::All)
        )
        .map_err(|e| warn!("{e}"));

        if self.is_fullscreen() || self.restore_fullscreen {
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
        let _ = execute!(
            backend,
            crossterm::terminal::Clear(ClearType::FromCursorDown)
        )
        .map_err(|e| warn!("{e}"));
        // } else {
        //     if let Err(e) = execute!(backend, cursor::MoveTo(0, 0)) {
        //         warn!("Failed to move cursor: {:?}", e);
        //     }
        // }

        let _ = execute!(backend, LeaveAlternateScreen, DisableMouseCapture)
        .map_err(|e| warn!("{e}"));

        let _ = self
        .terminal
        .show_cursor()
        .map_err(|e| warn!("{e}"));

        let _ = disable_raw_mode()
        .map_err(|e| warn!("{e}"));


        debug!("Terminal exited");
    }

    // wrappers to hide impl
    // note: do not start before event stream
    pub fn get_cursor_y() -> io::Result<u16> {
        crossterm::cursor::position().map(|x| x.1)
    }

    pub fn get_cursor() -> io::Result<(u16, u16)> {
        crossterm::cursor::position()
    }

    pub fn scroll_up(backend: &mut CrosstermBackend<W>, lines: u16) -> io::Result<u16> {
        execute!(backend, crossterm::terminal::ScrollUp(lines))?;
        Self::get_cursor_y() // note: do we want to skip this for speed
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
        self.layout.is_none()
    }
    pub fn set_fullscreen(&mut self) {
        self.layout = None;
    }
    pub fn layout(&self) -> &Option<TerminalLayoutSettings> {
        &self.layout
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