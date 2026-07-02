#![allow(irrefutable_let_patterns)]

// event
pub mod action;
pub use action::{Action, Actions};
pub mod binds;
pub mod config;
mod config_types;
pub mod event;
mod mode_filter;

pub mod message;
pub mod render;
pub mod ui;
// picker
pub mod collections;
pub mod nucleo;
pub mod preview;
pub use collections::Selector;
mod matchmaker;
pub use matchmaker::*;
pub mod tui;

// misc
mod aliases;
pub mod errors;
mod utils;
pub use aliases::*;
pub use errors::*;

pub mod noninteractive;
