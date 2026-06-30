use std::fmt;

use super::MMState;
use crate::{
    SSS, Selection,
    message::{Event, Interrupt},
};

// note: beware that same handler could be called multiple times for the same event in one iteration
// We choose not to return a Option<Result<S, E>> to simplify defining handlers, but will rather expose some mechanisms on state later on if a use case arises
pub type DynamicMethod<T, D, S, E> = Box<dyn Fn(&mut MMState<'_, '_, T, D, S>, &E)>;
pub type BoxedHandler<T, D, S> = Box<dyn FnMut(&mut MMState<'_, '_, T, D, S>)>;

pub type DynamicHandlers<T, D, S> = (EventHandlers<T, D, S>, InterruptHandlers<T, D, S>);

pub struct EventHandlers<T: SSS, D, S: Selection> {
    handlers: Vec<(Event, DynamicMethod<T, D, S, Event>)>,
}

pub struct InterruptHandlers<T: SSS, D, S: Selection> {
    handlers: Vec<(Interrupt, Vec<BoxedHandler<T, D, S>>)>,
}

impl<T: SSS, D, S: Selection> Default for EventHandlers<T, D, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SSS, D, S: Selection> EventHandlers<T, D, S> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, event: Event, handler: DynamicMethod<T, D, S, Event>) {
        self.handlers.push((event, handler));
    }

    pub fn get(&self, event: Event) -> impl Iterator<Item = &DynamicMethod<T, D, S, Event>> {
        self.handlers
            .iter()
            .filter(move |(mask, _)| mask.intersects(event))
            .map(|(_, handler)| handler)
    }
}

impl<T: SSS, D, S: Selection> Default for InterruptHandlers<T, D, S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SSS, D, S: Selection> InterruptHandlers<T, D, S> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, variant: Interrupt, handler: BoxedHandler<T, D, S>) {
        if let Some((_, handlers)) = self.handlers.iter_mut().find(|(v, _)| *v == variant) {
            handlers.push(handler);
        } else {
            self.handlers.push((variant, vec![handler]));
        }
    }

    pub fn get_mut(&mut self, variant: Interrupt) -> impl Iterator<Item = &mut BoxedHandler<T, D, S>> {
        self.handlers
            .iter_mut()
            .filter_map(move |(v, h)| (*v == variant).then_some(h))
            .flatten()
    }
}

// -------------------------------BOILERPLATE----------------------------------

impl<T: SSS, D, S: Selection> fmt::Debug for EventHandlers<T, D, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandlers")
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl<T: SSS, D, S: Selection> fmt::Debug for InterruptHandlers<T, D, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptHandlers")
            .field("variant_count", &self.handlers.len())
            .finish()
    }
}
