use cba::bring::split::split_on_nesting;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    config::{RowConnectionStyle, StatusConfig},
    ui::ResultsUI,
    utils::{string::substitute_escaped, text::expand_indents},
};

pub struct StatusUI {
    pub status_config: StatusConfig,
    pub status_template: Line<'static>,
    pub dim: Option<bool>,
}

impl StatusUI {
    pub fn new(status_config: StatusConfig) -> Self {
        let mut ret = Self {
            status_template: Line::from(status_config.template.clone()).style(status_config.style),
            status_config,
            dim: None,
        };
        ret.init();
        ret
    }

    pub fn init(&mut self) {
        self.status_config.interactions.sort_by_key(|(i, _)| *i);
    }

    pub fn make_status(&self, results_ui: &ResultsUI, full_width: u16) -> Paragraph<'_> {
        let status_config = &self.status_config;
        let replacements = [
            ('r', results_ui.index().to_string()),
            ('m', results_ui.status.matched_count.to_string()),
            ('t', results_ui.status.item_count.to_string()),
        ];

        // sub replacements into line
        let mut new_spans = Vec::new();

        if status_config.match_indent {
            new_spans.push(Span::raw(" ".repeat(results_ui.indentation())));
        }

        for span in &self.status_template {
            let subbed = substitute_escaped(&span.content, &replacements);
            new_spans.push(Span::styled(subbed, span.style));
        }

        let substituted_line = Line::from(new_spans);

        // sub whitespace expansions
        let effective_width = match self.status_config.row_connection {
            RowConnectionStyle::Full => full_width,
            _ => results_ui.width(),
        } as usize;

        let mut style = Style::from(status_config.style);
        if let Some(s) = self.dim {
            if s {
                style = style.add_modifier(Modifier::DIM);
            } else {
                style = style.remove_modifier(Modifier::DIM);
            }
        }

        let expanded = expand_indents(substituted_line, r"\s", r"\S", effective_width).style(style);

        Paragraph::new(expanded)
    }

    /// The style from the config overrides the Line style (but not the span styles).
    /// None restores the prompt defined in the config.
    pub fn set(&mut self, template: Option<Line<'static>>) {
        let status_config = &self.status_config;
        log::trace!("status line: {template:?}");

        self.status_template = template
            .unwrap_or(status_config.template.clone().into())
            .style(status_config.style)
            .into()
    }

    pub fn parse_template_to_status_line(s: &str) -> Line<'static> {
        let parts = match split_on_nesting(&s, ['{', '}']) {
            Ok(x) => x,
            Err(n) => {
                if n > 0 {
                    log::error!("Encountered {} unclosed parentheses", n)
                } else {
                    log::error!("Extra closing parenthesis at index {}", -n)
                }
                return Line::from(s.to_string());
            }
        };

        let mut spans = Vec::new();
        let mut in_nested = !s.starts_with('{');
        for part in parts {
            in_nested = !in_nested;
            let content = part.as_str();

            if in_nested {
                let inner = &content[1..content.len() - 1];

                // perform replacement fg:content
                spans.push(Self::span_from_template(inner));
            } else {
                spans.push(Span::raw(content.to_string()));
            }
        }

        Line::from(spans)
    }

    /// Converts a template string into a `Span` with colors and modifiers.
    ///
    /// The template string format is:
    /// ```text
    /// "style1,style2,...:text"
    /// ```
    /// - The **first valid color** token is used as foreground (fg).
    /// - The **second valid color** token is used as background (bg).
    /// - Remaining tokens are interpreted as **modifiers**: bold, dim, italic, underlined,
    ///   slow_blink, rapid_blink, reversed, hidden, crossed_out.
    /// - Empty tokens are ignored.
    /// - Unrecognized tokens are collected and logged once at the end.
    ///
    /// # Examples
    ///
    /// ```
    /// use matchmaker::ui::StatusUI;
    /// StatusUI::span_from_template("red,bg=blue,bold,italic:Hello");
    /// StatusUI::span_from_template("green,,underlined:World");
    /// StatusUI::span_from_template(",,dim:OnlyDim");
    /// ```
    ///
    /// Returns a `Span` with the specified styles applied to the text.
    pub fn span_from_template(inner: &str) -> Span<'static> {
        use std::str::FromStr;

        let (style_part, text) = inner.split_once(':').unwrap_or(("", inner));

        let mut style = Style::default();
        let mut fg_set = false;
        let mut bg_set = false;
        let mut unknown_tokens = Vec::new();

        for token in style_part.split(',') {
            let token = token.trim();
            if token.is_empty() {
                fg_set = true;
                continue;
            }

            if !fg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.fg(color);
                    fg_set = true;
                    continue;
                }
            }

            if !bg_set {
                if let Ok(color) = Color::from_str(token) {
                    style = style.bg(color);
                    bg_set = true;
                    continue;
                }
            }

            match token.to_lowercase().as_str() {
                "bold" => {
                    style = style.add_modifier(Modifier::BOLD);
                }
                "dim" => {
                    style = style.add_modifier(Modifier::DIM);
                }
                "italic" => {
                    style = style.add_modifier(Modifier::ITALIC);
                }
                "underlined" => {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                "slow_blink" => {
                    style = style.add_modifier(Modifier::SLOW_BLINK);
                }
                "rapid_blink" => {
                    style = style.add_modifier(Modifier::RAPID_BLINK);
                }
                "reversed" => {
                    style = style.add_modifier(Modifier::REVERSED);
                }
                "hidden" => {
                    style = style.add_modifier(Modifier::HIDDEN);
                }
                "crossed_out" => {
                    style = style.add_modifier(Modifier::CROSSED_OUT);
                }
                _ => {
                    if let Some(color_str) = token.strip_prefix("bg=") {
                        if let Ok(color) = Color::from_str(color_str) {
                            style = style.bg(color);
                            bg_set = true;
                        } else {
                            unknown_tokens.push(token.to_string());
                        }
                    } else if let Some(color_str) = token.strip_prefix("fg=") {
                        if let Ok(color) = Color::from_str(color_str) {
                            style = style.fg(color);
                            fg_set = true;
                        } else {
                            unknown_tokens.push(token.to_string());
                        }
                    } else {
                        unknown_tokens.push(token.to_string());
                    }
                }
            };
        }

        if !unknown_tokens.is_empty() {
            log::warn!(
                "Unknown style tokens in StatusUI template: {:?}",
                unknown_tokens
            );
        }

        Span::styled(text.to_string(), style)
    }
}
