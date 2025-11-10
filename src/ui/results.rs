use ratatui::{
    layout::Rect,
    style::Stylize,
    widgets::{Paragraph, Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    PickerItem, Selection, SelectionSet,
    config::ResultsConfig,
    nucleo::worker::{Status, Worker},
    utils::text::{clip_text_lines, fit_width, prefix_text, substitute_escaped},
};

// todo: possible to store rows in here?
#[derive(Debug, Clone)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u16,
    height: u16,      // actual height
    widths: Vec<u16>, // not sure how to support it yet
    col: Option<usize>,
    pub status: Status,
    pub config: ResultsConfig,
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
            config,
        }
    }

    pub fn col(&self) -> Option<usize> {
        self.col.clone()
    }

    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }

    // todo: support cooler things like only showing/outputting a specific column/cycling columns
    pub fn toggle_col(&mut self, col_idx: usize) -> bool {
        if self.col.map_or(false, |x| x == col_idx) {
            self.col = None
        } else {
            self.col = Some(col_idx);
            // if col_idx < self.widths.len() {
            //     self.col = Some(col_idx)
            // } else {
            //     warn!("Tried to set col = {col_idx} but widths = {}, ignoring", self.widths.len())
            // }
        }
        self.col.is_some()
    }

    pub fn reverse(&self) -> bool {
        self.config.reverse.unwrap()
    }

    fn scroll_padding(&self) -> u16 {
        self.config.scroll_padding.min(self.height / 2)
    }

    // as given by ratatui area
    pub fn update_dimensions(&mut self, area: &Rect) {
        let mut height = area.height;
        height -= self.config.border.height();
        self.height = height;
    }

    pub fn cursor_prev(&mut self) -> bool {
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
        let end = self.end();
        let index = index.min(end) as u16;

        if index < self.bottom || index >= self.bottom + self.height {
            self.bottom = (end as u16 + 1).saturating_sub(self.height).min(index);
            self.cursor = index - self.bottom;
        } else {
            self.cursor = index - self.bottom;
        }
    }

    pub fn end(&self) -> u32 {
        self.status.matched_count.saturating_sub(1)
    }

    pub fn index(&self) -> u32 {
        (self.cursor + self.bottom) as u32
    }

    // this updates the internal status, so be sure to call make_status afterward
    pub fn make_table<'a, T: PickerItem, C: 'a>(
        &'a mut self,
        worker: &'a mut Worker<T, C>,
        selections: &mut SelectionSet<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
    ) -> Table<'a> {
        let offset = self.bottom as u32;
        let end = (self.bottom + self.height) as u32;

        let (results, mut widths, status) = worker.results(offset, end, matcher);

        if status.matched_count < (self.bottom + self.cursor) as u32 {
            self.cursor_jump(status.matched_count);
        }

        self.status = status;

        widths[0] += self.config.multi_prefix.width() as u16;

        let mut rows = vec![];
        let mut total_height = 0;

        for (i, (mut row, item, height)) in results.into_iter().enumerate() {
            total_height += height;
            if total_height > self.height {
                clip_text_lines(&mut row[0], self.height - total_height, self.reverse());
                total_height = self.height;
            }

            let prefix = if selections.contains(item) {
                self.config.multi_prefix.clone()
            } else {
                fit_width(
                    &substitute_escaped(
                        &self.config.default_prefix,
                        &[('d', &i.to_string()), ('r', &self.index().to_string())],
                    ),
                    self.config.multi_prefix.width(),
                )
            };

            prefix_text(&mut row[0], prefix);

            if i as u16 == self.cursor {
                row = row
                    .into_iter()
                    .enumerate()
                    .map(|(i, t)| {
                        if self.col.map_or(true, |a| i == a) {
                            t.style(self.config.current_fg)
                                .bg(self.config.current_bg)
                                .add_modifier(self.config.current_modifier)
                        } else {
                            t
                        }
                    })
                    .collect();
            }

            let row = Row::new(row);
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

        let mut table = Table::new(rows, self.widths.clone()).column_spacing(self.config.column_spacing.0);

        table = table.block(self.config.border.as_block());
        table
    }

    pub fn make_status(&self) -> Paragraph<'_> {
        let input = Paragraph::new(format!(
            "  {}/{}",
            &self.status.matched_count, &self.status.item_count
        ))
        .style(self.config.count_fg)
        .add_modifier(self.config.count_modifier);

        input
    }
}
