// Original code from https://github.com/helix-editor/helix (MPL 2.0)

use std::mem::take;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Line, Span, Style, Text};

use crate::{
    SSS,
    config::AutoscrollSettings,
    utils::text::{hscroll_indicator, wrapping_indicator},
};

/// Renders a single cell by applying match highlighting, wrapping, and hscroll clipping.
///
/// ### Mutations:
/// - `matcher`: Mutated internally for calculating match sub-span indices.
/// - `col_indices_buffer`: Mutated (cleared and refilled) as a reusable scratch vector to avoid allocations.
///
/// ### Returns:
/// - `(Text<'static>, usize)`: A tuple where the first element is the styled static `Text`, and the second is the calculated maximum visual width of the cell.
pub fn render_cell<T: SSS>(
    cell: Text<'_>,
    col_idx: usize,
    snapshot: &nucleo::Snapshot<T>,
    item: &nucleo::Item<T>,
    matcher: &mut nucleo::Matcher,
    highlight_style: Style,
    wrap: bool,
    width_limit: u16,
    col_indices_buffer: &mut Vec<u32>,
    mut autoscroll: AutoscrollSettings,
    hscroll_offset: i8,
) -> (Text<'static>, usize) {
    // Disable autoscrolling by default if text wrapping is enabled, as wrapping naturally pushes
    // text down to new lines instead of overflowing horizontally, unless 'always' is configured.
    if !autoscroll.always {
        autoscroll.enabled &= !wrap;
    }

    let mut cell_width = 0;
    let mut wrapped = false;

    // Step 1: Query match indices for this specific column from nucleo snapshot.
    // The indices tell us which character positions inside this column's text match the search query.
    let indices_buffer = col_indices_buffer;
    indices_buffer.clear();
    snapshot.pattern().column_pattern(col_idx).indices(
        item.matcher_columns[col_idx].slice(..),
        matcher,
        indices_buffer,
    );
    // Sort and remove duplicates to guarantee match indices are processed sequentially from left to right.
    indices_buffer.sort_unstable();
    indices_buffer.dedup();

    let mut indices = indices_buffer.drain(..);

    let mut lines = vec![];
    let mut next_highlight_idx = indices.next().unwrap_or(u32::MAX);
    let mut grapheme_idx = 0u32;

    let mut line_graphemes = Vec::new();

    // Iterate through each raw line of the cell.
    for line in &cell {
        // Step 2: Collect graphemes, compute styles, and find the relevant match on this line.
        line_graphemes.clear();
        let mut match_idx = None;

        for span in line {
            // We iterate graphemes but treat them as char indices. Nucleo matches on
            // grapheme boundaries structurally so this mapping remains correct.
            for grapheme in span.content.graphemes(true) {
                let is_match = grapheme_idx == next_highlight_idx;

                // Patch the span style with the highlight style if this grapheme is part of the query match.
                let style = if is_match {
                    next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                    span.style.patch(highlight_style)
                } else {
                    span.style
                };

                if is_match && (autoscroll.end || match_idx.is_none()) {
                    match_idx = Some(line_graphemes.len());
                }

                line_graphemes.push((grapheme, style));
                grapheme_idx += 1;
            }
        }

        // Step 3: Calculate where to start rendering this line (HScroll calculation)
        let mut i; // start_idx of the rendered slice

        if autoscroll.enabled && autoscroll.end {
            // Horizontal autoscrolling focused on the end of matches:
            // Shift the start index leftwards so the end match remains fully visible.
            i = match_idx.unwrap_or(line_graphemes.len().saturating_sub(1));

            let preserved_width = line_graphemes
                [..autoscroll.initial_preserved.min(line_graphemes.len())]
                .iter()
                .map(|(g, _)| g.width())
                .sum::<usize>();

            let target_width = if let Some(x) = match_idx {
                (width_limit as usize)
                    .saturating_sub(autoscroll.context.min(line_graphemes.len() - x - 1))
            } else {
                width_limit as usize
            }
            .saturating_sub(preserved_width);

            let mut current_width = 0;

            while i > autoscroll.initial_preserved {
                let w = line_graphemes[i].0.width();
                let indicator_width = if i - 1 > autoscroll.initial_preserved {
                    1
                } else {
                    0
                };

                if current_width + w + indicator_width < target_width {
                    i -= 1;
                    current_width += w;
                } else {
                    break;
                }
            }

            i = i.saturating_add_signed(hscroll_offset as isize);

            if i <= autoscroll.initial_preserved {
                i = 0;
            }
        } else if autoscroll.enabled
            && let Some(m_idx) = match_idx
        {
            // Horizontal autoscrolling with context around the match:
            // Shift index leftwards to show context preceding the match.
            i = (m_idx as i32 - autoscroll.context as i32).max(0) as usize;

            let mut tail_width: usize = line_graphemes[i..].iter().map(|(g, _)| g.width()).sum();

            let preserved_width = line_graphemes
                [..autoscroll.initial_preserved.min(line_graphemes.len())]
                .iter()
                .map(|(g, _)| g.width())
                .sum::<usize>();

            // Expand leftwards as long as the total rendered width <= width_limit
            while i > autoscroll.initial_preserved {
                let prev_width = line_graphemes[i - 1].0.width();
                // Only reserve space for "..." if we aren't reaching the very start
                let indicator_width = if i - 1 > autoscroll.initial_preserved {
                    1
                } else {
                    0
                };

                if tail_width + preserved_width + indicator_width + prev_width
                    <= width_limit as usize
                {
                    i -= 1;
                    tail_width += prev_width;
                } else {
                    break;
                }
            }

            i = i.saturating_add_signed(hscroll_offset as isize);

            if i <= autoscroll.initial_preserved {
                i = 0;
            }
        } else {
            // No autoscrolling triggered; shift start index directly by the manual hscroll offset.
            i = hscroll_offset.max(0) as usize;
        };

        // Step 4: Apply standard wrapping and Span generation logic to the visible slice
        let mut current_spans = Vec::new();
        let mut current_span = String::new();
        let mut current_style = Style::default();
        let mut current_width = 0;

        // If shifting occurred, prepend the initial preserved segment and hscroll indicator (...)
        if i > 0 && autoscroll.enabled {
            for (g, s) in
                line_graphemes.drain(..autoscroll.initial_preserved.min(line_graphemes.len()))
            {
                if s != current_style {
                    if !current_span.is_empty() {
                        current_spans.push(Span::styled(current_span, current_style));
                    }
                    current_span = String::new();
                    current_style = s;
                }
                current_span.push_str(g);
            }
            if !current_span.is_empty() {
                current_spans.push(Span::styled(current_span, current_style));
            }
            i -= autoscroll.initial_preserved;

            current_width += current_spans.iter().map(|x| x.width()).sum::<usize>();
            current_spans.push(hscroll_indicator());
            current_width += 1;

            current_span = String::new();
            current_style = Style::default();
        }

        // Prevent rendering lockups on empty/invisible cells.
        if !line_graphemes.is_empty() {
            cell_width = cell_width.max(1);
            i = i.min(line_graphemes.len())
        }

        let mut graphemes = line_graphemes.drain(i..);

        // Process remaining graphemes, wrapping or breaking when limits are exceeded.
        while let Some((mut grapheme, mut style)) = graphemes.next() {
            if current_width + grapheme.width() > width_limit as usize {
                if !current_span.is_empty() {
                    current_spans.push(Span::styled(current_span, current_style));
                    current_span = String::new();
                }
                if wrap {
                    current_spans.push(wrapping_indicator());
                    lines.push(Line::from(take(&mut current_spans)));

                    current_width = 0;
                    wrapped = true;
                } else {
                    break;
                }
            } else if current_width + grapheme.width() == width_limit as usize {
                if wrap {
                    let mut new = grapheme.to_string();
                    if current_style != style {
                        current_spans.push(Span::styled(take(&mut current_span), current_style));
                        current_style = style;
                    };
                    for (grapheme2, style2) in graphemes.by_ref() {
                        if grapheme2.width() == 0 {
                            new.push_str(grapheme2);
                        } else {
                            if !current_span.is_empty() {
                                current_spans.push(Span::styled(current_span, current_style));
                            }
                            current_spans.push(wrapping_indicator());
                            lines.push(Line::from(take(&mut current_spans)));

                            current_span = new.clone(); // rust can't tell that clone is unnecessary here
                            current_width = grapheme.width();
                            wrapped = true;

                            grapheme = grapheme2;
                            style = style2;
                            break; // continue normal processing
                        }
                    }
                    if !wrapped {
                        current_span.push_str(&new);
                        // we reached the end of the line exactly, end line
                        current_spans.push(Span::styled(take(&mut current_span), style));
                        current_style = style;
                        current_width += grapheme.width();
                        break;
                    }
                } else {
                    if style != current_style {
                        if !current_span.is_empty() {
                            current_spans.push(Span::styled(current_span, current_style));
                        }
                        current_span = String::new();
                        current_style = style;
                    }
                    current_span.push_str(grapheme);
                    current_width += grapheme.width();
                    break;
                }
            }

            if style != current_style {
                if !current_span.is_empty() {
                    current_spans.push(Span::styled(current_span, current_style))
                }
                current_span = String::new();
                current_style = style;
            }
            current_span.push_str(grapheme);
            current_width += grapheme.width();
        }

        current_spans.push(Span::styled(current_span, current_style));
        lines.push(Line::from(current_spans));
        cell_width = cell_width.max(current_width);

        grapheme_idx += 1; // newline boundary
    }

    (
        Text::from(lines),
        if wrapped {
            width_limit as usize
        } else {
            cell_width
        },
    )
}

