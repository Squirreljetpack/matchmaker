use ratatui::{
    layout::{Position, Rect},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{config::InputConfig, utils::text::grapheme_index_to_byte_index};

#[derive(Debug, Clone)]
pub struct InputUI {
    pub cursor: u16, // grapheme index
    pub input: String,
    pub config: InputConfig,
}

impl InputUI {
    pub fn new(config: InputConfig) -> Self {
        Self {
            cursor: 0,
            input: "".into(),
            config,
        }
    }

    pub fn make_input(&self) -> Paragraph<'_> {
        let mut input = Paragraph::new(format!("{}{}", &self.config.prompt, self.input.as_str()))
            .style(self.config.input_fg);

        input = input.block(self.config.border.as_block());

        input
    }

    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        let border = self.config.border.sides;
        Position::new(
            rect.x + self.cursor + self.config.prompt.width() as u16 + !border.is_empty() as u16,
            rect.y + !border.is_empty() as u16,
        )
    }

    pub fn height(&self) -> u16 {
        let mut height = 1;
        height += 2 * !self.config.border.sides.is_empty() as u16;

        height
    }
    pub fn forward_char(&mut self) {
        // Check against the total number of graphemes
        if self.cursor < self.input.graphemes(true).count() as u16 {
            self.cursor += 1;
        }
    }

    pub fn backward_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    // todo: lowpri: maintain a grapheme buffer to optimize

    pub fn insert_char(&mut self, c: char) {
        let old_grapheme_count = self.input.graphemes(true).count() as u16;
        let byte_index = grapheme_index_to_byte_index(&self.input, self.cursor);
        self.input.insert(byte_index, c);
        let new_grapheme_count = self.input.graphemes(true).count() as u16;
        if new_grapheme_count > old_grapheme_count {
            self.cursor += 1;
        }
    }

    pub fn set_input(&mut self, new_input: String, new_cursor: u16) {
        let grapheme_count = new_input.graphemes(true).count() as u16;
        self.input = new_input;
        self.cursor = new_cursor.min(grapheme_count);
    }

    pub fn forward_word(&mut self) {
        let post = self.input.graphemes(true).skip(self.cursor as usize);

        let mut in_word = false;

        for g in post {
            self.cursor += 1;
            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    return;
                }
            } else {
                in_word = true;
            }
        }
    }

    pub fn backward_word(&mut self) {
        let mut in_word = false;

        let pre: Vec<&str> = self
            .input
            .graphemes(true)
            .take(self.cursor as usize)
            .collect();

        for g in pre.iter().rev() {
            self.cursor -= 1;

            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    return;
                }
            } else {
                in_word = true;
            }
        }

        self.cursor = 0;
    }

    pub fn delete(&mut self) {
        if self.cursor > 0 {
            let byte_start = grapheme_index_to_byte_index(&self.input, self.cursor - 1);
            let byte_end = grapheme_index_to_byte_index(&self.input, self.cursor);

            self.input.replace_range(byte_start..byte_end, "");
            self.cursor -= 1;
        }
    }

    pub fn delete_word(&mut self) {
        let old_cursor_grapheme = self.cursor;
        self.backward_word();
        let new_cursor_grapheme = self.cursor;

        let byte_start = grapheme_index_to_byte_index(&self.input, new_cursor_grapheme);
        let byte_end = grapheme_index_to_byte_index(&self.input, old_cursor_grapheme);

        self.input.replace_range(byte_start..byte_end, "");
    }

    pub fn delete_line_start(&mut self) {
        let byte_end = grapheme_index_to_byte_index(&self.input, self.cursor);

        self.input.replace_range(0..byte_end, "");
        self.cursor = 0;
    }

    pub fn delete_line_end(&mut self) {
        let byte_index = grapheme_index_to_byte_index(&self.input, self.cursor);

        // Truncate operates on the byte index
        self.input.truncate(byte_index);
    }

    pub fn cancel(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }
}
