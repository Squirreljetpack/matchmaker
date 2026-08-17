mod percentage;
pub use percentage::Percentage;

pub mod crossterm;
pub mod serde;
pub mod string;
pub mod text;

pub use crossterm::{color_to_crossterm, span_to_ansi, style_to_crossterm, text_to_ansi};
