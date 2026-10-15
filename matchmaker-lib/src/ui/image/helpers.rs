//! How an image is fitted and panned within the pane.
//!
//! Pure geometry: nothing here decodes or resizes pixels, it only works out how many
//! cells the image takes and which of its columns the pane can see. The worker turns
//! that into an encoded protocol.

use image::DynamicImage;
use ratatui::layout::{Rect, Size};
use ratatui_image::{FontSize, Resize};

use super::worker::Window;
use crate::config::{ImageFit, ImageOptions};

/// The area of the image the pane shows for a horizontal pan of `scroll` cells.
pub fn window(display: [u16; 2], area: Rect, scroll: u16) -> Window {
    let offset = scroll.min(max_pan(display, area));
    Window {
        offset,
        width: area.width.min(display[0] - offset),
    }
}

/// The furthest an image can be panned before it would leave the pane.
pub fn max_pan(display: [u16; 2], area: Rect) -> u16 {
    display[0].saturating_sub(area.width)
}

/// The size of `image` in cells, at the scale the axis fit modes ask for.
///
/// `area` is the pane the image is shown in; it must not be empty.
pub fn display_size(
    image: &DynamicImage,
    options: &ImageOptions,
    area: Rect,
    font: FontSize,
) -> [u16; 2] {
    let natural = Resize::natural_size(image, font);
    let scale = image_scale(options, natural, area);
    [
        (natural.width as f32 * scale).round().max(1.0) as u16,
        (natural.height as f32 * scale).round().max(1.0) as u16,
    ]
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
