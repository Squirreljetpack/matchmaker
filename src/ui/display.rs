#![allow(unused)]
use log::debug;
use ratatui::{
    style::{Style, Stylize}, text::Text, widgets::{Paragraph, Wrap}
};

use crate::{config::DisplayConfig, utils::{serde::StringOrVec, text::left_pad}};

#[derive(Debug, Clone)]
pub struct DisplayUI {
    height: u16,
    pub text: Text<'static>,
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
            show: config.content.is_some(),
            text,
            config,
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

    pub fn set(&mut self, text: impl Into<Text<'static>>) {
        let text = text.into();
        self.height = text.lines.len() as u16;
        self.text = text;
    }

    pub fn make_display(&self, result_indentation: usize) -> Paragraph<'_> {
        // debug!("{result_indentation}, {}, {text}", self.config.match_indent);

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
