use bitflags::bitflags;
use crossterm::event::MouseEvent;
use ratatui::layout::Rect;

use crate::{
    action::{Action, ActionExt, Exit},
    render::Effect,
};

bitflags! {
    #[derive(bitflags_derive::FlagsDisplay, bitflags_derive::FlagsFromStr, Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Default)]
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
#[non_exhaustive]
pub enum Interrupt {
    #[default]
    None,
    Become(String),
    Execute(String),
    Print(String),
    Reload(String),
    Custom(usize),
}

impl PartialEq for Interrupt {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}
impl Eq for Interrupt {}

// ---------------------------------------------------------------------

#[non_exhaustive]
#[derive(Debug, strum_macros::Display, Clone)]
pub enum RenderCommand<A: ActionExt> {
    Action(Action<A>),
    Mouse(MouseEvent),
    Resize(Rect),
    Effect(Effect),
    #[cfg(feature = "bracketed-paste")]
    Paste(String),
    Ack,
    Tick,
    Refresh,
}

impl<A: ActionExt> From<&Action<A>> for RenderCommand<A> {
    fn from(action: &Action<A>) -> Self {
        RenderCommand::Action(action.clone())
    }
}

impl<A: ActionExt> RenderCommand<A> {
    pub fn quit() -> Self {
        RenderCommand::Action(Action::Quit(Exit::default()))
    }
}
