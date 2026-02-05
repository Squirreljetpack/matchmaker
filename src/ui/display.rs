#![allow(unused)]
use log::debug;
use ratatui::{
    layout::{Constraint, Rect},
    style::{Style, Stylize},
    text::Text,
    widgets::{Cell, Paragraph, Row, Table, Wrap},
};

use crate::{
    config::DisplayConfig,
    utils::{
        serde::StringOrVec,
        text::{left_pad, wrapped_height},
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
            // todo
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
    pub fn update_width(&mut self, width: u16) {
        let border = self.config.border.width();
        self.width = width.saturating_sub(border);
        if self.config.wrap {
            self.height = self
                .text
                .iter()
                .map(|t| wrapped_height(t, self.width))
                .max()
                .unwrap_or_default();
        };
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
        let text = text.into();
        self.height = if self.config.wrap {
            wrapped_height(&text, self.width)
        } else {
            text.lines.len() as u16
        };
        self.text = vec![text];
        self.show = true;
    }

    pub fn clear(&mut self) {
        self.show = false;
        self.text.clear();
    }

    pub fn make_display(&self, result_indentation: usize, widths: &[u16]) -> Table<'_> {
        // Handle Case 0: Empty Text
        if self.text.is_empty() {
            return Table::default();
        }

        // Configure the Block (Border and Indentation logic)
        let block = {
            let b = self.config.border.as_block();
            if self.config.match_indent {
                let mut padding = self.config.border.padding;
                padding.left += result_indentation as u16;
                b.padding(padding)
            } else {
                b
            }
        };

        let style = Style::default()
            .fg(self.config.fg)
            .add_modifier(self.config.modifier);

        if self.text.len() == 1 {
            // Case 1: Single Cell (Full Width)
            let row = Row::new(vec![Cell::from(self.text[0].clone())]);
            Table::new(vec![row], [Constraint::Percentage(100)])
                .block(block)
                .style(style)
        } else {
            let row = Row::new(self.text[..widths.len()].to_vec());
            let mut constraints: Vec<_> = widths.iter().cloned().map(Constraint::Length).collect();
            // constraints.resize(self.text.len(), Constraint::Fill(1));

            Table::new(vec![row], constraints)
                .block(block)
                .style(style)
                .column_spacing(1)
        }
    }
}
