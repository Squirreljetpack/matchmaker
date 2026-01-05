use super::State;
use crate::{
    MAX_EFFECTS, SSS, Selection,
    message::Event,
    ui::{PickerUI, PreviewUI, UI},
};

use arrayvec::ArrayVec;
use ratatui::text::{Span, Text};

#[derive(Debug, Clone)]
pub enum Effect {
    ClearPreviewSet,
    Header(Text<'static>),
    Footer(Text<'static>),
    ClearFooter,
    ClearHeader,
    ClearSelections,
    RevalidateSelectons,
    /// Reload the nucleo matcher
    /// Note that the reload interrupt handler is NOT triggered if this is produced from a dynamic handler
    Reload,

    Prompt(Span<'static>),
    /// Set the input ui contents and cursor
    Input((String, u16)),
    RestoreInputPromptMarker,

    DisableCursor(bool),
    SetIndex(u32),
    TrySync,
}
#[derive(Debug, Default)]
pub struct Effects(ArrayVec<Effect, MAX_EFFECTS>);

#[macro_export]
macro_rules! efx {
    ( $( $x:expr ),* $(,)? ) => {
        {
            [$($x),*].into_iter().collect::<$crate::render::Effects>()
        }
    };
}
pub use crate::acs;

impl<S: Selection> State<S> {
    // note: apparently its important that this is a method on state to satisfy borrow checker
    pub fn apply_effects<T: SSS>(
        &mut self,
        effects: Effects,
        _ui: &mut UI,
        picker_ui: &mut PickerUI<T, S>,
        _preview_ui: &mut Option<PreviewUI>,
    ) {
        if !effects.is_empty() {
            log::debug!("{effects:?}");
        }
        for effect in effects {
            match effect {
                // ----- preview -------
                Effect::ClearPreviewSet => {
                    self.preview_set = None;
                }

                // ----- displays -------
                Effect::Header(text) => {
                    picker_ui.header.set(text);
                }
                Effect::Footer(text) => {
                    picker_ui.footer.set(text);
                }
                Effect::ClearHeader => {
                    picker_ui.header.show = false;
                }
                Effect::ClearFooter => {
                    picker_ui.footer.show = false;
                }

                // ----- input -------
                Effect::Input((input, cursor)) => {
                    picker_ui.input.set(input, cursor);
                }
                Effect::Prompt(prompt) => {
                    picker_ui.input.prompt = prompt;
                }
                Effect::RestoreInputPromptMarker => {
                    picker_ui.input.prompt = Span::from(picker_ui.input.config.prompt.clone());
                }

                // ----- results -------
                Effect::DisableCursor(disabled) => {
                    picker_ui.results.cursor_disabled = disabled;
                }
                Effect::SetIndex(index) => {
                    log::info!("{:?}", picker_ui.results);
                    picker_ui.results.cursor_jump(index);
                }

                // -------- selections ---------
                Effect::ClearSelections => {
                    picker_ui.selections.clear();
                }
                Effect::RevalidateSelectons => {
                    picker_ui.selections.revalidate();
                }

                // ---------- misc -------------
                // this may not be the best place for these? We're trying to trigger a handler
                Effect::Reload => {
                    // the reload handler is not triggered when a handler produces this effect
                    picker_ui.worker.restart(false);
                }
                Effect::TrySync => {
                    if !picker_ui.results.status.running {
                        self.insert(Event::Synced);
                    }
                }
            }
        }
    }
}

// ----------------------------------------------------

impl PartialEq for Effect {
    fn eq(&self, other: &Self) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }
}

impl Eq for Effect {}

impl Effects {
    pub fn new() -> Self {
        Self(ArrayVec::new())
    }

    /// Insert only if not already present
    pub fn insert(&mut self, effect: Effect) -> bool {
        if self.0.contains(&effect) {
            false
        } else {
            self.0.push(effect);
            true
        }
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn append(&mut self, other: Self) {
        for effect in other {
            self.insert(effect);
        }
    }
}

impl IntoIterator for Effects {
    type Item = Effect;
    type IntoIter = arrayvec::IntoIter<Effect, 12>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl FromIterator<Effect> for Effects {
    fn from_iter<I: IntoIterator<Item = Effect>>(iter: I) -> Self {
        let mut effects = Effects::new();
        for e in iter {
            effects.insert(e);
        }
        effects
    }
}
