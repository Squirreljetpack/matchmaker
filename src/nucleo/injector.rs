// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use std::{
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use super::worker::{Column, Worker, WorkerError};
use super::{Indexed, Segmented};
use crate::{SSS, SegmentableItem, SplitterFn};

pub trait Injector {
    type InputItem;
    type Inner: Injector;
    type Context;

    fn new(injector: Self::Inner, data: Self::Context) -> Self;
    fn inner(&self) -> &Self::Inner;
    fn wrap(
        &self,
        item: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError>;

    fn push(&self, item: Self::InputItem) -> Result<(), WorkerError> {
        let item = self.wrap(item)?;
        self.inner().push(item)
    }
}

impl Injector for () {
    fn inner(&self) -> &Self::Inner {
        unreachable!()
    }
    fn new(_: Self::Inner, _: Self::Context) -> Self {
        unreachable!()
    }
    fn wrap(
        &self,
        _: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        unreachable!()
    }

    type Context = ();
    type Inner = ();
    type InputItem = ();
}

pub struct WorkerInjector<T> {
    pub(super) inner: nucleo::Injector<T>,
    pub(super) columns: Arc<[Column<T>]>,
    pub(super) version: u32,
    pub(super) picker_version: Arc<AtomicU32>,
}

impl<T: SSS> Injector for WorkerInjector<T> {
    type InputItem = T;
    type Inner = ();
    type Context = Worker<T>;

    fn new(_: Self::Inner, data: Self::Context) -> Self {
        data.injector()
    }

    fn inner(&self) -> &Self::Inner {
        &()
    }

    fn wrap(
        &self,
        _: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        Ok(())
    }

    fn push(&self, item: T) -> Result<(), WorkerError> {
        if self.version != self.picker_version.load(Ordering::Relaxed) {
            return Err(WorkerError::InjectorShutdown);
        }
        push_impl(&self.inner, &self.columns, item);
        Ok(())
    }
}

pub(super) fn push_impl<T>(injector: &nucleo::Injector<T>, columns: &[Column<T>], item: T) {
    injector.push(item, |item, dst| {
        for (column, text) in columns.iter().filter(|column| column.filter).zip(dst) {
            *text = column.format_text(item).into()
        }
    });
}

// ----- Injectors

#[derive(Clone)]
pub struct IndexedInjector<T, I: Injector<InputItem = Indexed<T>>> {
    injector: I,
    counter: &'static AtomicU32,
    input_type: PhantomData<T>,
}

impl<T, I: Injector<InputItem = Indexed<T>>> Injector for IndexedInjector<T, I> {
    type InputItem = T;
    type Inner = I;
    type Context = &'static AtomicU32;

    fn new(injector: Self::Inner, counter: Self::Context) -> Self {
        Self {
            injector,
            counter,
            input_type: PhantomData,
        }
    }

    fn wrap(
        &self,
        item: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        let index = self.counter.fetch_add(1, Ordering::SeqCst);
        Ok(Indexed { index, inner: item })
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }
}

static GLOBAL_COUNTER: AtomicU32 = AtomicU32::new(0);

impl<T, I> IndexedInjector<T, I>
where
    I: Injector<InputItem = Indexed<T>>,
{
    pub fn new_globally_indexed(injector: <Self as Injector>::Inner) -> Self {
        GLOBAL_COUNTER.store(0, Ordering::SeqCst);
        Self::new(injector, &GLOBAL_COUNTER)
    }

    pub fn global_reset() {
        GLOBAL_COUNTER.store(0, Ordering::SeqCst);
    }
}

pub struct SegmentedInjector<T: SegmentableItem, I: Injector<InputItem = Segmented<T>>> {
    injector: I,
    splitter: SplitterFn<T>,
    input_type: PhantomData<T>,
}

impl<T: SegmentableItem, I: Injector<InputItem = Segmented<T>>> Injector
    for SegmentedInjector<T, I>
{
    type InputItem = T;
    type Inner = I;
    type Context = SplitterFn<T>;

    fn new(injector: Self::Inner, data: Self::Context) -> Self {
        Self {
            injector,
            splitter: data,
            input_type: PhantomData,
        }
    }

    fn wrap(
        &self,
        item: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        let ranges = (self.splitter)(&item);
        Ok(Segmented {
            inner: item,
            ranges,
        })
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }

    fn push(&self, item: Self::InputItem) -> Result<(), WorkerError> {
        let item = self.wrap(item)?;
        self.inner().push(item)
    }
}

// pub type SeenMap<T> = Arc<std::sync::Mutex<collections::HashSet<T>>>;
// #[derive(Clone)]
// pub struct UniqueInjector<T, I: Injector<InputItem = T>> {
//     injector: I,
//     seen: SeenMap<T>,
// }
// impl<T, I> Injector for UniqueInjector<T, I>
// where
//     T: Eq + std::hash::Hash + Clone,
//     I: Injector<InputItem = T>,
// {
//     type InputItem = T;
//     type Inner = I;
//     type Context = SeenMap<T>;

//     fn new(injector: Self::Inner, _ctx: Self::Context) -> Self {
//         Self {
//             injector,
//             seen: _ctx,
//         }
//     }

//     fn wrap(&self, item: Self::InputItem) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
//         let mut seen = self.seen.lock().unwrap();
//         if seen.insert(item.clone()) {
//             Ok(item)
//         } else {
//             Err(WorkerError::Custom("Duplicate"))
//         }
//     }

//     fn inner(&self) -> &Self::Inner {
//         &self.injector
//     }
// }

// ----------- CLONE ----------------------------
impl<T> Clone for WorkerInjector<T> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            columns: Arc::clone(&self.columns),
            version: self.version,
            picker_version: Arc::clone(&self.picker_version),
        }
    }
}

impl<T: SegmentableItem, I: Injector<InputItem = Segmented<T>> + Clone> Clone
    for SegmentedInjector<T, I>
{
    fn clone(&self) -> Self {
        Self {
            injector: self.injector.clone(),
            splitter: Arc::clone(&self.splitter),
            input_type: PhantomData,
        }
    }
}
