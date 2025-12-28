use std::fmt;

use super::MMState;
use crate::{
    MMItem, Selection,
    message::{Event, Interrupt}, render::Effect,
};

// note: beware that same handler could be called multiple times for the same event in one iteration
// We choose not to return a Option<Result<S, E>> to simplify defining handlers, but will rather expose some mechanisms on state later on if a use case arises
pub type DynamicMethod<T, S, E> = Box<dyn Fn(&mut MMState<'_, T, S>, &E) -> Vec<Effect> + Send + Sync>;
pub type DynamicHandlers<T, S> = (EventHandlers<T, S>, InterruptHandlers<T, S>);

#[allow(clippy::type_complexity)]
pub struct EventHandlers<T: MMItem, S: Selection> {
    handlers: Vec<(Vec<Event>, DynamicMethod<T, S, Event>)>,
}

#[allow(clippy::type_complexity)]
pub struct InterruptHandlers<T: MMItem, S: Selection> {
    handlers: Vec<(Interrupt, Vec<DynamicMethod<T, S, Interrupt>>)>,
}

impl<T: MMItem, S: Selection> Default for EventHandlers<T, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: MMItem, S: Selection> EventHandlers<T, S> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, events: Vec<Event>, handler: DynamicMethod<T, S, Event>) {
        self.handlers.push((events, handler));
    }

    pub fn get(
        &self,
        event: &Event,
    ) -> impl Iterator<Item = &DynamicMethod<T, S, Event>> {
        self.handlers
            .iter()
            .filter(move |(events, _)| events.contains(event))
            .map(|(_, handler)| handler)
    }
}

impl<T: MMItem, S: Selection> Default for InterruptHandlers<T, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: MMItem, S: Selection> InterruptHandlers<T, S> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, variant: Interrupt, handler: DynamicMethod<T, S, Interrupt>) {
        if let Some((_, handlers)) = self.handlers.iter_mut().find(|(v, _)| *v == variant) {
            handlers.push(handler);
        } else {
            self.handlers.push((variant, vec![handler]));
        }
    }

    pub fn get(&self, variant: &Interrupt) -> impl Iterator<Item = &DynamicMethod<T, S, Interrupt>> {
        self.handlers
            .iter()
            .filter_map(move |(v, h)| (v == variant).then_some(h))
            .flatten()
    }
}

// -------------------------------BOILERPLATE----------------------------------

impl<T: MMItem, S: Selection> fmt::Debug for EventHandlers<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandlers")
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl<T: MMItem, S: Selection> fmt::Debug for InterruptHandlers<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptHandlers")
            .field("variant_count", &self.handlers.len())
            .finish()
    }
}
