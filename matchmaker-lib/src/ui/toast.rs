//! Process-wide toast log and the widget that renders it.
//!
//! The log is global so messages can be pushed from outside the render loop,
//! e.g. from a client's action handler.

use std::sync::{Mutex, MutexGuard};

use ratatui::{
    Frame,
    layout::Rect,
    style::Modifier,
    text::{Line, Span},
    widgets::{Borders, Paragraph, Wrap},
};

use crate::{config::ToastConfig, ui::StatusUI};

/// A toast entry: a prefix rendered once, followed by its messages joined
/// with `, `.
#[derive(Debug, Clone, PartialEq)]
pub struct ToastEntry {
    pub prefix: Span<'static>,
    pub message: Vec<Line<'static>>,
}

static LOG: Mutex<Vec<ToastEntry>> = Mutex::new(Vec::new());

fn log() -> MutexGuard<'static, Vec<ToastEntry>> {
    LOG.lock().unwrap_or_else(|e| e.into_inner())
}

/// Push a new entry.
pub fn push(prefix: Span<'static>, message: Line<'static>) {
    log().push(ToastEntry {
        prefix,
        message: vec![message],
    });
}

/// Join a message onto the last entry with the same prefix. Renders the
/// messages separated by `, `. Pushes a new entry when no entry has that
/// prefix.
pub fn join(prefix: Span<'static>, message: Line<'static>) {
    let mut log = log();

    match find_prefix(&mut log, &prefix) {
        Some(entry) => entry.message.push(message),
        None => log.push(ToastEntry {
            prefix,
            message: vec![message],
        }),
    }
}

fn find_prefix<'a>(log: &'a mut [ToastEntry], prefix: &Span<'_>) -> Option<&'a mut ToastEntry> {
    log.iter_mut()
        .rev()
        .find(|e| e.prefix.content == prefix.content)
}

/// Remove every entry with the given prefix.
pub fn remove(prefix: &str) {
    log().retain(|e| e.prefix.content != prefix);
}

/// Remove the last `n` entries.
pub fn pop(n: usize) {
    let mut log = log();
    let len = log.len().saturating_sub(n);
    log.truncate(len);
}

/// Remove every entry.
pub fn clear() {
    log().clear();
}

/// A copy of the log, oldest first.
pub fn entries() -> Vec<ToastEntry> {
    log().clone()
}

/// Split a `prefix:message` payload and parse the message as a status-style
/// template.
///
/// A payload without a top-level `:` has no prefix. Colons inside `{...}`
/// style sections do not split the payload.
pub fn parse_payload(payload: &str) -> (Span<'static>, Line<'static>) {
    let (prefix, message) = split_prefix(payload);
    (Span::raw(prefix.to_owned()), templated(message))
}

fn split_prefix(payload: &str) -> (&str, &str) {
    let mut depth = 0u32;

    for (i, c) in payload.char_indices() {
        match c {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            ':' if depth == 0 => return (&payload[..i], &payload[i + 1..]),
            _ => {}
        }
    }

    ("", payload)
}

fn templated(message: &str) -> Line<'static> {
    let mut line = StatusUI::parse_template_to_status_line(message);

    // `i` marks an interaction region on the status line; a toast has none.
    for span in &mut line.spans {
        span.style = span.style.remove_modifier(Modifier::RAPID_BLINK);
    }

    line
}

/// The toast log as a widget.
///
/// Rendering reads the global log; the widget only tracks its allotted area
/// and scroll position.
#[derive(Debug, Clone)]
pub struct ToastUI {
    pub config: ToastConfig,
    area: Rect,
    offset: u16,
    /// Stick to the newest entry until the user scrolls up.
    follow: bool,
}

impl Default for ToastUI {
    fn default() -> Self {
        Self::new(ToastConfig::default())
    }
}

impl ToastUI {
    pub fn new(config: ToastConfig) -> Self {
        Self {
            config,
            area: Rect::default(),
            offset: 0,
            follow: true,
        }
    }

