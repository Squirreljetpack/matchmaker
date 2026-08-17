//! Crossterm styling and text utilities for Ratatui types.

use crossterm::style::{Attribute, Attributes, Color as CColor, ContentStyle};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Span, Text},
};

/// Convert a Ratatui [`Color`] into a Crossterm [`crossterm::style::Color`].
pub fn color_to_crossterm(color: Color) -> Option<CColor> {
    match color {
        Color::Reset => Some(CColor::Reset),
        Color::Black => Some(CColor::Black),
        Color::Red => Some(CColor::DarkRed),
        Color::Green => Some(CColor::DarkGreen),
        Color::Yellow => Some(CColor::DarkYellow),
        Color::Blue => Some(CColor::DarkBlue),
        Color::Magenta => Some(CColor::DarkMagenta),
        Color::Cyan => Some(CColor::DarkCyan),
        Color::Gray => Some(CColor::Grey),
        Color::DarkGray => Some(CColor::DarkGrey),
        Color::LightRed => Some(CColor::Red),
        Color::LightGreen => Some(CColor::Green),
        Color::LightYellow => Some(CColor::Yellow),
        Color::LightBlue => Some(CColor::Blue),
        Color::LightMagenta => Some(CColor::Magenta),
        Color::LightCyan => Some(CColor::Cyan),
        Color::White => Some(CColor::White),
        Color::Rgb(r, g, b) => Some(CColor::Rgb { r, g, b }),
        Color::Indexed(i) => Some(CColor::AnsiValue(i)),
    }
}

/// Convert a Ratatui [`Style`] into a Crossterm [`ContentStyle`].
pub fn style_to_crossterm(style: Style) -> ContentStyle {
    let mut cs = ContentStyle::default();
    if let Some(fg) = style.fg {
        cs.foreground_color = color_to_crossterm(fg);
    }
    if let Some(bg) = style.bg {
        cs.background_color = color_to_crossterm(bg);
    }
    if let Some(u) = style.underline_color {
        cs.underline_color = color_to_crossterm(u);
    }
    let mut attrs = Attributes::default();
    if style.add_modifier.contains(Modifier::BOLD) {
        attrs.set(Attribute::Bold);
    }
    if style.add_modifier.contains(Modifier::DIM) {
        attrs.set(Attribute::Dim);
    }
    if style.add_modifier.contains(Modifier::ITALIC) {
        attrs.set(Attribute::Italic);
    }
    if style.add_modifier.contains(Modifier::UNDERLINED) {
        attrs.set(Attribute::Underlined);
    }
    if style.add_modifier.contains(Modifier::SLOW_BLINK)
        || style.add_modifier.contains(Modifier::RAPID_BLINK)
    {
        attrs.set(Attribute::SlowBlink);
    }
    if style.add_modifier.contains(Modifier::REVERSED) {
        attrs.set(Attribute::Reverse);
    }
    if style.add_modifier.contains(Modifier::HIDDEN) {
        attrs.set(Attribute::Hidden);
    }
    if style.add_modifier.contains(Modifier::CROSSED_OUT) {
        attrs.set(Attribute::CrossedOut);
    }
    cs.attributes = attrs;
    cs
}

/// Convert a Ratatui [`Span`] into an ANSI-escaped string using Crossterm styling.
pub fn span_to_ansi(span: &Span<'_>) -> String {
    if span.style == Style::default() {
        return span.content.to_string();
    }
    let crossterm_style = style_to_crossterm(span.style);
    crossterm_style.apply(&span.content).to_string()
}

/// Convert a Ratatui [`Text`] into an ANSI-escaped multi-line string.
pub fn text_to_ansi(text: &Text<'_>) -> String {
    text.lines
        .iter()
        .map(|line| {
            let line_style = text.style.patch(line.style);
            let mut line_str = line
                .spans
                .iter()
                .map(|span| {
                    let mut s = span.clone();
                    s.style = line_style.patch(s.style);
                    span_to_ansi(&s)
                })
                .collect::<String>();
            if line_str.ends_with('\n') {
                line_str.pop();
                if line_str.ends_with('\r') {
                    line_str.pop();
                }
            }
            line_str
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::text::Line;

    #[test]
    fn test_text_to_ansi() {
        let text = Text::from(vec![
            Line::from(vec![
                Span::styled("Red ", Style::default().fg(Color::Red)),
                Span::styled("Bold", Style::default().add_modifier(Modifier::BOLD)),
            ]),
            Line::from("Plain"),
        ]);
        let ansi = text_to_ansi(&text);
        assert_eq!(
            ansi,
            "\u{1b}[38;5;1mRed \u{1b}[39m\u{1b}[1mBold\u{1b}[0m\nPlain"
        );
    }
}
