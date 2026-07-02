use ratatui::{
    layout::Rect,
    text::Text,
    widgets::{Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    collections::HiddenColumns,
    config::{HorizontalSeparator, ResultsConfig, RowConnectionStyle},
    nucleo::{Column, Status},
    utils::{
        string::{fit_width, substitute_escaped},
        text::{debug_row, prefix_span},
    },
};

mod render;
mod update;
mod widths;

#[derive(Debug)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u32,
    pub hscroll: i8,
    pub vscroll: u8,
    pub cursor_disabled: bool,
    cursor_moved: Option<bool>,

    /// available height
    height: u16,
    /// available width
    width: u16,
    // actual column widths.
    // Note that the first width include the indentation.
    widths: Vec<u16>,
    width_limits: Vec<u16>,
    pub(crate) hidden_columns: HiddenColumns,
    column_name_widths: Vec<u16>,

    // used to compute width_limits
    // valid after calling update_preferred_widths
    preferred_widths: Vec<u16>,
    // transient buffer for use within compute functions
    widths_buffer: Vec<u16>,
    col_indices_buffer: Vec<u32>,
    // for caching
    matched_count: u32,

    pub config: ResultsConfig,
    pub status: Status,

    row_cache: [Vec<(u32, Vec<Text<'static>>, Vec<u16>)>; 2],
    /// Visual-order row metadata from the most recent successful build.
    /// Each entry is `(item_idx, height)`; `u32::MAX` marks separator rows.
    /// Kept around so click positions can be mapped back to absolute
    /// indices after the table has been assembled.
    row_data: Vec<(u32, u16)>,
    pub table: Table<'static>,
}

impl ResultsUI {
    pub fn new<T, D>(config: ResultsConfig, cols: &[Column<T, D>]) -> Self {
        let mut ret = Self {
            cursor: 0,
            bottom: 0,
            hscroll: 0,
            vscroll: 0,

            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            widths: Vec::new(),
            hidden_columns: Default::default(),
            column_name_widths: Default::default(),

            width_limits: Vec::new(),
            preferred_widths: Vec::new(),
            widths_buffer: Vec::new(),
            col_indices_buffer: Vec::new(),
            matched_count: 0,

            status: Default::default(),
            config,

            cursor_disabled: false,
            cursor_moved: None,
            row_cache: [Vec::new(), Vec::new()],
            row_data: Vec::new(),
            table: ratatui::widgets::Table::default(),
        };
        ret.init(cols);
        ret
    }

    pub fn init<T, D>(&mut self, cols: &[Column<T, D>]) {
        // self.preferred_widths.resize(n_cols, 0);
        // self.width_limits.resize(n_cols, 0);
        self.hidden_columns = HiddenColumns::new_with_size(cols.len());
        self.column_name_widths = cols
            .into_iter()
            .zip(self.hidden_columns.mask())
            .filter_map(|(col, &flag)| {
                if !flag {
                    Some(col.name.len() as u16)
                } else {
                    None
                }
            })
            .collect();
    }

    pub fn hidden_cols(&self) -> &HiddenColumns {
        &self.hidden_columns
    }

    pub fn set_hidden_columns(&mut self, hidden_columns: impl IntoIterator<Item = usize>) {
        for i in hidden_columns {
            self.hidden_columns.push(i);
        }
    }

    pub fn update_dimensions(&mut self, area: &Rect) {
        let bw = self.config.border.width();
        let bh = self.config.border.height();
        let new_width = area.width.saturating_sub(bw);
        let new_height = area.height.saturating_sub(bh);
        if self.width != new_width || self.height != new_height {
            self.width = new_width;
            self.height = new_height;
            log::debug!("Updated results dimensions: {}x{}", self.width, self.height);
        }
        self.recompute_widths();
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
        if self.config.wrap != wrap {
            self.config.wrap = wrap;
            self.set_dirty();
        }
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

    /// Map a visual y-offset (in the rendered results table) back to the
    /// absolute nucleo item index, or `None` if `y` falls outside the
    /// populated range (e.g. in a padding/spacer row).
    pub fn get_index_of_row(&self, y: u16) -> Option<u32> {
        let target = if self.reverse() {
            self.height.saturating_sub(y).saturating_sub(1)
        } else {
            y
        };
        let mut acc: u16 = 0;
        for &(idx, h) in &self.row_data {
            if idx != u32::MAX && acc <= target && target < acc + h {
                return Some(idx);
            }
            acc += h;
        }
        None
    }

    fn set_cursor_changed(&mut self, jumped: bool) {
        if jumped || self.cursor_moved != Some(true) {
            self.cursor_moved = Some(jumped);
        }
    }

    pub fn cursor_prev(&mut self) -> bool {
        self.cursor_disabled = false;
        self.reset_current_scroll();
        self.set_cursor_changed(false);

        if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.bottom > 0 {
            self.bottom -= 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(self.end());
            return true;
        }

        false
    }

    pub fn cursor_next(&mut self) -> bool {
        self.reset_current_scroll();
        self.cursor_disabled = false;
        self.set_cursor_changed(false);

        if self.index() < self.end() {
            self.cursor = self.cursor.saturating_add(1);
        } else if self.config.scroll_wrap {
            self.cursor_jump(0);
            return true;
        }

        false
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.reset_current_scroll();
        self.cursor_disabled = false;
        self.set_cursor_changed(true);

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
            self.set_dirty();
        } else {
            self.vscroll = if x == 0 {
                0
            } else if x.is_negative() {
                self.vscroll.saturating_add(x.unsigned_abs())
            } else {
                self.vscroll.saturating_sub(x as u8)
            };

            self.set_dirty();

            // if !self.config.vscroll_current_only {
            //     self.set_dirty();
            // } else {
            //     let cursor_idx = self.bottom + self.cursor as u32;
            //     self.row_cache[0].retain(|(i, _, _)| *i != cursor_idx);
            //     self.row_cache[1].retain(|(i, _, _)| *i != cursor_idx);
            // }
        }
    }

    pub fn reset_current_scroll(&mut self) {
        if self.hscroll != 0 || self.vscroll != 0 {
            self.hscroll = 0;
            self.vscroll = 0;
        }
    }

    // ------- RENDERING GET/SET ------------
    pub fn indentation(&self) -> usize {
        self.config.multi_prefix.width()
    }

    /// Table column widths.
    /// Note that the indices don't correspond directly to the order of worker.columns as zero-width columns are skipped.
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }
    pub fn width_limits(&self) -> &Vec<u16> {
        &self.width_limits
    }

    pub fn available_width(&self) -> u16 {
        self.width
            .saturating_sub(self.indentation() as u16)
            .saturating_sub(self.column_spacing_width())
    }

    pub fn column_spacing_width(&self) -> u16 {
        let pos = self.widths.iter().rposition(|&x| x != 0);
        self.config.column_spacing.0 * (pos.unwrap_or_default() as u16)
    }

    pub fn table_width(&self) -> u16 {
        self.config.border.width()
            + if self.config.stacked_columns {
                self.width
            } else {
                self.widths.iter().sum::<u16>() + self.column_spacing_width()
            }
    }

    pub fn set_dirty(&mut self) {
        #[cfg(debug_assertions)]
        log::trace!("cache cleared");
        self.row_cache[0].clear();
        // self.row_cache[1].clear();
    }

    // ------- RENDERING ----------
    /// Call [`ResultsUI::update_table`] first
    pub fn get_table(&self) -> (&Table<'static>, u16) {
        (&self.table, self.table_width())
    }
}
