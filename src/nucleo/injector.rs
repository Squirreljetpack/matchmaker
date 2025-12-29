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
use crate::{MMItem, SegmentableItem, SplitterFn};

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

pub struct WorkerInjector<T> {
    pub(super) inner: nucleo::Injector<T>,
    pub(super) columns: Arc<[Column<T>]>,
    pub(super) version: u32,
    pub(super) picker_version: Arc<AtomicU32>,
}



impl<T: MMItem> Injector for WorkerInjector<T> {
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

pub struct IndexedInjector<T, I: Injector<InputItem = Indexed<T>>> {
    injector: I,
    count: Arc<AtomicU32>,
    input_type: PhantomData<T>,
}

impl<T, I: Injector<InputItem = Indexed<T>>> Injector for IndexedInjector<T, I> {
    type InputItem = T;
    type Inner = I;
    type Context = u32;

    fn new(injector: Self::Inner, count: Self::Context) -> Self {
        Self {
            injector,
            count: Arc::new(AtomicU32::new(count)),
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
