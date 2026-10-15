//! Parsing of the `<MM_IMAGE>` preview header.
//!
//! A preview command may start its output with a header that makes matchmaker show
//! an image instead of text. The first line is a real `RS` byte (`\x1e`, `0x1e`)
//! followed by the tag, whose byte count says how many bytes of the output belong
//! to the image:
//!
//! ```text
//! \x1e<MM_IMAGE:9> hfit=contain vfit=crop filter=triangle scroll=3
//! /path/img
//! ```
//!
//! In path mode (`<MM_IMAGE:N>`) the `N` bytes after the newline are the path of the
//! image. In inline mode (`<MM_IMAGE/fmt:N>`) they are the image data in the named
//! format (`fmt` = any name
//! [`PreviewFormat::from_extension`](super::image::PreviewFormat::from_extension)
//! selects):
//!
//! ```text
//! \x1e<MM_IMAGE/png:4>
//! <4 png bytes ...>
//! ```
//!
//! The count delimits the image, so no terminator byte is needed and a producer can
//! put ordinary output after it. Everything past the declared bytes is displayed as
//! text, in the same preview.
//!
//! The options are `hfit` and `vfit` (`contain`, `fill` or `crop`, the first two
//! optionally with a cell count, e.g. `fill:16`; see
//! [`ImageFit`](crate::config::ImageFit) for how they combine), `filter`
//! (`nearest`, `triangle`, `catmull_rom`, `gaussian`, `lanczos3`) and the initial
//! `scroll` offset, written `vertical`, `vertical,horizontal` or `,horizontal`
//! cell counts.
//!
//! Anything that does not match exactly is ordinary preview output and is passed
//! through unchanged.

use std::path::Path;

use super::image::{PreviewFormat, PreviewImage};
use crate::config::PartialImageOptions;
use log::error;

/// Text that introduces an image preview: `\x1e<MM_IMAGE`.
const MAGIC: &str = "\x1e<MM_IMAGE";

/// The path-mode `<MM_IMAGE>` header for `path`: the token that makes a preview
/// show the file as an image. The declared byte count is the path's length.
/// Returns `None` when the build has no decoder for the path's format.
pub fn path_header(path: &Path) -> Option<String> {
    PreviewFormat::from_path(path)?;

    let path = path.to_string_lossy();
    Some(format!("{MAGIC}:{}>\n{}", path.len(), path))
}

/// State of the `<MM_IMAGE>` header parser.
///
/// The output is accumulated by the caller, which hands it in with
/// [`HeaderParser::feed`]. Only bytes that are understood are consumed, so output
/// that turns out to be text is left untouched for the caller to display.
#[derive(Debug, PartialEq)]
pub enum HeaderParser {
    /// Looking for a header at the start of the output.
    Sniffing,
    /// The header line is parsed; the declared path bytes are collected next.
    Path {
        /// Option overrides from the header line.
        options: PartialImageOptions,
        /// Path bytes still expected.
        remaining: usize,
        /// Path bytes received so far.
        path: Vec<u8>,
    },
    /// An inline image; the declared payload bytes are collected next.
    Inline {
        /// Format the payload is in.
        format: PreviewFormat,
        /// Option overrides from the header line.
        options: PartialImageOptions,
        /// Payload bytes still expected.
        remaining: usize,
        /// Image data received so far.
        payload: Vec<u8>,
    },
    /// The output is not an image.
    Text,
}

impl HeaderParser {
    /// A parser looking for image headers, unless image previews are disabled.
    pub fn new(enabled: bool) -> Self {
        if enabled { Self::Sniffing } else { Self::Text }
    }

    /// Give the parser the output that was read, and return the state it is left in.
    ///
    /// The bytes are routed to the buffer they belong to: the header attempt (kept in
    /// `output`, so output that turns out to be text is still displayed), the image,
    /// or the text that follows it.
    pub fn feed(&mut self, chunk: &[u8], output: &mut Vec<u8>) -> &Self {
        match self {
            Self::Sniffing => self.sniff(chunk, output),
            Self::Path { .. } | Self::Inline { .. } => self.collect(chunk, output),
            Self::Text => output.extend_from_slice(chunk),
        }

        self
    }

