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

type ColumnFormatFn<T, D> = Box<dyn for<'a> Fn(&'a T, &'a D) -> Text<'a> + Send + Sync>;
type ColumnRawFn<T, D> = Box<dyn for<'a> Fn(&'a T, &'a D) -> Cow<'a, str> + Send + Sync>;
pub struct Column<T, D = ()> {
    pub name: Arc<str>,
    pub(super) format: ColumnFormatFn<T, D>,
    pub(super) raw: Option<ColumnRawFn<T, D>>,
    /// Whether the column should be passed to nucleo for matching and filtering.
    pub(super) filter: bool,
}

impl<T, D> Column<T, D> {
    pub fn new_boxed(name: impl Into<Arc<str>>, format: ColumnFormatFn<T, D>) -> Self {
        Self {
            name: name.into(),
            format,
            filter: true,
            raw: None,
        }
    }

    pub fn new<F>(name: impl Into<Arc<str>>, f: F) -> Self
    where
        F: for<'a> Fn(&'a T, &'a D) -> Text<'a> + SSS,
    {
        Self::new_boxed(name, Box::new(f))
    }

    pub fn with_raw<F>(mut self, f: F) -> Self
    where
        F: for<'a> Fn(&'a T, &'a D) -> Cow<'a, str> + SSS,
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

    pub fn format<'a>(&self, item: &'a T, d: &'a D) -> Text<'a> {
        (self.format)(item, d)
    }

    // Note: the characters should match the output of [`Self::format`]
    pub fn raw<'a>(&self, item: &'a T, d: &'a D) -> Cow<'a, str> {
        if let Some(r) = &self.raw {
            (r)(item, d)
        } else {
            Cow::Owned((self.format)(item, d).to_string())
        }
    }
}

/// Worker: can instantiate, push, and get results. A view into computation.
///
/// Additionally, the worker can affect the computation via find and restart.
pub struct Worker<T, D = ()>
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
    pub columns: Arc<[Column<T, D>]>,
    /// Preprocessor for raw column functions (used during injection)
    pub raw_preprocessor: Arc<dyn Fn(&T) -> Option<D> + Send + Sync>,
    /// Preprocessor for text column functions (used during rendering)
    pub text_preprocessor: Arc<dyn Fn(&T) -> D + Send + Sync>,

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

impl<T, D> Worker<T, D>
where
    T: SSS,
{
    /// Column names must be distinct!
    pub fn new(
        columns: impl IntoIterator<Item = Column<T, D>>,
        default_column: usize,
        raw_preprocessor: Arc<dyn Fn(&T) -> Option<D> + Send + Sync>,
        text_preprocessor: Arc<dyn Fn(&T) -> D + Send + Sync>,
    ) -> Self {
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
            raw_preprocessor,
            text_preprocessor,
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

    pub fn injector(&self) -> WorkerInjector<T, D> {
        WorkerInjector {
            inner: self.nucleo.injector(),
            columns: self.columns.clone(),
            raw_preprocessor: self.raw_preprocessor.clone(),
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

    /// Prefer [`crate::ui::PickerUI::restart`]
    pub fn restart(&mut self, clear_snapshot: bool) {
        self.nucleo.restart(clear_snapshot);
    }

    // ------------------------- GETTERS ---------------------
    pub fn get_nth(&self, n: u32) -> Option<&T> {
        self.nucleo
            .snapshot()
            .get_matched_item(n)
            .map(|item| item.data)
    }

    pub fn get_by_idx(&self, idx: u32) -> Option<&T> {
        self.nucleo.snapshot().get_item(idx).map(|item| item.data)
    }

    pub fn matched_results(&self) -> impl ExactSizeIterator<Item = &T> + DoubleEndedIterator + '_ {
        let snapshot = self.nucleo.snapshot();
        snapshot.matched_items(..).map(|item| item.data)
    }

    pub fn matched_indices(&self) -> impl ExactSizeIterator<Item = u32> + DoubleEndedIterator + '_ {
        let snapshot = self.nucleo.snapshot();
        snapshot.matches().iter().map(|m| m.idx)
    }

    /// Return the nucleo index and a reference to the data of the n-th matched item, if any.
    ///
    /// The returned `u32` is the stable nucleo item index (see [`nucleo::Match::idx`]).
    /// Callers can use this as a key into [`crate::Selector`] or as a row-cache key.
    pub fn get_nth_indexed(&self, n: u32) -> Option<(u32, &T)> {
        let snapshot = self.nucleo.snapshot();
        let m = snapshot.matches().get(n as usize)?;
        let idx = m.idx;
        // SAFETY: `idx` is taken from a match in the snapshot we just took, so it
        // points to an initialized item in that snapshot.
        let item = unsafe { snapshot.get_item_unchecked(idx) };
        Some((idx, item.data))
    }

    pub(crate) fn get_nth_indexed_item(&self, n: u32) -> Option<(u32, nucleo::Item<'_, T>)> {
        let snapshot = self.nucleo.snapshot();
        let m = snapshot.matches().get(n as usize)?;
        let idx = m.idx;
        // SAFETY: `idx` is taken from a match in the snapshot we just took, so it
        // points to an initialized item in that snapshot.
        let item = unsafe { snapshot.get_item_unchecked(idx) };
        Some((idx, item))
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
            let d = (self.raw_preprocessor)(item.data)?;
            let content = col.raw(item.data, &d);
            if content == query {
                Some(item.data)
            } else {
                None
            }
        })
    }

    // ----------- COLUMN ACCESSORS --------------

    pub fn format_with<'a>(&'a self, item: &'a T, col: &crate::config_types::StringOrInt) -> Option<Cow<'a, str>> {
        let col_val = match col {
            crate::config_types::StringOrInt::String(s) => {
                self.columns.iter().find(|c| &*c.name == s.as_str())?
            }
            crate::config_types::StringOrInt::Int(idx) => {
                let idx = *idx;
                if idx >= 0 {
                    self.columns.get(idx as usize)?
                } else {
                    return None;
                }
            }
        };
        let d = (self.raw_preprocessor)(item)?;
        Some(col_val.raw(item, &d).into_owned().into())
    }
}

#[derive(Debug, Default, Clone)]
pub struct Status {
    pub item_count: u32,
    pub matched_count: u32,
    pub running: bool,
    pub changed: bool,
}

/// Standalone function to create a snapshot from nucleo without requiring D type parameter
pub fn new_snapshot<T: Sync + Send + 'static>(
    nucleo: &mut nucleo::Nucleo<T>,
) -> (&nucleo::Snapshot<T>, Status) {
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

#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("the matcher injector has been shut down")]
    InjectorShutdown,
    #[error("{0}")]
    Custom(&'static str),
}
