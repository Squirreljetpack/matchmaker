use std::ops::{Index, Range};

/// Thread safe (items and fns)
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

// #[easy_ext::ext(MaybeExt)]
// pub impl<T> T
// where
//     T: Sized,
// {
//     fn maybe_take(&mut self, maybe: Option<T>) {
//         if let Some(v) = maybe {
//             *self = v;
//         }
//     }

//     fn maybe_clone(&mut self, maybe: &Option<T>)
//     where
//         T: Clone,
//     {
//         if let Some(v) = maybe {
//             *self = v.clone();
//         }
//     }
// }