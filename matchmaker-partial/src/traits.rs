use crate::PartialSetError;

pub trait Set {
    fn set(&mut self, path: &[String], val: &[String]) -> Result<(), PartialSetError>;
}

pub trait Merge {
    fn merge(&mut self, other: Self);
    fn clear(&mut self);
}

pub trait Apply {
    type Partial;

    fn apply(&mut self, partial: Self::Partial);
}

pub fn from<T: Default + Apply<Partial = Q>, Q>(partial: Q) -> T {
    let mut base = T::default();
    base.apply(partial);
    base
}
