#![allow(unused)]
use log::debug;
use ratatui::{
    style::{Style, Stylize},
    widgets::Paragraph,
};

use crate::{config::{DisplayConfig, StringOrVec}, utils::text::left_pad};

#[derive(Debug, Clone)]
pub struct DisplayUI {
    height: u16,
    pub text: String,
    pub show: bool,
    pub config: DisplayConfig,
}

impl DisplayUI {
    pub fn new(config: DisplayConfig) -> Self {
        let text = match &config.content {
            Some(StringOrVec::String(s)) => s.clone(),
            // todo
            _ => String::new(),
        };

        Self {
            height: text.lines().count() as u16,
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

    pub fn set(&mut self, text: String) {
        self.height = text.lines().count() as u16;
        self.text = text;
    }

    pub fn make_display(&self, result_indentation: usize) -> Paragraph<'_> {
        let text = if self.config.match_indent {
            left_pad(&self.text, result_indentation)
        } else {
            self.text.clone()
        };
        debug!("{result_indentation}, {}, {text}", self.config.match_indent);

        let mut ret = Paragraph::new(text)
        .style(Style::default().fg(self.config.fg))
        .add_modifier(self.config.modifier);



        ret = ret.block(self.config.border.as_block());

        ret
    }
}
