pub use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MatchmakerError {
    #[error("Aborted: {0}")]
    Abort(i32),
    #[error("Event loop closed.")]
    EventLoopClosed,
    #[error("Became: {0}")]
    Become(String)
}
