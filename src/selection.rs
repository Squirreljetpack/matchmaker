use crate::{Identifier, Selection};
use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;
use std::sync::Mutex;
use std::{borrow::Borrow, hash::Hash, sync::Arc};

#[derive(Debug)]
pub struct SelectionSet<T, S> {
    selections: SelectionSetImpl<u32, S>,
    pub identifier: Identifier<T, S>,
}

impl<T, S: Selection> SelectionSet<T, S> {
    pub fn new(identifier: Identifier<T, S>) -> Self {
        Self {
            selections: SelectionSetImpl::new(),
            identifier,
        }
    }

    pub fn sel(&mut self, item: &T) -> bool {
        let (k, v) = (self.identifier)(item);
        self.selections.insert(k, v)
    }

    pub fn desel(&mut self, item: &T) -> bool {
        let (k, _v) = (self.identifier)(item);
        self.selections.remove(&k)
    }

    pub fn contains(&self, item: &T) -> bool {
        let (k, _v) = (self.identifier)(item);
        self.selections.contains(&k)
    }

    pub fn toggle(&mut self, item: &T) {
        let (k, v) = (self.identifier)(item);
        if self.selections.contains(&k) {
            self.selections.remove(&k);
        } else {
            self.selections.insert(k, v);
        }
    }

    pub fn clear(&mut self) {
        self.selections.clear();
    }

    pub fn len(&self) -> usize {
        self.selections.len()
    }

    pub fn is_empty(&self) -> bool {
        self.selections.is_empty()
    }

    pub fn output(&mut self) -> impl Iterator<Item = S> {
        let mut set = self.selections.set.lock().unwrap();

        std::mem::take(&mut *set).into_values()
    }

    pub fn identify_to_vec<I>(&self, items: I) -> Vec<S>
    where
    I: IntoIterator,
    I::Item: std::borrow::Borrow<T> + Send,
    {
        // let items_vec: Vec<I::Item> = items.into_iter().collect();

        items
        .into_iter()
        // .into_par_iter()
        .map(|item| (self.identifier)(item.borrow()).1)
        .collect()
    }

    pub fn map_to_vec<U, F>(&self, f: F) -> Vec<U>
    where
    F: FnMut(&S) -> U,
    {
        // let items_vec: Vec<I::Item> = items.into_iter().collect();
        self.selections.map_to_vec(f)
    }

    pub fn cycle_all_bg<I>(&self, items: I)
    where
    I: IntoIterator,
    I::Item: std::borrow::Borrow<T> + Send,
    {
        let results: Vec<_> = items
        .into_iter()
        .map(|item| (self.identifier)(item.borrow()))
        .collect();

        let selections = self.selections.clone();

        tokio::task::spawn_blocking(move || {
            let mut all = true;
            let mut set_guard = selections.set.lock().unwrap();

            let mut seen = 0;
            for (i, (k, _v)) in results.iter().enumerate() {
                if !set_guard.contains_key(k) {
                    all = false;
                    seen = i;
                    break;
                }
            }

            if all {
                for (k, _v) in results {
                    set_guard.swap_remove(&k); // swap instead of shift for speed
                }
            } else {
                for (k, v) in results.into_iter().skip(seen) {
                    set_guard.insert(k, v);
                }
            }
        });
    }
}

// ---------- Selection Set ---------------
#[derive(Debug, Clone)]
struct SelectionSetImpl<K: Eq + Hash, S> {
    pub set: Arc<Mutex<IndexMap<K, S, FxBuildHasher>>>,
}

impl<K: Eq + Hash, S> SelectionSetImpl<K, S>
where
S: Selection,
{
    pub fn new() -> Self {
        Self {
            set: Arc::new(Mutex::new(IndexMap::with_hasher(FxBuildHasher))),
        }
    }

    pub fn insert(&self, key: K, value: S) -> bool {
        let mut set = self.set.lock().unwrap();
        set.insert(key, value).is_none()
    }

    pub fn remove(&self, key: &K) -> bool {
        let mut set = self.set.lock().unwrap();
        set.shift_remove(key).is_some()
    }

    pub fn contains(&self, key: &K) -> bool {
        let set = self.set.lock().unwrap();
        set.contains_key(key)
    }

    pub fn clear(&self) {
        let mut set = self.set.lock().unwrap();
        set.clear();
    }

    pub fn clone(&self) -> Self {
        Self {
            set: Arc::clone(&self.set),
        }
    }

    pub fn len(&self) -> usize {
        let set = self.set.lock().unwrap();
        set.len()
    }

    pub fn is_empty(&self) -> bool {
        let set = self.set.lock().unwrap();
        set.is_empty()
    }

    pub fn map_to_vec<U, F>(&self, f: F) -> Vec<U>
    where
    F: FnMut(&S) -> U,
    {
        let set = self.set.lock().unwrap();
        set.values().map(f).collect()
    }
}
