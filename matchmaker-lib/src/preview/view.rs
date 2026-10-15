use arc_swap::ArcSwap;
use cba::bait::ResultExt;
use cba::unwrap;
use ratatui::text::{Line, Text};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex};

use super::AppendOnly;
#[cfg(feature = "image")]
use super::image::PreviewImage;

/// What the previewer shows in place of the command's output.
#[derive(Debug, Clone, Default)]
pub enum Override {
    #[default]
    None,
    Text(Text<'static>),
    #[cfg(feature = "image")]
    Image(PreviewImage),
}

impl Override {
    pub fn is_set(&self) -> bool {
        !matches!(self, Self::None)
    }
}

#[derive(Debug)]
pub struct Preview {
    lines: AppendOnly<Line<'static>>,
    last_line: Arc<ArcSwap<Option<Line<'static>>>>,
    override_: Arc<Mutex<Override>>,
    generation: Arc<AtomicUsize>,
}

impl Preview {
    pub fn override_(&self) -> Option<Override> {
        let guard = unwrap!(self.override_.lock().prefix("Previewer panicked")._elog());
        guard.is_set().then(|| guard.clone())
    }

    #[cfg(feature = "image")]
    pub fn image(&self) -> Option<PreviewImage> {
        match self.override_()? {
            Override::Image(image) => Some(image),
            _ => None,
        }
    }

    pub fn generation_handle(&self) -> Arc<AtomicUsize> {
        Arc::clone(&self.generation)
    }

    /// The text to display: the text override, or the lines this preview accumulated.
    pub fn results(&self) -> Text<'static> {
        match self.override_() {
            Some(Override::Text(text)) => text,
            _ => {
                let output = self.lines.read();
                let mut text = Text::from_iter(output.iter().map(|(_, line)| line.clone()));
                if let Some(last_line) = self.last_line.load().as_ref() {
                    text.lines.push(last_line.clone());
                }
                text
            }
        }
    }

    /// Number of text lines in the current content.
    pub fn len(&self) -> usize {
        match self.override_() {
            Some(Override::Text(text)) => text.height(),
            _ => {
                let output = self.lines.read();
                output.iter().count() + usize::from(self.last_line.load().is_some())
            }
        }
    }

    /// Whether there is nothing to display: no text lines and no image override.
    pub fn is_empty(&self) -> bool {
        match self.override_() {
            Some(Override::Text(text)) => text.height() == 0,
            #[cfg(feature = "image")]
            Some(Override::Image(_)) => false,
            _ => {
                let output = self.lines.read();
                output.iter().next().is_none() && self.last_line.load().is_none()
            }
        }
    }

    pub fn new(
        lines: AppendOnly<Line<'static>>,
        last_line: Arc<ArcSwap<Option<Line<'static>>>>,
        override_: Arc<Mutex<Override>>,
        generation: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            lines,
            last_line,
            override_,
            generation,
        }
    }
}
