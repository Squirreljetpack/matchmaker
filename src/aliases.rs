use std::ops::{Index, Range};

use arrayvec::ArrayVec;

/// Thread safe (items and fns)
/// These traits are required by Nucleo since it works in a different thread
pub trait SSS: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> SSS for T {}

#[cfg(feature = "parallelism")]
pub trait Selection: Send + 'static {}

#[cfg(not(feature = "parallelism"))]
pub trait Selection {}

#[cfg(feature = "parallelism")]
impl<T: Send + 'static> Selection for T {}

#[cfg(not(feature = "parallelism"))]
impl<T> Selection for T {}

pub type Identifier<T, S> = fn(&T) -> (u32, S);

pub trait SegmentableItem: SSS + Index<Range<usize>, Output = str> {}
impl<T: SSS + Index<Range<usize>, Output = str>> SegmentableItem for T {}

// pub trait HashSetLike {}

// pub trait HashMapLike {}

pub const MAX_SPLITS: usize = 10;
pub type RenderFn<T> = Box<dyn for<'a> Fn(&'a T, &'a str) -> String + Send + Sync>;
pub type SplitterFn<T> =
    std::sync::Arc<dyn for<'a> Fn(&'a T) -> ArrayVec<(usize, usize), MAX_SPLITS> + Send + Sync>;

pub const MAX_ACTIONS: usize = 6;
pub const MAX_EFFECTS: usize = 12; // number of effect discriminants
