use cba::bring::split::split_on_nesting;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    SSS, Selection, Selector,
    config::{HorizontalSeparator, ResultsConfig, RowConnectionStyle, StatusConfig},
    nucleo::{Status, Worker},
    render::Click,
    utils::{
        string::{allocate_widths, fit_width, substitute_escaped},
        text::{clip_text_lines, expand_indents, prefix_text},
    },
};

#[derive(Debug)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u32,
    col: Option<usize>,
    pub hscroll: i8,
    pub vscroll: u8,

    /// available height
    height: u16,
    /// available width
    width: u16,
    // column widths.
    // Note that the first width doesn't include the indentation.
    widths: Vec<u16>,

    pub hidden_columns: Vec<bool>,

    pub status: Status,
    status_template: Line<'static>,
    pub status_config: StatusConfig,

    pub config: ResultsConfig,

    bottom_clip: Option<u16>,
    cursor_above: u16,

    pub cursor_disabled: bool,
}

impl ResultsUI {
    pub fn new(config: ResultsConfig, status_config: StatusConfig) -> Self {
        Self {
            cursor: 0,
            bottom: 0,
            col: None,
            hscroll: 0,
            vscroll: 0,

            widths: Vec::new(),
            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            hidden_columns: Default::default(),

            status: Default::default(),
            status_template: Line::from(status_config.template.clone())
                .style(status_config.base_style()),
            status_config,
            config,

            cursor_disabled: false,
            bottom_clip: None,
            cursor_above: 0,
        }
    }

    pub fn hidden_columns(&mut self, hidden_columns: Vec<bool>) {
        self.hidden_columns = hidden_columns;
    }

    // as given by ratatui area
    pub fn update_dimensions(&mut self, area: &Rect) {
        let [bw, bh] = [self.config.border.height(), self.config.border.width()];
        self.width = area.width.saturating_sub(bw);
        self.height = area.height.saturating_sub(bh);
        log::debug!("Updated results dimensions: {}x{}", self.width, self.height);
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    // ------ config -------
    pub fn reverse(&self) -> bool {
        self.config.reverse == Some(true)
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }
    pub fn wrap(&mut self, wrap: bool) {
        self.config.wrap = wrap;
    }

    // ----- columns --------
    // todo: support cooler things like only showing/outputting a specific column/cycling columns
    pub fn toggle_col(&mut self, col_idx: usize) -> bool {
        self.reset_current_scroll();

        if self.col == Some(col_idx) {
            self.col = None
        } else {
            self.col = Some(col_idx);
        }
        self.col.is_some()
    }
    pub fn cycle_col(&mut self) {
        self.reset_current_scroll();

        self.col = match self.col {
            None => self.widths.is_empty().then_some(0),
            Some(c) => {
                let next = c + 1;
                if next < self.widths.len() {
                    Some(next)
                } else {
                    None
                }
            }
        };
    }

    // ------- NAVIGATION ---------
    fn scroll_padding(&self) -> u16 {
        self.config.scroll_padding.min(self.height / 2)
    }
    pub fn end(&self) -> u32 {
        self.status.matched_count.saturating_sub(1)
    }

    /// Index in worker snapshot of current item.
    /// Use with worker.get_nth().
    //  Equivalently, the cursor progress in the match list
    pub fn index(&self) -> u32 {
        if self.cursor_disabled {
            u32::MAX
        } else {
            self.cursor as u32 + self.bottom
        }
    }
    // pub fn cursor(&self) -> Option<u16> {
    //     if self.cursor_disabled {
    //         None
    //     } else {
    //         Some(self.cursor)
    //     }
    // }
    pub fn cursor_prev(&mut self) {
        self.reset_current_scroll();

        log::trace!("cursor_prev: {self:?}");
        if self.cursor_above <= self.scroll_padding() && self.bottom > 0 {
            self.bottom -= 1;
            self.bottom_clip = None;
        } else if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(self.end());
        }
    }
    pub fn cursor_next(&mut self) {
        self.reset_current_scroll();

        if self.cursor_disabled {
            self.cursor_disabled = false
        }

        // log::trace!(
        //     "Cursor {} @ index {}. Status: {:?}.",
        //     self.cursor,
        //     self.index(),
        //     self.status
        // );
        if self.cursor + 1 + self.scroll_padding() >= self.height
            && self.bottom + (self.height as u32) < self.status.matched_count
        {
            self.bottom += 1; //
        } else if self.index() < self.end() {
            self.cursor += 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(0)
        }
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.reset_current_scroll();

        self.cursor_disabled = false;
        self.bottom_clip = None;

        let end = self.end();
        let index = index.min(end);

        if index < self.bottom as u32 || index >= self.bottom + self.height as u32 {
            self.bottom = (end + 1)
                .saturating_sub(self.height as u32) // don't exceed the first item of the last self.height items
                .min(index);
        }
        self.cursor = (index - self.bottom) as u16;
        log::debug!("cursor jumped to {}: {index}, end: {end}", self.cursor);
    }

