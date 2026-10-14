//! The image the pane is showing, and the protocol being built for it.
//!
//! An image is decoded once, by [`crate::preview::image`], and then has to be fitted to
//! the pane and encoded for the terminal before it can be drawn. That work is far too
//! slow for the render loop, so [`ImageState`] describes the protocol it wants and hands
//! the request to a worker thread ([`super::image_worker`]) that builds it.
//!
//! This module is the pane's half of that. [`ImageKey`] is the wanted protocol, resolved
//! from the image, the pane and the config: how large the image ends up in cells, which
//! columns of it the pane can see, and how its fits and filter combine. Turning that
//! into pixels is the worker's job.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread::JoinHandle;

use super::image_worker::{CachedImage, ImageKey, ImageRequest, image_worker};
use crate::config::{ImageFit, ImageOptions};
use crate::event::EventSender;
use crate::preview::image::PreviewImage;
use crate::ui::image_worker::Window;
use arc_swap::ArcSwapOption;
use log::error;
use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    widgets::Widget,
};
use ratatui_image::{
    FontSize, Resize,
    picker::Picker,
    sliced::{SignedPosition, SlicedImage},
};

/// The picker, the protocol on screen and the request being built.
#[derive(Debug, Default)]
pub(super) struct ImageState {
    picker: Option<Picker>,
    /// The newest protocol the worker built, which the pane renders.
    current: Arc<ArcSwapOption<CachedImage>>,
    /// Key of the newest request the worker was given.
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
    /// State that only builds what the previewer's current `generation` is.
    pub(super) fn new(generation: Arc<AtomicUsize>) -> Self {
        let mut state = Self::default();
        state.generation = generation;
        state
    }

    /// Build protocols with `picker`; anything built with the previous one is dropped.
    pub(super) fn set_picker(&mut self, picker: Option<Picker>) {
        self.picker = picker;
        self.clear();
    }

    /// Wake the render loop when a rebuilt protocol lands.
    pub(super) fn set_event_sender(&mut self, event_tx: EventSender) {
        self.event_tx = Some(event_tx);
    }

    /// The protocol the worker built for the image the pane is showing.
    ///
    /// A protocol for another image is not rendered. One for an older pane size or pan
    /// of this image is, clipped, until the worker finishes the current one.
    pub fn current(&self) -> Option<Arc<CachedImage>> {
        let built = self.current.load_full()?;
        self.sent
            .as_ref()
            .is_some_and(|k| k.id == built.key.id)
            .then_some(built)
    }

    /// The height of the protocol that is on screen, in rows; zero while there is none.
    pub(super) fn rows(&self) -> usize {
        self.current()
            .map_or(0, |built| built.protocol.size().height as usize)
    }

    pub(super) fn last_id(&self) -> Option<usize> {
        self.sent.as_ref().map(|s| s.id)
    }

    /// Cell size in pixels, which an image cannot be scaled without.
    ///
    /// ratatui-image calls it a font size: it is the pixel rectangle of one character
    /// cell, the unit the pane and every image size is counted in.
    pub(super) fn font_size(&self) -> Option<FontSize> {
        let font = self.picker.as_ref()?.font_size();
        (font.width > 0 && font.height > 0).then_some(font)
    }

    /// Forget the protocol that is on screen and anything that was built for it.
    pub(super) fn clear(&mut self) {
        self.sent = None;
        self.pending.store(None);
        self.current.store(None);
    }

