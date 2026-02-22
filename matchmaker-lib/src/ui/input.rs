use ratatui::{
    layout::{Position, Rect},
    style::Stylize,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::InputConfig;

#[derive(Debug)]
pub struct InputUI {
    cursor: usize, // index into graphemes, can = graphemes.len()
    pub input: String,
    /// (byte_index, width)
    graphemes: Vec<(usize, u16)>,
    pub prompt: Span<'static>,
    before: usize,  // index into graphemes of the first visible grapheme
    pub width: u16, // only relevant to cursor scrolling

    pub config: InputConfig,
}

impl InputUI {
    pub fn new(config: InputConfig) -> Self {
        let mut ui = Self {
            cursor: 0,
            input: "".into(),
            graphemes: Vec::new(),
            prompt: Span::from(config.prompt.clone()),
            config,
            before: 0,
            width: 0,
        };

        if !ui.config.initial.is_empty() {
            ui.input = ui.config.initial.clone();
            ui.recompute_graphemes();
            ui.cursor = ui.graphemes.len();
        }

        ui
    }

    // -------- UTILS -----------
    fn recompute_graphemes(&mut self) {
        self.graphemes = self
            .input
            .grapheme_indices(true)
            .map(|(idx, g)| (idx, g.width() as u16))
            .collect();
    }

    fn byte_index(&self, grapheme_idx: usize) -> usize {
        self.graphemes
            .get(grapheme_idx)
            .map(|(idx, _)| *idx)
            .unwrap_or(self.input.len())
    }

    // ---------- GETTERS ---------

    pub fn len(&self) -> usize {
        self.input.len()
    }
    pub fn is_empty(&self) -> bool {
        self.input.is_empty()
    }

    /// grapheme index
    pub fn cursor(&self) -> u16 {
        self.cursor as u16
    }

    /// Given a rect the widget is rendered with, produce the absolute position the cursor is rendered at.
    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        let left = self.config.border.left();
        let top = self.config.border.top();

        let offset_x: u16 = self.graphemes[self.before..self.cursor]
            .iter()
            .map(|(_, w)| *w)
            .sum();

        Position::new(
            rect.x + self.prompt.width() as u16 + left + offset_x,
            rect.y + top,
        )
    }

    // ------------ SETTERS ---------------
    pub fn update_width(&mut self, width: u16) {
        let text_width = width
            .saturating_sub(self.prompt.width() as u16)
            .saturating_sub(self.config.border.width());
        if self.width != text_width {
            self.width = text_width;
        }
    }

    pub fn set(&mut self, input: impl Into<Option<String>>, cursor: u16) {
        if let Some(input) = input.into() {
            self.input = input;
            self.recompute_graphemes();
        }
        self.cursor = (cursor as usize).min(self.graphemes.len());
    }

    pub fn push_char(&mut self, c: char) {
        let byte_idx = self.byte_index(self.cursor);
        self.input.insert(byte_idx, c);
        self.recompute_graphemes();
        self.cursor += 1;
    }

    pub fn push_str(&mut self, content: &str) {
        let byte_idx = self.byte_index(self.cursor);
        self.input.insert_str(byte_idx, content);
        let added_graphemes = content.graphemes(true).count();
        self.recompute_graphemes();
        self.cursor += added_graphemes;
    }

    pub fn scroll_to_cursor(&mut self) {
        if self.width == 0 {
            return;
        }
        let padding = self.config.scroll_padding as usize;

        // when cursor moves behind or on start, display grapheme before cursor as the first visible,
        if self.before >= self.cursor {
            self.before = self.cursor.saturating_sub(padding);
            return;
        }

        // move start up
        loop {
            let visual_dist: u16 = self.graphemes
                [self.before..=(self.cursor + padding).min(self.graphemes.len().saturating_sub(1))]
                .iter()
                .map(|(_, w)| *w)
                .sum();

            // ensures visual_start..=cursor is displayed
            // Padding ensures the following element after cursor if present is displayed.
            if visual_dist <= self.width {
                break;
            }

            if self.before < self.cursor {
                self.before += 1;
            } else {
                // never move before over cursor
                break;
            }
        }
    }

    pub fn cancel(&mut self) {
        self.input.clear();
        self.graphemes.clear();
        self.cursor = 0;
        self.before = 0;
    }
    pub fn reset_prompt(&mut self) {
        self.prompt = Span::from(self.config.prompt.clone());
    }

    /// Set cursor to a visual offset relative to start position
    pub fn set_at_visual_offset(&mut self, visual_offset: u16) {
        let mut current_width = 0;
        let mut target_cursor = self.before;

        for (i, &(_, width)) in self.graphemes.iter().enumerate().skip(self.before) {
            if current_width + width > visual_offset {
                // If clicked on the right half of a character, move cursor after it
                if visual_offset - current_width > width / 2 {
                    target_cursor = i + 1;
                } else {
                    target_cursor = i;
                }
                break;
            }
            current_width += width;
            target_cursor = i + 1;
        }

        self.cursor = target_cursor;
    }

    // ---------- EDITING -------------
    pub fn forward_char(&mut self) {
        if self.cursor < self.graphemes.len() {
            self.cursor += 1;
        }
    }
    pub fn backward_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    pub fn forward_word(&mut self) {
        let mut in_word = false;
        while self.cursor < self.graphemes.len() {
            let byte_start = self.graphemes[self.cursor].0;
            let byte_end = self
                .graphemes
                .get(self.cursor + 1)
                .map(|(idx, _)| *idx)
                .unwrap_or(self.input.len());
            let g = &self.input[byte_start..byte_end];

            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    break;
                }
            } else {
                in_word = true;
            }
            self.cursor += 1;
        }
    }

    pub fn backward_word(&mut self) {
        let mut in_word = false;
        while self.cursor > 0 {
            let byte_start = self.graphemes[self.cursor - 1].0;
            let byte_end = self
                .graphemes
                .get(self.cursor)
                .map(|(idx, _)| *idx)
                .unwrap_or(self.input.len());
            let g = &self.input[byte_start..byte_end];

            if g.chars().all(|c| c.is_whitespace()) {
                if in_word {
                    break;
                }
            } else {
                in_word = true;
            }
            self.cursor -= 1;
        }
    }

    pub fn delete(&mut self) {
        if self.cursor > 0 {
            let start = self.graphemes[self.cursor - 1].0;
            let end = self.byte_index(self.cursor);
            self.input.replace_range(start..end, "");
            self.recompute_graphemes();
            self.cursor -= 1;
        }
    }

    pub fn delete_word(&mut self) {
        let old_cursor = self.cursor;
        self.backward_word();
        let new_cursor = self.cursor;

        let start = self.byte_index(new_cursor);
        let end = self.byte_index(old_cursor);
        self.input.replace_range(start..end, "");
        self.recompute_graphemes();
    }

    pub fn delete_line_start(&mut self) {
        let end = self.byte_index(self.cursor);
        self.input.replace_range(0..end, "");
        self.recompute_graphemes();
        self.cursor = 0;
        self.before = 0;
    }

    pub fn delete_line_end(&mut self) {
        let start = self.byte_index(self.cursor);
        self.input.truncate(start);
        self.recompute_graphemes();
    }

    // ---------------------------------------
    // remember to call scroll_to_cursor beforehand
    pub fn make_input(&self) -> Paragraph<'_> {
        let mut visible_width = 0;
        let mut end_idx = self.before;

        while end_idx < self.graphemes.len() {
            let g_width = self.graphemes[end_idx].1;
            if self.width != 0 && visible_width + g_width > self.width {
                break;
            }
            visible_width += g_width;
            end_idx += 1;
        }

        let start_byte = self.byte_index(self.before);
        let end_byte = self.byte_index(end_idx);
        let visible_input = &self.input[start_byte..end_byte];

        let line = Line::from(vec![
            self.prompt.clone(),
            Span::raw(visible_input)
                .style(self.config.fg)
                .add_modifier(self.config.modifier),
        ]);

        Paragraph::new(line).block(self.config.border.as_block())
    }
}
