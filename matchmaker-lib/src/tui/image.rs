//! Terminal graphics capabilities, turned into the picker the preview pane
//! renders images with.
//!
//! The capabilities themselves are collected by [`super::query::startup`]; this
//! module only interprets them.

use super::{Tui, query::ImageQuery};
use crate::config::ImageProtocol;
use ratatui_image::{
    FontSize,
    picker::{Picker, ProtocolType},
};
use std::io::Write;

impl<W> Tui<W>
where
    W: Write,
{
    /// The picker to render preview images with.
    ///
    /// `protocol` is the configured protocol and wins over what the terminal
    /// reported: detection only decides [`ImageProtocol::Auto`], because a
    /// terminal can under-report (a tmux pane does not answer the kitty query in
    /// a form we parse) and forcing the protocol is the way around that. A
    /// protocol the terminal cannot render is therefore drawn as garbage rather
    /// than replaced. See `ImageQuery` for how each reported capability routes.
    pub fn image_picker(&self, protocol: ImageProtocol) -> Option<Picker> {
        build_picker(self.image_query, protocol)
    }
}

/// Build the picker from the capabilities the terminal reported and the
/// configured protocol. [`ImageProtocol::None`] disables images entirely.
fn build_picker(query: ImageQuery, protocol: ImageProtocol) -> Option<Picker> {
    if protocol == ImageProtocol::None {
        return None;
    }

    let font_size = query
        .font_size
        .map(FontSize::from)
        .or_else(font_size_fallback);
    let mut picker = match font_size {
        #[allow(deprecated)]
        // upstream only has from_query_stdio as recommended which for obvious reasons is insufficient for our purposes
        Some(font_size) => Picker::from_fontsize(font_size),
        // Without the cell size only halfblocks is an option -- the other protocols
        // place real pixels and cannot be laid out without it. Its cell is
        // ratatui-image's 10x20 placeholder: an arbitrary, roughly 1:2 rectangle that
        // only has to be proportionate, because halfblocks does not draw the image
        // pixel for pixel. The terminal env hints still decide between the text
        // protocols.
        None => Picker::halfblocks(),
    };

    if let Some(detected) = query.protocol() {
        picker.set_protocol_type(detected);
    }
    match protocol {
        ImageProtocol::Auto | ImageProtocol::None => {}
        ImageProtocol::Halfblocks => picker.set_protocol_type(ProtocolType::Halfblocks),
        ImageProtocol::Sixel => picker.set_protocol_type(ProtocolType::Sixel),
        ImageProtocol::Kitty => picker.set_protocol_type(ProtocolType::Kitty),
        ImageProtocol::Iterm2 => picker.set_protocol_type(ProtocolType::Iterm2),
    }

    Some(picker)
}

// todo: check if this fallback should be enabled by default
/// Cell size in pixels from the window size, for terminals that do not answer the
/// cell size query: the window's pixels divided by its text grid.
fn font_size_fallback() -> Option<FontSize> {
    let size = crossterm::terminal::window_size().ok()?;
    if size.width == 0 || size.height == 0 || size.columns == 0 || size.rows == 0 {
        return None;
    }
    Some(FontSize::new(
        size.width / size.columns,
        size.height / size.rows,
    ))
}
