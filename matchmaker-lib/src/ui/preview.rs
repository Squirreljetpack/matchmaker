use cba::_info;
use log::error;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Wrap},
};

use crate::{
    config::{
        BorderSetting, PreviewConfig, PreviewInitialSetting, PreviewSetting, ShowCondition, Side,
    },
    preview::Preview,
    utils::text::{trim_text_lines, wrapped_line_height},
};

#[derive(Debug)]
pub struct PreviewUI {
    pub view: Preview,
    pub config: PreviewConfig,
    /// content area
    pub(crate) area: Rect,

    // state
    layout_idx: usize,
    show: bool,
    #[cfg(feature = "partial")]
    initial: PreviewInitialSetting,
    pub current_dimension: Option<u16>,

    // scroll
    pub scroll: [u16; 2],
    offset: usize,
    target: Option<usize>,
    attained_target: bool,
    pub jump: (bool, usize), // end, initial
    pub last_count: usize,
}

impl PreviewUI {
    fn initial(&self) -> &PreviewInitialSetting {
        #[cfg(feature = "partial")]
        {
            &self.initial
        }
        #[cfg(not(feature = "partial"))]
        {
            &self.config.initial
        }
    }

    pub fn new(view: Preview, mut config: PreviewConfig, [ui_width, ui_height]: [u16; 2]) -> Self {
        for x in &mut config.layout {
            if let Some(b) = &mut x.border
                && b.sides.is_none()
                && !b.is_empty()
            {
                b.sides = Some(x.layout.side.opposite().into())
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
            ShowCondition::Bool(x) => {
                x && if let Some(l) = config.layout.first() {
                    (match l.layout.side {
                        Side::Bottom | Side::Top => ui_height,
                        _ => ui_width,
                    }) > 5 + (l.layout.min.max(0) as u16)
                } else {
                    false
                }
            }
        };

        // enforce invariant of valid index
        if config.layout.is_empty() {
            let mut s = PreviewSetting::default();
            s.layout.max = 0;
            config.layout.push(s);
        }

        let idx = config.initial_layout;

        let mut ret = Self {
            view,
            #[cfg(feature = "partial")]
            initial: config.initial.clone(),
            config,
            layout_idx: 0,
            scroll: Default::default(),
            offset: 0,
            area: Rect::default(),
            target: None,
            attained_target: false,
            last_count: 0,
            jump: Default::default(),
            show,
            current_dimension: None,
        };
        ret.set_layout(idx);

        ret
    }

    pub fn update_dimensions(&mut self, area: &Rect) {
        self.area = self.border().inner(*area);
        if self.config.reevaluate_show_on_resize {
            self.reevaluate_show_condition([area.width, area.height], false);
        }
    }

    pub fn reevaluate_show_condition(&mut self, [ui_width, ui_height]: [u16; 2], no_hide: bool) {
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
                    if no_hide && !show {
                        return;
                    }

                    self.show(show);
                };
            }
            ShowCondition::Bool(mut show) => {
                if no_hide && !show {
                    return;
                };
                show = show
                    && if let Some(l) = self.config.layout.first() {
                        (match l.layout.side {
                            Side::Bottom | Side::Top => ui_height,
                            _ => ui_width,
                        }) > 5 + (l.layout.min.max(0) as u16)
                    } else {
                        false
                    };
                self.show(show);
            }
        };
    }

    // -------- Setting getters -----------
    /// None if not show OR if max = 0 (disabled layour)
    pub fn setting(&self) -> Option<&PreviewSetting> {
        // if let Some(ret) = self.config.layout.get(self.layout_idx)
        if let ret = &self.config.layout[self.layout_idx]
            && ret.layout.max != 0
        {
            Some(ret)
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

    pub fn is_vertical(&self) -> bool {
        self.setting().is_some_and(|s| s.layout.side.is_vertical())
    }

    // -------- Layout -----------
    pub fn cycle_layout(&mut self, rev: bool) {
        let len = self.config.layout.len();

        for _ in 0..len {
            if rev {
                self.layout_idx = (self.layout_idx + len - 1) % len;
            } else {
                self.layout_idx = (self.layout_idx + 1) % len;
            }

            if self.config.layout[self.layout_idx].layout.max > 0 {
                self.reinit();
                return;
            }
        }
    }
    pub fn set_layout(&mut self, idx: u8) -> bool {
        let idx = idx as usize;
        if idx < self.config.layout.len() {
            let changed = self.layout_idx != idx;
            self.layout_idx = idx;
            self.reinit();
            changed
        } else {
            error!("Layout idx {idx} out of bounds, ignoring.");
            false
        }
    }
    pub fn reinit(&mut self) {
        #[cfg(feature = "partial")]
        {
            use matchmaker_partial::Apply;
            if let Some(s) = self.setting() {
                let mut new = self.config.initial.clone();
                new.apply(s.initial.clone());
                log::trace!("Applied: {:?} -> {:?}", s.initial, new);
                self.initial = new;
            }
        }
        self.current_dimension = None;
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
        self.initial().header_lines + self.offset
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
        _info!(target);

        if self.initial().tail {
            return;
        }

        let results = self.view.results().lines;
        let line_count = results.len();

        let Some(mut target) = target else {
            self.target = None;
            self.offset = 0;
            return;
        };

        target += self.initial().offset.unwrap_or(-1);

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

        _info!("Preview initial offset": self.offset; "index" : index);
    }

    pub fn jump(&mut self) {
        if self.initial().tail {
            if self.offset > 0 {
                // go to end
                self.jump = (false, self.offset);
                self.reset_scroll();
            } else {
                if !self.jump.0 {
                    // go to start

                    self.attained_target = true;
                    self.offset = 0;
                    self.jump.0 = true
                } else {
                    // go to saved
                    self.offset = self.jump.1;
                    self.attained_target = true;
                    self.jump = (false, 0)
                }
            }
        } else {
            match self.jump {
                (false, 0) => {
                    self.jump = (true, self.offset);
                    self.scroll_end();
                }
                (true, x) if x != 0 => {
                    self.jump.0 = false;
                    self.reset_scroll();
                }
                _ => {
                    self.offset = self.jump.1;
                    self.jump = (false, 0)
                }
            }
        }
    }
    pub fn reset_scroll(&mut self) {
        self.offset = 0;
        self.attained_target = false;
    }
    pub fn scroll_end(&mut self) {
        let results = self.view.results();
        let rl = results.lines.len();
        let height = self.area.height as usize;

        let header_count = self.initial().header_lines.min(height);
        let remaining_lines = rl.saturating_sub(header_count);

        self.offset = remaining_lines.saturating_sub(height);
    }

    fn target_to_offset(&self, mut target: usize, results: &Vec<Line>) -> usize {
        // decrement the index to put the target lower on the page.
        // The resulting height up to the top of target should >= p% of height.
        let mut lines_above =
            self.config
                .initial
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

    pub fn drag_width(&self) -> u16 {
        self.config.drag_width.unwrap_or_else(|| {
            let side = self.setting().map(|s| s.layout.side).unwrap_or(Side::Right);
            self.border().dimension(side.opposite())
        })
    }

    pub fn split(&self, area: Rect) -> [Rect; 2] {
        use ratatui::layout::{Constraint, Direction, Layout};

        let Some(setting) = self.setting() else {
            return [Rect::default(), area];
        };

        let layout = &setting.layout;

        let direction = layout.side.into();

        let side_first = matches!(layout.side, Side::Left | Side::Top);

        let total = if matches!(direction, Direction::Horizontal) {
            area.width
        } else {
            area.height
        };

        let border_offset = match layout.side {
            Side::Left | Side::Right => self.border().width(),
            Side::Top | Side::Bottom => self.border().height(),
        };

        let side_size = if let Some(size) = self.current_dimension {
            size.min(total)
        } else {
            let mut min = if layout.min < 0 {
                // negative min => ensure sufficient space for results => don't include border offset
                total.saturating_sub((-layout.min) as u16)
            } else {
                (layout.min as u16).saturating_add(border_offset)
            };

            let mut max = if layout.max < 0 {
                total.saturating_sub((-layout.max) as u16)
            } else {
                (layout.max as u16).saturating_add(border_offset)
            };

            min = min.min(total);
            max = max.min(total);

            if min <= max {
                layout.percentage.compute_clamped(total, min, max)
            } else {
                error!("PreviewLayout min > max: {min} > {max}. Ignoring max.");
                layout.percentage.compute_clamped(total, min, 0)
            }
        };

        let side_constraint = Constraint::Length(side_size);

        let constraints = if side_first {
            [side_constraint, Constraint::Min(0)]
        } else {
            [Constraint::Min(0), side_constraint]
        };

        let chunks = Layout::default()
            .direction(direction)
            .constraints(constraints)
            .split(area);

        if side_first {
            [chunks[0], chunks[1]]
        } else {
            [chunks[1], chunks[0]]
        }
    }

    pub fn expand(&mut self, n: u16) {
        if n == 0 {
            self.current_dimension = None;
            return;
        }
        let current = self.current_size();
        self.current_dimension = Some(current.saturating_add(n));
    }

    pub fn shrink(&mut self, n: u16) {
        if n == 0 {
            self.current_dimension = None;
            return;
        }

        let current = self.current_size();
        self.current_dimension = Some(current.saturating_sub(n));
    }

    fn current_size(&self) -> u16 {
        if let Some(dim) = self.current_dimension {
            dim
        } else {
            let setting = self.setting();
            let side = setting.map(|s| &s.layout.side).unwrap_or(&Side::Right);
            match side {
                Side::Left | Side::Right => self.area.width + self.border().width(),
                Side::Top | Side::Bottom => self.area.height + self.border().height(),
            }
        }
    }

    pub fn make_preview(&mut self) -> Paragraph<'_> {
        let mut results = self.view.results();
        if self.config.trim_ends {
            trim_text_lines(&mut results)
        }

        let rl = results.lines.len();
        let height = self.area.height as usize;
        let mut offset = self.offset;

        // this only triggers on preview change but not guaranteed on every preview change -- attaching it to the event handler is worse
        if rl < self.last_count {
            self.offset = 0;
            self.attained_target = false;
            self.jump = (false, 0)
        }
        self.last_count = rl;

        if self.initial().tail && !self.attained_target {
            let header_count = self.initial().header_lines.min(height);
            let remaining_lines = rl.saturating_sub(header_count);
            let remaining_space = height.saturating_sub(header_count);

            // get current offset
            offset = remaining_lines.saturating_sub(remaining_space);
            // apply initial offset: it's more natural to default 0 so we shift by 1
            if let Some(s) = self.initial().offset
                && s < 0
            {
                offset = offset.saturating_sub(s.unsigned_abs());
            }

            // stop scrolling
            if self.offset != 0 {
                if self.offset > offset || self.offset + offset > rl {
                    self.offset = self.offset.saturating_sub(rl.saturating_sub(offset));
                } else {
                    self.offset += offset;
                }
                self.attained_target = true;
            }
            // log::trace!("{} {} {}", offset, self.offset, self.attained_target);
        } else if let Some(target) = self.target
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

        for _ in 0..self.initial().header_lines.min(height) {
            if let Some(line) = results.next() {
                lines.push(line);
            } else {
                break;
            };
        }

        let mut results = results.skip(offset);

        for _ in self.initial().header_lines..height {
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
