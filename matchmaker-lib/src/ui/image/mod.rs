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

    /// [`Self::current`] as the widget to draw.
    ///
    /// `header_height` rows hold the header above the image, `scrolled` rows of the image
    /// have passed the pane's top, and the widget occupies `height` rows of the pane.
    ///
    /// The widget exists while the protocol is still being built: it draws the header and
    /// leaves the image's rows reserved and blank, so the pane does not reflow when the
    /// protocol lands. `None` only while the pane has made no request at all.
    pub fn widget(
        &self,
        header_height: usize,
        scrolled: usize,
        height: usize,
    ) -> Option<ImageWidget> {
        self.sent.as_ref()?;
        Some(ImageWidget {
            built: self.current(),
            header_height,
            scrolled,
            height,
            header: None,
        })
    }

    /// Rows the image takes in the document: its fitted rows, magnified by the zoom it
    /// is shown at. Known from the request, before the protocol is built.
    pub fn rows(&self) -> usize {
        self.sent
            .as_ref()
            .map_or(0, |key| zoom_rows(key.display, key.options.zoom) as usize)
    }

    /// Rows the image is fitted into, before the zoom magnifies them: the pane shows no
    /// more of the image than these at a time.
    pub fn display_rows(&self) -> usize {
        self.sent.as_ref().map_or(0, |key| key.display[1] as usize)
    }

    /// Header lines the image's header draws, from the request it is shown under: the
    /// document's header rows, which the text below the image is offset by.
    pub fn header_lines(&self) -> usize {
        self.sent
            .as_ref()
            .map_or(0, |key| key.options.header_lines as usize)
    }
    pub fn last_id(&self) -> Option<usize> {
        self.sent.as_ref().map(|s| s.id)
    }

    /// Whether the worker has built the protocol the pane last asked for.
    pub fn ready(&self) -> bool {
        self.current().is_some()
    }

    /// Cell size in pixels, which an image cannot be scaled without.
    ///
    /// ratatui-image calls it a font size: it is the pixel rectangle of one character
    /// cell, the unit the pane and every image size is counted in.
    fn font_size(&self) -> Option<FontSize> {
        let font = self.picker.font_size();
        (font.width > 0 && font.height > 0).then_some(font)
    }

    /// Prepare the protocol for `image` in the pane left with `area` for it, `hpan`
    /// cells across.
    ///
    /// The pane measures its header and leaves the image what is below it.
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
    /// The protocol the worker built, while it has finished the pane's current request.
    built: Option<Arc<CachedImage>>,
    /// Rows of the pane the header above the image renders into.
    header_height: usize,
    /// Rows of the image that have scrolled past the pane's top.
    scrolled: usize,
    /// Rows of the pane the widget occupies: the header's rows and the image's shown ones.
    height: usize,
    /// The header, drawn above the image, holding the lines the pane's scroll has left.
    header: Option<Paragraph<'static>>,
}

impl ImageWidget {
    /// Draw `header` in the rows the pane measured above the image.
    pub fn with_header(mut self, header: Paragraph<'static>) -> Self {
        self.header = Some(header);
        self
    }
}

impl Widget for ImageWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // The header is drawn in full, wrapped as the pane wraps it.
        if let Some(header) = self.header {
            header.render(area, buf);
        }
        // The image follows the header up the pane, and is cropped to the rows the widget
        // holds: a magnified image is panned through them rather than drawn over the text
        // below it.
        let Some(built) = &self.built else {
            return;
        };
        let y = self.header_height as i32 - self.scrolled as i32;
        let crop = Rect {
            height: self.height.min(area.height as usize) as u16,
            ..area
        };
        SlicedImage::new(
            &built.protocol,
            SignedPosition::from((0, y.clamp(i16::MIN as i32, i16::MAX as i32) as i16)),
        )
        .render(crop, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The header is drawn while the protocol is still being built, with the image's rows
    /// left reserved and blank.
    #[test]
    fn header_is_drawn_while_the_image_builds() {
        let widget = ImageWidget {
            built: None,
            header_height: 1,
            scrolled: 0,
            height: 3,
            header: Some(Paragraph::new("header")),
        };
        let area = Rect::new(0, 0, 10, 3);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let row = (0..10).map(|x| buf[(x, 0)].symbol()).collect::<String>();
        assert!(
            row.starts_with("header"),
            "the header was not drawn: {row:?}"
        );
    }
}