    pub fn current_scroll(&mut self, x: i8, horizontal: bool) {
        if horizontal {
            self.hscroll = if x == 0 {
                0
            } else {
                self.hscroll.saturating_add(x)
            };
        } else {
            self.vscroll = if x == 0 {
                0
            } else if x.is_negative() {
                self.vscroll.saturating_sub(x.unsigned_abs())
            } else {
                self.vscroll.saturating_add(x as u8)
            };
        }
    }

    pub fn reset_current_scroll(&mut self) {
        self.hscroll = 0;
        self.vscroll = 0;
    }

    // ------- RENDERING ----------
    pub fn indentation(&self) -> usize {
        self.config.multi_prefix.width()
    }
    pub fn col(&self) -> Option<usize> {
        self.col
    }

    /// Column widths.
    /// Note that the first width doesn't include the indentation.
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }

    /// Adapt the stored widths (initialized by [`Worker::results`]) to the fit within the available width (self.width)
    /// widths <= min_wrap_width don't shrink and aren't wrapped
    pub fn max_widths(&self) -> Vec<u16> {
        let mut base_widths = self.widths.clone();

        if base_widths.is_empty() {
            return base_widths;
        }
        base_widths.resize(self.hidden_columns.len().max(base_widths.len()), 0);

        for (i, is_hidden) in self.hidden_columns.iter().enumerate() {
            if *is_hidden {
                base_widths[i] = 0;
            }
        }

        let target = self.content_width();
        let sum: u16 = base_widths
            .iter()
            .map(|x| {
                (*x != 0)
                    .then_some(*x.max(&self.config.min_wrap_width))
                    .unwrap_or_default()
            })
            .sum();

        if sum < target {
            let nonzero_count = base_widths.iter().filter(|w| **w > 0).count();
            if nonzero_count > 0 {
                let extra_per_column = (target - sum) / nonzero_count as u16;
                let mut remainder = (target - sum) % nonzero_count as u16;

                for w in base_widths.iter_mut().filter(|w| **w > 0) {
                    *w += extra_per_column;
                    if remainder > 0 {
                        *w += 1;
                        remainder -= 1;
                    }
                }
            }
        }

        match allocate_widths(&base_widths, target, self.config.min_wrap_width) {
            Ok(s) | Err(s) => s,
        }
    }

    pub fn content_width(&self) -> u16 {
        self.width
            .saturating_sub(self.indentation() as u16)
            .saturating_sub(self.column_spacing_width())
    }

    pub fn column_spacing_width(&self) -> u16 {
        let pos = self.widths.iter().rposition(|&x| x != 0);
        self.config.column_spacing.0 * (pos.unwrap_or_default() as u16)
    }

    pub fn table_width(&self) -> u16 {
        if self.config.stacked_columns {
            self.width
        } else {
            self.widths.iter().sum::<u16>()
                + self.config.border.width()
                + self.indentation() as u16
                + self.column_spacing_width()
        }
    }

    // this updates the internal status, so be sure to call make_status afterward
    // some janky wrapping is implemented, dunno whats causing flickering, padding is fixed going down only
    pub fn make_table<'a, T: SSS>(
        &mut self,
        active_column: usize,
        worker: &'a mut Worker<T>,
        selector: &mut Selector<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
        click: &mut Click,
    ) -> Table<'a> {
        let offset = self.bottom as u32;
        let end = self.bottom + self.height as u32;
        let hz = !self.config.stacked_columns;

        let width_limits = if hz {
            self.max_widths()
        } else {
            let default = self.width.saturating_sub(self.indentation() as u16);

            (0..worker.columns.len())
                .map(|i| {
                    if self.hidden_columns.get(i).copied().unwrap_or(false) {
                        0
                    } else {
                        default
                    }
                })
                .collect()
        };

        let (mut results, mut widths, status) = worker.results(
            offset,
            end,
            &width_limits,
            self.config.wrap,
            self.match_style(),
            matcher,
            self.config.autoscroll,
            self.hscroll,
        );

        widths[0] += self.indentation() as u16;
        // should generally be true already, but act as a safeguard
        for x in widths.iter_mut().zip(&self.hidden_columns) {
            if *x.1 {
                *x.0 = 0
            }
        }
        let widths = widths;

        let match_count = status.matched_count;
        self.status = status;

        if match_count < self.bottom + self.cursor as u32 && !self.cursor_disabled {
            self.cursor_jump(match_count);
        } else {
            self.cursor = self.cursor.min(results.len().saturating_sub(1) as u16)
        }

        let mut rows = vec![];
        let mut total_height = 0;

        if results.is_empty() {
            return Table::new(rows, widths);
        }

        let height_of = |t: &(Vec<ratatui::text::Text<'a>>, _)| {
            self._hr()
                + if hz {
                    t.0.iter()
                        .map(|t| t.height() as u16)
                        .max()
                        .unwrap_or_default()
                } else {
                    t.0.iter().map(|t| t.height() as u16).sum::<u16>()
                }
        };

        // log::debug!("results initial: {}, {}, {}, {}, {}", self.bottom, self.cursor, total_height, self.height, results.len());
        let h_at_cursor = height_of(&results[self.cursor as usize]);
        let h_after_cursor = results[self.cursor as usize + 1..]
            .iter()
            .map(height_of)
            .sum();
        let h_to_cursor = results[0..self.cursor as usize]
            .iter()
            .map(height_of)
            .sum::<u16>();
        let cursor_end_should_lte = self.height - self.scroll_padding().min(h_after_cursor);
        // let cursor_start_should_gt = self.scroll_padding().min(h_to_cursor);

        // log::debug!(
        //     "Computed heights: {}, {h_at_cursor}, {h_to_cursor}, {h_after_cursor}, {cursor_end_should_lte}",
        //     self.cursor
        // );

        // begin adjustment
        let mut start_index = 0; // the index in results of the first complete item

        if h_at_cursor >= cursor_end_should_lte {
            start_index = self.cursor;
            self.bottom += self.cursor as u32;
            self.cursor = 0;
            self.cursor_above = 0;
            self.bottom_clip = None;
        } else
        // increase the bottom index so that cursor_should_above is maintained
        if let h_to_cursor_end = h_to_cursor + h_at_cursor
            && h_to_cursor_end > cursor_end_should_lte
        {
            let mut trunc_height = h_to_cursor_end - cursor_end_should_lte;
            // note that there is a funny side effect that scrolling up near the bottom can scroll up a bit, but it seems fine to me

            for r in results[start_index as usize..self.cursor as usize].iter_mut() {
                let h = height_of(r);
                let (row, item) = r;
                start_index += 1; // we always skip at least the first item

                if trunc_height < h {
                    let mut remaining_height = h - trunc_height;
                    let prefix = if selector.contains(item) {
                        self.config.multi_prefix.clone().to_string()
                    } else {
                        self.default_prefix(0)
                    };

                    total_height += remaining_height;

                    // log::debug!("r: {remaining_height}");
                    if hz {
                        if h - self._hr() < remaining_height {
                            for (_, t) in
                                row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0)
                            {
                                clip_text_lines(t, remaining_height, !self.reverse());
                            }
                        }

                        prefix_text(&mut row[0], prefix);

                        let last_visible = widths
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(i, w)| (*w != 0).then_some(i));

                        let mut row_texts: Vec<_> = row
                            .iter()
                            .take(last_visible.map(|x| x + 1).unwrap_or(0))
                            .cloned()
                            .collect();

                        if self.config.right_align_last && row_texts.len() > 1 {
                            row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                        }

                        let row = Row::new(row_texts).height(remaining_height);
                        rows.push(row);
                    } else {
                        let mut push = vec![];

                        for col in row.into_iter().rev() {
                            let mut height = col.height() as u16;
                            if remaining_height == 0 {
                                break;
                            } else if remaining_height < height {
                                clip_text_lines(col, remaining_height, !self.reverse());
                                height = remaining_height;
                            }
                            remaining_height -= height;
                            prefix_text(col, prefix.clone());
                            push.push(Row::new(vec![col.clone()]).height(height));
                        }
                        rows.extend(push.into_iter().rev());
                    }

                    self.bottom += start_index as u32 - 1;
                    self.cursor -= start_index - 1;
                    self.bottom_clip = Some(remaining_height);
                    break;
                } else if trunc_height == h {
                    self.bottom += start_index as u32;
                    self.cursor -= start_index;
                    self.bottom_clip = None;
                    break;
                }

                trunc_height -= h;
            }
        } else if let Some(mut remaining_height) = self.bottom_clip {
            start_index += 1;
            // same as above
            let h = height_of(&results[0]);
            let (row, item) = &mut results[0];
            let prefix = if selector.contains(item) {
                self.config.multi_prefix.clone().to_string()
            } else {
                self.default_prefix(0)
            };

            total_height += remaining_height;

            if hz {
                if self._hr() + remaining_height != h {
                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, remaining_height, !self.reverse());
                    }
                }

                prefix_text(&mut row[0], prefix);

                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                let row = Row::new(row_texts).height(remaining_height);
                rows.push(row);
            } else {
                let mut push = vec![];

                for col in row.into_iter().rev() {
                    let mut height = col.height() as u16;
                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        clip_text_lines(col, remaining_height, !self.reverse());
                        height = remaining_height;
                    }
                    remaining_height -= height;
                    prefix_text(col, prefix.clone());
                    push.push(Row::new(vec![col.clone()]).height(height));
                }
                rows.extend(push.into_iter().rev());
            }
        }

        // topside padding is not self-correcting, and can only do its best to stay at #padding lines without obscuring cursor on cursor movement events.
        let mut remaining_height = self.height.saturating_sub(total_height);

        for (mut i, (mut row, item)) in results.drain(start_index as usize..).enumerate() {
            i += self.bottom_clip.is_some() as usize;

            // this is technically one step out of sync but idc
            if let Click::ResultPos(c) = click
                && self.height - remaining_height > *c
            {
                let idx = self.bottom as u32 + i as u32 - 1;
                log::debug!("Mapped click position to index: {c} -> {idx}",);
                *click = Click::ResultIdx(idx);
            }
            if self.is_current(i) {
                self.cursor_above = self.height - remaining_height;
            }

            // insert hr
            if let Some(hr) = self.hr()
                && remaining_height > 0
            {
                rows.push(hr);
                remaining_height -= 1;
            }
            if remaining_height == 0 {
                break;
            }

            // determine prefix
            let prefix = if selector.contains(item) {
                self.config.multi_prefix.clone().to_string()
            } else {
                self.default_prefix(i)
            };

            if hz {
                // scroll down
                if self.is_current(i) && self.vscroll > 0 {
                    for (x, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        if self.col.is_none() || self.col() == Some(x) {
                            let scroll = self.vscroll as usize;

                            if scroll < t.lines.len() {
                                t.lines = t.lines.split_off(scroll);
                            } else {
                                t.lines.clear();
                            }
                        }
                    }
                }

                let mut height = row
                    .iter()
                    .map(|t| t.height() as u16)
                    .max()
                    .unwrap_or_default();

                if remaining_height < height {
                    height = remaining_height;

                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, height, self.reverse());
                    }
                }
                remaining_height -= height;

                // same as above
                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    // highlight
                    .enumerate()
                    .map(|(x, mut t)| {
                        let is_active_col = active_column == x;
                        let is_current_row = self.is_current(i);

                        if is_current_row && is_active_col {
                            // NOTE: hscroll is handled in worker.results -> render_cell
                        }

                        match self.config.row_connection {
                            RowConnectionStyle::Disjoint => {
                                if is_active_col {
                                    t = t.style(if is_current_row {
                                        self.current_style()
                                    } else {
                                        self.active_style()
                                    });
                                } else {
                                    t = t.style(if is_current_row {
                                        self.inactive_current_style()
                                    } else {
                                        self.inactive_style()
                                    });
                                }
                            }
                            RowConnectionStyle::Capped => {
                                if is_active_col {
                                    t = t.style(if is_current_row {
                                        self.current_style()
                                    } else {
                                        self.active_style()
                                    });
                                }
                            }
                            RowConnectionStyle::Full => {}
                        }

                        // prefix after hscroll
                        if x == 0 {
                            prefix_text(&mut t, prefix.clone());
                        };
                        t
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                // push
                let mut row = Row::new(row_texts).height(height);

                if self.is_current(i) {
                    match self.config.row_connection {
                        RowConnectionStyle::Capped => {
                            row = row.style(self.inactive_current_style())
                        }
                        RowConnectionStyle::Full => row = row.style(self.current_style()),
                        _ => {}
                    }
                }

                rows.push(row);
            } else {
                let mut push = vec![];
                let mut vscroll_to_skip = if self.is_current(i) {
                    self.vscroll as usize
                } else {
                    0
                };

                for (x, mut col) in row.into_iter().enumerate() {
                    if vscroll_to_skip > 0 {
                        let col_height = col.lines.len();
                        if vscroll_to_skip >= col_height {
                            vscroll_to_skip -= col_height;
                            continue;
                        } else {
                            col.lines = col.lines.split_off(vscroll_to_skip);
                            vscroll_to_skip = 0;
                        }
                    }

                    let mut height = col.height() as u16;

                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        height = remaining_height;
                        clip_text_lines(&mut col, remaining_height, self.reverse());
                    }
                    remaining_height -= height;

                    prefix_text(&mut col, prefix.clone());

                    let is_active_col = active_column == x;
                    let is_current_row = self.is_current(i);

                    match self.config.row_connection {
                        RowConnectionStyle::Disjoint => {
                            if is_active_col {
                                col = col.style(if is_current_row {
                                    self.current_style()
                                } else {
                                    self.active_style()
                                });
                            } else {
                                col = col.style(if is_current_row {
                                    self.inactive_current_style()
                                } else {
                                    self.inactive_style()
                                });
                            }
                        }
                        RowConnectionStyle::Capped => {
                            if is_active_col {
                                col = col.style(if is_current_row {
                                    self.current_style()
                                } else {
                                    self.active_style()
                                });
                            }
                        }
                        RowConnectionStyle::Full => {}
                    }

                    // push
                    let mut row = Row::new(vec![col]).height(height);
                    if is_current_row {
                        match self.config.row_connection {
                            RowConnectionStyle::Capped => {
                                row = row.style(self.inactive_current_style())
                            }
                            RowConnectionStyle::Full => row = row.style(self.current_style()),
                            _ => {}
                        }
                    }
                    push.push(row);
                }
                rows.extend(push);
            }
        }

        if self.reverse() {
            rows.reverse();
            if remaining_height > 0 {
                rows.insert(0, Row::new(vec![vec![]]).height(remaining_height));
            }
        }

        // ratatui column_spacing eats into the constraints
        let table_widths = if hz {
            // first 0 element after which all is 0
            let pos = widths.iter().rposition(|&x| x != 0);
            // column_spacing eats into the width
            let mut widths: Vec<_> = widths[..pos.map_or(0, |x| x + 1)].to_vec();
            if let Some(pos) = pos
                && pos > 0
                && self.config.right_align_last
            {
                let used = widths.iter().take(widths.len() - 1).sum();
                widths[pos] = self.width.saturating_sub(used);
            }
            if let Some(s) = widths.get_mut(0) {
                *s -= self.indentation() as u16
            }
            self.widths = widths.clone();

            if !self.config.wrap {
                widths
                    .iter_mut()
                    .zip(width_limits.iter())
                    .for_each(|(w, &limit)| {
                        *w = (*w).min(limit);
                    });
            }

            if let Some(s) = widths.get_mut(0) {
                *s += self.indentation() as u16;
            }

            widths
        } else {
            vec![self.width]
        };

        // log::debug!(
        //     "widths: {width_limits:?}, {widths:?}, {table_widths:?}, {:?}",
        //     self.widths
        // );

        let mut table = Table::new(rows, table_widths).column_spacing(self.config.column_spacing.0);

        table = match self.config.row_connection {
            RowConnectionStyle::Full => table.style(self.active_style()),
            RowConnectionStyle::Capped => table.style(self.inactive_style()),
            _ => table,
        };

        table = table.block(self.config.border.as_static_block());
        table
    }
}

