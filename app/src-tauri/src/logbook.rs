//! The app's recent log, kept in memory for the diagnostics screen.
//!
//! What a person can copy and send when something goes wrong. It holds this
//! process's own lines only: the daemon's log stays with the system, because
//! Xray's warnings there can name the sites someone visited, and the daemon's
//! socket is open to every local user. Nothing secret is logged in the first
//! place — tunnel settings and plans print without their credentials.

use std::collections::VecDeque;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};

use tracing_subscriber::fmt::MakeWriter;

/// How many lines are kept.
const LINES: usize = 400;

/// The recent lines, shared by the logger and the command that reads them.
#[derive(Clone, Default)]
pub(crate) struct Logbook(Arc<Mutex<VecDeque<String>>>);

impl Logbook {
    /// The kept lines, oldest first.
    pub(crate) fn lines(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    fn push(&self, line: String) {
        let mut lines = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if lines.len() == LINES {
            lines.pop_front();
        }
        lines.push_back(line);
    }
}

impl<'a> MakeWriter<'a> for Logbook {
    type Writer = Entry;

    fn make_writer(&'a self) -> Self::Writer {
        Entry {
            book: self.clone(),
            text: Vec::new(),
        }
    }
}

/// One log event: written to stderr as usual, and kept.
pub(crate) struct Entry {
    book: Logbook,
    text: Vec<u8>,
}

impl Write for Entry {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.text.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Drop for Entry {
    // The formatter writes an event in pieces and drops the writer after it.
    fn drop(&mut self) {
        let _ = std::io::stderr().write_all(&self.text);
        let line = String::from_utf8_lossy(&self.text).trim_end().to_owned();
        if !line.is_empty() {
            self.book.push(line);
        }
    }
}
