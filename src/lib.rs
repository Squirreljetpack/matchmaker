// event
pub mod config;
pub mod binds;
pub mod action;
pub mod event;

pub mod message;
#[allow(unused)]
pub mod render;
pub mod ui;
// picker
pub mod nucleo;
pub mod spawn;
mod selection;
pub use selection::SelectionSet;
mod matchmaker;
pub use matchmaker::*;
pub mod tui;

// misc
mod utils;
mod aliases;
pub mod errors;
pub use aliases::*;
pub use errors::*;

#[macro_export]
macro_rules! impl_int_wrapper {
    ($name:ident, $inner:ty, $default:expr) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
        pub struct $name(pub $inner);
        
        impl Default for $name {
            fn default() -> Self {
                $name($default)
            }
        }
        
        impl std::str::FromStr for $name {
            type Err = std::num::ParseIntError;
            
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Ok($name(s.parse()?))
            }
        }
        
        impl From<&$name> for $inner {
            fn from(c: &$name) -> Self {
                c.0
            }
        }
    };
}