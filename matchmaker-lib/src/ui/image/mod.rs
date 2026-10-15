//! The image the pane is showing, and the protocol being built for it.
//!
//! An image is decoded once, by [`crate::preview::image`], and then has to be fitted to
//! the pane and encoded for the terminal before it can be drawn. That work is far too
//! slow for the render loop, so [`ImageState`] describes the protocol it wants and hands
//! the request to a worker thread ([`worker`]) that builds it.
//!
//! This module is the pane's half of that. [`ImageKey`] is the wanted protocol, resolved
//! from the image, the pane and the config: how large the image ends up in cells, which
//! columns of it the pane can see, and how its fits and filter combine. Turning that
//! into pixels is the worker's job.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use arc_swap::ArcSwapOption;
use log::error;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::Line,
    widgets::{Paragraph, Widget},
};
use ratatui_image::{
    FontSize,
    picker::Picker,
    sliced::{SignedPosition, SlicedImage},
};

use crate::config::ImageOptions;
use crate::event::EventSender;
use crate::preview::image::PreviewImage;

mod helpers;
mod worker;

use helpers::{display_size, max_pan, window, zoom_rows};
use worker::{CachedImage, ImageKey, ImageRequest, image_worker};

/// The picker, the request for the image the pane shows, and the pixels built for it.
#[derive(Debug)]
pub struct ImageState {
    /// Builds protocols for the terminal the pane is drawn on.
    picker: Picker,
    /// The newest protocol the worker built, which the pane renders.
    current: Arc<ArcSwapOption<CachedImage>>,
    /// Key of the newest request the worker was given, which the pane's image geometry
    /// comes from.
    sent: Option<ImageKey>,
    /// The request the worker has not taken yet. A newer one replaces it.
    pending: Arc<ArcSwapOption<ImageRequest>>,
    /// The previewer's generation: a request for anything but its current value is not
    /// worth building.
    generation: Arc<AtomicUsize>,
    /// The worker thread, while it is running.
    worker: Option<JoinHandle<()>>,
    /// Tells the worker the pane is gone, so it stops waiting.
    closed: Arc<AtomicBool>,
    /// Wakes the render loop when a rebuilt protocol lands.
    event_tx: Option<EventSender>,
}

impl ImageState {
    /// State that builds for `picker` and only what the previewer's current
    /// `generation` is.
    pub fn new(generation: Arc<AtomicUsize>, picker: Picker) -> Self {
        Self {
            picker,
            current: Default::default(),
            sent: None,
            pending: Default::default(),
            generation,
            worker: None,
            closed: Default::default(),
            event_tx: None,
        }
    }

    /// Wake the render loop when a rebuilt protocol lands.
    pub fn set_event_sender(&mut self, event_tx: EventSender) {
        self.event_tx = Some(event_tx);
    }

    /// Forget the request the pane made and the pixels built for it, reporting whether
    /// it was holding an image.
    pub fn clear(&mut self) -> bool {
        let was_showing = self.sent.is_some();
        self.sent = None;
        self.pending.store(None);
        self.current.store(None);
        was_showing
    }

    /// The protocol the worker built for the image the pane is showing.
    ///
    /// A protocol for another image is not rendered. One for an older pane size or pan
    /// of this image is, clipped, until the worker finishes the current one.
    fn current(&self) -> Option<Arc<CachedImage>> {
        let built = self.current.load_full()?;
        self.sent
            .as_ref()
            .is_some_and(|k| k.id == built.key.id)
            .then_some(built)
    }

    /// [`Self::current`], sliced at the pane's scroll `offset`, as the widget to draw.
    ///
    /// `None` while no protocol for the current image has been built yet.
    pub fn widget(&self, offset: u16) -> Option<ImageWidget> {
        Some(ImageWidget {
            built: self.current()?,
            offset,
            header: Vec::new(),
        })
    }

    /// Rows the image takes in the document: its fitted rows, magnified by the zoom it
    /// is shown at. Known from the request, before the protocol is built.
    pub fn rows(&self) -> usize {
        self.sent
            .as_ref()
            .map_or(0, |key| zoom_rows(key.display, key.options.zoom) as usize)
    }

    /// Header rows the image holds above it, from the request it is shown under.
    pub fn header_lines(&self) -> usize {
        self.sent
            .as_ref()
            .map_or(0, |key| key.options.header_lines as usize)
    }

