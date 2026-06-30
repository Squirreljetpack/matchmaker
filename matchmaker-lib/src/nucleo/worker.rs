// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use super::Text;
use bitflags::bitflags;
use serde::{Deserialize, Serialize};
use std::{
    borrow::Cow,
    sync::{
        Arc,
        atomic::{self, AtomicU32},
    },
};

use super::{injector::WorkerInjector, query::PickerQuery};
use crate::SSS;

type ColumnFormatFn<T> = Box<dyn for<'a> Fn(&'a T) -> Text<'a> + Send + Sync>;
type ColumnRawFn<T> = Box<dyn for<'a> Fn(&'a T) -> Cow<'a, str> + Send + Sync>;
pub struct Column<T> {
    pub name: Arc<str>,
    pub(super) format: ColumnFormatFn<T>,
    pub(super) raw: Option<ColumnRawFn<T>>,
    /// Whether the column should be passed to nucleo for matching and filtering.
    pub(super) filter: bool,
}

impl<T> Column<T> {
    pub fn new_boxed(name: impl Into<Arc<str>>, format: ColumnFormatFn<T>) -> Self {
        Self {
            name: name.into(),
            format,
            filter: true,
            raw: None,
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
            raw: None,
        }
    }

    pub fn with_raw<F>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a T) -> Cow<'a, str> + SSS,
    {
        self.raw = Some(Box::new(f));
        self
    }

    /// Disable filtering.
    pub fn without_filtering(mut self) -> Self {
        self.filter = false;
        self
    }

    pub fn filter(&self) -> bool {
        self.filter
    }

    pub fn format<'a>(&self, item: &'a T) -> Text<'a> {
        (self.format)(item)
    }

    // Note: the characters should match the output of [`Self::format`]
    pub fn raw<'a>(&self, item: &'a T) -> Cow<'a, str> {
        if let Some(r) = &self.raw {
            (r)(item)
        } else {
            Cow::Owned((self.format)(item).to_string())
        }
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
    pub nucleo: nucleo::Nucleo<T>,
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
    #[derive(Default, Copy, Clone, Debug, Serialize, Deserialize, PartialEq)]
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

    // not a method due for lifetime flexibility
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

    /// Note: call set_dirty on ResultsUI afterward
    pub fn restart(&mut self, clear_snapshot: bool) {
        self.nucleo.restart(clear_snapshot);
    }

    // ----------------------------------------------

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
            let content = col.raw(item.data);
            if content == query {
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
            .map(|c| c.raw(item))
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
