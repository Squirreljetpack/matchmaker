pub mod injector;
mod worker;
pub mod query;
pub mod variants;

use std::{fmt::{self, Display, Formatter}, sync::Arc, hash::{Hash, Hasher}};

pub use variants::*;
pub use worker::*;

pub use ratatui::{
    style::{Style, Stylize},
    text::{Line, Span, Text},
};

use crate::SegmentableItem;

// ------------- Wrapper structs

/// This struct implements ColumnIndexable, and can instantiate a worker with columns.
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Segmented<T: SegmentableItem> {
    pub inner: T,
    ranges: Arc<[(usize, usize)]>,
}

impl<T: SegmentableItem> ColumnIndexable for Segmented<T> {
    fn index(&self, index: usize) -> &str {
        if let Some((start, end)) = self.ranges.get(index) {
            &self.inner[*start..*end]
        } else {
            ""
        }
    }
}

#[derive(Debug, Clone)]
pub struct Indexed<T> {
    pub index: u32,
    pub inner: T,
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

impl<T: Clone> Indexed<T> {

    /// Matchmaker requires a way to store and identify selected items from their references in the nucleo matcher. This method simply stores the clones of the items.
    pub fn identifier(&self) -> (u32, T) {
        (self.index, self.inner.clone())
    }
}

impl<T: ColumnIndexable> ColumnIndexable for Indexed<T> {
    fn index(&self, index: usize) -> &str {
        self.inner.index(index)
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