    pub fn decode(self, id: usize) -> Option<Result<PreviewImage, String>> {
        match self {
            HeaderParser::Sniffing | HeaderParser::Text => None,
            HeaderParser::Path {
                options,
                path,
                remaining,
            } => {
                if remaining > 0 {
                    return Some(Err(
                        "<MM_IMAGE> header declared more bytes than the output contained"
                            .to_string(),
                    ));
                }
                let path = String::from_utf8_lossy(&path);
                Some(PreviewImage::decode_path(path.as_ref(), options, id))
            }
            HeaderParser::Inline {
                options,
                remaining,
                format,
                payload,
            } => {
                if remaining > 0 {
                    return Some(Err(
                        "<MM_IMAGE> header declared more bytes than the output contained"
                            .to_string(),
                    ));
                }
                Some(PreviewImage::decode_payload(format, &payload, options, id))
            }
        }
    }

    /// Collect a header from `chunk`.
    ///
    /// The header line ends at a newline; until it arrives the bytes are kept in
    /// `output`, so output that turns out to be text is still displayed. A parsed header
    /// leaves the parser collecting the declared image bytes. Does nothing when the
    /// parser is not looking for a header.
    fn sniff(&mut self, chunk: &[u8], output: &mut Vec<u8>) {
        if !matches!(self, Self::Sniffing) {
            return;
        }

        let start = output.len();
        output.extend_from_slice(chunk);
        if !MAGIC
            .as_bytes()
            .starts_with(&output[..output.len().min(MAGIC.len())])
        {
            *self = Self::Text;
            return;
        }

        // Only the new bytes are searched for the end of the line.
        let Some(newline) = chunk.iter().position(|b| *b == b'\n') else {
            return;
        };
        let end = start + newline;
        let line = &output[..end];
        let Some((format, len, options)) = parse_first_line(line) else {
            error!(
                "invalid <MM_IMAGE> header: {:?}",
                String::from_utf8_lossy(line)
            );
            *self = Self::Text;
            return;
        };

        // The line is removed in place; whatever follows it starts the image.
        output.drain(..end + 1);
        let rest = std::mem::take(output);

        *self = match format {
            Some(format) => Self::Inline {
                format,
                options,
                remaining: len,
                payload: Vec::new(),
            },
            None => Self::Path {
                options,
                remaining: len,
                path: Vec::new(),
            },
        };
        self.collect(&rest, output);
    }

    /// Collect the declared image bytes from `chunk`.
    ///
    /// Anything left over once the image is complete is ordinary output and is passed
    /// to `output`, so text that follows an image is displayed as text.
    fn collect(&mut self, chunk: &[u8], output: &mut Vec<u8>) {
        let take;
        match self {
            Self::Path {
                remaining, path, ..
            } => {
                take = chunk.len().min(*remaining);
                path.extend_from_slice(&chunk[..take]);
                *remaining -= take;
            }
            Self::Inline {
                remaining, payload, ..
            } => {
                take = chunk.len().min(*remaining);
                payload.extend_from_slice(&chunk[..take]);
                *remaining -= take;
            }
            Self::Text => {
                output.extend_from_slice(chunk);
                return;
            }
            Self::Sniffing => unreachable!("collect is only called on a parsed header"),
        }

        if self.remaining() != Some(0) {
            return;
        }

        output.extend_from_slice(&chunk[take..]);
    }

    /// The bytes of the image that are still expected, if this is collecting one.
    fn remaining(&self) -> Option<usize> {
        match self {
            Self::Path { remaining, .. } | Self::Inline { remaining, .. } => Some(*remaining),
            Self::Sniffing | Self::Text => None,
        }
    }
}

