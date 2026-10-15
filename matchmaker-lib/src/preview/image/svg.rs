//! Rasterizing SVG preview images.
//!
//! An SVG has no pixels of its own, so unlike the other formats it cannot be
//! decoded straight into a [`DynamicImage`] and has to be rendered first. That
//! happens once, at decode time, so everything downstream of
//! [`PreviewImage`](super::PreviewImage) sees an ordinary raster image.
//!
//! The renderer is `resvg` without its optional text and font support: text
//! elements are dropped instead of laid out, which keeps fonts and shaping out
//! of the dependency tree.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

use image::{DynamicImage, RgbaImage};

/// The longest side of a rasterized SVG, in pixels.
///
/// A smaller SVG is scaled up to `MIN` so that it stays sharp when the pane
/// shows it larger; a larger one is scaled down to `MAX`, so the raster cannot
/// grow past these bounds however its size is declared.
const MIN_RASTER_PX: f32 = 256.0;
const MAX_RASTER_PX: f32 = 4096.0;

/// The name an SVG is selected by, as an inline header format or a file
/// extension.
const EXTENSION: &str = "svg";

/// Whether `extension` names an SVG.
pub(super) fn is_svg_extension(extension: &str) -> bool {
    extension.eq_ignore_ascii_case(EXTENSION)
}

/// Rasterize the SVG file at `path`.
pub(super) fn rasterize_path(path: &Path) -> Result<DynamicImage, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    rasterize(&bytes)
}

/// Rasterize `bytes` into an RGBA image.
///
/// The SVG is untrusted preview output, so a panic in the renderer is caught and
/// reported as an error like any other decode failure.
pub(super) fn rasterize(bytes: &[u8]) -> Result<DynamicImage, String> {
    catch_unwind(AssertUnwindSafe(|| rasterize_inner(bytes)))
        .unwrap_or_else(|_| Err("SVG renderer panicked".to_string()))
}

fn rasterize_inner(bytes: &[u8]) -> Result<DynamicImage, String> {
    let tree = resvg::usvg::Tree::from_data(bytes, &resvg::usvg::Options::default())
        .map_err(|error| format!("invalid SVG: {error}"))?;

    let size = tree.size();
    let scale = raster_scale(size.width(), size.height());
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;

    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| format!("SVG raster size {width}x{height} is invalid"))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    RgbaImage::from_raw(width, height, pixmap.take_demultiplied())
        .map(DynamicImage::ImageRgba8)
        .ok_or_else(|| "SVG raster size mismatch".to_string())
}

/// The scale an SVG of `width`x`height` is rasterized at to reach the bounds.
fn raster_scale(width: f32, height: f32) -> f32 {
    let longest = width.max(height);
    if longest < MIN_RASTER_PX {
        MIN_RASTER_PX / longest
    } else if longest > MAX_RASTER_PX {
        MAX_RASTER_PX / longest
    } else {
        1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 10x5 SVG is scaled up to the floor, keeping its proportions.
    #[test]
    fn small_svg_is_scaled_up() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="5">
            <rect width="10" height="5" fill="red"/>
        </svg>"#;
        let image = rasterize(svg).unwrap();
        assert_eq!([image.width(), image.height()], [256, 128]);
        assert_eq!(image.to_rgba8().get_pixel(0, 0).0, [255, 0, 0, 255]);
    }

    /// The default size is 100x100 when the SVG declares neither an extent nor a
    /// viewBox, which is below the floor.
    #[test]
    fn svg_without_a_size_still_renders() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg"><circle cx="5" cy="5" r="5"/></svg>"#;
        let image = rasterize(svg).unwrap();
        assert_eq!([image.width(), image.height()], [256, 256]);
    }

    /// A large SVG is scaled down to the ceiling instead of rasterized at its
    /// declared size.
    #[test]
    fn large_svg_is_scaled_down() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="100000" height="100000"/>"#;
        let image = rasterize(svg).unwrap();
        assert_eq!([image.width(), image.height()], [4096, 4096]);
    }
}
