use cba::bring::split::split_on_nesting;
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::{
    config::{RowConnectionStyle, StatusConfig, StatusInteractionSetting},
    ui::ResultsUI,
    utils::{string::substitute_escaped, text::expand_indents},
};

pub struct StatusUI {
    pub status_config: StatusConfig,
    pub status_template: Line<'static>,
    pub dim: Option<bool>,
    resolved_interactions: crate::config::ResolvedInteractionRegionSetting,
}

impl StatusUI {
    pub fn new(status_config: StatusConfig) -> Self {
        let mut ret = Self {
            status_template: Self::parse_template_to_status_line(&status_config.template)
                .style(status_config.style),
            status_config,
            dim: None,
            resolved_interactions: Vec::new(),
        };
        ret.init();
        ret
    }

    pub fn init(&mut self) {
        match &mut self.status_config.interactions {
            StatusInteractionSetting::Regions(regions) => {
                regions.sort_by_key(|(start, _)| *start);
                self.resolved_interactions = regions
                    .iter()
                    .map(|(start, action)| (u16::from(*start), action.clone()))
                    .collect();
            }
            StatusInteractionSetting::Actions(_) => self.resolved_interactions.clear(),
        }
    }

    pub fn make_status(&mut self, results_ui: &ResultsUI, full_width: u16) -> Paragraph<'_> {
        let replacements = [
            ('r', results_ui.index().to_string()),
            ('m', results_ui.status.matched_count.to_string()),
            ('t', results_ui.status.item_count.to_string()),
        ];

        let mut new_spans = Vec::new();

        if self.status_config.match_indent {
            new_spans.push(Span::raw(" ".repeat(results_ui.indentation())));
        }

        for span in &self.status_template {
            let subbed = substitute_escaped(&span.content, &replacements);
            new_spans.push(Span::styled(subbed, span.style));
        }

        let substituted_line = Line::from(new_spans);
        let effective_width = match self.status_config.row_connection {
            RowConnectionStyle::Full => full_width,
            _ => results_ui.width(),
        } as usize;

        let mut style = Style::from(self.status_config.style);
        if let Some(s) = self.dim {
            if s {
                style = style.add_modifier(Modifier::DIM);
            } else {
                style = style.remove_modifier(Modifier::DIM);
            }
        }

        let mut expanded =
            expand_indents(substituted_line, r"\s", r"\S", effective_width).style(style);
        self.resolve_interactions(&mut expanded);

        Paragraph::new(expanded)
    }

    fn resolve_interactions(&mut self, line: &mut Line<'static>) {
        let actions = match &self.status_config.interactions {
            StatusInteractionSetting::Actions(actions) => Some(actions),
            StatusInteractionSetting::Regions(_) => None,
        };
        let mut actions = actions.into_iter().flatten();
        let mut x = 0u16;
        let mut regions = Vec::new();

        for span in &mut line.spans {
            let width = u16::try_from(span.width()).unwrap_or(u16::MAX);
            if span.style.add_modifier.contains(Modifier::RAPID_BLINK) {
                span.style = span.style.remove_modifier(Modifier::RAPID_BLINK);
                if let Some(action) = actions.next() {
                    regions.push((x, action.clone()));
                    regions.push((x.saturating_add(width), String::new()));
                }
            }
            x = x.saturating_add(width);
        }

        if matches!(
            self.status_config.interactions,
            StatusInteractionSetting::Actions(_)
        ) {
            self.resolved_interactions = regions;
        }
    }

    pub fn interactions(&self) -> &crate::config::ResolvedInteractionRegionSetting {
        &self.resolved_interactions
    }
    /// The style from the config overrides the Line style (but not the span styles).
    /// None restores the prompt defined in the config.
    pub fn set(&mut self, template: Option<Line<'static>>) {
        let status_config = &self.status_config;
        log::trace!("status line: {template:?}");

        self.status_template = template
            .unwrap_or_else(|| Self::parse_template_to_status_line(&status_config.template))
            .style(status_config.style)
    }

    pub fn parse_template_to_status_line(s: &str) -> Line<'static> {
        let parts = match split_on_nesting(s, ['{', '}']) {
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

    /// Converts a template section into a styled status span.
    ///
    /// `i` and `interactive` mark the span as an interaction region. The
    /// marker uses `Modifier::RAPID_BLINK` internally and is removed before
    /// rendering.

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

            if matches!(token.to_lowercase().as_str(), "i" | "interactive") {
                style = style.add_modifier(Modifier::RAPID_BLINK);
                continue;
            }

            if !fg_set && let Ok(color) = Color::from_str(token) {
                style = style.fg(color);
                fg_set = true;
                continue;
            }

            if !bg_set && let Ok(color) = Color::from_str(token) {
                style = style.bg(color);
                bg_set = true;
                continue;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ResultsConfig, StatusInteractionSetting};
    use ratatui::{Terminal, backend::TestBackend};

    fn render_status(status: &mut StatusUI, results: &ResultsUI, width: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
        terminal
            .draw(|frame| {
                let widget = status.make_status(results, width);
                frame.render_widget(widget, frame.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        (0..width)
            .map(|x| buffer[(x, 0)].symbol())
            .collect::<String>()
    }

    #[test]
    fn resolves_interactive_spans_after_dynamic_expansion() {
        let mut status = StatusUI::new(StatusConfig {
            show: true,
            match_indent: false,
            template: r#"{i:prefix}\s{interactive:action}"#.to_string(),
            interactions: StatusInteractionSetting::Actions(vec![
                "prefix".to_string(),
                "action".to_string(),
            ]),
            ..Default::default()
        });
        let results = ResultsUI::new(ResultsConfig::default());

        assert_eq!(
            render_status(&mut status, &results, 20),
            "prefix        action"
        );
        assert_eq!(
            status.interactions(),
            &vec![
                (0, "prefix".to_string()),
                (6, String::new()),
                (14, "action".to_string()),
                (20, String::new()),
            ]
        );

        let click = |x| crate::render::find_interaction(status.interactions(), x);
        assert_eq!(click(0).as_deref(), Some("prefix"));
        assert_eq!(click(5).as_deref(), Some("prefix"));
        assert_eq!(click(6), None);
        assert_eq!(click(13), None);
        assert_eq!(click(14).as_deref(), Some("action"));
        assert_eq!(click(19).as_deref(), Some("action"));
    }

    #[test]
    fn static_regions_remain_available() {
        let status = StatusUI::new(StatusConfig {
            interactions: StatusInteractionSetting::Regions(vec![
                (2, "first".to_string()),
                (8, "second".to_string()),
            ]),
            ..Default::default()
        });

        assert_eq!(
            status.interactions(),
            &vec![(2, "first".to_string()), (8, "second".to_string())]
        );
    }

    #[test]
    fn parser_marks_interactive_aliases() {
        let line = StatusUI::parse_template_to_status_line("{i:a} {interactive:b}");

        let marked: Vec<_> = line
            .spans
            .iter()
            .filter(|span| span.style.add_modifier.contains(Modifier::RAPID_BLINK))
            .collect();
        assert_eq!(
            marked
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }
}
