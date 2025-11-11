use ratatui::text::{Line, Text};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use super::AppendOnly;

#[derive(Debug)]
pub struct Preview {
    lines: AppendOnly<Line<'static>>,
    changed: Arc<AtomicBool>,
}

impl Preview {
    pub fn results(&self) -> Text<'_> {
        let output = self.lines.read().unwrap(); // acquire read lock
        Text::from_iter(output.iter().map(|(_, line)| line.clone()))
    }

    pub fn len(&self) -> usize {
        let output = self.lines.read().unwrap();
        output.count()
        // todo: handle overflow possibility
    }

    pub fn is_empty(&self) -> bool {
        let output = self.lines.read().unwrap();
        output.is_empty()
    }

    pub fn changed(&self) -> bool {
        self.changed.swap(false, Ordering::Relaxed)
    }

    pub fn new(lines: AppendOnly<Line<'static>>, changed: Arc<AtomicBool>) -> Self {
        Self { lines, changed }
    }
}
