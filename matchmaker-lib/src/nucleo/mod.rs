pub mod injector;
pub mod query;
pub mod variants;
mod worker;

use std::{
    borrow::Cow,
    fmt::{self, Display, Formatter},
    hash::{Hash, Hasher},
    ops::Range,
};

use arrayvec::ArrayVec;
pub use variants::*;
pub use worker::*;

pub use nucleo;
pub use ratatui::prelude::*;

use crate::{MAX_SPLITS, SSS};

// ------------- Wrapper structs
pub trait SegmentableItem: SSS {
    fn slice(&self, range: Range<usize>) -> ratatui::text::Text<'_>;
}

impl SegmentableItem for String {
    fn slice(&self, range: Range<usize>) -> ratatui::text::Text<'_> {
        ratatui::text::Text::from(&self[range])
    }
}

/// This struct implements ColumnIndexable, and can instantiate a worker with columns.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Segmented<T> {
    pub inner: T,
    ranges: ArrayVec<(usize, usize), MAX_SPLITS>,
}

impl<T: SegmentableItem> ColumnIndexable for Segmented<T> {
    // fn get_str(&self, index: usize) -> Cow<'_, str> {
    //     if let Some((start, end)) = self.ranges.get(index) {
    //         &self.inner[*start..*end]
    //     } else {
    //         ""
    //     }
    //     .into()
    // }

    fn get_text(&self, i: usize) -> Text<'_> {
        if let Some((start, end)) = self.ranges.get(i) {
            self.inner.slice(*start..*end)
        } else {
            Text::default()
        }
    }
}

impl<T: SegmentableItem> Segmented<T> {
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

    pub fn map_to_vec<U, F>(&self, f: F) -> ArrayVec<U, MAX_SPLITS>
    where
        F: Fn(&T, usize, usize) -> U,
    {
        self.ranges
            .iter()
            .take(self.len()) // only map the "active" ranges
            .map(|&(start, end)| f(&self.inner, start, end))
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

impl<T> Indexed<T> {
    /// Matchmaker requires a way to identify and store selected items from their references in the nucleo matcher. This method simply identifies them by their insertion index and is intended when the output type is not needed (i.e. externally managed).
    pub fn dummy_identifier(&self) -> (u32, ()) {
        (self.index, ())
    }
}

impl<T: ColumnIndexable> ColumnIndexable for Indexed<T> {
    fn get_str(&self, index: usize) -> Cow<'_, str> {
        self.inner.get_str(index)
    }

    fn get_text(&self, i: usize) -> Text<'_> {
        self.inner.get_text(i)
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

impl<T> std::ops::Deref for Indexed<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.inner
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
