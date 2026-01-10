use crate::{Identifier, Selection};
use indexmap::IndexMap;
use rustc_hash::FxBuildHasher;
use std::sync::Mutex;
use std::{borrow::Borrow, hash::Hash, sync::Arc};

pub type SelectionValidator<S> = fn(&S) -> bool;

#[derive(Debug)]
pub struct Selector<T, S> {
    selections: Option<SelectorImpl<u32, S>>,
    pub identifier: Identifier<T, S>,
    pub validator: SelectionValidator<S>,
}

pub fn truthy_validator<S>(_: &S) -> bool {
    true
}

impl<T, S: Selection> Selector<T, S> {
    pub fn new(identifier: Identifier<T, S>) -> Self {
        Self::new_with_validator(identifier, truthy_validator)
    }

    pub fn new_with_validator(
        identifier: Identifier<T, S>,
        validator: SelectionValidator<S>,
    ) -> Self {
        Self {
            selections: Some(SelectorImpl::new()),
            identifier,
            validator,
        }
    }

    pub fn disabled(mut self) -> Self {
        self.selections = None;
        self
    }

    // --------------------------------------------

    pub fn sel(&mut self, item: &T) -> bool {
        let Some(selections) = &mut self.selections else {
            return false;
        };

        let (k, v) = (self.identifier)(item);
        selections.insert(k, v)
    }

    pub fn desel(&mut self, item: &T) -> bool {
        let Some(selections) = &mut self.selections else {
            return false;
        };

        let (k, _v) = (self.identifier)(item);
        selections.remove(&k)
    }

    pub fn contains(&self, item: &T) -> bool {
        let Some(selections) = &self.selections else {
            return false;
        };

        let (k, _v) = (self.identifier)(item);
        selections.contains(&k)
    }

    pub fn toggle(&mut self, item: &T) {
        let Some(selections) = &mut self.selections else {
            return;
        };

        let (k, v) = (self.identifier)(item);
        if selections.contains(&k) {
            selections.remove(&k);
        } else {
            selections.insert(k, v);
        }
    }

    pub fn clear(&mut self) {
        if let Some(selections) = &mut self.selections {
            selections.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.selections.as_ref().map_or(0, |s| s.len())
    }

    // -----------------------------------------------------

    pub fn is_empty(&self) -> bool {
        self.selections.as_ref().is_none_or(|s| s.is_empty())
    }

    pub fn output(&mut self) -> impl Iterator<Item = S> {
        if let Some(selections) = &mut self.selections {
            let mut set = selections.set.lock().unwrap();
            std::mem::take(&mut *set).into_values()
        } else {
            IndexMap::with_capacity(0).into_values()
        }
    }

    pub fn identify_to_vec<I>(&self, items: I) -> Vec<S>
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<T> + Send,
    {
        items
            .into_iter()
            .map(|item| (self.identifier)(item.borrow()).1)
            .collect()
    }

    pub fn map_to_vec<U, F>(&self, f: F) -> Vec<U>
    where
        F: FnMut(&S) -> U,
    {
        self.selections
            .as_ref()
            .map_or_else(Vec::new, |s| s.map_to_vec(f))
    }

    pub fn revalidate(&mut self) {
        let Some(selections) = &mut self.selections else {
            return;
        };

        let mut set = selections.set.lock().unwrap();
        let validator = &self.validator;

        set.retain(|_, v| validator(v));
    }

    pub fn cycle_all_bg<I>(&self, items: I)
    where
        I: IntoIterator,
        I::Item: std::borrow::Borrow<T> + Send,
    {
        let Some(selections) = &self.selections else {
            return;
        };

        let results: Vec<_> = items
            .into_iter()
            .map(|item| (self.identifier)(item.borrow()))
            .collect();

        let selections = selections.clone();

        #[cfg(feature = "parallelism")]
        tokio::task::spawn_blocking(move || {
            let mut set_guard = selections.set.lock().unwrap();

            let mut all = true;
            let mut seen = 0;
            for (i, (k, _)) in results.iter().enumerate() {
                if !set_guard.contains_key(k) {
                    all = false;
                    seen = i;
                    break;
                }
            }

            if all {
                for (k, _) in results {
                    set_guard.swap_remove(&k);
                }
            } else {
                for (k, v) in results.into_iter().skip(seen) {
                    set_guard.insert(k, v);
                }
            }
        });

        #[cfg(not(feature = "parallelism"))]
        {
            let mut set_guard = selections.set.lock().unwrap();

            let mut all = true;
            let mut seen = 0;
            for (i, (k, _)) in results.iter().enumerate() {
                if !set_guard.contains_key(k) {
                    all = false;
                    seen = i;
                    break;
                }
            }

            if all {
                for (k, _) in results {
                    set_guard.swap_remove(&k);
                }
            } else {
                for (k, v) in results.into_iter().skip(seen) {
                    set_guard.insert(k, v);
                }
            }
        }
    }
}

// ---------- Selection Set ---------------
#[derive(Debug, Clone)]
struct SelectorImpl<K: Eq + Hash, S> {
    pub set: Arc<Mutex<IndexMap<K, S, FxBuildHasher>>>,
}

impl<K: Eq + Hash, S> SelectorImpl<K, S>
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
