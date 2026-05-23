use bitflags::bitflags;
use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use crate::{
    Actions,
    action::{Action, ActionExt},
    binds::Trigger,
    ui::HeaderTable,
};

bitflags! {
    #[derive(bitflags_derive::FlagsDisplay, bitflags_derive::FlagsFromStr, Debug, PartialEq, Eq, Hash, Clone, Copy, Default, PartialOrd, Ord)]
    pub struct Event: u32 {
        /// Lifecycle start
        const Start        = 1 << 0;

        /// Input/query update
        const QueryChange  = 1 << 2;
        /// Cursor movement
        const CursorChange = 1 << 3;

        /// Preview update
        const PreviewChange = 1 << 4;
        /// Overlay update
        const OverlayChange = 1 << 5;
        /// Preview explicitly set
        const PreviewSet    = 1 << 6;

        /// First completion of matcher
        const Synced       = 1 << 7;
        /// Matcher finished processing current state
        const Resynced     = 1 << 8;

        /// Window/terminal resize
        const Resize = 1 << 9;
        /// Full redraw
        const Refresh = 1 << 10;

        /// Pause event listener
        const Pause  = 1 << 11;
        /// Resume event listener
        const Resume = 1 << 12;

        /// Reload interrupt
        const Reloaded = 1 << 13;

        /// Cursor disabled or no results
        const CursorLost = 1 << 14;
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
    ExecuteSilent,
    BecomeSilent,
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
    HeaderTable(HeaderTable),
    Ack,
    Tick,
    Refresh,
    NoMatch,
    Empty,
}

impl<A: ActionExt> From<Action<A>> for RenderCommand<A> {
    fn from(action: Action<A>) -> Self {
        RenderCommand::Action(action)
    }
}

impl<A: ActionExt> RenderCommand<A> {
    pub fn quit() -> Self {
        RenderCommand::Action(Action::Quit(1))
    }
}

// ---------------------------------------------------------------------
#[derive(Debug)]
pub enum BindDirective<A: ActionExt> {
    Bind(Trigger, Actions<A>),
    PushBind(Trigger, Action<A>),
    Unbind(Trigger),
    PopBind(Trigger),
    Action(Action<A>),
}
