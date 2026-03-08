// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use super::{Line, Span, Style, Text};
use bitflags::bitflags;

use ratatui::style::Modifier;
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{self, AtomicU32},
    },
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{injector::WorkerInjector, query::PickerQuery};
use crate::{
    SSS,
    nucleo::Render,
    utils::text::{text_to_string, wrap_text},
};

type ColumnFormatFn<T> = Box<dyn for<'a> Fn(&'a T) -> Text<'a> + Send + Sync>;
pub struct Column<T> {
    pub name: Arc<str>,
    pub(super) format: ColumnFormatFn<T>,
    /// Whether the column should be passed to nucleo for matching and filtering.
    pub(super) filter: bool,
}

impl<T> Column<T> {
    pub fn new_boxed(name: impl Into<Arc<str>>, format: ColumnFormatFn<T>) -> Self {
        Self {
            name: name.into(),
            format,
            filter: true,
        }
    }

    pub fn new<F>(name: impl Into<Arc<str>>, f: F) -> Self
    where
        F: for<'a> Fn(&'a T) -> Text<'a> + SSS,
    {
        Self {
            name: name.into(),
            format: Box::new(f),
            filter: true,
        }
    }

    /// Disable filtering.
    pub fn without_filtering(mut self) -> Self {
        self.filter = false;
        self
    }

    pub(super) fn format<'a>(&self, item: &'a T) -> Text<'a> {
        (self.format)(item)
    }

    // Note: the characters should match the output of [`Self::format`]
    pub(super) fn format_text<'a>(&self, item: &'a T) -> Cow<'a, str> {
        Cow::Owned(text_to_string(&(self.format)(item)))
    }
}

/// Worker: can instantiate, push, and get results. A view into computation.
///
/// Additionally, the worker can affect the computation via find and restart.
pub struct Worker<T>
where
    T: SSS,
{
    /// The inner `Nucleo` fuzzy matcher.
    pub(crate) nucleo: nucleo::Nucleo<T>,
    /// The last pattern that was matched against.
    pub query: PickerQuery,
    /// A pre-allocated buffer used to collect match indices when fetching the results
    /// from the matcher. This avoids having to re-allocate on each pass.
    pub col_indices_buffer: Vec<u32>,
    pub columns: Arc<[Column<T>]>,

    // Background tasks which push to the injector check their version matches this or exit
    pub(super) version: Arc<AtomicU32>,
    // pub settings: WorkerSettings,
    column_options: Vec<ColumnOptions>,
}

// #[derive(Debug, Default)]
// pub struct WorkerSettings {
//     pub stable: bool,
// }

bitflags! {
    #[derive(Default, Clone, Debug)]
    pub struct ColumnOptions: u8 {
        const Optional = 1 << 0;
        const OrUseDefault = 1 << 2;
    }
}

