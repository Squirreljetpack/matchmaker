use cba::_info;
use log::error;
use ratatui::{
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Wrap},
};

#[cfg(feature = "image")]
use crate::{
    event::EventSender,
    ui::image::{ImageState, ImageWidget},
};
#[cfg(feature = "image")]
use matchmaker_partial::Apply;
#[cfg(feature = "image")]
use ratatui_image::picker::Picker;

use crate::{
    config::{
        BorderSetting, PreviewConfig, PreviewInitialSetting, PreviewSetting, ShowCondition, Side,
        StyleSetting,
    },
    preview::Preview,
    ui::{RESULTS_MIN_H, RESULTS_MIN_W},
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

    /// [Vertical, Horizontal] to match what Paragraph expects
    pub scroll: [u16; 2],
    pub(super) offset: usize,
    pub(super) target: Option<usize>,
    pub(super) attained_target: bool,
    pub jump: (bool, usize), // end, initial
    pub last_count: usize,

    #[cfg(feature = "image")]
    image: Option<ImageState>,
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
                    match l.layout.side {
                        Side::Bottom | Side::Top => {
                            ui_height > RESULTS_MIN_H + (l.layout.min.max(0) as u16)
                        }
                        _ => ui_width > RESULTS_MIN_W + (l.layout.min.max(0) as u16),
                    }
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
            #[cfg(feature = "image")]
            image: None,
        };
        ret.set_layout(idx);

        ret
    }

    // --------------------------------------------------

    pub fn update_dimensions(&mut self, area: &Rect) {
        self.area = self.border().inner_of(*area);
        if self.config.reevaluate_show_on_resize {
            self.reevaluate_show_condition([area.width, area.height], false);
        }
    }

    pub fn reevaluate_show_condition(&mut self, [ui_width, ui_height]: [u16; 2], no_hide: bool) {
        if self.current_dimension.is_some() {
            return;
        }
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
                        match l.layout.side {
                            Side::Bottom | Side::Top => {
                                ui_height > RESULTS_MIN_H + (l.layout.min.max(0) as u16)
                            }
                            _ => ui_width > RESULTS_MIN_W + (l.layout.min.max(0) as u16),
                        }
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

    pub fn border(&self) -> BorderSetting {
        let global = &self.config.border;
        let mut b = self
            .setting()
            .and_then(|s| s.border.clone())
            .unwrap_or_else(|| global.clone());
        if b.title_style == StyleSetting::DEFAULT {
            b.title_style = global.title_style;
        }
        b
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

    pub fn layout_idx(&self) -> usize {
        self.layout_idx
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
    /// The source text's offset of the first non-header line of text displayed.
    pub fn text_offset(&self) -> usize {
        #[cfg(feature = "image")]
        if self.image_rows() > 0 {
            // The header sits above the image, the image above the text: the text
            // starts where the document leaves the image behind.
            let header_lines = self.image_header_source_lines();

            return header_lines + self.offset.saturating_sub(header_lines + self.image_rows());
        }
        self.initial().header_lines + self.offset
    }
    pub fn target_line(&self) -> Option<usize> {
        self.target
    }

    /// Number of rows the current contents can scroll through.
    fn total_lines(&self) -> usize {
        #[cfg(feature = "image")]
        if self.image_rows() > 0 {
            // The header rows, the image, then the rest of the text; a header longer
            // than the text still holds its rows.
            return self.image_rows() + self.view.len().max(self.image_header_source_lines());
        }
        self.view.len()
    }

    /// Rows the image takes in the document, from the request the pane made.
    #[cfg(feature = "image")]
    fn image_rows(&self) -> usize {
        self.image.as_ref().map_or(0, ImageState::rows)
    }

    /// Lines of the text the image's header draws, before they wrap into their rows.
    #[cfg(feature = "image")]
    fn image_header_source_lines(&self) -> usize {
        self.image.as_ref().map_or(0, ImageState::header_lines)
    }

    /// The offset that brings the contents' last row to the pane's last row.
    ///
    /// An action may pass it: a target's line is placed near the top of the pane, which
    /// is past the end when the target is at the end of the results. An image is held to
    /// it (`image_widget`), so the pane shows no blank rows below it.
    fn end_offset(&self) -> usize {
        self.total_lines().saturating_sub(self.area.height as usize)
    }

    // ----- actions --------
    pub fn up(&mut self, n: u16) {
        let total_lines = self.total_lines();
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
        let total_lines = self.total_lines();
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
        if horizontal {
            let a = &mut self.scroll[1];

            *a = if val == 0 {
                0
            } else {
                a.saturating_add_signed(val as i16)
            };
        } else if val == 0 {
            self.offset = 0;
        } else {
            let total = self.total_lines() as isize;

            self.offset = (self.offset as isize + val as isize).clamp(0, total) as usize;
        }
    }

    /// Zoom the displayed image by `steps` tenths of a factor.
    ///
    /// The counter is the preview's vertical scroll, anchored at the image's own
    /// zoom with `0` standing for zoom `1.0`; zooming never goes below that.
    pub fn zoom(&mut self, steps: i8) {
        self.scroll[0] = self.scroll[0].saturating_add_signed(steps as i16);
    }

    /// The step count an image's own `zoom` maps to, tenths rounded up.
    #[cfg(feature = "image")]
    fn zoom_steps(zoom: f32) -> u16 {
        ((zoom - 1.0).max(0.0) * 10.0).ceil() as u16
    }

    /// The zoom `steps` are worth from an image's own `zoom`, never below `1.0`.
    #[cfg(feature = "image")]
    fn zoom_at(zoom: f32, steps: u16) -> f32 {
        (zoom + (f32::from(steps) - f32::from(Self::zoom_steps(zoom))) * 0.1).max(1.0)
    }

    /// Whether a vertical scroll zooms the image rather than moving the preview.
    ///
    /// Only while image rows are on screen, so the text below an image still
    /// scrolls once the image has left the pane.
    pub fn zooms_on_scroll(&self) -> bool {
        #[cfg(feature = "image")]
        {
            self.image_rows() > 0
                && self.offset < self.image_header_source_lines() + self.image_rows()
        }
        #[cfg(not(feature = "image"))]
        {
            false
        }
    }

    pub fn set_target(&mut self, target: Option<isize>) {
        _info!(target);

        if self.initial().tail {
            return;
        }

        #[cfg(feature = "image")]
        if self.view.image().is_some() {
            // Images do not follow the active item's line.
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
        self.offset = self.end_offset();
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

    /// Hand the terminal-detected image protocol picker to the preview pane.
    #[cfg(feature = "image")]
    pub(super) fn set_picker(&mut self, picker: Option<Picker>) {
        self.image = picker.map(|picker| ImageState::new(self.view.generation_handle(), picker));
    }

    /// Wake the render loop when a rebuilt protocol is ready.
    #[cfg(feature = "image")]
    pub fn set_event_sender(&mut self, event_tx: EventSender) {
        if let Some(image) = &mut self.image {
            image.set_event_sender(event_tx);
        }
    }

    /// Whether the worker has built the protocol for the image the pane is showing.
    #[cfg(feature = "image")]
    pub fn image_ready(&self) -> bool {
        self.image.as_ref().is_some_and(ImageState::ready)
    }

    /// The image widget, cropped to the current scroll offset, leaving `body` at the
    /// part of the pane below the image block for the text to fill.
    ///
    /// `None` when the preview is not showing an image, and while the protocol for the
    /// current image and pane is still being built.
    ///
    /// This is where the pane learns the view has left an image: the protocol and the
    /// state the pane kept for it are dropped there.
    #[cfg(feature = "image")]
    pub fn image_widget(&mut self, body: &mut Rect) -> Option<ImageWidget> {
        let Some(image) = self.view.image() else {
            // The pane shows text now: the image's protocol, offset, target and zoom
            // counter go with it, while the horizontal scroll is the pane's and stays.
            if let Some(state) = self.image.as_mut()
                && state.clear()
            {
                self.offset = 0;
                self.scroll[0] = 0;
                self.attained_target = false;
                self.jump = (false, 0);
            }
            return None;
        };
        let mut options = self.config.image.clone();
        options.apply(image.options.clone());

        if Some(image.id) != self.image.as_ref().and_then(ImageState::last_id) {
            // The header's initial view: the vertical offset scrolls the preview,
            // the horizontal one pans the image, and the zoom counter starts at
            // what the image's own zoom is worth.
            self.offset = options.scroll[0] as usize;
            self.scroll[1] = options.scroll[1];
            self.scroll[0] = Self::zoom_steps(options.zoom);

            // dead parameters when image
            self.target = None;
            self.attained_target = false;
            self.jump = (false, 0);
        }
        // Every request carries the zoom the counter currently reaches.
        options.zoom = Self::zoom_at(options.zoom, self.scroll[0]);

        // The header is the output's first `header_lines` lines, drawn above the image.
        // The pane lays the paragraph out here, before it makes its request: the rows it
        // renders into are the fit the image is built for, and the lines it holds are the
        // ones the pane's scroll has left of it.
        let source_header_lines = options.header_lines as usize;
        let header = (source_header_lines != 0).then(|| {
            let mut text = self.view.results();
            if self.config.trim_ends {
                trim_text_lines(&mut text);
            }
            let first = self.offset.min(source_header_lines);
            let lines: Vec<Line<'static>> = text
                .lines
                .into_iter()
                .skip(first)
                .take(source_header_lines - first)
                .collect();
            let scroll = [0, self.scroll[1]];
            let mut header = Paragraph::new(lines).scroll(scroll.into());
            if self.config.wrap {
                header = header.wrap(Wrap { trim: false });
            }
            header
        });
        let header_height = header
            .as_ref()
            .map_or(0, |header| header.line_count(self.area.width));

        let mut fit = self.area;
        fit.height = fit
            .height
            .saturating_sub(header_height.min(u16::MAX as usize) as u16);
        if fit.height == 0 {
            // A header taller than the pane still leaves the image the whole pane.
            fit.height = self.area.height;
        }
        self.image
            .as_mut()?
            .prepare(image, &options, fit, &mut self.scroll[1]);

        // The pane does not scroll past what the contents hold.
        self.offset = self.offset.min(self.total_lines());

        // What the pane shows of the image block: the header's rows, then the image's,
        // which are held to the rows it was fitted into and to the document's own.
        let state = self.image.as_mut()?;
        let scrolled = self.offset.saturating_sub(state.header_lines());
        let image_rows = state
            .display_rows()
            .min(state.rows().saturating_sub(scrolled));
        let height = (header_height + image_rows).min(self.area.height as usize);

        body.y = body.y.saturating_add(height as u16);
        body.height = body.height.saturating_sub(height as u16);

        let widget = state.widget(header_height, scrolled, height)?;
        Some(match header {
            Some(header) => widget.with_header(header),
            None => widget,
        })
    }

    /// The pane's contents: its text, split around the image block when it holds one,
    /// in the `height` rows the renderer left it.
    ///
    /// The pane's frame is not part of them; the renderer draws it around the whole
    /// pane.
    pub fn make_preview(&mut self, height: u16) -> Paragraph<'static> {
        // The contents can shrink under the offset: the pane does not scroll past what
        // it holds.
        self.offset = self.offset.min(self.total_lines());

        let mut results = self.view.results();
        if self.config.trim_ends {
            trim_text_lines(&mut results)
        }

        let rl = results.lines.len();
        let height = height as usize;
        let mut offset = self.offset;

        // The image's rows are the pane's from the request on: it holds them whether
        // the protocol has been built yet or not.
        // this only triggers on preview change but not guaranteed on every preview change -- attaching it to the event handler is worse
        #[cfg(feature = "image")]
        let image_rows = self.image_rows();
        #[cfg(not(feature = "image"))]
        let image_rows = 0;
        #[cfg(feature = "image")]
        let header_source_lines = self.image_header_source_lines();
        #[cfg(not(feature = "image"))]
        let header_source_lines = 0;
        let image_on_screen = image_rows > 0;

        if rl < self.last_count && !image_on_screen {
            self.offset = 0;
            self.attained_target = false;
            self.jump = (false, 0)
        }
        self.last_count = rl;

        if self.initial().tail && !self.attained_target && !image_on_screen {
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
            offset = self.offset;
            self.attained_target = true;
        };

        let mut results = results.into_iter();

        if height == 0 {
            return Paragraph::new(Vec::new());
        }

        let mut lines = Vec::with_capacity(height);

        if image_on_screen {
            // The image block drew the document's first rows; only the text after the
            // header and below the image is left here.
            let body_start = header_source_lines + image_rows;
            let first = header_source_lines.min(rl) + offset.saturating_sub(body_start);
            let mut results = results.skip(first);
            for _ in 0..height {
                if let Some(line) = results.next() {
                    lines.push(line);
                }
            }
        } else {
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
        }

        // The horizontal scroll is the pane's: it pans the image and moves the text
        // below it alike. Slot zero is the image's zoom counter, never a text scroll.
        let scroll = [0, self.scroll[1]];
        let mut preview = Paragraph::new(lines).scroll(scroll.into());
        if self.config.wrap {
            preview = preview.wrap(Wrap { trim: false });
        }
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "image")]
    use crate::preview::image::PreviewImage;
    use crate::{
        config::PreviewLayout,
        preview::{AppendOnly, Override},
        utils::Percentage,
    };
    #[cfg(feature = "image")]
    use ratatui_image::picker::Picker;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{Arc, Mutex};

    fn test_preview(side: Side) -> PreviewUI {
        let override_ = Arc::new(Mutex::new(Override::Text(ratatui::text::Text::raw(
            "preview",
        ))));
        let config = PreviewConfig {
            show: ShowCondition::Bool(true),
            layout: vec![PreviewSetting {
                layout: PreviewLayout {
                    side,
                    percentage: Percentage::new(60),
                    min: 15,
                    max: 50,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        PreviewUI::new(
            Preview::new(
                AppendOnly::new(),
                Default::default(),
                override_,
                Arc::new(AtomicUsize::new(0)),
            ),
            config,
            [80, 24],
        )
    }

    #[test]
    fn split_right() {
        let preview = test_preview(Side::Right);
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [preview_rect, picker_rect] = preview.split(area);

        // 60% of 80 = 48, clamped to [15, 50] (+ 2 border padding)
        assert_eq!(
            preview_rect,
            Rect {
                x: 32,
                y: 0,
                width: 48,
                height: 24
            }
        );
        assert_eq!(
            picker_rect,
            Rect {
                x: 0,
                y: 0,
                width: 32,
                height: 24
            }
        );
    }

    #[test]
    fn split_bottom() {
        let preview = test_preview(Side::Bottom);
        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [preview_rect, picker_rect] = preview.split(area);

        // 60% of 24 = 14, clamped up to the 15 minimum
        assert_eq!(
            preview_rect,
            Rect {
                x: 0,
                y: 9,
                width: 80,
                height: 15
            }
        );
        assert_eq!(
            picker_rect,
            Rect {
                x: 0,
                y: 0,
                width: 80,
                height: 9
            }
        );
    }

    #[test]
    fn split_respects_current_dimension() {
        let mut preview = test_preview(Side::Right);
        preview.update_dimensions(&Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        preview.shrink(10);

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [preview_rect, picker_rect] = preview.split(area);

        // initial size is 80 (78 inner + 2 border padding); shrunk by 10
        assert_eq!(
            preview_rect,
            Rect {
                x: 10,
                y: 0,
                width: 70,
                height: 24
            }
        );
        assert_eq!(
            picker_rect,
            Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 24
            }
        );
    }

    #[test]
    fn split_expand_resets_current_dimension() {
        let mut preview = test_preview(Side::Right);
        preview.update_dimensions(&Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        preview.shrink(10);
        preview.expand(0);

        let area = Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        };
        let [preview_rect, _picker_rect] = preview.split(area);

        // expand(0) resets the custom dimension, back to the 60% default
        assert_eq!(
            preview_rect,
            Rect {
                x: 32,
                y: 0,
                width: 48,
                height: 24
            }
        );
    }

    /// Ending the scroll puts the contents' last line on the pane's last row, under a
    /// pinned header like anything else.
    #[test]
    fn scroll_end_puts_the_last_line_on_the_last_row() {
        let generation = Arc::new(AtomicUsize::new(0));
        let lines = (0..40)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        let override_ = Arc::new(Mutex::new(Override::Text(ratatui::text::Text::raw(lines))));
        let config = PreviewConfig {
            show: ShowCondition::Bool(true),
            initial: crate::config::PreviewInitialSetting {
                header_lines: 2,
                ..Default::default()
            },
            layout: vec![PreviewSetting {
                layout: PreviewLayout {
                    side: Side::Right,
                    percentage: Percentage::new(60),
                    min: 15,
                    max: 50,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut preview = PreviewUI::new(
            Preview::new(AppendOnly::new(), Default::default(), override_, generation),
            config,
            [80, 24],
        );
        preview.update_dimensions(&Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        assert_eq!(preview.initial().header_lines, 2, "the header is pinned");
        assert_eq!(preview.text_offset(), 2, "the body starts under the header");

        preview.scroll_end();
        assert_eq!(
            preview.offset,
            40 - preview.area.height as usize,
            "the header rows count toward the end of the scroll"
        );
        assert_eq!(
            preview.text_offset(),
            preview.initial().header_lines + preview.offset,
            "the text offset follows the pane's scroll"
        );
    }

    /// An image pane shows no blank rows below the contents: the offset is held to
    /// their end.
    #[test]
    #[cfg(feature = "image")]
    fn an_image_is_held_to_the_end_of_the_contents() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            Default::default(),
            &lines,
        );
        wait_for_image(&mut preview);

        preview.offset = preview.total_lines() + 10;
        image_block(&mut preview);
        assert_eq!(
            preview.offset,
            preview.total_lines(),
            "the offset is clamped to total lines"
        );
    }

    /// A header longer than the text still holds its rows: the pane can bring the
    /// image's last row to its last row.
    #[test]
    #[cfg(feature = "image")]
    fn a_header_longer_than_the_text_holds_its_rows() {
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                header_lines: Some(40),
                ..Default::default()
            },
            &[],
        );
        wait_for_image(&mut preview);

        assert_eq!(
            preview.total_lines(),
            preview.image_rows() + 40,
            "the header rows are part of the contents"
        );

        preview.down(u16::MAX);
        image_block(&mut preview);
        assert_eq!(
            preview.offset,
            preview.total_lines(),
            "the offset is clamped to total lines"
        );
    }

    /// The text offset steps over the header and the image's rows.
    #[test]
    #[cfg(feature = "image")]
    fn the_text_offset_steps_over_the_header_and_the_image() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                header_lines: Some(2),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);

        assert_eq!(
            preview.text_offset(),
            2,
            "the text starts where the header ends"
        );

        preview.offset = 12;
        assert_eq!(
            preview.text_offset(),
            2,
            "the text is still behind the image"
        );

        preview.offset = 20;
        assert_eq!(
            preview.text_offset(),
            10,
            "the text has scrolled up past the image by its own rows"
        );
    }

    /// A visible preview pane showing `image` with `lines` of text after it.
    #[cfg(feature = "image")]

    fn image_preview(
        image: Arc<image::DynamicImage>,
        options: crate::config::PartialImageOptions,
        lines: &[String],
    ) -> PreviewUI {
        image_preview_with_override(image, options, lines).0
    }

    /// [`image_preview`], with the override handle so a test can replace the image.
    #[cfg(feature = "image")]
    fn image_preview_with_override(
        image: Arc<image::DynamicImage>,
        options: crate::config::PartialImageOptions,
        lines: &[String],
    ) -> (PreviewUI, Arc<Mutex<Override>>) {
        // The image's id is the generation it was installed at.
        let generation = Arc::new(AtomicUsize::new(0));
        let override_ = Arc::new(Mutex::new(Override::Image(PreviewImage {
            data: image,
            options,
            id: 0,
        })));
        let config = PreviewConfig {
            show: ShowCondition::Bool(true),
            layout: vec![PreviewSetting {
                layout: crate::config::PreviewLayout {
                    side: Side::Right,
                    percentage: Percentage::new(60),
                    min: 15,
                    max: 50,
                },
                ..Default::default()
            }],
            ..Default::default()
        };
        let text = AppendOnly::new();
        for line in lines {
            text.push(Line::raw(line.clone()));
        }
        let mut preview = PreviewUI::new(
            Preview::new(text, Default::default(), Arc::clone(&override_), generation),
            config,
            [80, 24],
        );
        preview.set_picker(Some(Picker::halfblocks()));
        preview.update_dimensions(&Rect {
            x: 0,
            y: 0,
            width: 80,
            height: 24,
        });
        (preview, override_)
    }

    #[test]
    #[cfg(feature = "image")]
    fn image_restarts_at_its_own_offsets() {
        // 1000x2000 pixels is 100x100 halfblock cells, which is wider and taller than
        // the 80x24 pane, so both axes of the initial scroll can be kept.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(1000, 2000)),
            crate::config::PartialImageOptions {
                scroll: Some([3, 4]),
                horizontal_fit: Some(crate::config::ImageFit::Crop),
                ..Default::default()
            },
            &[],
        );

        preview.offset = 9;
        preview.scroll[1] = 7;
        // Preparing happens as part of rendering the widget.
        image_block(&mut preview);
        assert_eq!(preview.offset, 3);
        assert_eq!(preview.scroll[1], 4);
    }

    #[test]
    #[cfg(feature = "image")]
    fn image_pan_is_clamped_to_the_image() {
        // 40x20 in halfblocks' 10x20 cell is narrower than the pane, so there is no pan
        // to be had and the stored offset must come back to zero.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(40, 20)),
            Default::default(),
            &[],
        );

        preview.scroll[1] = 5;
        image_block(&mut preview);
        assert_eq!(preview.scroll[1], 0);
    }

    #[test]
    #[cfg(feature = "image")]
    fn image_builds_on_the_worker() {
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            Default::default(),
            &[],
        );

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !preview.image_ready() {
            image_block(&mut preview);
            assert!(
                std::time::Instant::now() < deadline,
                "the image worker never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[tokio::test]
    #[cfg(feature = "image")]
    async fn image_builds_from_a_runtime_context() {
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            Default::default(),
            &[],
        );

        let mut attempts = 0;
        while !preview.image_ready() {
            image_block(&mut preview);
            attempts += 1;
            assert!(attempts < 1000, "the image worker never finished");
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    /// The image block the renderer would draw, for the tests that only want it.
    #[cfg(feature = "image")]
    fn image_block(preview: &mut PreviewUI) -> Option<ImageWidget> {
        // The body it leaves is where the text goes, which these tests do not draw.
        let mut body = preview.area;
        preview.image_widget(&mut body)
    }

    /// Wait for the worker to build the protocol for the pane's image.
    #[cfg(feature = "image")]
    fn wait_for_image(preview: &mut PreviewUI) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while !preview.image_ready() {
            image_block(preview);
            assert!(
                std::time::Instant::now() < deadline,
                "the image worker never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Wait for the worker to build a protocol with `rows` image rows.
    #[cfg(feature = "image")]
    fn wait_for_image_rows(preview: &mut PreviewUI, rows: usize) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while preview.image_rows() != rows {
            image_block(preview);
            assert!(
                std::time::Instant::now() < deadline,
                "the image worker never finished"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    #[cfg(feature = "image")]
    fn text_is_split_around_the_image() {
        use ratatui::{buffer::Buffer, widgets::Widget};

        let mut lines = vec!["header 0".to_string(), "header 1".to_string()];
        lines.extend((0..40).map(|i| format!("body {i}")));
        // 100x200 pixels is a 10x10 halfblock cell image.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                header_lines: Some(2),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);

        assert_eq!(preview.image_rows(), 10);
        // The two header lines, the image, then the body.
        assert_eq!(preview.total_lines(), 10 + lines.len());

        let mut body = preview.area;
        let image = preview.image_widget(&mut body).unwrap();
        let widget = preview.make_preview(body.height);
        assert_eq!(
            body.y,
            preview.area.y + 12,
            "the body starts under the header and the image"
        );
        let mut buf = Buffer::empty(preview.area);
        image.render(preview.area, &mut buf);
        widget.render(body, &mut buf);

        let row = |buf: &Buffer, y: u16| {
            (preview.area.x..preview.area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        assert!(row(&buf, preview.area.y).starts_with("header 0"));
        assert!(row(&buf, preview.area.y + 1).starts_with("header 1"));
        assert!(
            row(&buf, body.y).starts_with("body 0"),
            "the body follows the image"
        );
    }

    #[test]
    #[cfg(feature = "image")]
    fn the_image_fits_below_the_header() {
        let lines = (0..40).map(|i| format!("line {i}")).collect::<Vec<_>>();
        // A 1000x2000 pixel image is 100x100 halfblock cells, taller than the pane.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(1000, 2000)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                header_lines: Some(2),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);

        assert_eq!(preview.image_rows(), preview.area.height as usize - 2);
    }

    /// A wrapped header grows past `header_lines` rows, the image is refitted into what
    /// is left, and the text below starts after both.
    #[test]
    #[cfg(feature = "image")]
    fn a_wrapped_header_grows_and_the_image_keeps_off_it() {
        use ratatui::{buffer::Buffer, widgets::Widget};

        let long = "x".repeat(200);
        let mut lines = vec![long];
        lines.extend((0..40).map(|i| format!("body {i}")));
        // 100x200 pixels is a 10x10 halfblock cell image.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                header_lines: Some(1),
                ..Default::default()
            },
            &lines,
        );
        preview.config.wrap = true;
        wait_for_image(&mut preview);
        let area = preview.area;

        let header_rows = 200usize.div_ceil(area.width as usize);
        assert!(header_rows > 1, "the header line wraps at the pane's width");
        assert_eq!(
            preview.image_rows(),
            10,
            "a short image still fits under the wrapped header"
        );
        assert_eq!(
            preview.total_lines(),
            10 + lines.len(),
            "the header's lines are part of the text"
        );

        let mut body = area;
        let image = preview.image_widget(&mut body).unwrap();
        let widget = preview.make_preview(body.height);
        assert_eq!(
            body.y,
            area.y + (header_rows + 10) as u16,
            "the body starts below the wrapped header and the image"
        );

        let mut buf = Buffer::empty(area);
        image.render(area, &mut buf);
        widget.render(body, &mut buf);

        let row = |buf: &Buffer, y: u16| {
            (area.x..area.right())
                .map(|x| buf[(x, y)].symbol())
                .collect::<String>()
        };
        for y in 0..header_rows as u16 {
            assert!(
                row(&buf, area.y + y).starts_with("xxxx"),
                "the header covers row {y}"
            );
        }
        assert!(row(&buf, body.y).starts_with("body 0"));

        // The pane's scroll counts the header's source lines, so the one line it has
        // takes its wrapped rows with it and the image rises to the pane's top.
        preview.offset = 1;
        let mut body = area;
        preview.image_widget(&mut body).unwrap();
        assert_eq!(body.y, area.y + 10, "the header has scrolled off");
        assert_eq!(preview.text_offset(), 1, "the body has not started yet");
    }

    /// Wrapping the header asks for a new image: the rows it renders into are part of
    /// the request, and the image is fitted into the pane below them.
    #[test]
    #[cfg(feature = "image")]
    fn a_wrapped_header_refits_the_image() {
        // 1000x2000 pixels is 100x100 halfblock cells, taller than the pane.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(1000, 2000)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                header_lines: Some(1),
                ..Default::default()
            },
            &["x".repeat(200)],
        );
        wait_for_image(&mut preview);
        let area = preview.area;

        assert_eq!(
            preview.image_rows(),
            area.height as usize - 1,
            "the image takes the pane below the header's one line"
        );

        preview.config.wrap = true;
        let mut body = area;
        preview.image_widget(&mut body).unwrap();

        let header_rows = 200usize.div_ceil(area.width as usize);
        assert!(header_rows > 1, "the header line wraps at the pane's width");
        assert_eq!(
            preview.image_rows(),
            area.height as usize - header_rows,
            "the image is fitted below the rows the wrapped header renders into"
        );
    }

    /// A header taller than the pane leaves the image the pane's whole height rather
    /// than collapsing its fit to nothing.
    #[test]
    #[cfg(feature = "image")]
    fn the_image_gets_the_pane_when_the_header_is_taller() {
        // A 1000x2000 pixel image is 100x100 halfblock cells, taller than the pane.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(1000, 2000)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                header_lines: Some(40),
                ..Default::default()
            },
            &[],
        );
        wait_for_image(&mut preview);

        assert_eq!(preview.image_rows(), preview.area.height as usize);
    }

    /// A text preview replacing an image drops the image's protocol, offset and zoom
    /// instead of skipping the rows it used to occupy. The pane keeps its horizontal
    /// scroll, which the image merely panned.
    #[test]
    #[cfg(feature = "image")]
    fn a_text_preview_after_an_image_forgets_the_image() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        let (mut preview, override_) = image_preview_with_override(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                header_lines: Some(2),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);
        preview.offset = 5;
        preview.scroll[1] = 3;
        preview.scroll[0] = 4;
        assert_eq!(preview.image_rows(), 10);

        *override_.lock().unwrap() = Override::Text(ratatui::text::Text::raw(lines.join("\n")));
        // The frame's first call reconciles the pane with the view.
        image_block(&mut preview);
        preview.make_preview(preview.area.height);

        assert_eq!(preview.image_rows(), 0, "the image protocol is dropped");
        assert_eq!(preview.offset, 0, "the image's offset does not carry over");
        assert_eq!(preview.scroll[1], 3, "the pane keeps its horizontal scroll");
        assert_eq!(preview.scroll[0], 0, "the image's zoom does not carry over");
    }

    /// Zoom magnifies the image's rows in the document, from the image's own zoom.
    #[test]
    #[cfg(feature = "image")]
    fn the_zoom_magnifies_the_image_down_the_document() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                zoom: Some(2.0),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);

        assert_eq!(preview.scroll[0], 10, "the image's own zoom is the base");
        assert_eq!(preview.image_rows(), 20);
        assert_eq!(preview.total_lines(), 20 + lines.len());

        preview.zoom(1);
        assert_eq!(preview.scroll[0], 11);
        // The widget has to run for the new zoom to reach the worker.
        image_block(&mut preview);
        assert_eq!(
            preview.image_rows(),
            21,
            "the pane takes the new rows with the counter, before the protocol lands"
        );
        wait_for_image_rows(&mut preview, 21);
        assert_eq!(preview.total_lines(), 21 + lines.len());
    }

    /// A zoom that shrinks the image pulls an offset past its end back in.
    #[test]
    #[cfg(feature = "image")]
    fn zooming_out_pulls_the_offset_back() {
        // 100x2000 pixels is 10x100 halfblock cells, taller than the 24-row pane.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 2000)),
            crate::config::PartialImageOptions {
                zoom: Some(2.0),
                ..Default::default()
            },
            &[],
        );
        wait_for_image(&mut preview);
        assert_eq!(preview.image_rows(), 200);

        preview.offset = 176;

        preview.zoom(-10);
        assert_eq!(preview.scroll[0], 0, "ten steps down reach zoom 1.0");
        image_block(&mut preview);
        assert_eq!(
            preview.offset, 100,
            "the offset comes back to the shorter image"
        );
        wait_for_image_rows(&mut preview, 100);
    }

    /// A zoom off the tenths grid keeps its own value: the steps count from it.
    #[test]
    #[cfg(feature = "image")]
    fn a_zoom_off_the_grid_counts_steps_from_its_own_value() {
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                zoom: Some(1.41),
                ..Default::default()
            },
            &[],
        );
        wait_for_image(&mut preview);

        assert_eq!(preview.scroll[0], 5, "1.41 is five tenths, rounded up");
        assert_eq!(preview.image_rows(), 14);

        preview.zoom(-2);
        image_block(&mut preview);
        wait_for_image_rows(&mut preview, 12);

        preview.zoom(-3);
        assert_eq!(preview.scroll[0], 0, "the counter stops at zero");
        image_block(&mut preview);
        wait_for_image_rows(&mut preview, 10);
    }

    /// The wheel only zooms while the image is on screen; past it, it scrolls.
    #[test]
    #[cfg(feature = "image")]
    fn the_wheel_zooms_only_while_the_image_is_on_screen() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            Default::default(),
            &lines,
        );
        wait_for_image(&mut preview);
        assert!(preview.zooms_on_scroll());

        preview.offset = preview.end_offset();
        assert!(!preview.zooms_on_scroll(), "the image left the pane");
    }

    /// The header's rendered rows are the fit the image is built for: as the pane's scroll
    /// takes the header's lines away, the image grows into the rows they leave.
    #[test]
    #[cfg(feature = "image")]
    fn scrolling_the_header_away_grows_the_image() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        // 1000x2000 pixels is 100x100 halfblock cells, bigger than the pane.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(1000, 2000)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                header_lines: Some(2),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);
        let area = preview.area;

        assert_eq!(preview.image_rows(), area.height as usize - 2);

        for (offset, rows) in [(1, area.height as usize - 1), (2, area.height as usize)] {
            preview.offset = offset;
            let mut body = area;
            preview.image_widget(&mut body).unwrap();
            assert_eq!(
                preview.image_rows(),
                rows,
                "the image takes the rows the header leaves"
            );
            assert_eq!(
                body.y,
                area.y + area.height,
                "the header's remaining rows and the image fill the pane"
            );
        }
    }

    /// A magnified image is panned through the rows it was fitted into: the pane shows its
    /// fitted rows rather than its magnified ones, and the scroll moves the window down it.
    #[test]
    #[cfg(feature = "image")]
    fn a_zoomed_image_is_panned_through_its_fitted_rows() {
        let lines = (0..40).map(|i| format!("body {i}")).collect::<Vec<_>>();
        // 100x200 pixels is a 10x10 halfblock cell image.
        let mut preview = image_preview(
            Arc::new(image::DynamicImage::new_rgb8(100, 200)),
            crate::config::PartialImageOptions {
                vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                zoom: Some(2.0),
                ..Default::default()
            },
            &lines,
        );
        wait_for_image(&mut preview);
        let area = preview.area;

        assert_eq!(
            preview.image_rows(),
            20,
            "the zoom doubles the image's rows in the document"
        );

        let mut body = area;
        preview.image_widget(&mut body).unwrap();
        assert_eq!(body.y, area.y + 10, "the image's fitted rows are all shown");

        preview.offset = 15;
        let mut body = area;
        preview.image_widget(&mut body).unwrap();
        assert_eq!(
            body.y,
            area.y + 5,
            "the magnified image's last rows are left"
        );

        preview.offset = 20;
        let mut body = area;
        preview.image_widget(&mut body).unwrap();
        assert_eq!(body.y, area.y, "the image has left the pane");
        assert_eq!(preview.text_offset(), 0, "the text is at its first line");
    }
}
