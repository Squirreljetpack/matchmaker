use cba::bait::TransformExt;
use ratatui::{
    layout::Constraint,
    text::{Line, Text},
    widgets::{Cell, Paragraph, Row, Table},
};

use crate::{
    config::{DisplayConfig, RowConnectionStyle},
    utils::{
        serde::StringOrVec,
        text::{wrap_line, wrap_text, wrapping_indicator},
    },
};
pub type HeaderTable = Vec<Vec<Line<'static>>>;

/// Left Indentation + Border, column spacing, widths
pub type ResultWidths = (u16, u16, Vec<u16>);

#[derive(Debug, Default)]
pub struct DisplayUI {
    width: u16,
    heights: Vec<u16>,
    text: Vec<Text<'static>>,
    lines: HeaderTable, // lines from input
    pub show: bool,
    pub config: DisplayConfig,
    dirty: bool,
    table: Vec<Vec<Text<'static>>>,
    cached_result_widths: ResultWidths,
}

impl DisplayUI {
    pub fn new(config: DisplayConfig) -> Self {
        let mut ret = Self {
            config,
            ..Default::default()
        };
        ret.init();

        ret
    }

    /// Refresh content and interactions from config.
    pub fn init(&mut self) {
        let text = match &self.config.content {
            Some(StringOrVec::String(s)) => {
                vec![Text::from(s.clone())]
            }
            Some(StringOrVec::Vec(s)) => s.iter().map(|s| Text::from(s.clone())).collect(),
            _ => vec![],
        };

        for line in &mut self.config.interactions {
            line.sort_by_key(|(i, _)| *i);
        }

        self.text = text;
        self.show = self.config.content.is_some() || self.config.header_lines > 0;
        self.dirty = true;
        self.update_table();
    }

    pub fn update_width(&mut self, width: u16) {
        let border_w = self.config.border.width();
        let new_w = width.saturating_sub(border_w);
        if self.width != new_w {
            self.width = new_w;
            self.dirty = true;
            self.update_table();
        }
    }

    pub fn wrap(&mut self, wrap: bool) {
        if self.config.wrap != wrap {
            self.config.wrap = wrap;
            self.dirty = true;
            self.update_table();
        }
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }

    pub fn height(&self) -> u16 {
        if !self.show {
            return 0;
        }
        let height: u16 = self.heights.iter().sum();
        height + self.config.border.height()
    }

