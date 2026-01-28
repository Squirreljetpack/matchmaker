#![allow(unused)]
use log::debug;
use ratatui::{
    layout::Rect,
    style::{Style, Stylize},
    text::Text,
    widgets::{Paragraph, Wrap},
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
    text: Text<'static>,
    pub show: bool,
    pub config: DisplayConfig,
}

impl DisplayUI {
    pub fn new(config: DisplayConfig) -> Self {
        let text = match &config.content {
            Some(StringOrVec::String(s)) => Text::from(s.clone()),
            // todo
            _ => Text::from(String::new()),
        };
        let height = text.lines.len() as u16;

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
            self.height = wrapped_height(&self.text, self.width)
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
        self.text = text;
        self.show = true;
    }

    pub fn clear(&mut self) {
        self.show = false;
    }

    pub fn make_display(&self, result_indentation: usize) -> Paragraph<'_> {
        let mut ret = Paragraph::new(self.text.clone())
            .style(Style::default().fg(self.config.fg))
            .add_modifier(self.config.modifier);

        if self.config.wrap {
            ret = ret.wrap(Wrap { trim: false });
        }

        let block = {
            let ret = self.config.border.as_block();
            if self.config.match_indent {
                let mut padding = self.config.border.padding;
                padding.left += result_indentation as u16;
                ret.padding(padding)
            } else {
                ret
            }
        };

        ret = ret.block(block);

        ret
    }
}
