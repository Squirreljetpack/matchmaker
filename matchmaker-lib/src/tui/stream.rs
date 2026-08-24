use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub enum IoStream {
    Stdout,
    #[default]
    BufferedStderr,
    /// Capture all output into [`crate::test::TEST_BUFFER`].
    ///
    /// For tests only: rendering does not depend on a real terminal
    /// (raw mode and terminal sizing are bypassed). Because there is no
    /// terminal to read input from, [`crate::Matchmaker::pick`] runs the
    /// event loop it creates in optional mode (input-less, no crossterm
    /// event stream); a caller-supplied event loop override is kept as-is.
    Test,
}

impl IoStream {
    pub fn to_stream(&self) -> Box<dyn std::io::Write + Send> {
        match self {
            IoStream::Stdout => Box::new(io::stdout()),
            IoStream::BufferedStderr => Box::new(io::LineWriter::new(io::stderr())),
            IoStream::Test => Box::new(crate::test::TestWriter),
        }
    }
}
