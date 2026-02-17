// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

#![allow(unused)]

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
    utils::text::{apply_style_at, plain_text, wrap_text},
};
use cli_boilerplate_automation::text::StrExt;

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
        Cow::Owned(plain_text(&(self.format)(item)))
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
    pub(super) query: PickerQuery,
    /// A pre-allocated buffer used to collect match indices when fetching the results
    /// from the matcher. This avoids having to re-allocate on each pass.
    pub(super) col_indices_buffer: Vec<u32>,
    pub(crate) columns: Arc<[Column<T>]>,

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

pub type WorkerResults<'a, T> = Vec<(Vec<Text<'a>>, &'a T, u16)>;

impl<T: SSS> Worker<T> {
    /// # Notes
    /// - width is at least header width
    pub fn results(
        &mut self,
        start: u32,
        end: u32,
        width_limits: &[u16],
        highlight_style: Style,
        matcher: &mut nucleo::Matcher,
    ) -> (WorkerResults<'_, T>, Vec<u16>, Status) {
        let (snapshot, status) = Self::new_snapshot(&mut self.nucleo);

        let mut widths = vec![0u16; self.columns.len()]; // first cell reserved for prefix

        let iter =
            snapshot.matched_items(start.min(status.matched_count)..end.min(status.matched_count));

        let table = iter
            .map(|item| {
                let mut widths = widths.iter_mut();
                let mut col_idx = 0;
                let mut height = 0;

                let row = self
                    .columns
                    .iter()
                    .zip(width_limits.iter().chain(std::iter::repeat(&u16::MAX)))
                    .map(|(column, &width_limit)| {
                        let max_width = widths.next().unwrap();
                        let cell = column.format(item.data);

                        // 0 represents hide
                        if width_limit == 0 {
                            return Text::from("");
                        }

                        let (cell, width) = if column.filter && width_limit == u16::MAX {
                            let mut cell_width = 0;

                            // get indices
                            let indices_buffer = &mut self.col_indices_buffer;
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

                            for line in cell {
                                let mut span_list = Vec::new();
                                let mut current_span = String::new();
                                let mut current_style = Style::default();
                                let mut width = 0;

                                for span in line {
                                    // this looks like a bug on first glance, we are iterating
                                    // graphemes but treating them as char indices. The reason that
                                    // this is correct is that nucleo will only ever consider the first char
                                    // of a grapheme (and discard the rest of the grapheme) so the indices
                                    // returned by nucleo are essentially grapheme indecies
                                    for grapheme in span.content.graphemes(true) {
                                        let style = if grapheme_idx == next_highlight_idx {
                                            next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                                            span.style.patch(highlight_style)
                                        } else {
                                            span.style
                                        };
                                        if style != current_style {
                                            if !current_span.is_empty() {
                                                span_list
                                                    .push(Span::styled(current_span, current_style))
                                            }
                                            current_span = String::new();
                                            current_style = style;
                                        }
                                        current_span.push_str(grapheme);
                                        grapheme_idx += 1;
                                    }
                                    width += span.width();
                                }

                                span_list.push(Span::styled(current_span, current_style));
                                lines.push(Line::from(span_list));
                                cell_width = cell_width.max(width);
                                grapheme_idx += 1; // newline?
                            }

                            col_idx += 1;
                            (Text::from(lines), cell_width)
                        } else if column.filter {
                            let mut cell_width = 0;
                            let mut wrapped = false;

                            // get indices
                            let indices_buffer = &mut self.col_indices_buffer;
                            indices_buffer.clear();
                            snapshot.pattern().column_pattern(col_idx).indices(
                                item.matcher_columns[col_idx].slice(..),
                                matcher,
                                indices_buffer,
                            );
                            indices_buffer.sort_unstable();
                            indices_buffer.dedup();
                            let mut indices = indices_buffer.drain(..);

                            let mut lines: Vec<Line<'_>> = vec![];
                            let mut next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                            let mut grapheme_idx = 0u32;

                            for line in cell {
                                let mut current_spans = Vec::new();
                                let mut current_span = String::new();
                                let mut current_style = Style::default();
                                let mut current_width = 0;

                                for span in line {
                                    let mut graphemes = span.content.graphemes(true).peekable();
                                    while let Some(grapheme) = graphemes.next() {
                                        let grapheme_width = UnicodeWidthStr::width(grapheme);

                                        if current_width + grapheme_width
                                            > (width_limit - 1) as usize
                                            && { grapheme_width > 1 || graphemes.peek().is_some() }
                                        {
                                            current_spans
                                                .push(Span::styled(current_span, current_style));
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

                                        let style = if grapheme_idx == next_highlight_idx {
                                            next_highlight_idx = indices.next().unwrap_or(u32::MAX);
                                            span.style.patch(highlight_style)
                                        } else {
                                            span.style
                                        };

                                        if style != current_style {
                                            if !current_span.is_empty() {
                                                current_spans
                                                    .push(Span::styled(current_span, current_style))
                                            }
                                            current_span = String::new();
                                            current_style = style;
                                        }
                                        current_span.push_str(grapheme);
                                        grapheme_idx += 1;
                                        current_width += grapheme_width;
                                    }
                                }

                                current_spans.push(Span::styled(current_span, current_style));
                                lines.push(Line::from(current_spans));
                                cell_width = cell_width.max(current_width);
                                grapheme_idx += 1; // newline?
                            }

                            col_idx += 1;

                            (
                                Text::from(lines),
                                if wrapped {
                                    width_limit as usize
                                } else {
                                    cell_width
                                },
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

                        if cell.height() as u16 > height {
                            height = cell.height() as u16;
                        }

                        cell
                    });

                (row.collect(), item.data, height)
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
