use crokey::KeyCombination;
use crossterm::event::{KeyModifiers, MouseEventKind};
use serde::Deserializer;
use std::{collections::HashMap, fmt, str::FromStr};

use crossterm::event::MouseButton;
use serde::de::{self, Visitor};

use crate::{action::Actions, message::Event};

pub type BindMap = HashMap<Trigger, Actions>;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum Trigger {
    Key(KeyCombination),
    Mouse(MouseEvent),
    Event(Event),
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct MouseEvent {
    pub kind: MouseEventKind,
    pub modifiers: KeyModifiers,
}

// ---------- BOILERPLATE
impl From<crossterm::event::MouseEvent> for Trigger {
    fn from(e: crossterm::event::MouseEvent) -> Self {
        Trigger::Mouse(MouseEvent {
            kind: e.kind,
            modifiers: e.modifiers,
        })
    }
}

impl From<KeyCombination> for Trigger {
    fn from(key: KeyCombination) -> Self {
        Trigger::Key(key)
    }
}

impl From<Event> for Trigger {
    fn from(event: Event) -> Self {
        Trigger::Event(event)
    }
}
// ------------ SERDE

impl<'de> serde::Deserialize<'de> for Trigger {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct TriggerVisitor;

        impl<'de> Visitor<'de> for TriggerVisitor {
            type Value = Trigger;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a string representing a Trigger")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // 1. Try KeyCombination
                if let Ok(key) = KeyCombination::from_str(value) {
                    return Ok(Trigger::Key(key));
                }

                // 2. Try MouseEvent: modifiers split by '+', last = mouse button
                let parts: Vec<&str> = value.split('+').collect();
                if let Some(last) = parts.last() {
                    if let Some(kind) = match last.to_lowercase().as_str() {
                        "left" => Some(MouseEventKind::Down(MouseButton::Left)),
                        "middle" => Some(MouseEventKind::Down(MouseButton::Middle)),
                        "right" => Some(MouseEventKind::Down(MouseButton::Right)),
                        "scrolldown" => Some(MouseEventKind::ScrollDown),
                        "scrollup" => Some(MouseEventKind::ScrollUp),
                        "scrollleft" => Some(MouseEventKind::ScrollLeft),
                        "scrollright" => Some(MouseEventKind::ScrollRight),
                        _ => None,
                    } {
                        let mut modifiers = KeyModifiers::empty();
                        for m in &parts[..parts.len() - 1] {
                            match m.to_lowercase().as_str() {
                                "shift" => modifiers |= KeyModifiers::SHIFT,
                                "ctrl" => modifiers |= KeyModifiers::CONTROL,
                                "alt" => modifiers |= KeyModifiers::ALT,
                                "super" => modifiers |= KeyModifiers::SUPER,
                                "hyper" => modifiers |= KeyModifiers::HYPER,
                                "meta" => modifiers |= KeyModifiers::META,
                                "none" => {}
                                unknown => {
                                    return Err(E::custom(format!(
                                        "Unknown modifier: {}",
                                        unknown
                                    )));
                                }
                            }
                        }
                        return Ok(Trigger::Mouse(MouseEvent { kind, modifiers }));
                    }
                }

                // 3. Try PickerEvent
                if let Ok(evt) = value.parse::<Event>() {
                    return Ok(Trigger::Event(evt));
                }

                Err(E::custom(format!(
                    "failed to parse trigger from '{}'",
                    value
                )))
            }
        }

        deserializer.deserialize_str(TriggerVisitor)
    }
}
