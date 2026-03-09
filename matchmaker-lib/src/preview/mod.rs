pub mod previewer;
mod view;
pub use view::Preview;

// -------------- APPENDONLY
use arc_swap::ArcSwap;
use std::sync::Arc;

/// Append-only Vec supporting concurrent writes
#[derive(Debug, Clone)]
pub struct AppendOnly<T>(Arc<ArcSwap<boxcar::Vec<T>>>);

impl<T> AppendOnly<T> {
    pub fn new() -> Self {
        Self(Arc::new(ArcSwap::from_pointee(boxcar::Vec::new())))
    }

    pub fn is_empty(&self) -> bool {
        self.0.load().is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.load().count()
    }

    pub fn clear(&self) {
        self.0.store(Arc::new(boxcar::Vec::new()));
    }

    pub fn push(&self, val: T) {
        self.0.load().push(val);
    }

    pub fn read(&self) -> arc_swap::Guard<Arc<boxcar::Vec<T>>> {
        self.0.load()
    }

    pub fn is_expired(&self, guard: &arc_swap::Guard<Arc<boxcar::Vec<T>>>) -> bool {
        !Arc::ptr_eq(guard, &self.0.load())
    }

    pub fn map_to_vec<U, F>(&self, mut f: F) -> Vec<U>
    where
        F: FnMut(&T) -> U,
    {
        self.0.load().iter().map(move |(_i, v)| f(v)).collect()
    }
}

impl<T> Default for AppendOnly<T> {
    fn default() -> Self {
        Self::new()
    }
}
