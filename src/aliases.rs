use std::ops::{Index, Range};

pub trait PickerItem: Sync + Send + 'static {}
impl<T: Sync + Send + 'static> PickerItem for T {}

pub trait Selection: Send + PartialEq +'static {}
impl<T:  Send + PartialEq + 'static> Selection for T {}

pub trait SegmentableItem: PickerItem + Index<Range<usize>, Output = str> {}
impl<T: PickerItem + Index<Range<usize>, Output = str>> SegmentableItem for T {}

// pub trait HashSetLike {}

// pub trait HashMapLike {}
