pub use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MatchError {
    /// Exited via [`crate::action::Action::Quit`]
    #[error("Aborted: {0}")]
    Abort(i32),
    /// Event loop closed
    #[error("Event loop closed.")]
    EventLoopClosed,
    /// Exited via [`crate::action::Action::Become`]
    #[error("Became: {0}")]
    Become(String),
    /// Critical error in TUI execution
    #[error("TUI Error: {0}")]
    TUIError(String),
    /// Should not arise in normal execution
    #[error("Empty")]
    Empty
}


