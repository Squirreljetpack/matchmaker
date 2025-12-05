use crokey::KeyCombination;
use crossterm::event::{KeyModifiers, MouseEventKind};
use serde::{Deserializer, Serialize, ser};
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::{fmt, str::FromStr};

use crossterm::event::MouseButton;
use serde::de::{self, Visitor};

use crate::config::TomlColorConfig;
use crate::{action::Actions, message::Event};

pub type BindMap = BTreeMap<Trigger, Actions>;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum Trigger {
    Key(KeyCombination),
    Mouse(SimpleMouseEvent),
    Event(Event),
}

impl Ord for Trigger {
    fn cmp(&self, other: &Self) -> Ordering {
        use Trigger::*;

        match (self, other) {
            (Key(a), Key(b)) => a.to_string().cmp(&b.to_string()),
            (Mouse(a), Mouse(b)) => mouse_event_kind_as_str(a.kind).cmp(mouse_event_kind_as_str(b.kind)),
            (Event(a), Event(b)) => a.to_string().cmp(&b.to_string()),

            // define variant order
            (Key(_), _) => Ordering::Less,
            (Mouse(_), Key(_)) => Ordering::Greater,
            (Mouse(_), Event(_)) => Ordering::Less,
            (Event(_), _) => Ordering::Greater,
        }
    }
}

impl PartialOrd for Trigger {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Crossterm mouse event without location
#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct SimpleMouseEvent {
    pub kind: MouseEventKind,
    pub modifiers: KeyModifiers,
}

// ---------- BOILERPLATE
impl From<crossterm::event::MouseEvent> for Trigger {
    fn from(e: crossterm::event::MouseEvent) -> Self {
        Trigger::Mouse(SimpleMouseEvent {
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
                s.push_str(mouse_event_kind_as_str(event.kind));
                serializer.serialize_str(&s)
            }
            Trigger::Event(event) => serializer.serialize_str(&event.to_string()),
        }
    }
}

pub fn mouse_event_kind_as_str(kind: MouseEventKind) -> &'static str {
    match kind {
        MouseEventKind::Down(MouseButton::Left) => "left",
        MouseEventKind::Down(MouseButton::Middle) => "middle",
        MouseEventKind::Down(MouseButton::Right) => "right",
        MouseEventKind::ScrollDown => "scrolldown",
        MouseEventKind::ScrollUp => "scrollup",
        MouseEventKind::ScrollLeft => "scrollleft",
        MouseEventKind::ScrollRight => "scrollright",
        _ => "", // Other kinds are not handled in deserialize
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
                    return Ok(Trigger::Mouse(SimpleMouseEvent { kind, modifiers }));
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

#[derive(Serialize)]
struct BindFmtWrapper<'a> {
    binds: &'a BindMap
}
use ratatui::style::{Style};
use ratatui::text::{Line, Span, Text};
use regex::Regex;



// random ai toml coloring cuz i dont wanna use bat just for this
pub fn display_binds(binds: &BindMap, cfg: Option<&TomlColorConfig>) -> Text<'static> {
    let toml_string = toml::to_string(&BindFmtWrapper { binds }).unwrap();

    let Some(cfg) = cfg else {
        return Text::from(toml_string);
    };

    let section_re = Regex::new(r"^\s*\[.*\]").unwrap();
    let key_re = Regex::new(r"^(\s*[\w_-]+)(\s*=\s*)").unwrap();
    let string_re = Regex::new(r#""[^"]*""#).unwrap();
    let number_re = Regex::new(r"\b\d+(\.\d+)?\b").unwrap();

    let mut text = Text::default();

    for line in toml_string.lines() {
        if section_re.is_match(line) {
            let mut style = Style::default().fg(cfg.section);
            if cfg.section_bold {
                style = style.add_modifier(ratatui::style::Modifier::BOLD);
            }
            text.extend(Text::from(Span::styled(line.to_string(), style)));
        } else {
            let mut spans = vec![];
            let mut remainder = line.to_string();

            // Highlight key
            if let Some(cap) = key_re.captures(&remainder) {
                let key = &cap[1];
                let eq = &cap[2];
                spans.push(Span::styled(key.to_string(), Style::default().fg(cfg.key)));
                spans.push(Span::raw(eq.to_string()));
                remainder = remainder[cap[0].len()..].to_string();
            }

            // Highlight strings
            let mut last_idx = 0;
            for m in string_re.find_iter(&remainder) {
                if m.start() > last_idx {
                    spans.push(Span::raw(remainder[last_idx..m.start()].to_string()));
                }
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    Style::default().fg(cfg.string),
                ));
                last_idx = m.end();
            }

            // Highlight numbers
            let remainder = &remainder[last_idx..];
            let mut last_idx = 0;
            for m in number_re.find_iter(remainder) {
                if m.start() > last_idx {
                    spans.push(Span::raw(remainder[last_idx..m.start()].to_string()));
                }
                spans.push(Span::styled(
                    m.as_str().to_string(),
                    Style::default().fg(cfg.number),
                ));
                last_idx = m.end();
            }

            if last_idx < remainder.len() {
                spans.push(Span::raw(remainder[last_idx..].to_string()));
            }

            text.extend(Text::from(Line::from(spans)));
        }
    }

    text
}