impl<T> Worker<T>
where
    T: SSS,
{
    /// Column names must be distinct!
    pub fn new(columns: impl IntoIterator<Item = Column<T>>, default_column: usize) -> Self {
        let columns: Arc<[_]> = columns.into_iter().collect();
        let matcher_columns = columns.iter().filter(|col| col.filter).count() as u32;

        let inner = nucleo::Nucleo::new(
            nucleo::Config::DEFAULT,
            Arc::new(|| {}),
            None,
            matcher_columns,
        );

        Self {
            nucleo: inner,
            col_indices_buffer: Vec::with_capacity(128),
            query: PickerQuery::new(columns.iter().map(|col| &col.name).cloned(), default_column),
            column_options: vec![ColumnOptions::default(); columns.len()],
            columns,
            version: Arc::new(AtomicU32::new(0)),
        }
    }

    #[cfg(feature = "experimental")]
    pub fn set_column_options(&mut self, index: usize, options: ColumnOptions) {
        if options.contains(ColumnOptions::Optional) {
            self.nucleo
                .pattern
                .configure_column(index, nucleo::pattern::Variant::Optional)
        }

        self.column_options[index] = options
    }

    #[cfg(feature = "experimental")]
    pub fn reverse_items(&mut self, reverse_items: bool) {
        self.nucleo.reverse_items(reverse_items);
    }

    pub fn injector(&self) -> WorkerInjector<T> {
        WorkerInjector {
            inner: self.nucleo.injector(),
            columns: self.columns.clone(),
            version: self.version.load(atomic::Ordering::Relaxed),
            picker_version: self.version.clone(),
        }
    }

    pub fn find(&mut self, line: &str) {
        let old_query = self.query.parse(line);
        if self.query == old_query {
            return;
        }
        for (i, column) in self
            .columns
            .iter()
            .filter(|column| column.filter)
            .enumerate()
        {
            let pattern = self
                .query
                .get(&column.name)
                .map(|s| &**s)
                .unwrap_or_else(|| {
                    self.column_options[i]
                        .contains(ColumnOptions::OrUseDefault)
                        .then(|| self.query.primary_column_query())
                        .flatten()
                        .unwrap_or_default()
                });

            let old_pattern = old_query
                .get(&column.name)
                .map(|s| &**s)
                .unwrap_or_else(|| {
                    self.column_options[i]
                        .contains(ColumnOptions::OrUseDefault)
                        .then(|| {
                            let name = self.query.primary_column_name()?;
                            old_query.get(name).map(|s| &**s)
                        })
                        .flatten()
                        .unwrap_or_default()
                });

            // Fastlane: most columns will remain unchanged after each edit.
            if pattern == old_pattern {
                continue;
            }
            let is_append = pattern.starts_with(old_pattern);

            self.nucleo.pattern.reparse(
                i,
                pattern,
                nucleo::pattern::CaseMatching::Smart,
                nucleo::pattern::Normalization::Smart,
                is_append,
            );
        }
    }

    // --------- UTILS
    pub fn get_nth(&self, n: u32) -> Option<&T> {
        self.nucleo
            .snapshot()
            .get_matched_item(n)
            .map(|item| item.data)
    }

    pub fn new_snapshot(nucleo: &mut nucleo::Nucleo<T>) -> (&nucleo::Snapshot<T>, Status) {
        let nucleo::Status { changed, running } = nucleo.tick(10);
        let snapshot = nucleo.snapshot();
        (
            snapshot,
            Status {
                item_count: snapshot.item_count(),
                matched_count: snapshot.matched_item_count(),
                running,
                changed,
            },
        )
    }

    pub fn raw_results(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + '_ {
        let snapshot = self.nucleo.snapshot();
        snapshot.matched_items(..).map(|item| item.data)
    }

    /// matched item count, total item count
    pub fn counts(&self) -> (u32, u32) {
        let snapshot = self.nucleo.snapshot();
        (snapshot.matched_item_count(), snapshot.item_count())
    }

    #[cfg(feature = "experimental")]
    pub fn set_stability(&mut self, threshold: u32) {
        self.nucleo.set_stability(threshold);
    }

    #[cfg(feature = "experimental")]
    pub fn get_stability(&self) -> u32 {
        self.nucleo.get_stability()
    }

    pub fn restart(&mut self, clear_snapshot: bool) {
        self.nucleo.restart(clear_snapshot);
    }
}

#[derive(Debug, Default, Clone)]
pub struct Status {
    pub item_count: u32,
    pub matched_count: u32,
    pub running: bool,
    pub changed: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("the matcher injector has been shut down")]
    InjectorShutdown,
    #[error("{0}")]
    Custom(&'static str),
}

/// A vec of ItemResult, each ItemResult being the Column Texts of the Item, and Item
pub type WorkerResults<'a, T> = Vec<(Vec<Text<'a>>, &'a T)>;

