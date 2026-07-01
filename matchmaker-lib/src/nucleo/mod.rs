pub mod injector;
pub mod query;
pub mod render_item;
pub mod variants;
mod worker;

use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    ops::Range,
};

use crate::SSS;
pub use variants::*;
pub use worker::*;

pub use nucleo;
pub use ratatui::prelude::*;

// ------------- Wrapper structs
pub trait SegmentableItem: SSS {
    fn slice(&self, range: Range<usize>) -> ratatui::text::Text<'_>;
    fn slice_str(&self, range: Range<usize>) -> Cow<'_, str> {
        self.slice(range).to_string().into()
    }
}

impl SegmentableItem for String {
    fn slice(&self, range: Range<usize>) -> ratatui::text::Text<'_> {
        ratatui::text::Text::from(&self[range])
    }
    fn slice_str(&self, range: Range<usize>) -> Cow<'_, str> {
        (&self[range]).into()
    }
}

/// This struct implements ColumnIndexable, and can instantiate a worker with columns.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Segmented<T> {
    pub inner: T,
    ranges: Box<[(u32, u32)]>,
}

impl<T: SegmentableItem + std::fmt::Debug> ColumnIndexable for Segmented<T> {
    fn get_str(&self, i: usize) -> std::borrow::Cow<'_, str> {
        if let Some(&(start, end)) = self.ranges.get(i) {
            self.inner.slice_str(start as usize..end as usize)
        } else {
            "".into()
        }
    }

    fn get_text(&self, i: usize) -> Text<'_> {
        if let Some(&(start, end)) = self.ranges.get(i) {
            self.inner.slice(start as usize..end as usize)
        } else {
            Text::default()
        }
    }
}

impl<T: SegmentableItem> Segmented<T> {
    pub fn new(inner: T, ranges: Box<[(u32, u32)]>) -> Self {
        Self { inner, ranges }
    }

    pub fn from_ranges(inner: T, ranges: impl IntoIterator<Item = (usize, usize)>) -> Self {
        let ranges: Box<[(u32, u32)]> = ranges
            .into_iter()
            .map(|(s, e)| (s as u32, e as u32))
            .collect();
        Self { inner, ranges }
    }

    pub fn len(&self) -> usize {
        // Find the last range that is nonempty (start != end)
        self.ranges
            .iter()
            .rposition(|&(start, end)| start != end)
            .map_or(0, |idx| idx + 1)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn map_to_vec<U, F>(&self, f: F) -> Vec<U>
    where
        F: Fn(&T, usize, usize) -> U,
    {
        self.ranges
            .iter()
            .take(self.len()) // only map the "active" ranges
            .map(|&(start, end)| f(&self.inner, start as usize, end as usize))
            .collect()
    }
}

impl<T> std::ops::Deref for Segmented<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

// ------------------------------------------------

impl<T: Display + SegmentableItem> Display for Segmented<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}