    /// Prepare the protocol for `image` in a pane of `area`, `hpan` cells across.
    ///
    /// `hpan` is clamped to the image in place, so a pan past the edge does not take as
    /// many steps to undo as it took to overshoot.
    pub(super) fn prepare(
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
        let display = image.display_size(options, area, font);
        *hpan = (*hpan).min(max_pan(display, area));
        let key = ImageKey {
            id: image.id,
            display,
            window: window(display, area, *hpan),
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
        let Some(picker) = self.picker.clone() else {
            return;
        };
        // The worker only leaves on a panic, and the request it was holding went with
        // it.
        if self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            self.worker = None;
            self.sent = None;
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

impl PreviewImage {
    /// The size of the image in cells, at the scale the axis fit modes ask for.
    ///
    /// `area` is the pane the image is shown in; it must not be empty.
    fn display_size(&self, options: &ImageOptions, area: Rect, font: FontSize) -> [u16; 2] {
        let natural = Resize::natural_size(&self.data, font);
        let scale = image_scale(options, natural, area);
        [
            (natural.width as f32 * scale).round().max(1.0) as u16,
            (natural.height as f32 * scale).round().max(1.0) as u16,
        ]
    }
}

/// The part of the image that is visible for a horizontal pan of `scroll` cells.
fn window(display: [u16; 2], area: Rect, scroll: u16) -> Window {
    let offset = scroll.min(max_pan(display, area));
    Window {
        offset,
        width: area.width.min(display[0] - offset),
    }
}

/// The furthest an image can be panned before it would leave the pane.
fn max_pan(display: [u16; 2], area: Rect) -> u16 {
    display[0].saturating_sub(area.width)
}

/// The uniform scale factor for an image given the axis fit modes.
///
/// See [`ImageFit`] for how `contain`, `fill` and `crop` combine across the axes.
/// Crop axes do not constrain the scale; `contain` axes only shrink and `fill` axes
/// scale to their extent, capped by any `contain` axis.
fn image_scale(options: &ImageOptions, natural: Size, area: Rect) -> f32 {
    let horizontal = options.horizontal_fit;
    let vertical = options.vertical_fit;
    let x = axis_ratio(horizontal, natural.width, area.width);
    let y = axis_ratio(vertical, natural.height, area.height);

    // The image keeps its proportions, so both axes scale together: `fill` asks for
    // its extent and `contain` caps it at the pane. Only `fill` on both axes covers
    // the pane, which the larger of the two scales achieves.
    let cover = matches!(horizontal, ImageFit::Fill(_)) && matches!(vertical, ImageFit::Fill(_));
    match (x, y) {
        (Some(x), Some(y)) if cover => x.max(y),
        (Some(x), Some(y)) => x.min(y),
        (Some(scale), None) | (None, Some(scale)) => scale,
        (None, None) => 1.0,
    }
}

/// The scale one axis asks for, or `None` for a `crop` axis, which follows the other.
fn axis_ratio(fit: ImageFit, natural: u16, available: u16) -> Option<f32> {
    let extent = match fit {
        ImageFit::Contain(0) | ImageFit::Fill(0) => available,
        ImageFit::Contain(cells) | ImageFit::Fill(cells) => cells,
        ImageFit::Crop => return None,
    };
    let ratio = extent.max(1) as f32 / natural.max(1) as f32;
    match fit {
        // `contain` only shrinks.
        ImageFit::Contain(_) => Some(ratio.min(1.0)),
        _ => Some(ratio),
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
    pub built: Arc<CachedImage>,
    pub offset: u16,
}
impl Widget for ImageWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        SlicedImage::new(
            &self.built.protocol,
            SignedPosition::from((0, -(self.offset as i16))),
        )
        .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The pane is 40x20 cells, the image is 100x100 cells, so the pane is narrower
    /// (0.4) than it is shorter (0.2).
    #[test]
    fn fit_matrix() {
        use ImageFit::{Contain, Crop, Fill};

        let natural = Size::new(100, 100);
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let scale = |horizontal_fit, vertical_fit| {
            image_scale(
                &ImageOptions {
                    horizontal_fit,
                    vertical_fit,
                    ..Default::default()
                },
                natural,
                area,
            )
        };

        // Only crop on both axes leaves the native size alone.
        assert_eq!(scale(Crop, Crop), 1.0);
        // Contain never exceeds either axis, so it shrinks to the pane height.
        assert_eq!(scale(Contain(0), Contain(0)), 0.2);
        // The contained axis wins over the filled one.
        assert_eq!(scale(Fill(0), Contain(0)), 0.2);
        assert_eq!(scale(Contain(0), Fill(0)), 0.2);
        // Fill alone scales to its own axis, cropping the other.
        assert_eq!(scale(Fill(0), Crop), 0.4);
        assert_eq!(scale(Crop, Fill(0)), 0.2);
        // Fill on both axes covers the pane, so the larger scale wins.
        assert_eq!(scale(Fill(0), Fill(0)), 0.4);
        assert_eq!(scale(Contain(0), Crop), 0.4);
        assert_eq!(scale(Crop, Contain(0)), 0.2);
    }

    /// A cell count replaces the pane extent on that axis only.
    #[test]
    fn fit_extents() {
        use ImageFit::{Contain, Crop, Fill};

        let natural = Size::new(100, 100);
        let area = Rect {
            x: 0,
            y: 0,
            width: 40,
            height: 20,
        };
        let scale = |horizontal_fit, vertical_fit| {
            image_scale(
                &ImageOptions {
                    horizontal_fit,
                    vertical_fit,
                    ..Default::default()
                },
                natural,
                area,
            )
        };

        // A fill extent scales to it, in either direction.
        assert_eq!(scale(Fill(4), Crop), 0.04);
        assert_eq!(scale(Fill(200), Crop), 2.0);
        // A contain extent only shrinks, so a larger one does nothing.
        assert_eq!(scale(Contain(80), Crop), 0.8);
        assert_eq!(scale(Contain(200), Crop), 1.0);
        // Only the extents that are set interact, the pane no longer does.
        assert_eq!(scale(Fill(0), Fill(4)), 0.4);
        assert_eq!(scale(Fill(4), Contain(0)), 0.04);
    }
}
