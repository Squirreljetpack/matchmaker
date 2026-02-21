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

    fn extend(
        &self,
        items: impl IntoIterator<Item = T> + ExactSizeIterator,
    ) -> Result<(), WorkerError> {
        if self.version != self.picker_version.load(Ordering::Relaxed) {
            return Err(WorkerError::InjectorShutdown);
        }
        extend_impl(&self.inner, &self.columns, items);
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

pub(super) fn extend_impl<T, I>(injector: &nucleo::Injector<T>, columns: &[Column<T>], items: I)
where
    I: IntoIterator<Item = T> + ExactSizeIterator,
{
    injector.extend(items, |item, dst| {
        for (column, text) in columns.iter().filter(|column| column.filter).zip(dst) {
            *text = column.format_text(item).into()
        }
    });
}

// ----- Injectors

/// Wraps the injected item with an atomic index which is incremented on push.
#[derive(Clone)]
pub struct IndexedInjector<T, I: Injector<InputItem = Indexed<T>>> {
    injector: I,
    counter: &'static AtomicU32,
    input_type: PhantomData<T>,
}

// note that invalidation can be handled
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
        Self::global_reset();
        Self::new(injector, &GLOBAL_COUNTER)
    }

    pub fn global_reset() {
        GLOBAL_COUNTER.store(0, Ordering::SeqCst);
    }
}

// ------------------------------------------------------------------------------------------------
pub type SplitterFn<T, const MAX_SPLITS: usize = { crate::MAX_SPLITS }> = std::sync::Arc<
    dyn for<'a> Fn(&'a T) -> arrayvec::ArrayVec<(usize, usize), MAX_SPLITS> + Send + Sync,
>;

pub struct SegmentedInjector<T, I: Injector<InputItem = Segmented<T>>> {
    injector: I,
    splitter: SplitterFn<T>,
}

impl<T, I: Injector<InputItem = Segmented<T>>> Injector for SegmentedInjector<T, I> {
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
        Ok(Segmented {
            inner: item,
            ranges,
        })
    }

    fn inner(&self) -> &Self::Inner {
        &self.injector
    }
}

mod ansi {
    use std::ops::Range;

    pub use crate::utils::Either;
    use crate::{
        nucleo::Text,
        utils::text::{scrub_text_styles, slice_ratatui_text},
    };
    use ansi_to_tui::IntoText;

    pub use super::*;
    pub struct AnsiInjector<I> {
        pub injector: I,
        parse: bool,
    }

    impl<I: Injector<InputItem = Either<String, Text<'static>>>> Injector for AnsiInjector<I> {
        type InputItem = String;
        type Inner = I;
        type Context = bool;

        fn new(injector: Self::Inner, wrap: Self::Context) -> Self {
            Self {
                injector,
                parse: wrap,
            }
        }

        fn wrap(
            &self,
            item: Self::InputItem,
        ) -> Result<<Self::Inner as Injector>::InputItem, WorkerError> {
            let ret = if !self.parse {
                Either::Left(item)
            } else {
                let mut parsed = item.as_bytes().into_text().unwrap_or(Text::from(item));
                scrub_text_styles(&mut parsed);
                Either::Right(parsed)
            };
            Ok(ret)
        }

        fn inner(&self) -> &Self::Inner {
            &self.injector
        }
    }

    impl SegmentableItem for Either<String, Text<'static>> {
        fn slice(&self, range: Range<usize>) -> Text<'_> {
            match self {
                Either::Left(s) => {
                    if range.start == range.end {
                        Text::default()
                    } else {
                        Text::raw(&s[range.start..range.end])
                    }
                }
                Either::Right(text) => slice_ratatui_text(text, range),
            }
        }
    }
}
pub use ansi::*;

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
        }
    }
}
