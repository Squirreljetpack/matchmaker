use crokey::KeyCombination;
use crossterm::event::{KeyModifiers, MouseEventKind};
use serde::{Deserializer, ser};
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

impl ser::Serialize for Trigger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        match self {
            Trigger::Key(key) => serializer.serialize_str(&key.to_string()),
            Trigger::Mouse(event) => {
                let mut s = String::new();
                if event.modifiers.contains(KeyModifiers::SHIFT) {
                    s.push_str("shift+");
                }
                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    s.push_str("ctrl+");
                }
                if event.modifiers.contains(KeyModifiers::ALT) {
                    s.push_str("alt+");
                }
                if event.modifiers.contains(KeyModifiers::SUPER) {
                    s.push_str("super+");
                }
                if event.modifiers.contains(KeyModifiers::HYPER) {
                    s.push_str("hyper+");
                }
                if event.modifiers.contains(KeyModifiers::META) {
                    s.push_str("meta+");
                }
                let kind_str = match event.kind {
                    MouseEventKind::Down(MouseButton::Left) => "left",
                    MouseEventKind::Down(MouseButton::Middle) => "middle",
                    MouseEventKind::Down(MouseButton::Right) => "right",
                    MouseEventKind::ScrollDown => "scrolldown",
                    MouseEventKind::ScrollUp => "scrollup",
                    MouseEventKind::ScrollLeft => "scrollleft",
                    MouseEventKind::ScrollRight => "scrollright",
                    _ => "", // Other kinds are not handled in deserialize
                };
                s.push_str(kind_str);
                serializer.serialize_str(&s)
            }
            Trigger::Event(event) => serializer.serialize_str(&event.to_string()),
        }
    }
}

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
                if let Some(last) = parts.last()
                    && let Some(kind) = match last.to_lowercase().as_str() {
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

                // 3. Try Event
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
