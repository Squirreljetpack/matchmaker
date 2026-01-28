use ratatui::{
    layout::{Position, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
// use unicode_width::UnicodeWidthStr;

use crate::{config::InputConfig, utils::text::grapheme_index_to_byte_index};

#[derive(Debug)]
pub struct InputUI {
    pub cursor: u16, // grapheme index
    pub input: String,
    pub config: InputConfig,
    pub prompt: Span<'static>,
}

impl InputUI {
    pub fn new(config: InputConfig) -> Self {
        Self {
            cursor: 0,
            input: "".into(),
            prompt: Span::from(config.prompt.clone()),
            config,
        }
    }
    // ---------- GETTERS ---------

    pub fn len(&self) -> usize {
        self.input.len()
    }
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        let left = self.config.border.left();
        let top = self.config.border.top();
        Position::new(
            rect.x + self.cursor + self.prompt.width() as u16 + left,
            rect.y + top,
        )
    }

    // ------------ SETTERS ---------------
    pub fn set(&mut self, input: String, cursor: u16) {
        let grapheme_count = input.graphemes(true).count() as u16;
        self.input = input;
        self.cursor = cursor.min(grapheme_count);
    }
    pub fn cancel(&mut self) {
        self.input.clear();
        self.cursor = 0;
    }
    pub fn reset_prompt(&mut self) {
        self.prompt = Span::from(self.config.prompt.clone());
    }

    // ---------- EDITING -------------
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
    pub fn insert_char(&mut self, c: char) {
        let old_grapheme_count = self.input.graphemes(true).count() as u16;
        let byte_index = grapheme_index_to_byte_index(&self.input, self.cursor);
        self.input.insert(byte_index, c);
        let new_grapheme_count = self.input.graphemes(true).count() as u16;
        if new_grapheme_count > old_grapheme_count {
            self.cursor += 1;
        }
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

    // ---------------------------------------
    pub fn make_input(&self) -> Paragraph<'_> {
        let line = Line::from(vec![
            self.prompt.clone(),
            Span::raw(self.input.as_str())
                .style(self.config.fg)
                .add_modifier(self.config.modifier),
        ]);

        let mut input = Paragraph::new(line);

        input = input.block(self.config.border.as_block());

        input
    }
}