/// Parse the first line (without the newline) after the magic prefix.
///
/// Returns the format for inline mode (`None` for path mode), the declared byte
/// count, and the parsed options, or `None` if the line is not a valid image header.
fn parse_first_line(line: &[u8]) -> Option<(Option<PreviewFormat>, usize, PartialImageOptions)> {
    let rest = line.strip_prefix(MAGIC.as_bytes())?;

    // `<MM_IMAGE/N>` selects inline mode, `<MM_IMAGE:N>` path mode.
    let (format, rest) = if let Some(rest) = rest.strip_prefix(b"/") {
        let colon = rest.iter().position(|b| *b == b':')?;
        let name = std::str::from_utf8(&rest[..colon]).ok()?;
        (
            Some(PreviewFormat::from_extension(name)?),
            &rest[colon + 1..],
        )
    } else {
        (None, rest.strip_prefix(b":")?)
    };

    // The byte count runs to the tag's closing `>`.
    let close = rest.iter().position(|b| *b == b'>')?;
    let len: usize = std::str::from_utf8(&rest[..close]).ok()?.parse().ok()?;
    let tokens = &rest[close + 1..];

    // Only whitespace may separate the tag from its options.
    if !tokens.first().is_none_or(u8::is_ascii_whitespace) {
        return None;
    }

    let mut options = PartialImageOptions::default();

    for token in std::str::from_utf8(tokens).ok()?.split_ascii_whitespace() {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        let value = value.to_ascii_lowercase();

        match key {
            "hfit" => options.horizontal_fit = Some(parse_fit(&value)?),
            "vfit" => options.vertical_fit = Some(parse_fit(&value)?),
            "filter" => options.filter = Some(parse_filter(&value)?),
            "scroll" => options.scroll = Some(parse_scroll(&value)?),
            // Unknown keys are ignored so newer producers stay readable.
            _ => {}
        }
    }

    Some((format, len, options))
}

fn parse_fit(value: &str) -> Option<crate::config::ImageFit> {
    crate::config::ImageFit::parse(value)
}

fn parse_filter(value: &str) -> Option<crate::config::ImageFilter> {
    use crate::config::ImageFilter;
    // Separators are ignored, so `catmull_rom` and `catmull-rom` both parse, as in
    // the config file.
    Some(match value.replace(['-', '_'], "").as_str() {
        "nearest" => ImageFilter::Nearest,
        "triangle" => ImageFilter::Triangle,
        "catmullrom" => ImageFilter::CatmullRom,
        "gaussian" => ImageFilter::Gaussian,
        "lanczos3" => ImageFilter::Lanczos3,
        _ => return None,
    })
}

/// The initial crop offset in cells, written `vertical` or
/// `vertical,horizontal`; an omitted axis is `0`.
fn parse_scroll(value: &str) -> Option<[u16; 2]> {
    let (vertical, horizontal) = value.split_once(',').unwrap_or((value, "0"));
    Some([parse_scroll_axis(vertical)?, parse_scroll_axis(horizontal)?])
}

