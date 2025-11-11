#![allow(unused)]
use ratatui::{style::Stylize, widgets::Paragraph};

use crate::config::DisplayConfig;

#[derive(Debug, Clone)]
pub struct DisplayUI {
    height: u16,
    pub text: String, // empty sentinels for hide
    pub config: DisplayConfig,
}

impl DisplayUI {
    pub fn new(config: DisplayConfig) -> Self {
        Self {
            height: 0,
            text: Default::default(),
            config,
        }
    }

    pub fn height(&self) -> u16 {
        if self.text.is_empty() {
            return 0;
        }
        let mut height = self.height;
        height += 2 * !self.config.border.sides.is_empty() as u16;

        height
    }

    pub fn set(&mut self, text: String) {
        self.text = text;
    }

    pub fn make_header(&self) -> Paragraph<'_> {
        Paragraph::new(format!(
            "{} ",
            &self.text
        ))
        .add_modifier(self.config.modifier)
        // todo: colors of fg, bg should be supported in a config, border maybe?
    }
}