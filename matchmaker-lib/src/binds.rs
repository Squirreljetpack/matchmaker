use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fmt::{self, Display},
    str::FromStr,
};

use serde::{
    Deserializer, Serialize,
    de::{self, Visitor},
    ser,
};

use crate::{
    action::{Action, ActionExt, Actions, NullActionExt},
    config::TomlColorConfig,
    message::Event,
};

pub use crate::bindmap;
pub use crokey::{KeyCombination, key};
pub use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

#[allow(type_alias_bounds)]
pub type BindMap<A: ActionExt = NullActionExt> = BTreeMap<Trigger, Actions<A>>;

#[easy_ext::ext(BindMapExt)]
impl<A: ActionExt> BindMap<A> {
    #[allow(unused_mut)]
    pub fn default_binds() -> Self {
        let mut ret = bindmap!(
            key!(ctrl-c) => Action::Quit(1),
            key!(esc) => Action::Quit(1),
            key!(up) => Action::Up(1),
            key!(down) => Action::Down(1),
            key!(enter) => Action::Accept,
            key!(right) => Action::ForwardChar,
            key!(left) => Action::BackwardChar,
            key!(backspace) => Action::DeleteChar,
            key!(ctrl-right) => Action::ForwardWord,
            key!(ctrl-left) => Action::BackwardWord,
            key!(ctrl-h) => Action::DeleteWord,
            key!(ctrl-u) => Action::Cancel,
            key!(alt-h) => Action::Help("".to_string()),
            key!(ctrl-'[') => Action::ToggleWrap,
            key!(ctrl-']') => Action::TogglePreviewWrap,
            key!(shift-right) => Action::HScroll(1),
            key!(shift-left) => Action::HScroll(-1),
            key!(PageDown) => Action::PageDown,
            key!(PageUp) => Action::PageUp,
            key!(Home) => Action::Pos(0),
            key!(End) => Action::Pos(-1),
            key!(shift-PageDown) => Action::PreviewHalfPageDown,
            key!(shift-PageUp) => Action::PreviewHalfPageUp,
            key!(shift-Home) => Action::PreviewJump,
            key!(shift-End) => Action::PreviewJump,
        );

        #[cfg(target_os = "macos")]
        {
            let ext = bindmap!(
                key!(alt-left) => Action::ForwardWord,
                key!(alt-right) => Action::BackwardWord,
                key!(alt-backspace) => Action::DeleteWord,
            );
            ret.extend(ext);
        }

        ret
    }
}

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
            (Mouse(a), Mouse(b)) => a.cmp(b),
            (Event(a), Event(b)) => a.cmp(b),

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
#[derive(Debug, Eq, Clone, PartialEq, Hash)]
pub struct SimpleMouseEvent {
    pub kind: MouseEventKind,
    pub modifiers: KeyModifiers,
}

impl Ord for SimpleMouseEvent {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.kind.partial_cmp(&other.kind) {
            Some(Ordering::Equal) | None => self.modifiers.bits().cmp(&other.modifiers.bits()),
            Some(o) => o,
        }
    }
}

impl PartialOrd for SimpleMouseEvent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
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

impl FromStr for Trigger {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // 1. Try KeyCombination
        if let Ok(key) = KeyCombination::from_str(value) {
            return Ok(Trigger::Key(key));
        }

        // 2. Try MouseEvent
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
            }
        {
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
                        return Err(format!("Unknown modifier: {}", unknown));
                    }
                }
            }

            return Ok(Trigger::Mouse(SimpleMouseEvent { kind, modifiers }));
        }

        // 3. Try Event
        if let Ok(evt) = value.parse::<Event>() {
            return Ok(Trigger::Event(evt));
        }

        Err(format!("failed to parse trigger from '{}'", value))
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
                value.parse::<Trigger>().map_err(E::custom)
            }
        }

        deserializer.deserialize_str(TriggerVisitor)
    }
}

#[derive(Serialize)]
#[serde(bound(serialize = "",))]
struct BindFmtWrapper<'a, A: ActionExt + Display> {
    binds: &'a BindMap<A>,
}
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use regex::Regex;

// random ai toml coloring cuz i dont wanna use bat just for this
pub fn display_binds<A: ActionExt + Display>(
    binds: &BindMap<A>,
    cfg: Option<&TomlColorConfig>,
) -> Text<'static> {
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

#[cfg(test)]
mod test {
    use super::*;
    use crossterm::event::MouseEvent;

    #[test]
    fn test_bindmap_trigger() {
        let mut bind_map: BindMap = BindMap::new();

        // Insert trigger with default actions
        let trigger0 = Trigger::Mouse(SimpleMouseEvent {
            kind: MouseEventKind::ScrollDown,
            modifiers: KeyModifiers::empty(),
        });
        bind_map.insert(trigger0.clone(), Actions::default());

        // Construct via From<MouseEvent>
        let mouse_event = MouseEvent {
            kind: MouseEventKind::ScrollDown,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::empty(),
        };
        let from_event: Trigger = mouse_event.into();

        // Should be retrievable
        assert!(bind_map.contains_key(&from_event));

        // Shift-modified trigger should NOT be found
        let shift_trigger = Trigger::Mouse(SimpleMouseEvent {
            kind: MouseEventKind::ScrollDown,
            modifiers: KeyModifiers::SHIFT,
        });
        assert!(!bind_map.contains_key(&shift_trigger));
    }
}
