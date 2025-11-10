use ratatui::layout::Rect;
use strum_macros::{Display, EnumString};

use crate::action::{Action, Exit};

#[derive(Debug, Hash, PartialEq, Eq, EnumString, Clone, Display)]
#[strum(serialize_all = "lowercase")]
#[non_exhaustive]
pub enum Event {
    Start,
    Complete,
    QueryChange,
    CursorChange,
    PreviewChange,
    Resize,
    Refresh,
    Pause,
    Resume,
}

// Like payload, but doesn't persist ig
#[derive(Default, Debug, Clone)]
#[non_exhaustive]
pub enum Interrupt {
    #[default]
    None,
    Become(String),
    Execute(String),
    Print(String),
    Reload(String),
}

impl PartialEq for Interrupt {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for Interrupt {}

#[non_exhaustive]
#[derive(Debug, strum_macros::Display, Clone)]
pub enum RenderCommand {
    Bind,
    Action(Action),
    Input(char),
    Resize(Rect),
    Ack,
    Tick,
    Refresh
}

impl From<&Action> for RenderCommand {
    fn from(action: &Action) -> Self {
        RenderCommand::Action(action.clone())
    }
}

impl RenderCommand {
    pub fn quit() -> Self {
        RenderCommand::Action(Action::Quit(Exit::default()))
    }
}