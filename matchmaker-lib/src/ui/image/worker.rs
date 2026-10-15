//! The thread that turns a wanted protocol into an encoded one.
//!
//! Cropping, resampling and encoding an image is far too slow to do while rendering, so
//! the pane's [`ImageState`](super::ImageState) hands it to a worker thread and picks
//! the result up on a later frame.
//!
//! Only the newest request is ever built: a newer one replaces whatever is waiting, and
//! one the previewer has moved past is dropped.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use arc_swap::ArcSwapOption;
use log::error;
use ratatui::layout::Size;
use ratatui_image::{Resize, picker::Picker, sliced::SlicedProtocol};

use crate::config::ImageOptions;
use crate::event::EventSender;
use crate::message::Event;
use crate::preview::image::PreviewImage;

pub struct CachedImage {
    pub key: ImageKey,
    pub protocol: SlicedProtocol,
}

impl std::fmt::Debug for CachedImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CachedImage")
            .field("key", &self.key)
            .field("protocol", &"<SlicedProtocol>")
            .finish()
    }
}

/// A key to build a protocol for, with what building it needs.
#[derive(Debug)]
pub struct ImageRequest {
    key: ImageKey,
    image: PreviewImage,
    picker: Picker,
}

/// Which protocol the pane wants, resolved from the image, the pane and the config.
///
/// Everything the build needs is in here, so two equal keys want the same protocol: a
/// pane resize that leaves the displayed size and window alone does not rebuild one.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageKey {
    /// Id of the image, which is also its generation.
    pub id: usize,
    /// Full size of the image in cells, at the scale the axis fit modes ask for.
    pub display: [u16; 2],
    /// The part of it the pane can see across its width.
    pub window: Window,
    /// How to fit, crop and resample it.
    pub options: ImageOptions,
}

/// The part of the image the pane shows across its width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First cell column of the image that is visible.
    pub offset: u16,
    /// How many cell columns of it are.
    pub width: u16,
}

impl ImageRequest {
    pub fn new(key: ImageKey, image: PreviewImage, picker: Picker) -> Self {
        Self { key, image, picker }
    }

    /// Crop the image to the visible window, scale it for the pane and encode it for
    /// the terminal.
    fn build(&self) -> Result<SlicedProtocol, String> {
        let source = &self.image.data;
        if source.width() == 0 || source.height() == 0 {
            return Err("image has no pixels".to_string());
        }

        let (x, width) = self.window_source();
        let cropped = source.crop_imm(x, 0, width, source.height());

        let font = self.picker.font_size();
        // A `fill` extent is the only thing that makes the display larger than the
        // image's native cell size, and only then does the protocol have to grow:
        // `Scale` grows to the target while `Fit` caps at it. Below that both land on
        // the same cells, whether the source covers the target or not, so the image is
        // only re-encoded onto the cell grid, never stretched past its own size.
        let natural = Resize::natural_size(source, font);
        let filter = self.key.options.filter;
        let resize = if self.key.display[0] > natural.width || self.key.display[1] > natural.height
        {
            Resize::Scale(Some(filter.into()))
        } else {
            Resize::Fit(Some(filter.into()))
        };

        SlicedProtocol::new_with_resize(
            &self.picker,
            cropped,
            Size::new(self.key.window.width, self.key.display[1]),
            resize,
        )
        .map_err(|error| error.to_string())
    }

    /// The source columns covering the visible pan window.
    ///
    /// The scale comes from the full image, so panning never changes it.
    fn window_source(&self) -> (u32, u32) {
        let source = &self.image.data;
        let font = self.picker.font_size();
        let display = self.key.display;
        let window = self.key.window;
        let ratio = source.width() as f32 / (display[0] as f32 * font.width as f32);
        let x = (window.offset as f32 * font.width as f32 * ratio).floor() as u32;
        let x = x.min(source.width() - 1);
        let width = (window.width as f32 * font.width as f32 * ratio).ceil() as u32;
        (x, width.clamp(1, source.width() - x))
    }
}

/// Builds the protocols the pane publishes, until the pane is gone.
///
/// It waits on a thread park, which the pane ends with an unpark whenever it publishes a
/// request; an unpark is remembered, so a request published between a check and the park
/// that follows it is not missed.
///
/// A request is dropped before it is built when a newer one is already waiting, so a
/// burst of them costs one build, the newest. One that arrives while a build runs waits
/// in the slot and is built next.
pub fn image_worker(
    pending: Arc<ArcSwapOption<ImageRequest>>,
    current: Arc<ArcSwapOption<CachedImage>>,
    generation: Arc<AtomicUsize>,
    event_tx: Option<EventSender>,
    closed: Arc<AtomicBool>,
) {
    loop {
        if closed.load(Ordering::Relaxed) {
            return;
        }
        let Some(request) = pending.swap(None) else {
            std::thread::park();
            continue;
        };

        if request.key.id != generation.load(Ordering::Relaxed) {
            continue;
        }

        match request.build() {
            Ok(protocol) => {
                if request.key.id != generation.load(Ordering::Relaxed) {
                    continue;
                }
                current.store(Some(Arc::new(CachedImage {
                    key: request.key.clone(),
                    protocol,
                })));
                if let Some(event_tx) = &event_tx {
                    let _ = event_tx.send(Event::PreviewFinished);
                }
            }
            // The protocol that is on screen stays; the pane only renders one it asked
            // for, so a request that failed leaves the pane empty rather than wrong.
            Err(error) => error!("Failed to prepare preview image: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use image::DynamicImage;

    use super::*;

    /// A request for a `dimensions` image showing `display` cells, of which `window`
    /// across is visible.
    fn image_request(display: [u16; 2], window: Window, dimensions: [u32; 2]) -> ImageRequest {
        ImageRequest::new(
            ImageKey {
                id: 0,
                display,
                window,
                options: Default::default(),
            },
            PreviewImage {
                data: Arc::new(DynamicImage::new_rgb8(dimensions[0], dimensions[1])),
                options: Default::default(),
                id: 0,
            },
            Picker::halfblocks(),
        )
    }

    fn window(offset: u16, width: u16) -> Window {
        Window { offset, width }
    }

    #[test]
    fn build_sizes_to_the_window() {
        // 100x200 pixels is 10x10 halfblock cells, which is the pane.
        let request = image_request([10, 10], window(0, 10), [100, 200]);
        let protocol = request.build().unwrap();
        assert_eq!(protocol.size(), ratatui::layout::Size::new(10, 10));
    }

    #[test]
    fn window_source() {
        // Half blocks use a 10x20 font, so a 100px wide image spans 10 cells.
        let request = image_request([10, 10], window(3, 5), [100, 200]);
        assert_eq!(request.window_source(), (30, 50));

        // Displayed at half size, one pan cell covers 20 source pixels.
        let request = image_request([5, 10], window(1, 3), [100, 200]);
        assert_eq!(request.window_source(), (20, 60));

        // Past the end only the last column is left.
        let request = image_request([10, 10], window(20, 5), [100, 200]);
        assert_eq!(request.window_source(), (99, 1));
    }
}
