use std::fmt;

use super::MMState;
use crate::{
    message::{Event, Interrupt},
    SSS,
};

// note: a handler whose mask intersects multiple set bits in the event is
// only called once per propagation, not once per set bit.
// We choose not to return a Option<Result<S, E>> to simplify defining handlers, but will rather expose some mechanisms on state later on if a use case arises
pub type DynamicMethod<T, D, E> = Box<dyn Fn(&mut MMState<'_, '_, T, D>, &E)>;
pub type BoxedHandler<T, D> = Box<dyn FnMut(&mut MMState<'_, '_, T, D>)>;

pub type DynamicHandlers<T, D> = (EventHandlers<T, D>, InterruptHandlers<T, D>);

pub struct EventHandlers<T: SSS, D> {
    handlers: Vec<(Event, DynamicMethod<T, D, Event>)>,
}

pub struct InterruptHandlers<T: SSS, D> {
    handlers: Vec<(Interrupt, Vec<BoxedHandler<T, D>>)>,
}

impl<T: SSS, D> Default for EventHandlers<T, D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SSS, D> EventHandlers<T, D> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, event: Event, handler: DynamicMethod<T, D, Event>) {
        self.handlers.push((event, handler));
    }

    /// Iterate all registered handlers whose mask intersects `event`.
    /// Each handler is yielded at most once per call regardless of how
    /// many bits its mask overlaps with the `event` bitflag.
    pub fn try_all(&self, event: Event) -> impl Iterator<Item = &DynamicMethod<T, D, Event>> {
        self.handlers
            .iter()
            .filter(move |(mask, _)| mask.intersects(event))
            .map(|(_, handler)| handler)
    }
}

impl<T: SSS, D> Default for InterruptHandlers<T, D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: SSS, D> InterruptHandlers<T, D> {
    pub fn new() -> Self {
        Self { handlers: vec![] }
    }

    pub fn set(&mut self, variant: Interrupt, handler: BoxedHandler<T, D>) {
        if let Some((_, handlers)) = self.handlers.iter_mut().find(|(v, _)| *v == variant) {
            handlers.push(handler);
        } else {
            self.handlers.push((variant, vec![handler]));
        }
    }

    pub fn get_mut(&mut self, variant: Interrupt) -> impl Iterator<Item = &mut BoxedHandler<T, D>> {
        self.handlers
            .iter_mut()
            .filter_map(move |(v, h)| (*v == variant).then_some(h))
            .flatten()
    }
}

// -------------------------------BOILERPLATE----------------------------------

impl<T: SSS, D> fmt::Debug for EventHandlers<T, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandlers")
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl<T: SSS, D> fmt::Debug for InterruptHandlers<T, D> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptHandlers")
            .field("variant_count", &self.handlers.len())
            .finish()
    }
}
