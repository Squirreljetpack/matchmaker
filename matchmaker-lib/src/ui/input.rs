use std::ops::{Deref, DerefMut};

use ratatui::{
    layout::{Position, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::{
    config::{FrameSetting, QueryConfig},
    ui::RESULTS_MIN_W,
    utils::text::trim_line_left_to_width,
};

#[derive(Debug, Default, Clone)]
pub struct InputUI {
    cursor: usize, // index into graphemes, can = graphemes.len()
    input: String, // remember to call recompute_graphemes() after modifying directly
    /// (byte_index, width)
    graphemes: Vec<(usize, u16)>,
    before: usize, // index into graphemes of the first visible grapheme
    width: u16,    // only relevant to cursor scrolling
    /// Word movement stops at these characters in addition to whitespace.
    word_boundaries: Vec<char>,
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
    pub fn input(&self) -> String {
        self.input.clone()
    }

    /// grapheme index
    pub fn cursor(&self) -> u16 {
        self.cursor as u16
    }

    pub fn width(&self) -> u16 {
        self.width
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

    // Right padding is at most one grapheme.
    pub fn scroll_to_cursor(
        &mut self,
        left_scroll_padding: usize,
        recenter_left_scroll_edge: bool,
    ) {
        if self.width == 0 {
            self.before = 0;
            return;
        }

        let width = self.width;
        let right_padding = if recenter_left_scroll_edge {
            1
        } else {
            left_scroll_padding.min(1)
        };
        let left_scroll_padding =
            left_scroll_padding.min(width.saturating_sub(1 + right_padding as u16) as usize);

        // 1. Keep the cursor inside the left scroll edge.
        if self.cursor <= self.before + left_scroll_padding {
            self.before = if recenter_left_scroll_edge {
                self.cursor.saturating_sub(width as usize / 2)
            } else {
                self.cursor.saturating_sub(left_scroll_padding)
            };
        }

        // 2. Ensure cursor fits within the visible window [before ..).
        // If cursor is at or beyond the right edge, advance `before`.
        while self.before < self.cursor {
            let cursor_incl =
                (self.cursor + right_padding).min(self.graphemes.len().saturating_sub(1));
            let span_width: u16 = self.graphemes[self.before..=cursor_incl]
                .iter()
                .map(|(_, w)| *w)
                .sum();
            if span_width <= width {
                break;
            }
            self.before += 1;
        }

        // 3. Guarantee before never exceeds cursor
        self.before = self.before.min(self.cursor);
    }

    pub fn clear(&mut self) {
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
        self.cursor = self.cursor.min(self.graphemes.len());
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

    /// Whether the grapheme counts as a word boundary for word movement.
    fn is_separator(&self, g: &str) -> bool {
        g.chars()
            .all(|c| c.is_whitespace() || self.word_boundaries.contains(&c))
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

            if self.is_separator(g) {
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

            if self.is_separator(g) {
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

    /// Note: Preserves ^, ', ! in the initial position
    pub fn delete_word(&mut self) {
        let old_cursor = self.cursor;
        self.backward_word();
        let mut new_cursor = self.cursor;

        if new_cursor == 0
            && old_cursor > 1
            && let Some(first) = self.input.chars().next()
            && matches!(first, '^' | '!' | '\'')
        {
            new_cursor = 1;
        }

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

        (&self.input[start_byte..end_byte]) as _
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
    /// When false, the query bar renders without its border: border lines,
    /// title, and padding are skipped and no space is reserved for them in
    /// layout.
    pub show_border: bool,
    /// Last known total widget width, used to cap prompt width.
    widget_width: u16,
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
            show_border: true,
            widget_width: u16::MAX,
        };

        ui.state.word_boundaries = ui.config.word_boundaries.clone();

        if !ui.config.initial.is_empty() {
            ui.input = ui.config.initial.clone();
            ui.recompute_graphemes();
            ui.cursor = ui.graphemes.len();
        }

        ui
    }

    /// The effective frame setting: `None` while `show_border` is false.
    fn frame(&self) -> Option<&FrameSetting> {
        self.show_border.then_some(&self.config.frame)
    }

    fn effective_prompt_width(&self) -> u16 {
        let max_prompt = self.widget_width.saturating_sub(RESULTS_MIN_W);
        (self.prompt.width() as u16).min(max_prompt)
    }

    pub fn left(&self) -> u16 {
        self.frame().map_or(0, FrameSetting::left) + self.effective_prompt_width()
    }

    pub fn top(&self) -> u16 {
        self.frame().map_or(0, FrameSetting::top)
    }

    /// Total height of the input bar, including the frame.
    pub fn height(&self) -> u16 {
        1 + self.frame().map_or(0, FrameSetting::height)
    }

    /// Given a rect the widget is rendered with, produce the absolute position the cursor is rendered at.
    pub fn cursor_offset(&self, rect: &Rect) -> Position {
        Position::new(
            rect.x + self.left() + self.cursor_rel_offset(),
            rect.y + self.top(),
        )
    }

    // ------------ SETTERS ---------------
    pub fn update_width(&mut self, width: u16) {
        self.widget_width = width;
        let text_width = width
            .saturating_sub(self.effective_prompt_width())
            .saturating_sub(self.frame().map_or(0, FrameSetting::width));
        if self.width != text_width {
            self.width = text_width;
        }
    }

    pub fn scroll_to_cursor(&mut self) {
        let padding = self.config.scroll_padding as usize;
        self.state
            .scroll_to_cursor(padding, self.config.recenter_left_scroll_edge);
    }

    // ---------------------------------------
    /// remember to call scroll_to_cursor beforehand
    pub fn make_input(&self) -> Paragraph<'_> {
        let max_prompt = self.widget_width.saturating_sub(RESULTS_MIN_W);
        let mut line = trim_line_left_to_width(&self.prompt, max_prompt);
        line.push_span(Span::styled(
            self.state.render(),
            self.config.style.r#override(Style::reset()),
        ));

        let paragraph = Paragraph::new(line);
        match self.frame() {
            Some(frame) => paragraph.block(frame.as_block()),
            None => paragraph,
        }
    }

    /// Set the input ui prefix. The prompt style from the config overrides the Line style (but not the span styles).
    /// None restores the prompt defined in the config.
    pub fn set_prompt(&mut self, template: Option<Line<'static>>) {
        let line = template
            .unwrap_or_else(|| self.config.prompt.clone().into())
            .style(self.config.prompt_style);
        self.set_prompt_line(line);
    }

    /// Set the input ui prefix directly.
    pub fn set_prompt_line(&mut self, prompt: Line<'static>) {
        let old_effective = self.effective_prompt_width();
        self.prompt = prompt;
        let new_effective = self.effective_prompt_width();

        if new_effective > old_effective {
            self.width = self.width.saturating_sub(new_effective - old_effective);
        } else if old_effective > new_effective {
            self.width += old_effective - new_effective;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ui_with_word_boundaries() -> InputUI {
        let mut ui = InputUI::new();
        ui.word_boundaries = vec!['.', '/'];
        ui.set("foo.bar/baz qux".to_string(), u16::MAX);
        ui
    }

    #[test]
    fn forward_word_stops_at_word_boundaries_and_whitespace() {
        let mut ui = ui_with_word_boundaries();
        ui.set("foo.bar/baz qux".to_string(), 0);
        ui.forward_word();
        assert_eq!(ui.cursor(), 3); // on '.'
        ui.forward_word();
        assert_eq!(ui.cursor(), 7); // on '/'
        ui.forward_word();
        assert_eq!(ui.cursor(), 11); // on the space
        ui.forward_word();
        assert_eq!(ui.cursor(), 15); // end
    }

    #[test]
    fn backward_word_stops_at_word_boundaries_and_whitespace() {
        let mut ui = ui_with_word_boundaries();
        ui.backward_word();
        assert_eq!(ui.cursor(), 12); // start of "qux"
        ui.backward_word();
        assert_eq!(ui.cursor(), 8); // start of "baz"
        ui.backward_word();
        assert_eq!(ui.cursor(), 4); // start of "bar"
        ui.backward_word();
        assert_eq!(ui.cursor(), 0); // start of "foo"
    }

    #[test]
    fn default_word_movement_treats_only_whitespace_as_separator() {
        let mut ui = InputUI::new();
        ui.set("foo.bar/baz".to_string(), u16::MAX);
        ui.forward_word();
        assert_eq!(ui.cursor(), 11);
        ui.set("foo.bar/baz".to_string(), 0);
        ui.forward_word();
        assert_eq!(ui.cursor(), 11);
        ui.set("foo.bar/baz".to_string(), 11);
        ui.backward_word();
        assert_eq!(ui.cursor(), 0);
    }

    #[test]
    fn scroll_to_cursor_does_not_oscillate_on_small_width() {
        let mut ui = InputUI::new();
        ui.width = 2;
        ui.set("abc".to_string(), 3);
        // Repeated calls to scroll_to_cursor with padding > width must be idempotent
        ui.scroll_to_cursor(3, false);
        let before_first = ui.before;
        ui.scroll_to_cursor(3, false);
        let before_second = ui.before;
        assert_eq!(before_first, before_second);

        // Move cursor back 1 to position 2
        ui.backward_char();
        assert_eq!(ui.cursor(), 2);
        ui.scroll_to_cursor(3, false);
        let before_back_1 = ui.before;
        ui.scroll_to_cursor(3, false);
        let before_back_2 = ui.before;
        assert_eq!(before_back_1, before_back_2);
    }

    #[test]
    fn recenter_left_scroll_edge_moves_cursor_to_half_width() {
        let mut ui = InputUI::new();
        ui.width = 10;
        ui.set("abcdefghijklmnopqrstuvwxyz".to_string(), 17);
        ui.before = 17; // cursor is at the left edge

        ui.scroll_to_cursor(0, true);

        // The cursor is recentered instead of staying at the left edge.
        assert_eq!(ui.cursor_rel_offset(), 5);
        assert_eq!(ui.before, ui.cursor() as usize - 5);

        // Repeated calls after recentering are idempotent.
        let before = ui.before;
        ui.scroll_to_cursor(0, true);
        assert_eq!(ui.before, before);
    }
}
