// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use super::Segmented;
use super::worker::{Column, Worker, WorkerError};
use crate::{SSS, nucleo::SegmentableItem};

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

    #[cfg(feature = "experimental")]
    fn extend(
        &self,
        items: impl IntoIterator<Item = Self::InputItem> + ExactSizeIterator,
    ) -> Result<(), WorkerError> {
        let items =
        items.into_iter().map(|item| self.wrap(item)).collect::<Result<Vec<<<Self as Injector>::Inner as Injector>::InputItem>, WorkerError>>()?;
        self.inner().extend(items.into_iter())
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

pub struct WorkerInjector<T, D = ()> {
    pub(super) inner: nucleo::Injector<T>,
    pub(super) columns: Arc<[Column<T, D>]>,
    pub(super) raw_preprocessor: Arc<dyn Fn(&T) -> Option<D> + Send + Sync>,
    pub(super) version: u32,
    pub(super) picker_version: Arc<AtomicU32>,
}

impl<T: SSS, D> Injector for WorkerInjector<T, D> {
    type InputItem = T;
    type Inner = ();
    type Context = Worker<T, D>;

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
        if let Some(d) = (self.raw_preprocessor)(&item) {
            push_impl(&self.inner, &self.columns, item, &d);
        }
        Ok(())
    }

    #[cfg(feature = "experimental")]
    fn extend(
        &self,
        items: impl IntoIterator<Item = T> + ExactSizeIterator,
    ) -> Result<(), WorkerError> {
        if self.version != self.picker_version.load(Ordering::Relaxed) {
            return Err(WorkerError::InjectorShutdown);
        }
        let items: Vec<T> = items
            .into_iter()
            .filter(|item| (self.raw_preprocessor)(item).is_some())
            .collect();
        extend_impl(&self.inner, &self.columns, &self.raw_preprocessor, items.into_iter());
        Ok(())
    }
}

pub(crate) fn push_impl<T, D>(
    injector: &nucleo::Injector<T>,
    columns: &[Column<T, D>],
    item: T,
    d: &D,
) {
    injector.push(item, |item, dst| {
        for (column, text) in columns.iter().filter(|column| column.filter).zip(dst) {
            *text = column.raw(item, d).into()
        }
    });
}

#[cfg(feature = "experimental")]
pub(super) fn extend_impl<T, D, I>(
    injector: &nucleo::Injector<T>,
    columns: &[Column<T, D>],
    raw_preprocessor: &Arc<dyn Fn(&T) -> Option<D> + Send + Sync>,
    items: I,
) where
    I: IntoIterator<Item = T> + ExactSizeIterator,
{
    injector.extend(items, |item, dst| {
        if let Some(d) = raw_preprocessor(item) {
            for (column, text) in columns.iter().filter(|column| column.filter).zip(dst) {
                *text = column.raw(item, &d).into()
            }
        }
    });
}

// ------------------------------------------------------------------------------------------------
pub type SplitterFn<T> = std::sync::Arc<dyn for<'a> Fn(&'a T) -> Box<[(u32, u32)]> + Send + Sync>;

pub struct SegmentedInjector<T, I: Injector<InputItem = Segmented<T>>> {
    injector: I,
    splitter: SplitterFn<T>,
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
        }
    }

    fn wrap(
        &self,
        item: Self::InputItem,
    ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
        let ranges = (self.splitter)(&item);
        Ok(Segmented::new(item, ranges))
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }
}

// ----------- CLONE ----------------------------
impl<T, D> Clone for WorkerInjector<T, D> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            columns: Arc::clone(&self.columns),
            raw_preprocessor: Arc::clone(&self.raw_preprocessor),
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
        }
    }
}
