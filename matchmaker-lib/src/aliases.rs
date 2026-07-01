/// Thread safe (items and fns)
/// These traits are required by Nucleo since it works in a different thread
pub trait SSS: Send + Sync + 'static {}
impl<T: Send + Sync + 'static> SSS for T {}

pub type RenderFn<T> = Box<dyn for<'a> Fn(&'a T, &'a str) -> String + Send + Sync>;