impl<T: SSS> Worker<T> {
    /// Returns:
    /// 1. Table of (Row, item, height)
    /// 2. Final column widths
    /// 3. Status
    ///
    /// # Notes
    /// - width is at least header width
    pub fn results(
        &mut self,
        start: u32,
        end: u32,
        width_limits: &[u16],
        highlight_style: Style,
        matcher: &mut nucleo::Matcher,
        match_start_context: Option<usize>,
    ) -> (WorkerResults<'_, T>, Vec<u16>, Status) {
        let (snapshot, status) = Self::new_snapshot(&mut self.nucleo);

        let mut widths = vec![0u16; self.columns.len()];

        let iter =
            snapshot.matched_items(start.min(status.matched_count)..end.min(status.matched_count));

        let table = iter
            .map(|item| {
                let mut widths = widths.iter_mut();

                let row = self
                    .columns
                    .iter()
                    .enumerate()
                    .zip(width_limits.iter().chain(std::iter::repeat(&u16::MAX)))
                    .map(|((col_idx, column), &width_limit)| {
                        let max_width = widths.next().unwrap();
                        let cell = column.format(item.data);

                        // 0 represents hide
                        if width_limit == 0 {
                            return Text::default();
                        }

                        let (cell, width) = if column.filter {
                            render_cell(
                                cell,
                                col_idx,
                                snapshot,
                                &item,
                                matcher,
                                highlight_style,
                                width_limit,
                                &mut self.col_indices_buffer,
                                match_start_context,
                            )
                        } else if width_limit != u16::MAX {
                            let (cell, wrapped) = wrap_text(cell, width_limit - 1);

                            let width = if wrapped {
                                width_limit as usize
                            } else {
                                cell.width()
                            };
                            (cell, width)
                        } else {
                            let width = cell.width();
                            (cell, width)
                        };

                        // update col width, row height
                        if width as u16 > *max_width {
                            *max_width = width as u16;
                        }

                        cell
                    });

                (row.collect(), item.data)
            })
            .collect();

        // Nonempty columns should have width at least their header
        for (w, c) in widths.iter_mut().zip(self.columns.iter()) {
            let name_width = c.name.width() as u16;
            if *w != 0 {
                *w = (*w).max(name_width);
            }
        }

        (table, widths, status)
    }

    pub fn exact_column_match(&mut self, column: &str) -> Option<&T> {
        let (i, col) = self
            .columns
            .iter()
            .enumerate()
            .find(|(_, c)| column == &*c.name)?;

        let query = self.query.get(column).map(|s| &**s).or_else(|| {
            self.column_options[i]
                .contains(ColumnOptions::OrUseDefault)
                .then(|| self.query.primary_column_query())
                .flatten()
        })?;

        let snapshot = self.nucleo.snapshot();
        snapshot.matched_items(..).find_map(|item| {
            let content = col.format_text(item.data);
            if content.as_str() == query {
                Some(item.data)
            } else {
                None
            }
        })
    }

    pub fn format_with<'a>(&'a self, item: &'a T, col: &str) -> Option<Cow<'a, str>> {
        self.columns
            .iter()
            .find(|c| &*c.name == col)
            .map(|c| c.format_text(item))
    }
}

