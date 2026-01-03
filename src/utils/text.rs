use std::borrow::Cow;

use log::error;
use ratatui::text::{Line, Span, Text};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn plain_text(text: &Text) -> String {
    text.lines
        .iter()
        .map(|line| line.iter().map(|s| s.content.as_ref()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

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

pub fn grapheme_index_to_byte_index(input: &str, grapheme_index: u16) -> usize {
    let offsets: Vec<usize> = input.grapheme_indices(true).map(|(i, _)| i).collect();

    *offsets.get(grapheme_index as usize).unwrap_or(&input.len())
}

pub fn substitute_escaped(input: &str, map: &[(char, &str)]) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some(&'\\') => {
                    out.push('\\');
                    chars.next();
                }
                Some(&k) => {
                    if let Some(&(_, replacement)) = map.iter().find(|(key, _)| *key == k) {
                        out.push_str(replacement);
                        chars.next();
                    } else {
                        out.push('\\');
                        out.push(k);
                        chars.next();
                    }
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

pub fn fit_width(input: &str, width: usize) -> String {
    let mut s: String = input.chars().take(width).collect(); // truncate
    let len = s.chars().count();
    if len < width {
        s.extend(std::iter::repeat_n(' ', width - len));
    }
    s
}

pub fn left_pad(text: &str, pad: usize) -> String {
    let padding = " ".repeat(pad);
    text.lines()
        .map(|line| format!("{}{}", padding, line))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_escapes(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('0') => out.push('\0'),
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some('\'') => out.push('\''),
                Some('x') => {
                    // hex byte e.g. \x1b
                    let h1 = chars.next();
                    let h2 = chars.next();
                    if let (Some(h1), Some(h2)) = (h1, h2)
                        && let Ok(v) = u8::from_str_radix(&format!("{h1}{h2}"), 16)
                    {
                        out.push(v as char);
                        continue;
                    }
                    out.push_str("\\x"); // fallback
                }
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }

    out
}

pub fn wrap_text<'a>(text: Text<'a>, max_width: u16) -> (Text<'a>, bool) {
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
                    current_line_spans.push(Span::raw("↵"));
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
}
