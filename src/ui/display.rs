#![allow(unused)]
use cli_boilerplate_automation::bait::BoolExt;
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
    height: u16,
    width: u16,
    text: Vec<Text<'static>>,
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
            show: config.content.is_some(),
            text,
            config,
        }
    }

    // not update_dimensions to remind that we only want to call this on tui resize, not layout resize
    // todo: this doesn't update the line contents
    pub fn update_width(&mut self, width: u16) {
        let border_w = self.config.border.width();
        let new_w = width.saturating_sub(border_w);
        if new_w != self.width {
            self.width = new_w;
            if self.config.wrap && self.text.len() == 1 {
                let text = wrap_text(self.text.remove(0), self.width).0;
                self.text.push(text);
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
    pub fn set(&mut self, text: impl Into<Text<'static>>) {
        let (text, _) = wrap_text(text.into(), self.config.wrap as u16 * self.width);
        self.height = text.height() as u16;
        self.text = vec![text];
        self.show = true;
    }

    pub fn clear(&mut self) {
        self.show = false;
        self.text.clear();
    }

    pub fn single(&self) -> bool {
        self.text.len() == 1
    }

    // todo: lowpri: cache texts to not have to always rewrap?
    pub fn make_display(
        &self,
        result_indentation: usize,
        widths: &[u16],
        col_spacing: u16,
    ) -> Table<'_> {
        if self.text.is_empty() {
            return Table::default();
        }

        let block = {
            let b = self.config.border.as_block();
            if self.config.match_indent && self.text.len() == 1 {
                let mut padding = self.config.border.padding;

                padding.left =
                    (result_indentation as u16).saturating_sub(self.config.border.left());
                b.padding(padding)
            } else {
                b
            }
        };

        let style = Style::default()
            .fg(self.config.fg)
            .add_modifier(self.config.modifier);

        if self.text.len() == 1 {
            // Single Cell (Full Width)
            let row = Row::new(vec![Cell::from(self.text[0].clone())]);
            Table::new(vec![row], [Constraint::Percentage(100)])
                .block(block)
                .style(style)
        } else {
            let cells = self.text[..widths.len()]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, text)| {
                    let mut ret = wrap_text(text, widths[i]).0;
                    if i == 0 && self.config.match_indent {
                        prefix_text(
                            &mut ret,
                            " ".repeat(
                                result_indentation
                                    .saturating_sub(self.config.border.left() as usize),
                            ),
                        );
                    }

                    matches!(
                        self.config.row_connection_style,
                        RowConnectionStyle::Disjoint
                    )
                    .then_modify(ret, |r| r.style(style))
                });

            let row = Row::new(cells);
            // let mut constraints: Vec<_> = widths.iter().cloned().map(Constraint::Length).collect();
            // we omit header columns after the last result column, an alternative could be supported when ::Full, something like : constraints.resize(self.text.len(), Constraint::Fill(1));

            let mut ret = Table::new(vec![row], widths.to_vec())
                .block(block)
                .column_spacing(col_spacing);

            (!matches!(
                self.config.row_connection_style,
                RowConnectionStyle::Disjoint
            ))
            .then_modify(ret, |r| r.style(style))
        }
    }
}
