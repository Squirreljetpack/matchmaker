//! Ratatui text utils

use std::{borrow::Cow, ops::Range};

use log::error;
use ratatui::{
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn plain_text(text: &Text) -> String {
    text.lines
        .iter()
        .map(|line| line.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

use std::cmp::{max, min};
#[allow(unused)]
pub fn apply_style_at(mut text: Text<'_>, start: usize, len: usize, style: Style) -> Text<'_> {
    let mut global_pos = 0;
    let end = start + len;

    for line in text.lines.iter_mut() {
        let mut new_spans = Vec::new();
        // Take the spans to avoid borrow checker issues while rebuilding the Vec
        let old_spans = std::mem::take(&mut line.spans);

        for span in old_spans {
            let content = span.content.as_ref();
            let span_chars: Vec<char> = content.chars().collect();
            let span_len = span_chars.len();
            let span_end = global_pos + span_len;

            // Check if the current span overlaps with the [start, end) range
            if global_pos < end && span_end > start {
                // Calculate local overlap boundaries relative to this span
                let local_start = max(0, start as isize - global_pos as isize) as usize;
                let local_end = min(span_len, end - global_pos);

                // 1. Part before the styled range
                if local_start > 0 {
                    new_spans.push(Span::styled(
                        span_chars[0..local_start].iter().collect::<String>(),
                        span.style,
                    ));
                }

                // 2. The styled part (patch the existing style with the new one)
                let styled_part: String = span_chars[local_start..local_end].iter().collect();
                new_spans.push(Span::styled(styled_part, span.style.patch(style)));

                // 3. Part after the styled range
                if local_end < span_len {
                    new_spans.push(Span::styled(
                        span_chars[local_end..span_len].iter().collect::<String>(),
                        span.style,
                    ));
                }
            } else {
                // No overlap, keep the span as is
                new_spans.push(span);
            }

            global_pos += span_len;
        }
        line.spans = new_spans;

        // Ratatui Lines are usually separated by a newline in the buffer.
        // If you treat Text as a continuous string, increment for the '\n'.
        global_pos += 1;
    }

    text
}

/// Add a prefix to all lines of the original text
pub fn prefix_text<'a, 'b: 'a>(
    original: &'a mut Text<'b>,
    prefix: impl Into<Cow<'b, str>> + Clone,
) {
    let new_lines: Vec<Line> = original
        .lines
        .iter()
        .map(|line| {
            let mut new_line = vec![Span::raw(prefix.clone())];
            new_line.extend(line.iter().cloned());
            Line::from(new_line)
        })
        .collect();

    *original = Text::from(new_lines);
}

pub fn clip_text_lines<'a, 'b: 'a>(original: &'a mut Text<'b>, max_lines: u16, reverse: bool) {
    let max = max_lines as usize;

    let new_lines: Vec<Line> = if reverse {
        // take the last `max` lines
        original
            .lines
            .iter()
            .rev()
            .take(max)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .cloned()
            .collect()
    } else {
        // take the first `max` lines
        original.lines.iter().take(max).cloned().collect()
    };

    *original = Text::from(new_lines);
}

pub fn wrapped_line_height(line: &Line<'_>, width: u16) -> u16 {
    let w = line.width();
    if w == 0 {
        1
    } else {
        w.div_ceil(width as usize) as u16
    }
}

pub fn wrap_text<'a>(text: Text<'a>, max_width: u16) -> (Text<'a>, bool) {
    // todo: lowpri: refactor to support configuring
    let wrapping_span = Span::raw("↵").fg(Color::DarkGray).dim();

    if max_width == 0 {
        return (text, false);
    }
    if max_width <= 1 {
        error!("Invalid width for text: {text:?}");
        return (text, false);
    }

    let mut new_lines = Vec::new();
    let mut wrapped = false;

    for line in text.lines {
        let mut current_line_spans = Vec::new();
        let mut current_line_width = 0;

        if line.spans.is_empty() {
            new_lines.push(line);
            continue;
        }

        for span in line.spans {
            let graphemes: Vec<&str> = span.content.graphemes(true).collect();
            let mut current_grapheme_start_idx = 0;

            while current_grapheme_start_idx < graphemes.len() {
                let mut graphemes_in_chunk = 0;

                for (i, grapheme) in graphemes
                    .iter()
                    .skip(current_grapheme_start_idx)
                    .enumerate()
                {
                    let grapheme_width = UnicodeWidthStr::width(*grapheme);

                    if current_line_width + grapheme_width > (max_width - 1) as usize {
                        let is_last_in_span = current_grapheme_start_idx + i + 1 == graphemes.len();
                        if !is_last_in_span {
                            break;
                        }
                    }

                    current_line_width += grapheme_width;
                    graphemes_in_chunk += 1;
                }

                if graphemes_in_chunk > 0 {
                    let chunk_end_idx = current_grapheme_start_idx + graphemes_in_chunk;
                    let chunk_content =
                        graphemes[current_grapheme_start_idx..chunk_end_idx].concat();
                    current_line_spans.push(Span::styled(chunk_content, span.style));
                    current_grapheme_start_idx += graphemes_in_chunk;
                }

                if current_grapheme_start_idx < graphemes.len() {
                    // line wrapped
                    wrapped = true;
                    current_line_spans.push(wrapping_span.clone());
                    new_lines.push(Line::from(current_line_spans));
                    current_line_spans = Vec::new();
                    current_line_width = 0;
                }
            }
        }

        if !current_line_spans.is_empty() {
            new_lines.push(Line::from(current_line_spans));
        }
    }

    (Text::from(new_lines), wrapped)
}

/// Convert `Text` into lines of plain `String`s
pub fn text_to_lines(text: &Text) -> Vec<String> {
    text.iter()
        .map(|spans| {
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect()
}

/// Convert `Text` into a single `String` with newlines
pub fn text_to_string(text: &Text) -> String {
    text_to_lines(text).join("\n")
}

/// Helper function to slice a `ratatui::text::Text` based on global byte indices,
/// assuming lines were virtually joined with a single `\n` (1 byte).
pub fn slice_ratatui_text<'a>(text: &'a Text<'_>, range: Range<usize>) -> Text<'a> {
    if range.start == range.end {
        return Text::default();
    }

    let mut result_lines = Vec::new();
    let mut current_line_spans = Vec::new();

    let mut current_byte_idx = 0;
    let mut started_capturing = false;

    let num_lines = text.lines.len();

    for (line_idx, line) in text.lines.iter().enumerate() {
        for span in &line.spans {
            let span_bytes = span.content.len();
            let span_end = current_byte_idx + span_bytes;

            if span_end > range.start {
                started_capturing = true;

                let overlap_start = current_byte_idx.max(range.start);
                let overlap_end = span_end.min(range.end);

                let local_start = overlap_start - current_byte_idx;
                let local_end = overlap_end - current_byte_idx;

                let sliced_content = &span.content[local_start..local_end];

                current_line_spans.push(Span::styled(sliced_content, span.style));
            }

            current_byte_idx = span_end;

            if current_byte_idx >= range.end {
                break;
            }
        }

        if line_idx < num_lines - 1 {
            if current_byte_idx >= range.start {
                started_capturing = true;
                result_lines.push(Line::from(std::mem::take(&mut current_line_spans)));
            }

            current_byte_idx += 1; // Advance 1 byte for the '\n'

            if current_byte_idx >= range.end {
                started_capturing = false;
                break;
            }
        }
    }

    // 3. Flush remaining
    if started_capturing {
        result_lines.push(Line::from(current_line_spans));
    }

    Text::from(result_lines)
}

/// Cleans a Text object by removing explicit 'Reset' colors and 'Not' modifiers.
/// This allows the Text to properly inherit styles from its parent container.
pub fn scrub_text_styles(text: &mut Text<'_>) {
    for line in &mut text.lines {
        for span in &mut line.spans {
            // 1. Handle Colors: If it's explicitly Reset, make it None (transparent/inherit)
            if span.style.fg == Some(Color::Reset) {
                span.style.fg = None;
            }
            if span.style.bg == Some(Color::Reset) {
                span.style.bg = None;
            }
            if span.style.underline_color == Some(Color::Reset) {
                span.style.underline_color = None;
            }

            span.style.sub_modifier = Modifier::default();
        }
    }
}

/// Expand `placeholder` inside a Line and distribute spaces to reach `target_width`.
pub fn expand_indents<'a>(input: Line<'a>, placeholder: &str, target_width: usize) -> Line<'a> {
    let mut count = 0;
    let mut base_width = 0;

    // Compute display width excluding placeholders
    for span in &input.spans {
        count += span.content.matches(placeholder).count();
        for segment in span.content.split(placeholder) {
            base_width += segment.width();
        }
    }

    // No placeholders, return a fully owned version of the original line
    if count == 0 {
        let owned_spans: Vec<Span<'static>> = input
            .spans
            .iter()
            .map(|span| Span::styled(span.content.to_string(), span.style))
            .collect();
        return Line::from(owned_spans);
    }

    // If we exceed or meet the target width, just strip the placeholders
    if base_width >= target_width {
        let new_spans: Vec<Span<'static>> = input
            .spans
            .iter()
            .map(|span| {
                let new_content = span.content.replace(placeholder, "");
                Span::styled(new_content, span.style) // String becomes Cow::Owned
            })
            .collect();
        return Line::from(new_spans);
    }

    let total_spaces = target_width - base_width;
    let per = total_spaces / count;
    let mut remainder = total_spaces % count;

    let mut new_spans = Vec::new();

    for span in input.spans {
        // If this span doesn't have the placeholder, clone it as an owned String
        if !span.content.contains(placeholder) {
            new_spans.push(Span::styled(span.content.to_string(), span.style));
            continue;
        }

        let mut new_content = String::new();
        let mut parts = span.content.split(placeholder).peekable();

        while let Some(part) = parts.next() {
            // Safely push the segment (preserves graphemes naturally)
            new_content.push_str(part);

            // Add the distributed spaces if there is a next part
            if parts.peek().is_some() {
                let extra = if remainder > 0 {
                    remainder -= 1;
                    1
                } else {
                    0
                };

                new_content.push_str(&" ".repeat(per + extra));
            }
        }

        // Reconstruct the span with the expanded String content (becomes Cow::Owned)
        new_spans.push(Span::styled(new_content, span.style));
    }

    Line::from(new_spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_wrap_needed() {
        let text = Text::from(Line::from("abc"));
        let (wrapped_text, wrapped) = wrap_text(text, 10);
        assert!(!wrapped);
        assert_eq!(wrapped_text.lines.len(), 1);
        assert_eq!(wrapped_text.lines[0].spans[0].content, "abc");
    }

    #[test]
    fn test_simple_wrap() {
        let text = Text::from(Line::from("abcdef"));
        let (wrapped_text, wrapped) = wrap_text(text, 4);
        assert!(wrapped);
        assert_eq!(wrapped_text.lines.len(), 2);
        assert_eq!(wrapped_text.lines[0].spans.last().unwrap().content, "↵");
    }

    #[test]
    fn test_multiline_input_preserved() {
        let text = Text::from(vec![Line::from("abc"), Line::from("defghij")]);
        let (wrapped_text, wrapped) = wrap_text(text, 5);
        assert!(wrapped);
        assert_eq!(wrapped_text.lines.len(), 3);
        assert_eq!(wrapped_text.lines[0].spans[0].content, "abc");
    }

    #[test]
    fn test_handles_empty_line() {
        let text = Text::from(vec![Line::from(""), Line::from("abc")]);
        let (wrapped_text, wrapped) = wrap_text(text, 3);
        assert!(!wrapped);
        assert_eq!(wrapped_text.lines.len(), 2);
        assert!(wrapped_text.lines[0].spans.is_empty());
    }

    #[test]
    fn test_unicode_emoji_width() {
        let text = Text::from(Line::from("🙂🙂🙂"));
        let (wrapped_text, wrapped) = wrap_text(text, 4); // each emoji width=2
        assert!(wrapped);
        assert!(wrapped_text.lines.len() > 1);
    }

    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span, Text};

    #[test]
    fn test_apply_style_multiline_partial_spans() {
        // Construct a Text with 3 lines, each with multiple spans
        let text = Text::from_iter([
            // 12
            Line::from(vec![
                Span::raw("Hello".to_string()),
                Span::styled(", ".to_string(), Style::default().fg(Color::Green)),
                Span::raw("world".to_string()),
            ]),
            // 14
            Line::from(vec![
                Span::raw("This ".to_string()),
                Span::styled("is ".to_string(), Style::default().bg(Color::Yellow)),
                Span::raw("line 2".to_string()),
            ]),
            Line::from(vec![
                Span::raw("Line ".to_string()),
                Span::styled("three".to_string(), Style::default().fg(Color::Cyan)),
                Span::raw(" ends here".to_string()),
            ]),
        ]);

        // Apply a red style from line 1 to the first 2 (3 + 27 - (26 + 2)) chars of line 3.
        let styled_text = apply_style_at(text, 3, 27, Style::default().fg(Color::Red));

        // Build the expected spans manually
        let expected_spans = [
            // Line 1
            vec![
                Span::raw("Hel".to_string()),
                Span::styled("lo".to_string(), Style::default().fg(Color::Red)),
                Span::styled(", ".to_string(), Style::default().fg(Color::Red)),
                Span::styled("world".to_string(), Style::default().fg(Color::Red)), // continues styled into next span
            ],
            // Line 2
            vec![
                Span::styled("This ".to_string(), Style::default().fg(Color::Red)),
                Span::styled(
                    "is ".to_string(),
                    Style::default().bg(Color::Yellow).fg(Color::Red), //merge
                ),
                Span::styled("line 2".to_string(), Style::default().fg(Color::Red)),
            ],
            // Line 3
            vec![
                Span::styled("Li".to_string(), Style::default().fg(Color::Red)),
                Span::styled("ne ".to_string(), Style::default()),
                Span::styled("three".to_string(), Style::default().fg(Color::Cyan)),
                Span::raw(" ends here".to_string()),
            ],
        ];

        assert_eq!(styled_text, Text::from_iter(expected_spans));
    }

    // ------------------------------------------------------------------------
    // Helper to generate a multi-styled, multi-line Ratatui Text object.
    // Equivalent string when joined with \n: "Hello World\nRust🦀"
    // Byte offsets:
    // "Hello " (6) + "World" (5) = 11 bytes.
    // "\n" = 1 byte.
    // "Rust🦀" (4 + 4) = 8 bytes.
    // Total = 20 bytes.
    fn sample_text() -> Text<'static> {
        Text::from(vec![
            Line::from(vec![
                Span::styled("Hello ", Style::default().fg(Color::Red)),
                Span::styled("World", Style::default().fg(Color::Blue)),
            ]),
            Line::from(vec![Span::styled(
                "Rust🦀",
                Style::default().fg(Color::Green),
            )]),
        ])
    }

    #[test]
    fn test_slice_exact_span_boundary() {
        let text = sample_text();
        let sliced = slice_ratatui_text(&text, 0..6);

        let expected = Text::from(vec![Line::from(vec![Span::styled(
            "Hello ",
            Style::default().fg(Color::Red),
        )])]);
        assert_eq!(sliced, expected);
    }

    #[test]
    fn test_slice_across_spans() {
        let text = sample_text();
        // Slice "lo Wo"
        let sliced = slice_ratatui_text(&text, 3..9);

        let expected = Text::from(vec![Line::from(vec![
            Span::styled("lo ", Style::default().fg(Color::Red)),
            Span::styled("Wor", Style::default().fg(Color::Blue)),
        ])]);
        assert_eq!(sliced, expected);
    }

    #[test]
    fn test_slice_across_newline() {
        let text = sample_text();
        // Slice "World\nRus" -> byte indices 6 to 15
        let sliced = slice_ratatui_text(&text, 6..15);

        let expected = Text::from(vec![
            Line::from(vec![Span::styled(
                "World",
                Style::default().fg(Color::Blue),
            )]),
            Line::from(vec![Span::styled("Rus", Style::default().fg(Color::Green))]),
        ]);
        assert_eq!(sliced, expected);
    }

    #[test]
    fn test_slice_multi_byte_emoji() {
        let text = sample_text();
        // Slice just the crab emoji. "Rust" is 4 bytes, so emoji starts at index 12 + 4 = 16.
        // Emoji is 4 bytes, so it ends at 20.
        let sliced = slice_ratatui_text(&text, 16..20);

        let expected = Text::from(vec![Line::from(vec![Span::styled(
            "🦀",
            Style::default().fg(Color::Green),
        )])]);
        assert_eq!(sliced, expected);
    }

    #[test]
    fn test_slice_empty_range() {
        let text = sample_text();
        let sliced = slice_ratatui_text(&text, 5..5);
        assert_eq!(sliced, Text::default());
    }
}
