// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use std::{
    fmt::{self, Display, Formatter},
    marker::PhantomData,
    sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    },
};

use super::worker::{Column, Worker, WorkerError};
use crate::{PickerItem, SegmentableItem, nucleo::variants::ColumnIndexable};

pub trait Injector: Clone {
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

pub struct WorkerInjector<T, C = ()> {
    pub(super) inner: nucleo::Injector<T>,
    pub(super) columns: Arc<[Column<T, C>]>,
    pub(super) context: Arc<C>,
    pub(super) version: u32,
    pub(super) picker_version: Arc<AtomicU32>,
}



impl<T: PickerItem, C> Injector for WorkerInjector<T, C> {
    type InputItem = T;
    type Inner = ();
    type Context = Worker<T, C>;

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
        push_impl(&self.inner, &self.columns, item, &self.context);
        Ok(())
    }
}

pub(super) fn push_impl<T, C>(injector: &nucleo::Injector<T>, columns: &[Column<T, C>], item: T, context: &C) {
    injector.push(item, |item, dst| {
        for (column, text) in columns.iter().filter(|column| column.filter).zip(dst) {
            *text = column.format_text(item, context).into()
        }
    });
}

// ------------- Wrapper structs
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

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct Indexed<T> {
    pub index: u32,
    pub inner: T,
}

impl<T: Clone> Indexed<T> {
    pub fn identifier(&self) -> (u32, T) {
        (self.index, self.inner.clone())
    }
}

impl<T: ColumnIndexable> ColumnIndexable for Indexed<T> {
    fn index(&self, index: usize) -> &str {
        self.inner.index(index)
    }
}

// ----- Injectors

pub struct IndexedInjector<T, I: Injector<InputItem = Indexed<T>>> {
    injector: I,
    count: Arc<AtomicU32>,
    input_type: PhantomData<T>,
}

impl<T, I: Injector<InputItem = Indexed<T>>> Injector for IndexedInjector<T, I> {
    type InputItem = T;
    type Inner = I;
    type Context = ();

    fn new(injector: Self::Inner, _data: Self::Context) -> Self {
        Self {
            injector,
            count: Arc::new(AtomicU32::new(0)),
            input_type: PhantomData,
        }
    }

    fn wrap(
        &self,
        item: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        let index = self.count.fetch_add(1, Ordering::Relaxed);
        Ok(Indexed { index, inner: item })
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }
}

pub struct SegmentedInjector<T: SegmentableItem, I: Injector<InputItem = Segmented<T>>> {
    injector: I,
    splitter: Arc<dyn Fn(&T) -> Vec<(usize, usize)> + Send + Sync + 'static>,
    input_type: PhantomData<T>,
}

impl<T: SegmentableItem, I: Injector<InputItem = Segmented<T>>> Injector
    for SegmentedInjector<T, I>
{
    type InputItem = T;
    type Inner = I;
    type Context = Arc<dyn Fn(&T) -> Vec<(usize, usize)> + Send + Sync + 'static>;

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
        let ranges = Arc::from((self.splitter)(&item).into_boxed_slice());
        Ok(Segmented {
            inner: item,
            ranges,
        })
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }
}


// ----------- CLONE ----------------------------
impl<T, C> Clone for WorkerInjector<T, C> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            columns: Arc::clone(&self.columns),
            context: Arc::clone(&self.context),
            version: self.version,
            picker_version: Arc::clone(&self.picker_version),
        }
    }
}

impl<T, I: Injector<InputItem = Indexed<T>>> Clone for IndexedInjector<T, I> {
    fn clone(&self) -> Self {
        Self {
            injector: self.injector.clone(),
            count: Arc::clone(&self.count),
            input_type: PhantomData,
        }
    }
}

impl<T: SegmentableItem, I: Injector<InputItem = Segmented<T>>> Clone for SegmentedInjector<T, I> {
    fn clone(&self) -> Self {
        Self {
            injector: self.injector.clone(),
            splitter: Arc::clone(&self.splitter),
            input_type: PhantomData,
        }
    }
}
