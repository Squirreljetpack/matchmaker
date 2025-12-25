pub use anyhow::Result;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MatchError {
    #[error("Aborted: {0}")]
    Abort(i32),
    #[error("Event loop closed.")]
    EventLoopClosed,
    #[error("Became: {0}")]
    Become(String),
    #[error("TUI Error: {0}")]
    TUIError(String),
    #[error("No matcher")]
    NoMatcher
}


#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MapReaderError<E> {
    #[error("Failed to read chunk: {0}")]
    ChunkError(usize),
    #[error("Aborted: {0}")]
    Custom(E),
}