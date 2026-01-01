use crossterm::event::MouseEvent;
use ratatui::layout::Rect;
use strum_macros::{Display, EnumString};

use crate::{action::{Action, ActionExt, Exit}, render::Effect};

#[derive(Debug, Hash, PartialEq, Eq, EnumString, Clone, Display)]
#[strum(serialize_all = "lowercase")]
#[non_exhaustive]
pub enum Event {
    Start,
    Complete,
    QueryChange,
    CursorChange,
    PreviewChange,
    OverlayChange,
    PreviewSet,
    Synced,
    Resize,
    Refresh,
    Pause,
    Resume,
    Custom(usize)
}

#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub enum Interrupt {
    #[default]
    None,
    Become(String),
    Execute(String),
    Print(String),
    Reload(String),
    Custom(usize)
}

impl PartialEq for Interrupt {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for Interrupt {}

#[non_exhaustive]
#[derive(Debug, strum_macros::Display, Clone)]
pub enum RenderCommand<A: ActionExt> {
    Action(Action<A>),
    Input(char),
    Mouse(MouseEvent),
    Resize(Rect),
    Effect(Effect),
    Ack,
    Tick,
    Refresh
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