#[cfg(test)]
mod tests {
    use crate::config::AutoscrollSettings;

    use super::*;
    use nucleo::{Matcher, Nucleo};
    use ratatui::style::{Color, Style};
    use ratatui::text::Text;
    use std::sync::Arc;

    /// Sets up the necessary Nucleo state to trigger a match
    fn setup_nucleo_mocks(
        search_query: &str,
        item_text: &str,
    ) -> (Nucleo<String>, Matcher, Vec<u32>) {
        let mut nucleo = Nucleo::<String>::new(nucleo::Config::DEFAULT, Arc::new(|| {}), None, 1);

        let injector = nucleo.injector();
        injector.push(item_text.to_string(), |item, columns| {
            columns[0] = item.clone().into();
        });

        nucleo.pattern.reparse(
            0,
            search_query,
            nucleo::pattern::CaseMatching::Ignore,
            nucleo::pattern::Normalization::Smart,
            false,
        );

        nucleo.tick(10); // Process the item

        let matcher = Matcher::default();
        let buffer = Vec::new();

        (nucleo, matcher, buffer)
    }

    #[test]
    fn test_no_scroll_context_renders_normally() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "hello match world");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("hello match world");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            u16::MAX,
            &mut buffer,
            AutoscrollSettings {
                enabled: false,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "hello match world");
        assert_eq!(width, 17);
    }

    #[test]
    fn test_scroll_context_cuts_prefix_correctly() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "hello match world");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("hello match world");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, _) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            u16::MAX,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 0,
                context: 2,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "hello match world");
    }

    #[test]
    fn test_scroll_context_backfills_to_fill_width_limit() {
        // Query "match". Starts at index 10.
        // "abcdefghijmatch"
        // autoscroll = Some((preserved=0, context=1))
        // initial_start_idx = 10 + 0 - 1 = 9 ("jmatch").
        // width_limit = 10.
        // tail_width ("jmatch") = 6.
        // Try to decrease start_idx.
        // start_idx=8 ("ijmatch"), tail_width=7.
        // start_idx=7 ("hijmatch"), tail_width=8.
        // start_idx=6 ("ghijmatch"), tail_width=9.
        // start_idx=5 ("fghijmatch"), tail_width=10.
        // start_idx=4 ("efghijmatch"), tail_width=11 > 10 (STOP).
        // Result start_idx = 5. Output: "fghijmatch"

        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 0,
                context: 1,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…ghijmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_preserved_prefix_and_ellipsis() {
        // Query "match". Starts at index 10.
        // "abcdefghijmatch"
        // autoscroll = Some((preserved=3, context=1))
        // initial_start_idx = 10 + 0 - 1 = 9.
        // start_idx = 9.
        // width_limit = 10.
        // preserved_width ("abc") = 3.
        // gap_indicator_width ("…") = 1.
        // tail_width ("jmatch") = 6.
        // total = 3 + 1 + 6 = 10.
        // start_idx=9, preserved=3. 9 > 3 + 1 (9 > 4) -> preserved_prefix = "abc", output: "abc…jmatch"

        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 3,
                context: 1,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "abc…jmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_wrap() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            true,
            10,
            &mut buffer,
            AutoscrollSettings {
                initial_preserved: 3,
                context: 1,
                ..Default::default()
            },
            -2,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "abcdefmat↵\nch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_wrap_edge_case_6_chars_width_5() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("", "123456");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("123456");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            true,
            5,
            &mut buffer,
            AutoscrollSettings {
                enabled: false,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        // Expecting "1234↵" and "56"
        assert_eq!(output_str, "1234↵\n56");
        assert_eq!(width, 5);
    }

    #[test]
    fn test_autoscroll_end() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                end: true,
                context: 4,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…ghijmatch");
        assert_eq!(width, 10);
    }

    #[test]
    fn test_autoscroll_end_context() {
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("ma", "abcdefghijmatch");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("abcdefghijmatch");
        let highlight = Style::default().fg(Color::Red);

        let (result_text, width) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            false,
            10,
            &mut buffer,
            AutoscrollSettings {
                end: true,
                context: 2,
                ..Default::default()
            },
            0,
        );

        let output_str = result_text.to_string();
        assert_eq!(output_str, "…fghijmatc");
        assert_eq!(width, 10);
    }
}
