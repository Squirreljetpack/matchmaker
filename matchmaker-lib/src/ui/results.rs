use ratatui::{
    layout::{Alignment, Rect},
    widgets::{Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    SSS, Selection, Selector,
    config::{HorizontalSeparator, ResultsConfig, RowConnectionStyle},
    nucleo::{Status, Worker},
    render::Click,
    utils::{
        string::{allocate_widths, fit_width, substitute_escaped},
        text::{clip_text_lines, prefix_span},
    },
};

#[derive(Debug)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u32,
    pub hscroll: i8,
    pub vscroll: u8,

    /// available height
    height: u16,
    /// available width
    width: u16,
    // column widths.
    // Note that the first width include the indentation.
    widths: Vec<u16>,
    medians: Vec<u16>,

    pub hidden_columns: Vec<bool>,

    pub status: Status,

    pub config: ResultsConfig,

    bottom_clip: Option<u16>,
    cursor_above: u16,

    pub cursor_disabled: bool,
}

impl ResultsUI {
    pub fn new(config: ResultsConfig) -> Self {
        Self {
            cursor: 0,
            bottom: 0,
            hscroll: 0,
            vscroll: 0,

            widths: Vec::new(),
            medians: Vec::new(),
            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            hidden_columns: Default::default(),

            status: Default::default(),
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

    pub fn width(&self) -> u16 {
        self.width
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

    /// Returns whether scroll wrap caused it to jump to the end
    pub fn cursor_prev(&mut self) -> bool {
        self.reset_current_scroll();

        if (self.cursor_above <= self.scroll_padding() || self.cursor <= self.scroll_padding())
            && self.bottom > 0
        {
            self.bottom -= 1;
            self.bottom_clip = None;
        } else if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.config.scroll_wrap {
            log::trace!("d");

            log::trace!(
                "Cursor prev caused jump: above: {} bottom: {}",
                self.cursor_above,
                self.bottom
            );
            self.cursor_jump(self.end());
            return true;
        }

        false
    }

    /// Returns whether scroll wrap caused it to jump to start
    pub fn cursor_next(&mut self) -> bool {
        self.reset_current_scroll();

        if self.cursor_disabled {
            self.cursor_disabled = false
        }

        if self.cursor + 1 + self.scroll_padding() >= self.height
            && self.bottom + (self.height as u32) < self.status.matched_count
        {
            self.bottom += 1;
        } else if self.index() < self.end() {
            self.cursor += 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(0);
            return true;
        }
        false
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.reset_current_scroll();

        self.cursor_disabled = false;
        self.bottom_clip = None;

        let end = self.end();
        let index = index.min(end);

        if index < self.bottom || index >= self.bottom + self.height as u32 {
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
                self.vscroll.saturating_add(x.unsigned_abs())
            } else {
                self.vscroll.saturating_sub(x as u8)
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

    /// Column widths.
    /// Note that the first width doesn't include the indentation.
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }

    /// Adapt the stored widths (initialized by [`Worker::results`]) to the fit within the available width (self.width)
    /// widths <= min_wrap_width don't shrink and aren't wrapped
    pub fn max_widths(&self, _active_column: usize) -> Vec<u16> {
        let mut base_widths = self.medians.clone();

        // uninitialized
        if base_widths.is_empty() || base_widths.iter().all(|x| *x == 0) {
            return vec![];
        }

        for w in base_widths.iter_mut() {
            *w = (*w).max(self.config.min_width);
        }

        base_widths.resize(self.hidden_columns.len().max(base_widths.len()), 0);

        for (i, is_hidden) in self.hidden_columns.iter().enumerate() {
            if *is_hidden {
                base_widths[i] = 0;
            }
        }

        let target = self.content_width();

        let sum: u16 = base_widths.iter().sum();

        if sum < target {
            let nonzero_count = base_widths.iter().filter(|w| **w > 0).count();

            let extra = target - sum;
            let extra_per_column = extra / nonzero_count as u16;
            let mut remainder = extra % nonzero_count as u16;

            for w in base_widths.iter_mut().filter(|w| **w > 0) {
                if *w > 0 {
                    *w += extra_per_column;

                    if remainder > 0 {
                        *w += 1;
                        remainder -= 1;
                    }
                }
            }
        }

        // log::trace!("base_widths: {:?}, target: {target}", base_widths);

        match allocate_widths(&base_widths, target, self.config.min_width) {
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
        let offset = self.bottom;
        let end = self.bottom + self.height as u32;
        let as_cols = !self.config.stacked_columns;

        let width_limits = if as_cols {
            self.max_widths(active_column)
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

        let (mut results, mut widths, medians, status) = worker.results(
            offset,
            end,
            &width_limits,
            self.config.wrap,
            self.config.max_height,
            self.config.match_style.into(),
            matcher,
            self.config.autoscroll,
            self.hscroll,
            (
                if self.config.vscroll_current_only {
                    0
                } else {
                    self.vscroll
                },
                !as_cols,
            ),
            self.config.show_skipped,
        );

        // log::trace!(
        //     "len: {}, hscroll: {},  offset: {}, end: {}, limits: {:?}, medians: {:?}, last_widths: {:?}",
        //     results.len(),
        //     self.hscroll,
        //     offset,
        //     end,
        //     width_limits,
        //     medians,
        //     self.widths
        // );

        self.medians = medians;
        widths[0] += self.indentation() as u16;
        // should generally be true already, but act as a safeguard
        // for x in widths.iter_mut().zip(&self.hidden_columns) {
        //     if *x.1 {
        //         *x.0 = 0
        //     }
        // }
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
                + if as_cols {
                    t.0.iter()
                        .map(|t| t.height() as u16)
                        .max()
                        .unwrap_or_default()
                } else {
                    t.0.iter().map(|t| t.height() as u16).sum::<u16>()
                }
        };

        let style_text = |mut t: ratatui::text::Text<'a>, x: usize, is_current_row: bool| {
            let is_active_col = active_column == x;
            match self.config.row_connection {
                RowConnectionStyle::Disjoint => {
                    if is_active_col {
                        t = t.style(if is_current_row {
                            self.config.current_style
                        } else {
                            self.config.style
                        });
                    } else {
                        t = t.style(if is_current_row {
                            self.config.inactive_current_style
                        } else {
                            self.config.inactive_style
                        });
                    }
                }
                RowConnectionStyle::Capped => {
                    if is_active_col {
                        t = t.style(if is_current_row {
                            self.config.current_style
                        } else {
                            self.config.style
                        });
                    }
                }
                RowConnectionStyle::Full => {}
            }
            t
        };

        let style_row = |mut row: Row<'a>, is_current: bool| {
            if is_current {
                match self.config.row_connection {
                    RowConnectionStyle::Capped => {
                        row = row.style(self.config.inactive_current_style)
                    }
                    RowConnectionStyle::Full => row = row.style(self.config.current_style),
                    _ => {}
                }
            }
            row
        };

        // log::trace!("results initial: {}, {}, {}, {}, {}", self.bottom, self.cursor, total_height, self.height, results.len());
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

        // log::trace!(
        //     "Computed heights: {}, {h_at_cursor}, {h_to_cursor}, {h_after_cursor}, {cursor_end_should_lte}",
        //     self.cursor
        // );

        // begin adjustment
        let mut start_index = 0; // the index in results of the first complete item
        let is_current_row = false;
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
                    if as_cols {
                        if remaining_height < h - self._hr() {
                            for (_, t) in
                                row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0)
                            {
                                clip_text_lines(t, remaining_height, !self.reverse());
                            }
                        }

                        let last_visible = widths
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(i, w)| (*w != 0).then_some(i));

                        let mut row_texts: Vec<_> = row
                            .iter()
                            .take(last_visible.map(|x| x + 1).unwrap_or(0))
                            .cloned()
                            .enumerate()
                            .map(|(x, mut t)| {
                                t = style_text(t, x, is_current_row);
                                if x == 0 {
                                    prefix_span(
                                        &mut t,
                                        prefix.clone(),
                                        self.config.prefix_style,
                                        self.config.prefix_inactive_style,
                                        is_current_row,
                                    );
                                }
                                t
                            })
                            .collect();

                        if self.config.right_align_last && row_texts.len() > 1 {
                            row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                        }

                        let row = Row::new(row_texts).height(remaining_height);
                        rows.push(style_row(row, is_current_row));
                    } else {
                        let mut push = vec![];

                        for (x, col) in row.iter_mut().enumerate().rev() {
                            let mut col = col.clone();
                            let mut height = col.height() as u16;
                            if remaining_height == 0 {
                                break;
                            } else if remaining_height < height {
                                clip_text_lines(&mut col, remaining_height, !self.reverse());
                                height = remaining_height;
                            }
                            remaining_height -= height;

                            prefix_span(
                                &mut col,
                                prefix.clone(),
                                self.config.prefix_style,
                                self.config.prefix_inactive_style,
                                is_current_row,
                            );

                            let col = style_text(col, x, is_current_row);
                            let row = Row::new(vec![col]).height(height);
                            push.push(style_row(row, is_current_row));
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

            if as_cols {
                if self._hr() + remaining_height != h {
                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, remaining_height, !self.reverse());
                    }
                }

                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    .enumerate()
                    .map(|(x, mut t)| {
                        t = style_text(t, x, is_current_row);
                        if x == 0 {
                            prefix_span(
                                &mut t,
                                prefix.clone(),
                                self.config.prefix_style,
                                self.config.prefix_inactive_style,
                                is_current_row,
                            );
                        }
                        t
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                let row = Row::new(row_texts).height(remaining_height);
                rows.push(style_row(row, is_current_row));
            } else {
                let mut push = vec![];

                for (x, col) in row.iter_mut().enumerate().rev() {
                    let mut col = col.clone();
                    let mut height = col.height() as u16;
                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        clip_text_lines(&mut col, remaining_height, !self.reverse());
                        height = remaining_height;
                    }
                    remaining_height -= height;

                    prefix_span(
                        &mut col,
                        prefix.clone(),
                        self.config.prefix_style,
                        self.config.prefix_inactive_style,
                        is_current_row,
                    );

                    let col = style_text(col, x, is_current_row);
                    let row = Row::new(vec![col]).height(height);
                    push.push(style_row(row, is_current_row));
                }
                rows.extend(push.into_iter().rev());
            }
        }

        // topside padding is not self-correcting, and can only do its best to stay at #padding lines without obscuring cursor on cursor movement events.
        let mut remaining_height = self.height.saturating_sub(total_height);

        let mut i = self.bottom_clip.is_some() as usize;

        for (mut row, item) in results.drain(start_index as usize..) {
            // note that the index changes *next* frame
            if let Click::ResultPos(c) = click {
                let c = if self.reverse() {
                    self.height.saturating_sub(*c).saturating_sub(1)
                } else {
                    *c
                };
                if self.height - remaining_height > c {
                    let idx = self.bottom + i as u32 - 1;
                    log::debug!(
                        "Mapped click position to index: {c} -> {idx} with remaining {remaining_height}",
                    );
                    *click = Click::ResultIdx(idx);
                }
            }

            if self.is_current(i) {
                self.cursor_above = self.height - remaining_height;
            }

            // insert hr
            if let Some(hr) = self.hr()
                && remaining_height > 0
            {
                rows.push(hr);
                remaining_height -= self._hr();
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

            if as_cols {
                // scroll down
                if self.is_current(i) && self.config.vscroll_current_only && self.vscroll > 0 {
                    for (x, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        if active_column == x {
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
                        t = style_text(t, x, self.is_current(i));

                        // prefix after hscroll
                        if x == 0 {
                            prefix_span(
                                &mut t,
                                prefix.clone(),
                                self.config.prefix_style,
                                self.config.prefix_inactive_style,
                                self.is_current(i),
                            );
                        };
                        t
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                // push
                let row = style_row(Row::new(row_texts).height(height), self.is_current(i));
                rows.push(row);
            } else {
                let mut push = vec![];
                let mut vscroll_to_skip = if self.is_current(i) && self.config.vscroll_current_only
                {
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

                    let is_current_row = self.is_current(i);
                    prefix_span(
                        &mut col,
                        prefix.clone(),
                        self.config.prefix_style,
                        self.config.prefix_inactive_style,
                        is_current_row,
                    );

                    let col = style_text(col, x, is_current_row);
                    let row = Row::new(vec![col]).height(height);
                    push.push(style_row(row, is_current_row));
                }
                rows.extend(push);
            }
            i += 1;
        }

        // doesn't loop back after results is exhausted so we have to set here
        if let Click::ResultPos(_c) = click {
            log::debug!("Mapped click to last row = {i}");
            *click = Click::ResultIdx(self.bottom + i as u32 - 1);
        }

        if self.reverse() {
            rows.reverse();
            if remaining_height > 0 {
                rows.insert(0, Row::new(vec![vec![]]).height(remaining_height));
            }
        }

        // ratatui column_spacing eats into the constraints
        let table_widths = if as_cols {
            // first 0 element after which all is 0
            let pos = widths.iter().rposition(|&x| x != 0);
            // column_spacing eats into the width
            let mut widths: Vec<_> = widths[..pos.map_or(0, |x| x + 1)].to_vec();

            let surplus = self.content_width().saturating_sub(widths.iter().sum());

            if surplus > 0 {
                // until we finish rendering v2, trigger this to keep column widths usable
                if let Some(s) = widths.last_mut() {
                    *s += surplus;
                }
                // occupy full row
                // if matches!(self.config.row_connection, RowConnectionStyle::Full)
                //     || (matches!(self.config.row_connection, RowConnectionStyle::Disjoint)
                //         && self.config.right_align_last)
                // {
                //     if let Some(s) = widths.last_mut() {
                //         *s += surplus;
                //     }
                // }
            }

            // save actual widths of each column
            self.widths = widths.clone();

            widths
        } else {
            vec![self.width]
        };

        // log::trace!(
        //     "limits: {width_limits:?}, widths: {widths:?}, {:?}, medians {:?}",
        //     self.width,
        //     self.medians
        // );

        let mut table = Table::new(rows, table_widths).column_spacing(self.config.column_spacing.0);

        table = match self.config.row_connection {
            RowConnectionStyle::Full => table.style(self.config.style),
            RowConnectionStyle::Capped => table.style(self.config.inactive_style),
            _ => table,
        };

        // log::trace!("{table:?}");

        table = table.block(self.config.border.as_static_block());
        table
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

    fn is_current(&self, i: usize) -> bool {
        !self.cursor_disabled && self.cursor == i as u16
    }

    fn hr(&self) -> Option<Row<'static>> {
        let sep = self.config.separator;

        if matches!(sep, HorizontalSeparator::None) {
            return None;
        }

        let unit = sep.as_str();
        let line = unit.repeat(self.width as usize);

        // todo: support non_stacked properly by doing a seperate rendering pass
        if !self.config.stacked_columns && self.widths.len() > 1 {
            // Some(Row::new(vec![vec![]]))
            Some(Row::new(vec![line; self.widths().len()]).style(self.config.separator_style))
        } else {
            Some(Row::new(vec![line]).style(self.config.separator_style))
        }
    }

    fn _hr(&self) -> u16 {
        !matches!(self.config.separator, HorizontalSeparator::None) as u16
    }
}
