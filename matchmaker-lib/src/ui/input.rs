use std::ops::{Deref, DerefMut};

use ratatui::{
    layout::{Position, Rect},
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::config::QueryConfig;

#[derive(Debug, Default, Clone)]
pub struct InputUI {
    pub cursor: usize, // index into graphemes, can = graphemes.len()
    pub input: String, // remember to call recompute_graphemes() after modifying directly
    /// (byte_index, width)
    pub graphemes: Vec<(usize, u16)>,
    pub before: usize, // index into graphemes of the first visible grapheme
    pub width: u16,    // only relevant to cursor scrolling
}

impl InputUI {
    pub fn new() -> Self {
        Self::default()
    }

    // -------- UTILS -----------
    pub fn recompute_graphemes(&mut self) {
        self.graphemes = self
            .input
            .grapheme_indices(true)
            .map(|(idx, g)| (idx, g.width() as u16))
            .collect();
    }

    pub fn byte_index(&self, grapheme_idx: usize) -> usize {
        self.graphemes
            .get(grapheme_idx)
            .map(|(idx, _)| *idx)
            .unwrap_or(self.input.len())
    }

    pub fn str_at_cursor(&self) -> &str {
        &self.input[..self.byte_index(self.cursor)]
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

    // ------------ SETTERS ---------------
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

    pub fn insert_str(&mut self, content: &str) {
        let byte_idx = self.byte_index(self.cursor);
        self.input.insert_str(byte_idx, content);
        let added_graphemes = content.graphemes(true).count();
        self.recompute_graphemes();
        self.cursor += added_graphemes;
    }

    pub fn push_str(&mut self, content: &str) {
        self.input.push_str(content);
        self.recompute_graphemes();
        self.cursor = self.graphemes.len();
    }

    pub fn scroll_to_cursor(&mut self, padding: usize) {
        if self.width == 0 {
            return;
        }

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

    pub fn prepare_column_change(&mut self) {
        let trimmed = self.input.trim_end();
        if let Some(pos) = trimmed.rfind(' ') {
            let last_word = &trimmed[pos + 1..];
            if last_word.starts_with('%') {
                let bytes = trimmed[..pos].len();
                self.input.truncate(bytes);
            }
        } else if trimmed.starts_with('%') {
            self.input.clear();
        }

        if !self.input.is_empty() && !self.input.ends_with(' ') {
            self.input.push(' ');
        }
        self.recompute_graphemes();
        self.cursor = self.graphemes.len();
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

    pub fn render(&self) -> &str {
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

        visible_input
    }

    pub fn cursor_rel_offset(&self) -> u16 {
        self.graphemes[self.before..self.cursor]
            .iter()
            .map(|(_, w)| *w)
            .sum()
    }
}

#[derive(Debug)]
pub struct QueryUI {
    pub state: InputUI,
    prompt: Line<'static>,
    pub config: QueryConfig,
}

impl Deref for QueryUI {
    type Target = InputUI;
    fn deref(&self) -> &Self::Target {
        &self.state
    }
}

impl DerefMut for QueryUI {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.state
    }
}

impl QueryUI {
    pub fn new(config: QueryConfig) -> Self {
        let mut ui = Self {
            state: InputUI::new(),
            prompt: Line::styled(config.prompt.clone(), config.prompt_style),
            config,
        };

        if !ui.config.initial.is_empty() {
            ui.input = ui.config.initial.clone();
            ui.recompute_graphemes();
            ui.cursor = ui.graphemes.len();
        }

        ui
    }

    pub fn left(&self) -> u16 {
        self.config.border.left() + self.prompt.width() as u16
    }

    /// Given a rect the widget is rendered with, produce the absolute position the cursor is rendered at.
    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        let top = self.config.border.top();
        Position::new(
            rect.x + self.left() + self.cursor_rel_offset(),
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

    pub fn scroll_to_cursor(&mut self) {
        let padding = self.config.scroll_padding as usize;
        self.state.scroll_to_cursor(padding);
    }

    // ---------------------------------------
    // remember to call scroll_to_cursor beforehand

    pub fn make_input(&self) -> Paragraph<'_> {
        let mut line = self.prompt.clone();
        line.push_span(Span::styled(self.state.render(), self.config.style));

        Paragraph::new(line).block(self.config.border.as_block())
    }

    /// Set the input ui prefix. The prompt style from the config overrides the Line style (but not the span styles).
    pub fn set_prompt(&mut self, template: Option<Line<'static>>) {
        let line = template
            .unwrap_or_else(|| self.config.prompt.clone().into())
            .style(self.config.prompt_style);
        self.set_prompt_line(line);
    }

    /// Set the input ui prefix directly.
    pub fn set_prompt_line(&mut self, prompt: Line<'static>) {
        let old_width = self.prompt.to_string().width();
        let new_width = prompt.to_string().width();

        if new_width > old_width {
            self.width = self.width.saturating_sub((new_width - old_width) as u16);
        } else if old_width > new_width {
            self.width += (old_width - new_width) as u16;
        }

        self.prompt = prompt;
    }
}