impl ResultsUI {
    pub fn make_status(&self, full_width: u16) -> Paragraph<'_> {
        let status_config = &self.status_config;
        let replacements = [
            ('r', self.index().to_string()),
            ('m', self.status.matched_count.to_string()),
            ('t', self.status.item_count.to_string()),
        ];

        // sub replacements into line
        let mut new_spans = Vec::new();

        if status_config.match_indent {
            new_spans.push(Span::raw(" ".repeat(self.indentation())));
        }

        for span in &self.status_template {
            let subbed = substitute_escaped(&span.content, &replacements);
            new_spans.push(Span::styled(subbed, span.style));
        }

        let substituted_line = Line::from(new_spans);

        // sub whitespace expansions
        let effective_width = match self.status_config.row_connection {
            RowConnectionStyle::Full => full_width,
            _ => self.width,
        } as usize;
        let expanded = expand_indents(substituted_line, r"\s", r"\S", effective_width)
            .style(status_config.base_style());

        Paragraph::new(expanded)
    }

    pub fn set_status_line(&mut self, template: Option<Line<'static>>) {
        let status_config = &self.status_config;
        log::trace!("status line: {template:?}");

        self.status_template = template
            .unwrap_or(status_config.template.clone().into())
            .style(status_config.base_style())
            .into()
    }
}

