pub mod previewer;
mod view;
pub use view::Preview;

// -------------- APPENDONLY
use std::ops::Deref;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct AppendOnly<T>(Arc<RwLock<boxcar::Vec<T>>>);

impl<T> Default for AppendOnly<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AppendOnly<T> {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(boxcar::Vec::new())))
    }

    pub fn is_empty(&self) -> bool {
        let guard = self.0.read().unwrap();
        guard.is_empty()
    }

    pub fn len(&self) -> usize {
        let guard = self.0.read().unwrap();
        guard.count()
    }

    pub fn clear(&self) {
        let mut guard = self.0.write().unwrap(); // acquire write lock
        guard.clear();
    }

    pub fn push(&self, val: T) {
        let guard = self.0.read().unwrap();
        guard.push(val);
    }

    pub fn map_to_vec<U, F>(&self, mut f: F) -> Vec<U>
    where
        F: FnMut(&T) -> U,
    {
        let guard = self.0.read().unwrap();
        guard.iter().map(move |(_i, v)| f(v)).collect()
    }
}

impl<T> Deref for AppendOnly<T> {
    type Target = RwLock<boxcar::Vec<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
