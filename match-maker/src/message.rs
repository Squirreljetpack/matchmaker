use bitflags::bitflags;
use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use crate::action::{Action, ActionExt};

bitflags! {
    #[derive(bitflags_derive::FlagsDisplay, bitflags_derive::FlagsFromStr, Debug, PartialEq, Eq, Hash, Clone, Copy, Default, PartialOrd, Ord)]
    pub struct Event: u32 {
        const Start        = 1 << 0;  // Lifecycle start
        const Complete     = 1 << 1;  // Lifecycle end
        const Synced       = 1 << 7;  // First completion of matcher
        const Resynced     = 1 << 8;  // Matcher finished processing current state

        const QueryChange  = 1 << 2;  // Input/query update
        const CursorChange = 1 << 3;  // Cursor movement

        const PreviewChange = 1 << 4; // Preview update
        const OverlayChange = 1 << 5; // Overlay update
        const PreviewSet    = 1 << 6; // Preview explicitly set

        const Resize = 1 << 9;  // Window/terminal resize
        const Refresh = 1 << 10; // Full redraw

        const Pause  = 1 << 11; // Pause events
        const Resume = 1 << 12; // Resume events
    }
}

// ---------------------------------------------------------------------

#[derive(Default, Debug, Copy, Clone, PartialEq, Eq)]
#[repr(u8)]
pub enum Interrupt {
    #[default]
    None,
    Become,
    Execute,
    Print,
    Reload,
    Custom,
}

// ---------------------------------------------------------------------

#[non_exhaustive]
#[derive(Debug, strum_macros::Display, Clone)]
pub enum RenderCommand<A: ActionExt> {
    Action(Action<A>),
    Mouse(MouseEvent),
    Resize(Rect),
    #[cfg(feature = "bracketed-paste")]
    Paste(String),
    HeaderColumns(Vec<ratatui::text::Text<'static>>),
    Ack,
    Tick,
    Refresh,
    QuitEmpty,
}

impl<A: ActionExt> From<&Action<A>> for RenderCommand<A> {
    fn from(action: &Action<A>) -> Self {
        RenderCommand::Action(action.clone())
    }
}

impl<A: ActionExt> RenderCommand<A> {
    pub fn quit() -> Self {
        RenderCommand::Action(Action::Quit(1))
    }
}