fn render_cell<T: SSS>(
    // Assuming T implements the required SSS/Config trait
    cell: Text<'_>,
    col_idx: usize,
    snapshot: &nucleo::Snapshot<T>,
    item: &nucleo::Item<T>,
    matcher: &mut nucleo::Matcher,
    highlight_style: Style,
    width_limit: u16,
    col_indices_buffer: &mut Vec<u32>,
    match_start_context: Option<usize>,
) -> (Text<'static>, usize) {
    let mut cell_width = 0;
    let mut wrapped = false;

    // get indices
    let indices_buffer = col_indices_buffer;
    indices_buffer.clear();
    snapshot.pattern().column_pattern(col_idx).indices(
        item.matcher_columns[col_idx].slice(..),
        matcher,
        indices_buffer,
    );
    indices_buffer.sort_unstable();
    indices_buffer.dedup();
    let mut indices = indices_buffer.drain(..);

    let mut lines = vec![];
    let mut next_highlight_idx = indices.next().unwrap_or(u32::MAX);
    let mut grapheme_idx = 0u32;

    for line in &cell {
        // 1: Collect graphemes, compute styles, and find the first match on this line.
        let mut line_graphemes = Vec::new();
        let mut first_match_idx = None;

        for span in line {
            // this looks like a bug on first glance, we are iterating
            // graphemes but treating them as char indices. The reason that
            // this is correct is that nucleo will only ever consider the first char
            // of a grapheme (and discard the rest of the grapheme) so the indices
            // returned by nucleo are essentially grapheme indecies
            let mut graphemes = span.content.graphemes(true).peekable();

            while let Some(grapheme) = graphemes.next() {
                let is_match = grapheme_idx == next_highlight_idx;

                let style = if is_match {
                    next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                    span.style.patch(highlight_style)
                } else {
                    span.style
                };

                if is_match && first_match_idx.is_none() {
                    first_match_idx = Some(line_graphemes.len());
                }

                line_graphemes.push((grapheme, style));
                grapheme_idx += 1;
            }
        }

        // 2: Calculate where to start rendering this line
        let mut start_idx = 0;

        if let (Some(n), Some(first_idx)) = (match_start_context, first_match_idx) {
            start_idx = first_idx.saturating_sub(n);

            // If width_limit is given, Try to start earlier provided we don't exceed width_limit.
            if width_limit != u16::MAX {
                let mut tail_width: usize = line_graphemes[start_idx..]
                    .iter()
                    .map(|(g, _)| g.width())
                    .sum();

                // Expand leftwards as long as the total rendered width <= width_limit
                while start_idx > 0 {
                    let prev_width = line_graphemes[start_idx - 1].0.width();
                    if tail_width + prev_width <= width_limit as usize {
                        start_idx -= 1;
                        tail_width += prev_width;
                    } else {
                        break;
                    }
                }
            }
        }

        // 3: Apply the standard wrapping and Span generation logic to the visible slice
        let mut current_spans = Vec::new();
        let mut current_span = String::new();
        let mut current_style = Style::default();
        let mut current_width = 0;

        let mut graphemes = line_graphemes[start_idx..].iter().peekable();

        while let Some(&(grapheme, style)) = graphemes.next() {
            let grapheme_width = grapheme.width();

            if width_limit != u16::MAX {
                if current_width + grapheme_width > (width_limit - 1) as usize && {
                    grapheme_width > 1 || graphemes.peek().is_some()
                } {
                    if !current_span.is_empty() {
                        current_spans.push(Span::styled(current_span, current_style));
                    }
                    current_spans.push(Span::styled(
                        "↵",
                        Style::default().add_modifier(Modifier::DIM),
                    ));
                    lines.push(Line::from(current_spans));

                    current_spans = Vec::new();
                    current_span = String::new();
                    current_width = 0;
                    wrapped = true;
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
            current_width += grapheme_width;
        }

        current_spans.push(Span::styled(current_span, current_style));
        lines.push(Line::from(current_spans));
        cell_width = cell_width.max(current_width);

        grapheme_idx += 1; // newline
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
            u16::MAX,
            &mut buffer,
            None,
        );

        let output_str = text_to_string(&result_text);
        assert_eq!(output_str, "hello match world");
        assert_eq!(width, 17);
    }

    #[test]
    fn test_scroll_context_cuts_prefix_correctly() {
        // Match starts at index 6 ("match"). Context is 2.
        // We expect it to drop "hell", keep "o ", and render "o match world"
        let (nucleo, mut matcher, mut buffer) = setup_nucleo_mocks("match", "hello match world");
        let snapshot = nucleo.snapshot();
        let item = snapshot.get_item(0).unwrap();

        let cell = Text::from("hello match world");
        let highlight = Style::default().fg(Color::Red);

        // Width limit MAX so no backfill constraint triggers
        let (result_text, _) = render_cell(
            cell,
            0,
            &snapshot,
            &item,
            &mut matcher,
            highlight,
            u16::MAX,
            &mut buffer,
            Some(2),
        );

        let output_str = text_to_string(&result_text);
        assert_eq!(output_str, "o match world");
    }

    #[test]
    fn test_scroll_context_backfills_to_fill_width_limit() {
        // Query "match". Starts at index 10.
        // "abcdefghijmatch"
        // If context = 1, normally we'd start at index 9 ("jmatch"). Width = 6.
        // If width_limit = 10, the backfill logic should expand backwards
        // to consume 4 more characters to fill the 10-char limit without wrapping.
        // Expected start: "fghijmatch"

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
            10,
            &mut buffer,
            Some(1),
        );

        let output_str = text_to_string(&result_text);
        assert_eq!(output_str, "fghijmatch");
        assert_eq!(width, 10);
    }
}
