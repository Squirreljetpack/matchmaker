use super::State;
use crate::{
    MMItem, Selection,
    action::ActionExt,
    ui::{OverlayUI, PickerUI, PreviewUI, UI},
};

use ratatui::text::{Span, Text};

#[derive(Debug, Clone)]
pub enum Effect {
    ClearPreviewSet,
    OverlayWidget(usize),
    Header(Text<'static>),
    Footer(Text<'static>),
    ClearFooter,
    ClearHeader,
    ClearState,

    Prompt(Span<'static>),
    Input((String, u16)),

    DisableCursor(bool),
    SetIndex(u32),
}

#[derive(Debug, Default)]
pub struct Effects(Vec<Effect>);

impl<S: Selection> State<S> {
    // note: apparently its important that this is a method on state to satisfy borrow checker
    pub fn apply_effects<T: MMItem, A: ActionExt, W: std::io::Write>(
        &mut self,
        effects: Effects,
        ui: &mut UI,
        picker_ui: &mut PickerUI<T, S>,
        _preview_ui: &mut Option<PreviewUI>,
        overlay_ui: &mut Option<OverlayUI<A>>,
        tui: &mut crate::tui::Tui<W>
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
                    picker_ui.header.text = text;
                    picker_ui.header.show = true;
                }
                Effect::Footer(text) => {
                    picker_ui.footer.text = text;
                    picker_ui.footer.show = true;
                }
                Effect::ClearHeader => {
                    picker_ui.header.show = true;
                }
                Effect::ClearFooter => {
                    picker_ui.footer.show = true;
                }

                // ----- other -------
                Effect::ClearState => {
                    picker_ui.input.set(Default::default(), 0);
                    picker_ui.selections.clear();
                }

                Effect::OverlayWidget(index) => {
                    if let Some(x) = overlay_ui.as_mut() {
                        x.enable(index, &ui.area);
                        tui.redraw();
                    }
                }

                // ----- input -------
                Effect::Input((input, cursor)) => {
                    picker_ui.input.set(input, cursor);
                }
                Effect::Prompt(prompt) => {
                    picker_ui.input.prompt = prompt;
                }

                // ----- results -------
                Effect::DisableCursor(disabled) => {
                    picker_ui.results.cursor_disabled = disabled;
                }
                Effect::SetIndex(index) => {
                    picker_ui.results.cursor_jump(index);
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
        Self(Vec::new())
    }

    pub fn append(&mut self, end: Vec<Effect>) {
        for effect in end {
            self.insert(effect);
        }
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
}

impl IntoIterator for Effects {
    type Item = Effect;
    type IntoIter = std::vec::IntoIter<Effect>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl From<Vec<Effect>> for Effects {
    fn from(vec: Vec<Effect>) -> Self {
        let mut unique = Vec::new();
        for e in vec {
            if !unique.contains(&e) {
                unique.push(e);
            }
        }
        Effects(unique)
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