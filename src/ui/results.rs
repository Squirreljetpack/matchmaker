#[allow(unused)]
use log::debug;

use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    widgets::{Paragraph, Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    SSS, Selection, Selector,
    config::ResultsConfig,
    nucleo::{Status, Worker},
    utils::text::{clip_text_lines, fit_width, prefix_text, substitute_escaped},
};

// todo: possible to store rows in here?
#[derive(Debug, Clone)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u16,
    height: u16,      // actual height
    width: u16,
    widths: Vec<u16>, // not sure how to support it yet
    col: Option<usize>,
    pub status: Status,
    pub config: ResultsConfig,

    pub cursor_disabled: bool,
}

impl ResultsUI {
    pub fn new(config: ResultsConfig) -> Self {
        Self {
            cursor: 0,
            bottom: 0,
            col: None,
            widths: Vec::new(),
            status: Default::default(),
            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            config,
            cursor_disabled: false
        }
    }
    // as given by ratatui area
    pub fn update_dimensions(&mut self, area: &Rect) {
        let border = self.config.border.height();
        self.width = area.width.saturating_sub(border);
        self.height = area.height.saturating_sub(border);
    }

    // ------ config -------
    pub fn reverse(&self) -> bool {
        self.config.reverse.unwrap()
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
        if self.col == Some(col_idx) {
            self.col = None
        } else {
            self.col = Some(col_idx);
        }
        self.col.is_some()
    }
    pub fn cycle_col(&mut self) {
        self.col = match self.col {
            None => {
                if !self.widths.is_empty() { Some(0) } else { None }
            }
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
    pub fn index(&self) -> u32 {
        if self.cursor_disabled {
            u32::MAX
        } else {
            (self.cursor + self.bottom) as u32
        }
    }
    // pub fn cursor(&self) -> Option<u16> {
    //     if self.cursor_disabled {
    //         None
    //     } else {
    //         Some(self.cursor)
    //     }
    // }
    pub fn cursor_prev(&mut self) -> bool {
        if self.cursor_disabled {
            return false
        }

        if self.cursor <= self.scroll_padding() && self.bottom > 0 {
            self.bottom -= 1;
        } else if self.cursor > 0 {
            self.cursor -= 1;
            return self.cursor == 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(self.end());
        }
        false
    }
    pub fn cursor_next(&mut self) -> bool {
        if self.cursor_disabled {
            self.cursor_disabled = false
        }

        if self.cursor + 1 + self.scroll_padding() >= self.height
        && self.bottom + self.height < self.status.matched_count as u16
        {
            self.bottom += 1;
        } else if self.index() < self.end() {
            self.cursor += 1;
            if self.index() == self.end() {
                return true;
            }
        } else if self.config.scroll_wrap {
            self.cursor_jump(0)
        }
        false
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.cursor_disabled = false;

        let end = self.end();
        let index = index.min(end) as u16;

        if index < self.bottom || index >= self.bottom + self.height {
            self.bottom = (end as u16 + 1).saturating_sub(self.height).min(index);
            self.cursor = index - self.bottom;
        } else {
            self.cursor = index - self.bottom;
        }
    }

    // ------- RENDERING ----------
    pub fn indentation(&self) -> usize {
        self.config.multi_prefix.width()
    }
    pub fn col(&self) -> Option<usize> {
        self.col
    }
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }
    pub fn width(&self) -> u16 {
        self.width.saturating_sub(self.indentation() as u16)
    }
    pub fn match_style(&self) -> Style {
        Style::default()
        .fg(self.config.match_fg)
        .add_modifier(self.config.match_modifier)
    }

    pub fn max_widths(&self) -> Vec<u16> {
        if ! self.config.wrap {
            return vec![];
        }

        let mut widths = vec![u16::MAX; self.widths.len()];

        let total: u16 = self.widths.iter().sum();
        if total <= self.width() {
            return vec![];
        }

        let mut available = self.width();
        let mut scale_total = 0;
        let mut scalable_indices = Vec::new();

        for (i, &w) in self.widths.iter().enumerate() {
            if w <= 5 {
                available = available.saturating_sub(w);
            } else {
                scale_total += w;
                scalable_indices.push(i);
            }
        }

        for &i in &scalable_indices {
            let old = self.widths[i];
            let new_w = old * available / scale_total;
            widths[i] = new_w.max(5);
        }

        // give remainder to the last scalable column
        if let Some(&last_idx) = scalable_indices.last() {
            let used_total: u16 = widths.iter().sum();
            if used_total < self.width() {
                widths[last_idx] += self.width() - used_total;
            }
        }

        widths
    }

    // this updates the internal status, so be sure to call make_status afterward
    // some janky wrapping is implemented, dunno whats causing flickering, padding is fixed going down only
    pub fn make_table<'a, T: SSS>(
        &'a mut self,
        worker: &'a mut Worker<T>,
        selections: &mut Selector<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
    ) -> Table<'a> {
        let offset = self.bottom as u32;
        let end = (self.bottom + self.height) as u32;

