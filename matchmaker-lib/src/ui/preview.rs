use log::error;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Wrap},
};

use crate::{
    config::{BorderSetting, PreviewConfig, PreviewSetting, ShowCondition, Side},
    preview::Preview,
    utils::text::wrapped_line_height,
};

#[derive(Debug)]
pub struct PreviewUI {
    pub view: Preview,
    pub config: PreviewConfig,
    layout_idx: usize,
    /// content area
    pub(crate) area: Rect,
    pub scroll: [u16; 2],
    offset: usize,
    target: Option<usize>,
    attained_target: bool,

    show: bool,
}

impl PreviewUI {
    pub fn new(view: Preview, mut config: PreviewConfig, [ui_width, ui_height]: [u16; 2]) -> Self {
        for x in &mut config.layout {
            if let Some(b) = &mut x.border
                && b.sides.is_none()
                && !b.is_empty()
            {
                b.sides = Some(x.layout.side.opposite())
            }
        }

        let show = match config.show {
            ShowCondition::Free(x) => {
                if let Some(l) = config.layout.first() {
                    match l.layout.side {
                        Side::Bottom | Side::Top => ui_height >= x,
                        _ => ui_width >= x,
                    }
                } else {
                    false
                }
            }
            ShowCondition::Bool(x) => x,
        };

        // enforce invariant of valid index
        if config.layout.is_empty() {
            let mut s = PreviewSetting::default();
            s.layout.max = 0;
            config.layout.push(s);
        }

        Self {
            view,
            config,
            layout_idx: 0,
            scroll: Default::default(),
            offset: 0,
            area: Rect::default(),
            target: None,
            attained_target: false,
            show,
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

    pub fn reevaluate_show_condition(&mut self, [ui_width, ui_height]: [u16; 2], hide: bool) {
        match self.config.show {
            ShowCondition::Free(x) => {
                if let Some(setting) = self.setting() {
                    let l = &setting.layout;

                    let show = match l.side {
                        Side::Bottom | Side::Top => ui_height >= x,
                        _ => ui_width >= x,
                    };
                    log::debug!(
                        "Evaluated ShowCondition(Free({x})) against {ui_width}x{ui_height} => {show}"
                    );
                    if !hide && !show {
                        return;
                    }

                    self.show(show);
                };
            }
            ShowCondition::Bool(show) => {
                if !hide && !show {
                    return;
                }
                self.show(show);
            }
        };
    }

    // -------- Layout -----------
    /// None if not show OR if max = 0 (disabled layour)
    pub fn setting(&self) -> Option<&PreviewSetting> {
        // if let Some(ret) = self.config.layout.get(self.layout_idx)
        if let ret = &self.config.layout[self.layout_idx]
            && ret.layout.max != 0
        {
            Some(&ret)
        } else {
            None
        }
    }

    pub fn visible(&self) -> bool {
        self.setting().is_some() && self.show
    }

    pub fn command(&self) -> &str {
        self.setting().map(|x| x.command.as_str()).unwrap_or("")
    }

    pub fn border(&self) -> &BorderSetting {
        self.setting()
            .and_then(|s| s.border.as_ref())
            .unwrap_or(&self.config.border)
    }

    pub fn get_initial_command(&self) -> &str {
        let x = self.command();
        if !x.is_empty() {
            return x;
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

    pub fn show(&mut self, show: bool) -> bool {
        log::trace!("toggle preview with: {show}");
        let changed = self.show != show;
        self.show = show;
        changed
    }

    pub fn toggle_show(&mut self) {
        self.show = !self.show;
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
            target as usize
        });

        let index = self.target.unwrap();

        self.offset = if index >= results.len() {
            self.attained_target = false;
            results.len().saturating_sub(self.area.height as usize / 2)
        } else {
            self.attained_target = true;
            self.target_to_offset(index, &results)
        };

        log::trace!("Preview initial offset: {}, index: {}", self.offset, index);
    }

    fn target_to_offset(&self, mut target: usize, results: &Vec<Line>) -> usize {
        // decrement the index to put the target lower on the page.
        // The resulting height up to the top of target should >= p% of height.
        let mut lines_above =
            self.config
                .scroll
                .percentage
                .complement()
                .compute_clamped(self.area.height, 0, 0);

        // shoddy approximation to how Paragraph wraps lines
        while target > 0 && lines_above > 0 {
            let prev = results
                .get(target)
                .map(|x| wrapped_line_height(x, self.area.width))
                .unwrap_or(1);
            if prev > lines_above {
                break;
            } else {
                target -= 1;
                lines_above -= prev;
            }
        }

        target
    }
    // --------------------------

    pub fn make_preview(&mut self) -> Paragraph<'_> {
        let results = self.view.results();
        let rl = results.lines.len();
        let height = self.area.height as usize;

        if let Some(target) = self.target
            && !self.attained_target
            && target < rl
        {
            self.offset = self.target_to_offset(target, &results.lines);
            self.attained_target = true;
        };

        let mut results = results.into_iter();

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
