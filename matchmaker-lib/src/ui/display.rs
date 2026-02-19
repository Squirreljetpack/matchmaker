#![allow(unused)]
use cli_boilerplate_automation::bait::{BoolExt, TransformExt};
use log::debug;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::Text,
    widgets::{Cell, Paragraph, Row, Table, Wrap},
};

use crate::{
    config::{DisplayConfig, RowConnectionStyle},
    utils::{
        serde::StringOrVec,
        text::{left_pad, prefix_text, wrap_text, wrapped_height},
    },
};

#[derive(Debug)]
pub struct DisplayUI {
    width: u16,
    height: u16,
    text: Vec<Text<'static>>,
    text_split_index: usize, // everything at and after this index is a header_line
    pub show: bool,
    pub config: DisplayConfig,
}

impl DisplayUI {
    pub fn new(config: DisplayConfig) -> Self {
        let (text, height) = match &config.content {
            Some(StringOrVec::String(s)) => {
                let text = Text::from(s.clone());
                let height = text.height() as u16;
                (vec![text], height)
            }
            Some(StringOrVec::Vec(s)) => {
                let text: Vec<_> = s.iter().map(|s| Text::from(s.clone())).collect();
                let height = text.iter().map(|t| t.height()).max().unwrap_or_default() as u16;
                (text, height)
            }
            _ => (vec![], 0),
        };

        Self {
            height,
            width: 0,
            show: config.content.is_some() || config.header_lines > 0,
            text_split_index: text.len(),
            text,
            config,
        }
    }

    pub fn update_width(&mut self, width: u16) {
        let border_w = self.config.border.width();
        let new_w = width.saturating_sub(border_w);
        if new_w != self.width {
            self.width = new_w;
            // only rewrap of single cell is supported for now
            if self.config.wrap && self.text_split_index == 1 {
                let text = wrap_text(self.text[0].clone(), self.width).0;
                self.text[0] = text;
            }
        }
    }

    pub fn height(&self) -> u16 {
        if !self.show {
            return 0;
        }
        let mut height = self.height;
        height += self.config.border.height();

        height
    }

    /// Set text and visibility. Compute wrapped height.
    pub fn set(&mut self, text: impl Into<Text<'static>>, keep_header_columns: bool) {
        let (text, _) = wrap_text(text.into(), self.config.wrap as u16 * self.width);

        if keep_header_columns {
            // Keep everything after text_split_index (header)
            let header = self.text.split_off(self.text_split_index);

            self.text = vec![text];
            self.text.extend(header);
        } else {
            self.text = vec![text];
        }

        self.text_split_index = 1;
        self.show = true;
    }

    pub fn clear(&mut self, keep_header_columns: bool) {
        self.show = false;

        if keep_header_columns {
            let header = self.text.split_off(self.text_split_index);
            self.text = header;
        } else {
            self.text.clear();
        }

        self.text_split_index = 0;
    }

    /// Whether this is table has just one column (ignoring header_lines)
    pub fn single(&self) -> bool {
        self.text_split_index == 1
    }

    pub fn header_columns(&mut self, columns: Vec<Text<'static>>) {
        self.text.truncate(self.text_split_index);
        self.text.extend(columns);
    }

    // todo: lowpri: cache texts to not have to always rewrap?
    pub fn make_display(
        &mut self,
        result_indentation: u16,
        mut widths: Vec<u16>,
        col_spacing: u16,
    ) -> Table<'_> {
        if self.text.is_empty() || widths.is_empty() {
            return Table::default();
        }

        let block = {
            let b = self.config.border.as_block();
            if self.config.match_indent {
                let mut padding = self.config.border.padding;

                padding.left = result_indentation.saturating_sub(self.config.border.left());
                widths[0] -= result_indentation;
                b.padding(padding)
            } else {
                b
            }
        };

        let style = Style::default()
            .fg(self.config.fg)
            .add_modifier(self.config.modifier);

        let (cells, height) = if self.text_split_index == 1 {
            // Single Cell (Full Width)
            // reflow is handled in update_width
            let cells = if self.text.len() > 1 {
                vec![]
            } else {
                vec![Cell::from(self.text[0].clone())]
            };
            let height = self.text[0].height() as u16;

            (cells, height)
        } else {
            let mut height = 0;
            // todo: for header, instead of reflowing on every render, the widths should be dynamically proportionate to the available width similar to results. Then results should take the max_widths from here instead of computing them.
            let cells = self.text[..self.text_split_index]
                .iter()
                .cloned()
                .zip(widths.iter().copied())
                .map(|(text, width)| {
                    let mut ret = wrap_text(text, width).0;
                    height = height.max(ret.height() as u16);

                    Cell::from(ret.transform_if(
                        matches!(
                            self.config.row_connection_style,
                            RowConnectionStyle::Disjoint
                        ),
                        |r| r.style(style),
                    ))
                })
                .collect();

            (cells, height)
        };

        let row = Row::new(cells).style(style).height(height);
        let mut rows = vec![row];
        self.height = height;

        // add header cells
        if self.text_split_index < self.text.len() {
            let mut height = 0;

            let cells = self.text[self.text_split_index..].iter().map(|x| {
                height = height.max(x.height() as u16);
                Cell::from(x.clone())
            });

            rows.push(Row::new(cells).style(style).height(height));

            self.height += height;
        }

        let widths = if self.single() && self.text.len() == 1 {
            vec![Constraint::Percentage(100)]
        } else {
            widths.into_iter().map(Constraint::Length).collect()
        };

        Table::new(rows, widths)
            .block(block)
            .column_spacing(col_spacing)
            .transform_if(
                !matches!(
                    self.config.row_connection_style,
                    RowConnectionStyle::Disjoint
                ),
                |t| t.style(style),
            )
    }

    /// Draw in the same area as display when self.single() to produce a full width row over the table area
    pub fn make_full_width_row(&self, result_indentation: u16) -> Paragraph<'_> {
        let style = Style::default()
            .fg(self.config.fg)
            .add_modifier(self.config.modifier);

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
            .style(style)
    }
}