    pub fn update_table(&mut self) {
        if self.text.is_empty() && self.lines.is_empty() {
            self.heights.clear();
            self.table.clear();
            return;
        }

        let (_result_indentation, _col_spacing, ref widths) = self.cached_result_widths;
        let use_wrap = self.config.wrap;

        let mut table_rows = Vec::new();
        let mut heights = Vec::new();

        if !self.text.is_empty() {
            if self.is_single_column() {
                let (text, _) =
                    wrap_text(self.text[0].clone(), if use_wrap { self.width } else { 0 });
                let row_height = text.height() as u16;
                table_rows.push(vec![text]);
                heights.push(row_height);
            } else {
                let mut row_height = 0;
                let mut row_cells = Vec::with_capacity(self.text.len());
                for (i, text) in self.text.iter().enumerate() {
                    let w = if use_wrap {
                        widths.get(i).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    let (wrapped, _) = wrap_text(text.clone(), w);
                    let is_visible = widths.is_empty() || widths.get(i).copied().unwrap_or(0) > 0;
                    if is_visible {
                        row_height = row_height.max(wrapped.height() as u16);
                    }
                    row_cells.push(wrapped);
                }
                table_rows.push(row_cells);
                heights.push(row_height.max(1));
            }
        }

        if !self.lines.is_empty() {
            for row in &self.lines {
                let mut row_height = 1;
                let mut row_cells = Vec::with_capacity(row.len());
                for (i, l) in row.iter().enumerate() {
                    let w = if use_wrap {
                        widths.get(i).copied().unwrap_or(0)
                    } else {
                        0
                    };
                    let wrapped = wrap_line(l.clone(), w, &wrapping_indicator());
                    let is_visible = widths.is_empty() || widths.get(i).copied().unwrap_or(0) > 0;
                    if is_visible {
                        row_height = row_height.max(wrapped.len() as u16);
                    }
                    row_cells.push(Text::from(wrapped));
                }
                table_rows.push(row_cells);
                heights.push(row_height);
            }
        }

        self.table = table_rows;
        self.heights = heights;
    }

    /// Set text (single column) and show. The base style is applied "under" the text's styling.
    pub fn set(&mut self, text: impl Into<Text<'static>>) {
        self.text = vec![text.into()];

        self.show = true;
        self.dirty = true;
        self.update_table();
    }

    /// Add a column and show.
    pub fn push(&mut self, text: impl Into<Text<'static>>) {
        self.text.push(text.into());

        self.show = true;
        self.dirty = true;
        self.update_table();
    }

    pub fn clear(&mut self, keep_header: bool) {
        if !keep_header {
            self.lines.clear();
            self.show = false;
        } else if self.lines.is_empty() {
            self.show = false;
        }

        self.text.clear();
        self.dirty = true;
        self.update_table();
    }

    /// Whether this is table has just one column
    pub fn is_single_column(&self) -> bool {
        self.text.len() == 1
    }

    pub fn header_table(&mut self, table: HeaderTable) {
        self.lines = table;
        self.show = true;
        self.dirty = true;
        self.update_table();
    }

    pub fn make_display(
        &mut self,
        result_widths: ResultWidths,
    ) -> (Table<'static>, Option<Paragraph<'static>>) {
        if self.dirty || self.cached_result_widths != result_widths {
            self.cached_result_widths = result_widths;
            self.dirty = false;
            self.update_table();
        }

        let (result_indentation, col_spacing, ref widths) = self.cached_result_widths;

        if self.text.is_empty() && self.lines.is_empty() {
            return (Table::default(), None);
        }

        let block = {
            let b = self.config.border.as_static_block();
            if self.config.match_indent {
                let mut padding = self.config.border.padding;

                padding.left = result_indentation.saturating_sub(self.config.border.left());
                b.padding(padding.0)
            } else {
                b
            }
        };

        let mut rows = Vec::with_capacity(self.table.len());
        for (r, row_cells) in self.table.iter().enumerate() {
            let row_h = self.heights.get(r).copied().unwrap_or(1);
            let is_content_row = r == 0 && !self.text.is_empty();

            if is_content_row && self.is_single_column() {
                let cells = vec![Cell::from(row_cells[0].clone())];
                let row = Row::new(cells).style(self.config.style).height(row_h);
                rows.push(row);
            } else {
                let cells: Vec<Cell> = row_cells
                    .iter()
                    .enumerate()
                    .filter_map(|(i, text)| {
                        if widths.get(i).is_none_or(|x| *x == 0) {
                            return None;
                        }

                        let cell_text = text.clone().transform_if(
                            matches!(self.config.row_connection, RowConnectionStyle::Disjoint),
                            |t| t.style(self.config.style),
                        );

                        Some(Cell::from(cell_text))
                    })
                    .collect();

                let mut row = Row::new(cells).height(row_h);
                if is_content_row {
                    row = row.style(self.config.style);
                } else if matches!(self.config.row_connection, RowConnectionStyle::Disjoint) {
                    row = row.style(self.config.style);
                }
                rows.push(row);
            }
        }

        let col_widths = if self.is_single_column() && self.lines.iter().all(|x| x.len() == 1) {
            vec![Constraint::Percentage(100)]
        } else {
            widths
                .iter()
                .filter_map(|&x| (x > 0).then_some(Constraint::Length(x)))
                .collect()
        };

        let table = Table::new(rows, col_widths)
            .block(block)
            .column_spacing(col_spacing)
            .transform_if(
                !matches!(self.config.row_connection, RowConnectionStyle::Disjoint),
                |t| t.style(self.config.style),
            );

        let paragraph = if self.is_single_column() && !self.text.is_empty() {
            Some(self.make_full_width_row(result_indentation))
        } else {
            None
        };

        (table, paragraph)
    }

    /// Draw in the same area as display when self.single() to produce a full width row over the table area
    pub fn make_full_width_row(&self, result_indentation: u16) -> Paragraph<'static> {
        // Compute padding
        let left = if self.config.match_indent {
            result_indentation.saturating_sub(self.config.border.left())
        } else {
            self.config.border.left()
        };
        let top = self.config.border.top();
        let right = self.config.border.width().saturating_sub(left);
        let bottom = self.config.border.height() - top;

        let block = ratatui::widgets::Block::default().padding(ratatui::widgets::Padding {
            left,
            top,
            right,
            bottom,
        });

        Paragraph::new(self.text[0].clone())
            .block(block)
            .style(self.config.style)
    }