    /// Rows the image block covers in a pane `height` rows tall, with the document
    /// scrolled `offset` rows down: its header rows and the image, less whatever has
    /// gone above the pane.
    pub fn painted_rows(&self, offset: usize, height: u16) -> usize {
        let rows = self.rows();
        if rows == 0 {
            return 0;
        }
        (self.header_lines() + rows)
            .saturating_sub(offset)
            .min(height as usize)
    }

    pub fn last_id(&self) -> Option<usize> {
        self.sent.as_ref().map(|s| s.id)
    }

    /// Cell size in pixels, which an image cannot be scaled without.
    ///
    /// ratatui-image calls it a font size: it is the pixel rectangle of one character
    /// cell, the unit the pane and every image size is counted in.
    fn font_size(&self) -> Option<FontSize> {
        let font = self.picker.font_size();
        (font.width > 0 && font.height > 0).then_some(font)
    }

    /// Prepare the protocol for `image` in a pane of `area`, `hpan` cells across.
    ///
    /// `hpan` is clamped to the image in place, so a pan past the edge does not take as
    /// many steps to undo as it took to overshoot.
    pub fn prepare(
        &mut self,
        image: PreviewImage,
        options: &ImageOptions,
        area: Rect,
        hpan: &mut u16,
    ) {
        let Some(font) = self.font_size() else {
            return;
        };
        if area.width == 0 || area.height == 0 {
            return;
        }
        let display = display_size(&image.data, options, area, font);
        *hpan = (*hpan).min(max_pan(display, area, options.zoom));
        let key = ImageKey {
            id: image.id,
            display,
            window: window(display, area, options.zoom, *hpan),
            options: options.clone(),
        };
        if self.sent.as_ref() == Some(&key) {
            return;
        }
        self.request(key, image);
    }

    /// Ask the worker to build for a `key`, unless it has it or is already on it.
    ///
    /// Starts the worker on the first request.
    fn request(&mut self, key: ImageKey, image: PreviewImage) {
        let picker = self.picker.clone();
        // Restart worker if panicked
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            self.worker = None;
        }
        if self.worker.is_none() {
            let pending = Arc::clone(&self.pending);
            let current = Arc::clone(&self.current);
            let generation = Arc::clone(&self.generation);
            let closed = Arc::clone(&self.closed);
            let event_tx = self.event_tx.clone();
            let spawned = std::thread::Builder::new()
                .name("mm-image".to_string())
                .spawn(move || image_worker(pending, current, generation, event_tx, closed));
            match spawned {
                Ok(worker) => self.worker = Some(worker),
                Err(error) => {
                    error!("Failed to spawn the preview image worker: {error}");
                    return;
                }
            }
        }

        self.pending.store(Some(Arc::new(ImageRequest::new(
            key.clone(),
            image,
            picker,
        ))));
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
        self.sent = Some(key);
    }
}

impl Drop for ImageState {
    fn drop(&mut self) {
        // The worker waits on a park, so it has to be woken to see this.
        self.closed.store(true, Ordering::Relaxed);
        if let Some(worker) = &self.worker {
            worker.thread().unpark();
        }
    }
}

/// Owned widget wrapping the built protocol so [`ImageState`] does not have to borrow it
/// across the render call.
pub struct ImageWidget {
    built: Arc<CachedImage>,
    offset: u16,
    /// Lines drawn in the header rows the request reserved above the image.
    header: Vec<Line<'static>>,
}

impl ImageWidget {
    /// Text to draw in the request's header rows, above the image.
    pub fn with_header(mut self, header: Vec<Line<'static>>) -> Self {
        self.header = header;
        self
    }
}

impl Widget for ImageWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let header_rows = self.built.key.options.header_lines;
        // The header never wraps; it scrolls with the image.
        if !self.header.is_empty() {
            Paragraph::new(self.header)
                .scroll((self.offset, 0))
                .render(area, buf);
        }
        let y = i32::from(header_rows) - i32::from(self.offset);
        SlicedImage::new(
            &self.built.protocol,
            SignedPosition::from((0, y.clamp(i16::MIN as i32, i16::MAX as i32) as i16)),
        )
        .render(area, buf);
    }
}
