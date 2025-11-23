use std::ops::{Index, Range};

pub trait MMItem: Sync + Send + 'static {}
impl<T: Sync + Send + 'static> MMItem for T {}

pub trait Selection: Send + 'static {}
impl<T:  Send + 'static> Selection for T {}

pub trait SegmentableItem: MMItem + Index<Range<usize>, Output = str> {}
impl<T: MMItem + Index<Range<usize>, Output = str>> SegmentableItem for T {}

// pub trait HashSetLike {}

// pub trait HashMapLike {}

pub type RenderFn<T> = Box<dyn for<'a> Fn(&'a T, &'a str) -> String + Send + Sync>;
pub type SplitterFn<T> = std::sync::Arc<dyn for<'a> Fn(&'a T) -> Vec<(usize, usize)> + Send + Sync>;