// helpers
impl ResultsUI {
    fn default_prefix(&self, i: usize) -> String {
        let substituted = substitute_escaped(
            &self.config.default_prefix,
            &[
                ('d', &(i + 1).to_string()),                        // cursor index
                ('r', &(i + 1 + self.bottom as usize).to_string()), // absolute index
            ],
        );

        fit_width(&substituted, self.indentation())
    }

    fn current_style(&self) -> Style {
        Style::from(self.config.current_fg)
            .bg(self.config.current_bg)
            .add_modifier(self.config.current_modifier)
    }

    fn active_style(&self) -> Style {
        Style::from(self.config.fg)
            .bg(self.config.bg)
            .add_modifier(self.config.modifier)
    }

    fn inactive_style(&self) -> Style {
        Style::from(self.config.inactive_fg)
            .bg(self.config.inactive_bg)
            .add_modifier(self.config.inactive_modifier)
    }

    fn inactive_current_style(&self) -> Style {
        Style::from(self.config.inactive_current_fg)
            .bg(self.config.inactive_current_bg)
            .add_modifier(self.config.inactive_current_modifier)
    }

    fn is_current(&self, i: usize) -> bool {
        !self.cursor_disabled && self.cursor == i as u16
    }

    pub fn match_style(&self) -> Style {
        Style::default()
            .fg(self.config.match_fg)
            .add_modifier(self.config.match_modifier)
    }