    /// Resolves a relative (x, y) position to an interactive action if one was defined.
    #[allow(non_snake_case)]
    pub fn get_interaction(&self, x: u16, y: u16) -> Option<String> {
        if !self.show || self.config.interactions.is_empty() || self.heights.is_empty() {
            return None;
        }

        let border_top = self.config.border.top();
        if y < border_top {
            return None;
        }
        let content_y = y - border_top;

        let mut remaining_y = content_y;
        let mut target_row = None;
        let mut surplus_y = 0;

        for (r, &row_h) in self.heights.iter().enumerate() {
            if remaining_y < row_h {
                target_row = Some(r);
                surplus_y = remaining_y;
                break;
            }
            remaining_y -= row_h;
        }

        let Y = match target_row {
            Some(r) => r,
            None => return None,
        };

        let setting = match self.config.interactions.get(Y) {
            Some(s) if !s.is_empty() => s,
            _ => return None,
        };

        let (result_indentation, col_spacing, ref widths) = self.cached_result_widths;
        let left_offset = if self.config.match_indent {
            (self
                .config
                .border
                .sides()
                .contains(ratatui::widgets::Borders::LEFT) as u16)
                + result_indentation.saturating_sub(self.config.border.left())
        } else {
            self.config.border.left()
        };

        if x < left_offset {
            return None;
        }
        let content_x = x - left_offset;

        let is_content_row = Y == 0 && !self.text.is_empty();
        let X = if is_content_row && self.is_single_column() {
            let col_w = if self.config.wrap && self.width > 0 {
                self.width
            } else {
                0
            };
            if col_w > 0 {
                surplus_y * col_w + content_x
            } else {
                content_x
            }
        } else {
            let num_cols = self.table.get(Y).map(|r| r.len()).unwrap_or(0);
            if num_cols == 0 {
                return None;
            }

            let mut cur_screen_x = 0;
            let mut raw_width_sum = 0;
            let mut target_col = None;
            let mut target_surplus_x = 0;
            let mut target_col_w = 0;
            let mut is_first_visible = true;

            for i in 0..num_cols {
                let col_w = widths.get(i).copied().unwrap_or(0);
                let raw_w = if is_content_row {
                    self.text
                        .get(i)
                        .map(|t| t.lines.iter().map(|l| l.width() as u16).sum::<u16>())
                        .unwrap_or(0)
                } else {
                    let line_idx = if self.text.is_empty() { Y } else { Y - 1 };
                    self.lines
                        .get(line_idx)
                        .and_then(|row| row.get(i))
                        .map(|l| l.width() as u16)
                        .unwrap_or(0)
                };

                if col_w == 0 {
                    raw_width_sum += raw_w;
                    continue;
                }

                if !is_first_visible {
                    cur_screen_x += col_spacing;
                }
                is_first_visible = false;

                let col_start = cur_screen_x;
                let col_end = col_start + col_w;

                if content_x < col_end {
                    target_col = Some(i);
                    target_surplus_x = content_x.saturating_sub(col_start);
                    target_col_w = col_w;
                    break;
                }

                cur_screen_x = col_end;
                raw_width_sum += raw_w;
            }

            if target_col.is_some() {
                raw_width_sum + surplus_y * target_col_w + target_surplus_x
            } else {
                return None;
            }
        };

        crate::render::find_interaction(setting, X)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DisplayConfig;

    #[test]
    fn test_single_column_make_display_and_full_width() {
        let mut display = DisplayUI::default();
        display.set("hello world");
        display.update_width(80);

        let (_table, full_width) = display.make_display((0, 0, vec![10, 20]));
        assert!(full_width.is_some());
        assert_eq!(display.table.len(), 1);
        assert_eq!(display.table[0].len(), 1);
        assert_eq!(display.height(), 1);
    }

    #[test]
    fn test_multi_column_make_display_no_full_width() {
        let mut display = DisplayUI::default();
        display.push("col1");
        display.push("col2");

        let (_table, full_width) = display.make_display((0, 0, vec![10, 20]));
        assert!(full_width.is_none());
        assert_eq!(display.table.len(), 1);
        assert_eq!(display.table[0].len(), 2);
    }

    #[test]
    fn test_interaction_single_column_wrapping() {
        let config = DisplayConfig {
            wrap: true,
            interactions: vec![vec![
                (0, "first_line".to_string()),
                (4, "second_line".to_string()),
            ]],
            ..Default::default()
        };
        let mut display = DisplayUI::new(config);
        display.set("123456");
        display.update_width(5);
        display.make_display((0, 0, vec![5]));

        // Heights should be 2 lines ("1234↵", "56")
        assert_eq!(display.heights, vec![2]);
        assert_eq!(display.height(), 2);

        // Click on (x=2, y=0) -> X = 2 -> "first_line"
        assert_eq!(
            display.get_interaction(2, 0),
            Some("first_line".to_string())
        );

        // Click on (x=1, y=1) -> X = 1 * 5 + 1 = 6 -> "second_line"
        assert_eq!(
            display.get_interaction(1, 1),
            Some("second_line".to_string())
        );
    }

    #[test]
    fn test_interaction_hidden_columns() {
        // Reproduces regions.md scenario:
        // Rendered displayed width 3, raw width 6 for col 0.
        // Col 1 is hidden (width 0), raw width 2.
        // Col 2 displayed width 3, raw width 3.
        // x value 4, y value 0 -> maps to X = 6 + 2 + 1 = 9
        let config = DisplayConfig {
            wrap: false,
            interactions: vec![vec![
                (0, "col0".to_string()),
                (6, "col1_hidden".to_string()),
                (8, "col2".to_string()),
            ]],
            ..Default::default()
        };
        let mut display = DisplayUI::new(config);
        display.push("123456"); // raw width 6
        display.push("ab"); // raw width 2
        display.push("xyz"); // raw width 3

        display.make_display((0, 0, vec![3, 0, 3]));

        // x=4: starts in col 2 (at screen x=3), surplus x = 1.
        // X = raw_w(col0) [6] + raw_w(col1) [2] + surplus_x [1] = 9.
        // Index 9 in interactions -> "col2" (since 9 >= 8)
        assert_eq!(display.get_interaction(4, 0), Some("col2".to_string()));

        // x=1: inside col 0 -> surplus x = 1 -> X = 1 -> "col0"
        assert_eq!(display.get_interaction(1, 0), Some("col0".to_string()));
    }

    #[test]
    fn test_interaction_multi_row_header() {
        let config = DisplayConfig {
            interactions: vec![
                vec![(0, "header_r0".to_string())],
                vec![(0, "header_r1".to_string())],
            ],
            ..Default::default()
        };
        let mut display = DisplayUI::new(config);
        display.header_table(vec![
            vec![Line::from("row0_col0"), Line::from("row0_col1")],
            vec![Line::from("row1_col0"), Line::from("row1_col1")],
        ]);

        display.make_display((0, 0, vec![10, 10]));
        assert_eq!(display.heights, vec![1, 1]);

        assert_eq!(display.get_interaction(2, 0), Some("header_r0".to_string()));
        assert_eq!(display.get_interaction(2, 1), Some("header_r1".to_string()));
        assert_eq!(display.get_interaction(2, 2), None);
    }

    #[test]
    fn test_interaction_multi_column_wrapping() {
        let config = DisplayConfig {
            wrap: true,
            interactions: vec![vec![
                (0, "col0_line0".to_string()),
                (4, "col0_line1".to_string()),
                (6, "col1".to_string()),
            ]],
            ..Default::default()
        };
        let mut display = DisplayUI::new(config);
        display.push("123456"); // wraps to 2 lines at width 5 ("1234↵", "56")
        display.push("abc"); // 1 line at width 5

        display.make_display((0, 0, vec![5, 5]));
        assert_eq!(display.heights, vec![2]);
        assert_eq!(display.height(), 2);

        // Click on col 0, line 0 -> (x=1, y=0) -> X = 1 -> "col0_line0"
        assert_eq!(
            display.get_interaction(1, 0),
            Some("col0_line0".to_string())
        );

        // Click on col 0, line 1 -> (x=1, y=1) -> X = 0 + 1 * 5 + 1 = 6 -> "col1" (or at x=0, y=1: X = 5 -> "col0_line1")
        assert_eq!(
            display.get_interaction(0, 1),
            Some("col0_line1".to_string())
        );

        // Click on col 1, line 0 -> (x=6, y=0) -> col 1 start is at x=5, surplus x=1 -> X = 6 + 0*5 + 1 = 7 -> "col1"
        assert_eq!(display.get_interaction(6, 0), Some("col1".to_string()));
    }

    #[test]
    fn test_interaction_column_spacing() {
        let config = DisplayConfig {
            interactions: vec![vec![(0, "col0".to_string()), (5, "col1".to_string())]],
            ..Default::default()
        };
        let mut display = DisplayUI::new(config);
        display.push("hello"); // raw width 5
        display.push("world"); // raw width 5

        // Col 0 width 5, Col spacing 2, Col 1 width 5
        display.make_display((0, 2, vec![5, 5]));

        // x=2 is in col 0 (x: 0..5) -> X = 2 -> "col0"
        assert_eq!(display.get_interaction(2, 0), Some("col0".to_string()));

        // x=5 or 6 is in spacing (5..7) -> past col 0 end (5), before col 1 start (7)
        // x=7 is start of col 1 -> surplus x = 0 -> X = 5 + 0 = 5 -> "col1"
        assert_eq!(display.get_interaction(7, 0), Some("col1".to_string()));
        assert_eq!(display.get_interaction(8, 0), Some("col1".to_string()));
    }
}
