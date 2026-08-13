//! Test-only I/O backend.
//!
//! [`crate::tui::IoStream::Test`] writes into the global [`TEST_BUFFER`] instead
//! of a real terminal, so tests can inspect exactly what the TUI rendered.

use std::{
    io::{self, Write},
    sync::Mutex,
};

/// Global capture buffer for the [`crate::tui::IoStream::Test`] backend.
///
/// Every byte written to a `Test` stream is appended here. Clear it before
/// starting a pick and lock it afterwards to inspect the captured output.
pub static TEST_BUFFER: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// Clears the global [`TEST_BUFFER`].
pub fn clear() {
    if let Ok(mut buffer) = TEST_BUFFER.lock() {
        buffer.clear();
    }
}

/// Returns the captured output as a (lossy) string.
pub fn contents() -> String {
    TEST_BUFFER
        .lock()
        .map(|buffer| String::from_utf8_lossy(&buffer).into_owned())
        .unwrap_or_default()
}

/// Write sink used by [`crate::tui::IoStream::Test`].
#[derive(Debug, Default)]
pub struct TestWriter;

impl Write for TestWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match TEST_BUFFER.lock() {
            Ok(mut buffer) => {
                buffer.extend_from_slice(buf);
                Ok(buf.len())
            }
            Err(_) => Err(io::Error::other("TEST_BUFFER is poisoned")),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
