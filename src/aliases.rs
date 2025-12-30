use std::ops::{Index, Range};

use arrayvec::ArrayVec;

/// Thread safe (items and fns)
/// Sync and Send is required by Nucleo
pub trait MMItem: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> MMItem for T {}

// Send is required just for cycle_all_bg
// todo: lowpri: get rid of this
pub trait Selection: Send + 'static {}
impl<T: Send + 'static> Selection for T {}
pub type Identifier<T, S> = fn(&T) -> (u32, S);

pub trait SegmentableItem: MMItem + Index<Range<usize>, Output = str> {}
impl<T: MMItem + Index<Range<usize>, Output = str>> SegmentableItem for T {}

// pub trait HashSetLike {}

// pub trait HashMapLike {}

pub const MAX_SPLITS: usize = 10;
pub type RenderFn<T> = Box<dyn for<'a> Fn(&'a T, &'a str) -> String + Send + Sync>;
pub type SplitterFn<T> = std::sync::Arc<dyn for<'a> Fn(&'a T) -> ArrayVec<(usize, usize), MAX_SPLITS> + Send + Sync>;

pub const MAX_ACTIONS: usize = 6;
pub const MAX_EFFECTS: usize = 12; // number of effect discriminants