    /// Rows the toast occupies: up to `max_height` message rows plus the frame.
    pub fn height(&self, width: u16) -> u16 {
        if self.config.max_height == 0 {
            return 0;
        }

        let entries = entries();
        if entries.is_empty() {
            return 0;
        }

        let rows = self.rows(&entries, width);
        if rows == 0 {
            return 0;
        }

        (rows.min(self.config.max_height as usize) as u16)
            .saturating_add(self.config.frame.height())
    }

    /// Store the allotted area and update the scroll offset.
    pub fn update(&mut self, area: Rect) {
        self.area = area;

        let entries = entries();
        let rows = self.rows(&entries, area.width);
        let max = self.max_offset(rows);

        self.offset = if self.follow {
            max
        } else {
            self.offset.min(max)
        };

        if rows <= self.visible_rows() as usize {
            self.follow = true;
        }
    }

    /// Scroll by `delta` message rows.
    pub fn scroll(&mut self, delta: i16) {
        let rows = self.rows(&entries(), self.area.width);
        let max = self.max_offset(rows);

        self.offset = (self.offset as i32 + delta as i32).clamp(0, max as i32) as u16;
        self.follow = self.offset == max;
    }

    /// Render into `area`, mirroring the frame vertically when `reverse` is set
    /// because the toast then sits at the top edge of the ui.
    pub fn render(&self, frame: &mut Frame, area: Rect, reverse: bool) {
        if area.height == 0 {
            return;
        }

        let entries = entries();
        if entries.is_empty() {
            return;
        }

        let widget = Self::paragraph(&entries)
            .block(self.block(reverse))
            .scroll((self.offset, 0));

        frame.render_widget(widget, area);
    }

    fn block(&self, reverse: bool) -> ratatui::widgets::Block<'static> {
        let mut frame = self.config.frame.clone();

        if reverse {
            frame.sides = frame.sides.map(mirror_vertical);
        }

        frame.as_static_block()
    }

    fn visible_rows(&self) -> u16 {
        self.config.frame.inner_of(self.area).height
    }

    fn max_offset(&self, rows: usize) -> u16 {
        rows.saturating_sub(self.visible_rows() as usize)
            .min(u16::MAX as usize) as u16
    }

    /// Message rows after wrapping; the frame is not counted.
    fn rows(&self, entries: &[ToastEntry], width: u16) -> usize {
        let width = width.saturating_sub(self.config.frame.width());
        if width == 0 {
            return 0;
        }
        Self::paragraph(entries).line_count(width)
    }

    fn paragraph(entries: &[ToastEntry]) -> Paragraph<'static> {
        Paragraph::new(entries.iter().map(Self::entry_line).collect::<Vec<_>>())
            .wrap(Wrap { trim: false })
    }

    fn entry_line(entry: &ToastEntry) -> Line<'static> {
        let mut spans = Vec::new();

        if !entry.prefix.content.is_empty() {
            spans.push(entry.prefix.clone());
            spans.push(Span::raw(": "));
        }

        for (i, message) in entry.message.iter().enumerate() {
            if i > 0 {
                spans.push(Span::raw(", "));
            }

            spans.extend(message.spans.iter().map(|span| {
                let mut span = span.clone();
                span.style = message.style.patch(span.style);
                span
            }));
        }

        Line::from(spans)
    }
}

/// Mirror the horizontal border sides so a frame drawn at the top edge of the
/// ui separates the toast from the footer below it.
fn mirror_vertical(sides: Borders) -> Borders {
    let mut mirrored = sides;

    if sides.contains(Borders::TOP) {
        mirrored.insert(Borders::BOTTOM);
    } else {
        mirrored.remove(Borders::BOTTOM);
    }

    if sides.contains(Borders::BOTTOM) {
        mirrored.insert(Borders::TOP);
    } else {
        mirrored.remove(Borders::TOP);
    }

    mirrored
}