        let (mut results, mut widths, status) = worker.results(offset, end, &self.max_widths(), self.match_style(), matcher);

        let match_count = status.matched_count;

        self.status = status;
        if match_count < (self.bottom + self.cursor) as u32 && !self.cursor_disabled {
            self.cursor_jump(match_count);
        } else {
            self.cursor = self.cursor.min(results.len().saturating_sub(1) as u16)
        }

        widths[0] += self.indentation() as u16;


        let mut rows = vec![];
        let mut total_height = 0;

        if results.is_empty() {
            return Table::new(rows, widths)
        }

        // debug!("sb: {}, {}, {}, {}, {}", self.bottom, self.cursor, total_height, self.height, results.len());
        let cursor_result_h = results[self.cursor as usize].2;
        let mut start_index = 0;

        let cursor_should_above = self.height - self.scroll_padding();

        if cursor_result_h >= cursor_should_above {
            start_index = self.cursor;
            self.bottom += self.cursor;
            self.cursor = 0;
        } else if let cursor_cum_h = results[0..=self.cursor as usize].iter().map(|(_, _, height)| height).sum::<u16>() && cursor_cum_h > cursor_should_above && self.bottom + self.height < self.status.matched_count as u16 {
            start_index = 1;
            let mut height = cursor_cum_h - cursor_should_above;
            for (row, item, h) in results[..self.cursor as usize].iter_mut() {
                let h = *h;

                if height < h {
                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _) | widths[*i] != 0 ) {
                        clip_text_lines(t, height, !self.reverse());
                    }
                    total_height += height;

                    let prefix = if selections.contains(item) {
                        self.config.multi_prefix.clone().to_string()
                    } else {
                        fit_width(
                            &substitute_escaped(
                                &self.config.default_prefix,
                                &[('d', &(start_index - 1).to_string()), ('r', &self.index().to_string())],
                            ),
                            self.indentation(),
                        )
                    };

                    prefix_text(&mut row[0], prefix);

                    let row = Row::from_iter(row.clone().into_iter().enumerate().filter_map(|(i, v) | (widths[i] != 0).then_some(v) )).height(height);
                    // debug!("1: {} {:?} {}", start_index, row, h_exceedance);

                    rows.push(row);

                    self.bottom += start_index - 1;
                    self.cursor -= start_index - 1;
                    break
                } else if height == h {
                    self.bottom += start_index;
                    self.cursor -= start_index;
                    // debug!("2: {} {}", start_index, h);
                    break
                }

                start_index += 1;
                height -= h;
            }

        }

        // debug!("si: {start_index}, {}, {}, {}", self.bottom, self.cursor, total_height);

        for (i, (mut row, item, mut height)) in (start_index..).zip(results.drain(start_index as usize..)) {
            if self.height - total_height == 0 {
                break
            } else if self.height - total_height < height {
                height = self.height - total_height;

                for (_, t) in row.iter_mut().enumerate().filter(|(i, _) | widths[*i] != 0 ) {
                    clip_text_lines(t, height, self.reverse());
                }
                total_height = self.height;
            } else {
                total_height += height;
            }

            let prefix = if selections.contains(item) {
                self.config.multi_prefix.clone().to_string()
            } else {
                fit_width(
                    &substitute_escaped(
                        &self.config.default_prefix,
                        &[('d', &i.to_string()), ('r', &self.index().to_string())],
                    ),
                    self.indentation(),
                )
            };

            prefix_text(&mut row[0], prefix);

            if !self.cursor_disabled && i == self.cursor {
                row = row
                .into_iter()
                .enumerate()
                .map(|(i, t)| {
                    if self.col.is_none_or(|a| i == a) {
                        t.style(self.config.current_fg)
                        .bg(self.config.current_bg)
                        .add_modifier(self.config.current_modifier)
                    } else {
                        t
                    }
                })
                .collect();
            }

            let row = Row::from_iter(row.into_iter().enumerate().filter_map(|(i, v) | (widths[i] != 0).then_some(v) )).height(height);

            rows.push(row);
        }


        if self.reverse() {
            rows.reverse();
            if total_height < self.height {
                let spacer_height = self.height - total_height;
                rows.insert(0, Row::new(vec![vec![]]).height(spacer_height));
            }
        }

        self.widths = {
            let pos = widths.iter().rposition(|&x| x != 0).map_or(0, |p| p + 1);
            widths[..pos].to_vec()
        };


        let mut table = Table::new(rows, self.widths.clone()).column_spacing(self.config.column_spacing.0)
        .style(self.config.fg)
        .add_modifier(self.config.modifier);

        table = table.block(self.config.border.as_block());
        table
    }

    pub fn make_status(&self) -> Paragraph<'_> {
        Paragraph::new(format!(
            "  {}/{}",
            &self.status.matched_count, &self.status.item_count
        ))
        .style(self.config.count_fg)
        .add_modifier(self.config.count_modifier)
    }
}