fn parse_scroll_axis(value: &str) -> Option<u16> {
    if value.is_empty() {
        Some(0)
    } else {
        value.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    #[test]
    fn path_mode() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        let state = parser.feed(
            b"\x1e<MM_IMAGE:9> hfit=fill scroll=3 filter=lanczos3\n/path.img and text",
            &mut output,
        );
        assert_eq!(
            state,
            &HeaderParser::Path {
                options: PartialImageOptions {
                    horizontal_fit: Some(crate::config::ImageFit::Fill(0)),
                    filter: Some(crate::config::ImageFilter::Lanczos3),
                    scroll: Some([3, 0]),
                    ..Default::default()
                },
                remaining: 0,
                path: b"/path.img".to_vec(),
            }
        );
        assert_eq!(
            output.as_slice(),
            b" and text",
            "only the declared bytes are image"
        );
    }

    #[test]
    fn header_and_path_split_across_chunks() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert!(matches!(
            parser.feed(b"\x1e<MM_I", &mut output),
            HeaderParser::Sniffing
        ));

        // The header line is complete, the path is not.
        assert!(matches!(
            parser.feed(b"MAGE:9>\n/path", &mut output),
            HeaderParser::Path { remaining: 4, .. }
        ));
        assert!(output.is_empty(), "the header is not displayed as text");

        assert_eq!(
            parser.feed(b".imgtrailing", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: b"/path.img".to_vec(),
            }
        );
        assert_eq!(output.as_slice(), b"trailing");
    }

    #[test]
    fn a_path_may_contain_a_newline() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(b"\x1e<MM_IMAGE:4>\n/a\nb", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: b"/a\nb".to_vec(),
            }
        );
    }

    #[test]
    fn empty_path_is_still_an_image() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(b"\x1e<MM_IMAGE:0>\ntext", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: Vec::new(),
            }
        );
        assert_eq!(output.as_slice(), b"text");
    }

    #[test]
    fn inline_mode_collects_the_payload() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(
                b"\x1e<MM_IMAGE/png:8> vfit=contain\n\x89PNGmoretext",
                &mut output
            ),
            &HeaderParser::Inline {
                format: PreviewFormat::Raster(ImageFormat::Png),
                options: PartialImageOptions {
                    vertical_fit: Some(crate::config::ImageFit::Contain(0)),
                    ..Default::default()
                },
                remaining: 0,
                payload: b"\x89PNGmore".to_vec(),
            }
        );
        assert_eq!(
            output.as_slice(),
            b"text",
            "text after the payload is output"
        );
    }

    #[test]
    fn inline_payload_split_across_chunks() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert!(matches!(
            parser.feed(b"\x1e<MM_IMAGE/png:4>\n\x89P", &mut output),
            HeaderParser::Inline { remaining: 2, .. }
        ));
        assert_eq!(
            parser.feed(b"NGtail", &mut output),
            &HeaderParser::Inline {
                format: PreviewFormat::Raster(ImageFormat::Png),
                options: PartialImageOptions::default(),
                remaining: 0,
                payload: b"\x89PNG".to_vec(),
            }
        );
        assert_eq!(output.as_slice(), b"tail");
    }

    #[test]
    fn text_after_the_image_may_arrive_later() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        parser.feed(b"\x1e<MM_IMAGE:4>\n/img", &mut output);
        assert!(output.is_empty());

        assert_eq!(
            parser.feed(b"later", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: b"/img".to_vec(),
            }
        );
        assert_eq!(output.as_slice(), b"later");
    }

    #[test]
    fn inline_header_needs_its_newline() {
        // A header line without a newline is not a header, it is text.
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert!(matches!(
            parser.feed(b"\x1e<MM_IMAGE/png:4>", &mut output),
            HeaderParser::Sniffing
        ));
        assert!(parser.decode(0).is_none());
        assert_eq!(
            output.as_slice(),
            b"\x1e<MM_IMAGE/png:4>",
            "the header is displayed as text"
        );

        // Neither the header line nor the payload need a trailing newline.
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(b"\x1e<MM_IMAGE/png:4>\n\x89PNG", &mut output),
            &HeaderParser::Inline {
                format: PreviewFormat::Raster(ImageFormat::Png),
                options: PartialImageOptions::default(),
                remaining: 0,
                payload: b"\x89PNG".to_vec(),
            }
        );
    }

    #[test]
    fn image_bytes_may_end_at_eof() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert!(matches!(
            parser.feed(b"\x1e<MM_IMAGE:5>\n", &mut output),
            HeaderParser::Path { remaining: 5, .. }
        ));
        assert_eq!(
            parser.feed(b"/path", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: b"/path".to_vec(),
            }
        );
    }

    #[test]
    fn truncated_image_is_not_displayed() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        parser.feed(b"\x1e<MM_IMAGE:9>\n/path", &mut output);
        assert!(matches!(parser.decode(0), Some(Err(_))));
        assert!(output.is_empty(), "an incomplete image is not text");
    }

    #[test]
    fn not_an_image() {
        for bytes in [
            &b"plain text"[..],
            b"\x1e<OTHER>",
            b"\x1e<MM_IMAGE>\n",
            b"\x1e<MM_IMAGE:abc>\n",
            b"\x1e<MM_IMAGE:9\n",
            b"\x1e<MM_IMAGE/notaformat:4>\n",
            b"\x1e<MM_IMAGE hfit=sideways>\n",
            b"\x1e<MM_IMAGE:9> hfit=crop:3\n/path.img",
            b"\x1e<MM_IMAGE:9> hfit=fill:x\n/path.img",
            b"\x1e<MM_IMAGE:9> hfit=16\n/path.img",
        ] {
            let mut parser = HeaderParser::new(true);
            let mut output = Vec::new();
            assert!(matches!(
                parser.feed(bytes, &mut output),
                HeaderParser::Text
            ));
            assert_eq!(output.as_slice(), bytes, "text is left untouched");
        }
    }

    #[test]
    fn disabled_parser_never_parses() {
        let mut parser = HeaderParser::new(false);
        let mut output = Vec::new();
        assert!(matches!(parser, HeaderParser::Text));
        parser.feed(b"\x1e<MM_IMAGE:4>\n/img", &mut output);
        assert!(parser.decode(0).is_none());
        assert_eq!(output.as_slice(), b"\x1e<MM_IMAGE:4>\n/img");
    }

    #[test]
    fn unknown_fit_is_not_an_image() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert!(matches!(
            parser.feed(b"\x1e<MM_IMAGE:9> hfit=stretch\n/path.img", &mut output),
            HeaderParser::Text
        ));
    }

    #[test]
    fn fit_extents() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(
                b"\x1e<MM_IMAGE:9> hfit=fill:16 vfit=contain:8\n/path.img",
                &mut output
            ),
            &HeaderParser::Path {
                options: PartialImageOptions {
                    horizontal_fit: Some(crate::config::ImageFit::Fill(16)),
                    vertical_fit: Some(crate::config::ImageFit::Contain(8)),
                    ..Default::default()
                },
                remaining: 0,
                path: b"/path.img".to_vec(),
            }
        );
    }

    #[test]
    fn incomplete_header_is_pending() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        // A prefix of the magic is not enough to decide anything.
        assert!(matches!(
            parser.feed(b"\x1e<MM_IMAG", &mut output),
            HeaderParser::Sniffing
        ));
        // The bytes are kept for the text path, which shows nothing without a newline.
        assert_eq!(output.as_slice(), b"\x1e<MM_IMAG");
        assert!(parser.decode(0).is_none());
        assert_eq!(output.as_slice(), b"\x1e<MM_IMAG", "nothing was consumed");
    }

    #[test]
    fn crlf_header_is_parsed() {
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        assert_eq!(
            parser.feed(b"\x1e<MM_IMAGE:4>\r\n/img", &mut output),
            &HeaderParser::Path {
                options: PartialImageOptions::default(),
                remaining: 0,
                path: b"/img".to_vec(),
            }
        );
    }

    #[test]
    fn both_catmull_rom_spellings_parse() {
        for spelling in ["catmull_rom", "catmull-rom", "CatmullRom"] {
            let mut parser = HeaderParser::new(true);
            let mut output = Vec::new();
            let state = parser.feed(
                format!("\x1e<MM_IMAGE:4> filter={spelling}\n/img").as_bytes(),
                &mut output,
            );
            match state {
                HeaderParser::Path { options, .. } => {
                    assert_eq!(
                        options.filter,
                        Some(crate::config::ImageFilter::CatmullRom),
                        "{spelling}"
                    );
                }
                other => panic!("expected HeaderParser::Path, got {other:?}"),
            }
        }
    }

    /// `scroll` sets the vertical axis alone, both when a comma separates them,
    /// or the horizontal one alone when the first axis is left out.
    #[test]
    fn scroll_covers_both_axes() {
        let scroll = |value: &str| {
            let mut parser = HeaderParser::new(true);
            let mut output = Vec::new();
            match parser.feed(
                format!("\x1e<MM_IMAGE:4> scroll={value}\n/img").as_bytes(),
                &mut output,
            ) {
                HeaderParser::Path { options, .. } => options.scroll,
                other => panic!("expected HeaderParser::Path, got {other:?}"),
            }
        };

        assert_eq!(scroll("3"), Some([3, 0]), "a single value is vertical");
        assert_eq!(
            scroll("3,1"),
            Some([3, 1]),
            "a pair is vertical then horizontal"
        );
        assert_eq!(scroll(",1"), Some([0, 1]), "an omitted axis is zero");
    }

    #[cfg(feature = "image-svg")]
    #[test]
    fn svg_inline_decodes() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="4"><rect width="8" height="4" fill="red"/></svg>"#;
        let mut parser = HeaderParser::new(true);
        let mut output = Vec::new();
        parser.feed(
            format!("\x1e<MM_IMAGE/svg:{}>\n", svg.len()).as_bytes(),
            &mut output,
        );
        parser.feed(svg.as_bytes(), &mut output);

        let image = parser.decode(7).unwrap().unwrap();
        assert_eq!(image.id, 7);
        assert_eq!([image.data.width(), image.data.height()], [256, 128]);
    }
}
