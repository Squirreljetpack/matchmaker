use serde::{Deserialize, Serialize};
use std::io;

#[derive(Debug, Clone, Deserialize, Default, Serialize, PartialEq)]
pub enum IoStream {
    /// Pick a render target automatically at startup; see [`IoStream::resolve`].
    #[default]
    Auto,
    Stdout,
    BufferedStderr,
    /// The controlling terminal device (`/dev/tty` on unix).
    Tty,
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
    /// Resolve [`IoStream::Auto`] into a concrete render target.
    ///
    /// Selection order: stderr when it is a terminal, then the controlling
    /// terminal device. Stdout is never selected so it stays reserved for
    /// selection output. Fails when nothing terminal-like is available:
    /// rendering into a redirected stderr would corrupt its target while
    /// the UI stays invisible.
    pub fn resolve(self) -> io::Result<Self> {
        match self {
            Self::Auto => auto_detect(),
            other => Ok(other),
        }
    }

    pub fn to_stream(&self) -> io::Result<Box<dyn std::io::Write + Send>> {
        match self {
            // Resolved by [`crate::tui::Tui`] before any writer is built.
            Self::Auto => Err(io::Error::other(
                "IoStream::Auto must be resolved before building a writer",
            )),
            Self::Stdout => Ok(Box::new(io::stdout())),
            Self::BufferedStderr => Ok(Box::new(io::LineWriter::new(io::stderr()))),
            Self::Tty => Ok(Box::new(open_tty()?)),
            Self::Test => Ok(Box::new(crate::test::TestWriter)),
        }
    }
}

fn auto_detect() -> io::Result<IoStream> {
    use std::io::IsTerminal;

    if io::stderr().is_terminal() {
        return Ok(IoStream::BufferedStderr);
    }
    match open_tty() {
        // The controlling terminal is available; the probe handle is dropped
        // and [`IoStream::to_stream`] opens its own.
        Ok(_) => Ok(IoStream::Tty),
        Err(e) => Err(io::Error::other(format!(
            "no usable render target: stderr is not a terminal and the controlling terminal is unavailable ({e})"
        ))),
    }
}

#[cfg(unix)]
fn open_tty() -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
}

#[cfg(windows)]
fn open_tty() -> io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("CONOUT$")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_passes_concrete_streams_through() {
        assert_eq!(IoStream::Stdout.resolve().unwrap(), IoStream::Stdout);
        assert_eq!(IoStream::Tty.resolve().unwrap(), IoStream::Tty);
        assert_eq!(IoStream::Test.resolve().unwrap(), IoStream::Test);
        assert_eq!(
            IoStream::BufferedStderr.resolve().unwrap(),
            IoStream::BufferedStderr
        );
    }

    #[test]
    fn auto_cannot_build_a_writer_directly() {
        assert!(IoStream::Auto.to_stream().is_err());
    }
}