    fn hr(&self) -> Option<Row<'static>> {
        let sep = self.config.horizontal_separator;

        if matches!(sep, HorizontalSeparator::None) {
            return None;
        }

        let unit = sep.as_str();
        let line = unit.repeat(self.width as usize);

        // todo: support non_stacked properly by doing a seperate rendering pass
        if !self.config.stacked_columns && self.widths.len() > 1 {
            // Some(Row::new(vec![vec![]]))
            Some(Row::new(vec![line; self.widths().len()]))
        } else {
            Some(Row::new(vec![line]))
        }
    }

    fn _hr(&self) -> u16 {
        !matches!(self.config.horizontal_separator, HorizontalSeparator::None) as u16
    }
}

pub struct StatusUI {}

impl StatusUI {
    pub fn parse_template_to_status_line(s: &str) -> Line<'static> {
        let parts = match split_on_nesting(&s, ['{', '}']) {
            Ok(x) => x,
            Err(n) => {
                if n > 0 {
                    log::error!("Encountered {} unclosed parentheses", n)
                } else {
                    log::error!("Extra closing parenthesis at index {}", -n)
                }
                return Line::from(s.to_string());
            }
        };

        let mut spans = Vec::new();
        let mut in_nested = !s.starts_with('{');
        for part in parts {
            in_nested = !in_nested;
            let content = part.as_str();

            if in_nested {
                let inner = &content[1..content.len() - 1];

                // perform replacement fg:content
                spans.push(Self::span_from_template(inner));
            } else {
                spans.push(Span::raw(content.to_string()));
            }
        }

