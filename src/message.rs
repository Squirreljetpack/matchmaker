use bitflags::bitflags;
use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use crate::action::{Action, ActionExt};

bitflags! {
    #[derive(bitflags_derive::FlagsDisplay, bitflags_derive::FlagsFromStr, Debug, PartialEq, Eq, Hash, Clone, Copy, Default)]
    pub struct Event: u32 {
        const Start = 1 << 0;
        const Complete = 1 << 1;
        const QueryChange = 1 << 2;
        const CursorChange = 1 << 3;
        const PreviewChange = 1 << 4;
        const OverlayChange = 1 << 5;
        const PreviewSet = 1 << 6;
        const Synced = 1 << 7;
        const Resize = 1 << 8;
        const Refresh = 1 << 9;
        const Pause = 1 << 10;
        const Resume = 1 << 11;
    }
}

// ---------------------------------------------------------------------

#[derive(Default, Debug, Clone)]
#[repr(u8)]
pub enum Interrupt {
    #[default]
    None,
    Become(String),
    Execute(String),
    Print(String),
    Reload(String),
    Custom(usize),
}

impl Interrupt {
    pub fn discriminant(&self) -> u8 {
        // SAFETY: Because `Self` is marked `repr(u8)`, its layout is a `repr(C)` `union`
        // between `repr(C)` structs, each of which has the `u8` discriminant as its first
        // field, so we can read the discriminant without offsetting the pointer.
        unsafe { *<*const _>::from(self).cast::<u8>() }
    }
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
