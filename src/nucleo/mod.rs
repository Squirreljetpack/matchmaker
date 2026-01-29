pub mod injector;
pub mod query;
pub mod variants;
mod worker;

use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
};

use arrayvec::ArrayVec;
pub use variants::*;
pub use worker::*;

pub use nucleo;
pub use ratatui::prelude::*;

use crate::{MAX_SPLITS, SegmentableItem};

// ------------- Wrapper structs

/// This struct implements ColumnIndexable, and can instantiate a worker with columns.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Segmented<T: SegmentableItem> {
    pub inner: T,
    ranges: ArrayVec<(usize, usize), MAX_SPLITS>,
}

impl<T: SegmentableItem> ColumnIndexable for Segmented<T> {
    fn as_str(&self, index: usize) -> Cow<'_, str> {
        if let Some((start, end)) = self.ranges.get(index) {
            &self.inner[*start..*end]
        } else {
            ""
        }
        .into()
    }
}

#[derive(Debug, Clone)]
pub struct Indexed<T> {
    pub index: u32,
    pub inner: T,
}

impl<T: Clone> Indexed<T> {
    /// Matchmaker requires a way to identify and store selected items from their references in the nucleo matcher. This method simply identifies them by their insertion index and stores the clones of the items.
    pub fn identifier(&self) -> (u32, T) {
        (self.index, self.inner.clone())
    }
}

impl<T: ColumnIndexable> ColumnIndexable for Indexed<T> {
    fn as_str(&self, index: usize) -> Cow<'_, str> {
        self.inner.as_str(index)
    }
}

impl<T: Render> Render for Indexed<T> {
    fn as_str(&self) -> Cow<'_, str> {
        self.inner.as_str()
    }
    fn as_text(&self) -> Text<'_> {
        self.inner.as_text()
    }
}

// ------------------------------------------
impl<T: Display + SegmentableItem> Display for Segmented<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl<T: Display> Display for Indexed<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner)
    }
}

impl<T> PartialEq for Indexed<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for Indexed<T> {}

impl<T> Hash for Indexed<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state)
    }
}