        Line::from(spans)
    }

    /// Converts a template string into a `Span` with colors and modifiers.
    ///
    /// The template string format is:
    /// ```text
    /// "style1,style2,...:text"
    /// ```
    /// - The **first valid color** token is used as foreground (fg).
    /// - The **second valid color** token is used as background (bg).
    /// - Remaining tokens are interpreted as **modifiers**: bold, dim, italic, underlined,
    ///   slow_blink, rapid_blink, reversed, hidden, crossed_out.
    /// - Empty tokens are ignored.
    /// - Unrecognized tokens are collected and logged once at the end.
    ///
    /// # Examples
    ///
    /// ```
    /// use matchmaker::ui::StatusUI;
    /// StatusUI::span_from_template("red,bg=blue,bold,italic:Hello");
    /// StatusUI::span_from_template("green,,underlined:World");
    /// StatusUI::span_from_template(",,dim:OnlyDim");
    /// ```
    ///
    /// Returns a `Span` with the specified styles applied to the text.
    pub fn span_from_template(inner: &str) -> Span<'static> {
        use std::str::FromStr;

        let (style_part, text) = inner.split_once(':').unwrap_or(("", inner));

        let mut style = Style::default();
        let mut fg_set = false;
        let mut bg_set = false;
        let mut unknown_tokens = Vec::new();

        for token in style_part.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }

            if !fg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.fg(color);
                    fg_set = true;
                    continue;
                }
            }

            if !bg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.bg(color);
                    bg_set = true;
                    continue;
                }
            }

            match token.to_lowercase().as_str() {
                "bold" => {
                    style = style.add_modifier(Modifier::BOLD);
                }
                "dim" => {
                    style = style.add_modifier(Modifier::DIM);
                }
                "italic" => {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                "underlined" => {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                "slow_blink" => {
                    style = style.add_modifier(Modifier::SLOW_BLINK);
                }
                "rapid_blink" => {
                    style = style.add_modifier(Modifier::RAPID_BLINK);
                }
                "reversed" => {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                "hidden" => {
                    style = style.add_modifier(Modifier::HIDDEN);
                }
                "crossed_out" => {
                    style = style.add_modifier(Modifier::CROSSED_OUT);
                }
                _ => unknown_tokens.push(token.to_string()),
            };
        }

        if !unknown_tokens.is_empty() {
            log::warn!("Unknown style tokens: {:?}", unknown_tokens);
        }

        Span::styled(text.to_string(), style)
    }
}
