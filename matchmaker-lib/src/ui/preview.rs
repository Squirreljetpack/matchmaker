use log::error;
use ratatui::{
    layout::Rect,
    widgets::{Paragraph, Wrap},
};

use crate::{
    config::{BorderSetting, PreviewConfig, PreviewLayout},
    preview::Preview,
    utils::text::wrapped_line_height,
};

#[derive(Debug)]
pub struct PreviewUI {
    pub view: Preview,
    pub config: PreviewConfig,
    pub layout_idx: usize,
    /// content area
    pub(crate) area: Rect,
    pub scroll: [u16; 2],
    offset: usize,
    target: Option<usize>,
}

impl PreviewUI {
    pub fn new(view: Preview, mut config: PreviewConfig) -> Self {
        // todo: lowpri: this is not strictly correct
        for x in &mut config.layout {
            if let Some(b) = &mut x.border
                && b.sides.is_none()
            {
                b.sides = Some(x.layout.side.opposite())
            }
        }

        Self {
            view,
            config,
            layout_idx: 0,
            scroll: Default::default(),
            offset: 0,
            area: Rect::default(),
            target: None,
        }
    }
    pub fn update_dimensions(&mut self, area: &Rect) {
        let mut height = area.height;
        height -= self.config.border.height().min(height);
        self.area.height = height;

        let mut width = area.width;
        width -= self.config.border.width().min(width);
        self.area.width = width;
    }

    // -------- Layout -----------
    /// None if not show
    pub fn layout(&self) -> Option<&PreviewLayout> {
        if !self.config.show || self.config.layout.is_empty() {
            None
        } else {
            let ret = &self.config.layout[self.layout_idx].layout;
            if ret.max == 0 { None } else { Some(ret) }
        }
    }
    pub fn command(&self) -> &str {
        if self.config.layout.is_empty() {
            ""
        } else {
            self.config.layout[self.layout_idx].command.as_str()
        }
    }

    pub fn border(&self) -> &BorderSetting {
        self.config.layout[self.layout_idx]
            .border
            .as_ref()
            .unwrap_or(&self.config.border)
    }

    pub fn get_initial_command(&self) -> &str {
        if let Some(current) = self.config.layout.get(self.layout_idx) {
            if !current.command.is_empty() {
                return current.command.as_str();
            }
        }

        self.config
            .layout
            .iter()
            .map(|l| l.command.as_str())
            .find(|cmd| !cmd.is_empty())
            .unwrap_or("")
    }

    pub fn cycle_layout(&mut self) {
        self.layout_idx = (self.layout_idx + 1) % self.config.layout.len()
    }
    pub fn set_layout(&mut self, idx: u8) -> bool {
        let idx = idx as usize;
        if idx < self.config.layout.len() {
            let changed = self.layout_idx != idx;
            self.layout_idx = idx;
            changed
        } else {
            error!("Layout idx {idx} out of bounds, ignoring.");
            false
        }
    }

    // ----- config && getters ---------
    pub fn is_show(&self) -> bool {
        self.layout().is_some()
    }
    // cheap show toggle + change tracking
    pub fn show(&mut self, show: bool) -> bool {
        let previous = self.config.show;
        self.config.show = show;
        previous != show
    }
    pub fn toggle_show(&mut self) {
        self.config.show = !self.config.show;
    }

    pub fn wrap(&mut self, wrap: bool) {
        self.config.wrap = wrap;
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }
    pub fn offset(&self) -> usize {
        self.config.scroll.header_lines + self.offset
    }
    pub fn target_line(&self) -> Option<usize> {
        self.target
    }

    // ----- actions --------
    pub fn up(&mut self, n: u16) {
        let total_lines = self.view.len();
        let n = n as usize;

        if self.offset >= n {
            self.offset -= n;
        } else if self.config.scroll_wrap {
            self.offset = total_lines.saturating_sub(n - self.offset);
        } else {
            self.offset = 0;
        }
    }
    pub fn down(&mut self, n: u16) {
        let total_lines = self.view.len();
        let n = n as usize;

        if self.offset + n > total_lines {
            if self.config.scroll_wrap {
                self.offset = 0;
            } else {
                self.offset = total_lines;
            }
        } else {
            self.offset += n;
        }
    }

    pub fn scroll(&mut self, horizontal: bool, val: i8) {
        let a = &mut self.scroll[horizontal as usize];

        if val == 0 {
            *a = 0;
        } else {
            let new = (*a as i8 + val).clamp(0, u16::MAX as i8);
            *a = new as u16;
        }
    }

    pub fn set_target(&mut self, target: Option<isize>) {
        let results = self.view.results().lines;
        let line_count = results.len();

        let Some(mut target) = target else {
            self.target = None;
            self.offset = 0;
            return;
        };

        target += self.config.scroll.offset;

        self.target = Some(if target < 0 {
            line_count.saturating_sub(target.unsigned_abs())
        } else {
            line_count.saturating_sub(1).min(target.unsigned_abs())
        });
        let mut index = self.target.unwrap();

        // decrement the index to put the target lower on the page.
        // The resulting height up to the top of target should >= p% of height.
        let mut lines_above =
            self.config
                .scroll
                .percentage
                .complement()
                .compute_clamped(self.area.height, 0, 0);
        // shoddy approximation to how Paragraph wraps lines
        while index > 0 && lines_above > 0 {
            let prev = wrapped_line_height(&results[index], self.area.width);
            if prev > lines_above {
                break;
            } else {
                index -= 1;
                lines_above -= prev;
            }
        }
        self.offset = index;
        log::trace!(
            "Preview initial offset: {}, index: {}",
            self.offset,
            self.target.unwrap()
        );
    }

    // --------------------------

    pub fn make_preview(&self) -> Paragraph<'_> {
        assert!(self.is_show());

        let mut results = self.view.results().into_iter();
        let height = self.area.height as usize;
        if height == 0 {
            return Paragraph::new(Vec::new());
        }

        let mut lines = Vec::with_capacity(height);

        for _ in 0..self.config.scroll.header_lines.min(height) {
            if let Some(line) = results.next() {
                lines.push(line);
            } else {
                break;
            };
        }
        let mut results = results.skip(self.offset);
        for _ in self.config.scroll.header_lines..height {
            if let Some(line) = results.next() {
                lines.push(line);
            }
        }

        let mut preview = Paragraph::new(lines);
        preview = preview.block(self.border().as_block());
        if self.config.wrap {
            preview = preview
                .wrap(Wrap { trim: false })
                .scroll(self.scroll.into());
        }
        preview
    }
}
