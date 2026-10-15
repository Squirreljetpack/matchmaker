//! The images a preview shows, and the formats they come in.
//!
//! An image arrives as the path of a file or as an inline payload, in one of the
//! raster formats [`PreviewFormat`] names or as SVG, which is rasterized on the way
//! in. [`PreviewImage`] is what comes out: the pixels, together with the options to
//! display them with and the id of the preview they belong to. Fitting those pixels
//! to the pane's cells is the UI's job.
//!
//! [`path_header`] builds the `<MM_IMAGE>` token that makes the default previewer
//! show a file as an image.

use std::path::Path;
use std::sync::Arc;

use image::{DynamicImage, ImageFormat};

use crate::config::PartialImageOptions;

#[cfg(feature = "image-svg")]
mod svg;

pub use super::image_header::path_header;

/// Decoded image data with the options it should be displayed with.
#[derive(Debug, Clone)]
pub struct PreviewImage {
    pub data: Arc<DynamicImage>,
    /// Option overrides, applied over the configured options by the UI.
    pub options: PartialImageOptions,
    /// Bumped for every new image, so the UI knows to rebuild its protocol.
    pub id: usize,
}

impl PreviewImage {
    /// Read and decode the image file at `path`.
    ///
    /// `options` are the display option overrides and `id` identifies the preview
    /// this belongs to.
    pub fn decode_path(
        path: &str,
        options: PartialImageOptions,
        id: usize,
    ) -> Result<Self, String> {
        let data = match PreviewFormat::from_path(Path::new(path)) {
            #[cfg(feature = "image-svg")]
            Some(PreviewFormat::Svg) => svg::rasterize_path(Path::new(path)),
            // A path that names no image is still handed to `image`, which reports why
            // it is not one.
            _ => image::open(path).map_err(|error| error.to_string()),
        }?;

        Ok(Self::new(data, options, id))
    }

    /// Decode an inline `payload` that is in `format`.
    ///
    /// `options` are the display option overrides and `id` identifies the preview
    /// this belongs to.
    pub fn decode_payload(
        format: PreviewFormat,
        payload: &[u8],
        options: PartialImageOptions,
        id: usize,
    ) -> Result<Self, String> {
        let data = match format {
            PreviewFormat::Raster(format) => image::load_from_memory_with_format(payload, format)
                .map_err(|error| error.to_string()),
            #[cfg(feature = "image-svg")]
            PreviewFormat::Svg => svg::rasterize(payload),
        }?;

        Ok(Self::new(data, options, id))
    }

    /// Wrap already-decoded `data` with the display option overrides and the id of
    /// the preview it belongs to.
    pub fn new(data: DynamicImage, options: PartialImageOptions, id: usize) -> Self {
        Self {
            data: Arc::new(data),
            options,
            id,
        }
    }
}

/// The format an image is in.
///
/// [`from_extension`](Self::from_extension) and [`from_path`](Self::from_path) only
/// answer with a variant this build can read, so a `Some` means the image can be
/// shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PreviewFormat {
    /// A raster format the `image` crate decodes.
    Raster(ImageFormat),
    /// SVG, rasterized by the `image-svg` feature.
    #[cfg(feature = "image-svg")]
    Svg,
}

impl PreviewFormat {
    /// The format an extension or header name selects, e.g. `png` or `svg`.
    ///
    /// Case is ignored, as it is by [`ImageFormat::from_extension`]. `None` when the
    /// name does not select a format this build has a decoder for.
    pub fn from_extension(extension: &str) -> Option<Self> {
        #[cfg(feature = "image-svg")]
        if svg::is_svg_extension(extension) {
            return Some(Self::Svg);
        }

        let format = ImageFormat::from_extension(extension)?;
        format.reading_enabled().then_some(Self::Raster(format))
    }

    /// The format of the file at `path`, from its extension.
    ///
    /// `None` when the extension selects no format this build can read, which is
    /// also the check for whether a path is an image at all.
    pub fn from_path(path: &Path) -> Option<Self> {
        Self::from_extension(path.extension()?.to_str()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path resolves through its extension, so an image the previewer is handed by
    /// path is one it can decode.
    #[test]
    fn from_path_uses_the_extension() {
        let png = Some(PreviewFormat::Raster(ImageFormat::Png));
        assert_eq!(PreviewFormat::from_extension("png"), png);
        assert_eq!(PreviewFormat::from_extension("PNG"), png);
        assert_eq!(PreviewFormat::from_path(Path::new("/img.png")), png);
        assert_eq!(
            PreviewFormat::from_extension("svg"),
            PreviewFormat::from_path(Path::new("/i.svg"))
        );
        assert_eq!(PreviewFormat::from_path(Path::new("/img")), None);
        assert_eq!(PreviewFormat::from_path(Path::new("/img.unknown")), None);
    }
}
