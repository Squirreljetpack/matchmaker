use std::borrow::Cow;

use ratatui::text::{Line, Span, Text};
use unicode_segmentation::UnicodeSegmentation;

pub fn plain_text(text: &Text) -> String {
    text.lines
    .iter()
    .map(|line| line.iter().map(|s| s.content.as_ref()).collect::<String>())
    .collect::<Vec<_>>()
    .join("\n")
}

pub fn prefix_text<'a, 'b: 'a>(original: &'a mut Text<'b>, prefix: impl Into<Cow<'b, str>> + Clone) {
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

pub fn clip_text_lines<'a, 'b: 'a>(
    original: &'a mut Text<'b>,
    max_lines: u16,
    reverse: bool,
) {
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
        original
            .lines
            .iter()
            .take(max)
            .cloned()
            .collect()
    };

    *original = Text::from(new_lines);
}

pub fn grapheme_index_to_byte_index(input: &str, grapheme_index: u16) -> usize {
    let offsets: Vec<usize> = input.grapheme_indices(true)
        .map(|(i, _)| i)
        .collect();
        
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
        s.extend(std::iter::repeat(' ').take(width - len));
    }
    s
}