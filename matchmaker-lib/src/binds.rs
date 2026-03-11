use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt::{self, Display},
    str::FromStr,
};

use serde::{
    Deserializer,
    de::{self, Visitor},
    ser,
};

use crate::{
    action::{Action, ActionExt, Actions, NullActionExt},
    config::HelpColorConfig,
    message::Event,
};

pub use crate::bindmap;
pub use crokey::{KeyCombination, key};
pub use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

#[allow(type_alias_bounds)]
pub type BindMap<A: ActionExt = NullActionExt> = HashMap<Trigger, Actions<A>>;

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
            key!(alt-a) => Action::QueryPos(0),
            key!(alt-h) => Action::Help("".to_string()),
            key!(ctrl-'[') => Action::ToggleWrap,
            key!(ctrl-']') => Action::TogglePreviewWrap,
            key!(ctrl-shift-right) => Action::HScroll(1),
            key!(ctrl-shift-left) => Action::HScroll(-1),
            key!(ctrl-shift-up) => Action::VScroll(1),
            key!(ctrl-shift-down) => Action::VScroll(-1),
            key!(PageDown) => Action::PageDown,
            key!(PageUp) => Action::PageUp,
            key!(Home) => Action::Pos(0),
            key!(End) => Action::Pos(-1),
            key!(shift-PageDown) => Action::PreviewHalfPageDown,
            key!(shift-PageUp) => Action::PreviewHalfPageUp,
            key!(shift-Home) => Action::PreviewJump,
            key!(shift-End) => Action::PreviewJump,
            key!('?') => Action::SwitchPreview(None)
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

    /// Check for infinite loops in semantic actions.
    pub fn check_cycles(&self) -> Result<(), String> {
        for actions in self.values() {
            for action in actions {
                if let Action::Semantic(s) = action {
                    let mut path = Vec::new();
                    self.dfs_semantic(s, &mut path)?;
                }
            }
        }
        Ok(())
    }

    pub fn dfs_semantic(&self, current: &str, path: &mut Vec<String>) -> Result<(), String> {
        if path.contains(&current.to_string()) {
            return Err(format!(
                "Infinite loop detected in semantic actions: {} -> {}",
                path.join(" -> "),
                current
            ));
        }

        path.push(current.to_string());
        if let Some(actions) = self.get(&Trigger::Semantic(current.to_string())) {
            for action in actions {
                if let Action::Semantic(next) = action {
                    self.dfs_semantic(next, path)?;
                }
            }
        }
        path.pop();

        Ok(())
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub enum Trigger {
    Key(KeyCombination),
    Mouse(SimpleMouseEvent),
    Event(Event),
    /// A "semantic" trigger, such as `Open`, which should be resolved or rejected before starting the picker.
    /// This is serialized/deserialized with a `::` prefix, such as "::Open" = "Execute(open {})"
    Semantic(String),
}

// impl Ord for Trigger {
//     fn cmp(&self, other: &Self) -> Ordering {
//         use Trigger::*;

//         match (self, other) {
//             (Key(a), Key(b)) => a.to_string().cmp(&b.to_string()),
//             (Mouse(a), Mouse(b)) => a.cmp(b),
//             (Event(a), Event(b)) => a.cmp(b),

//             // define variant order
//             (Key(_), _) => Ordering::Less,
//             (Mouse(_), Key(_)) => Ordering::Greater,
//             (Mouse(_), Event(_)) => Ordering::Less,
//             (Event(_), _) => Ordering::Greater,
//         }
//     }
// }

// impl PartialOrd for Trigger {
//     fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
//         Some(self.cmp(other))
//     }
// }

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

impl Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trigger::Key(key) => write!(f, "{}", key),
            Trigger::Mouse(event) => {
                if event.modifiers.contains(KeyModifiers::SHIFT) {
                    write!(f, "shift+")?;
                }
                if event.modifiers.contains(KeyModifiers::CONTROL) {
                    write!(f, "ctrl+")?;
                }
                if event.modifiers.contains(KeyModifiers::ALT) {
                    write!(f, "alt+")?;
                }
                if event.modifiers.contains(KeyModifiers::SUPER) {
                    write!(f, "super+")?;
                }
                if event.modifiers.contains(KeyModifiers::HYPER) {
                    write!(f, "hyper+")?;
                }
                if event.modifiers.contains(KeyModifiers::META) {
                    write!(f, "meta+")?;
                }
                write!(f, "{}", mouse_event_kind_as_str(event.kind))
            }
            Trigger::Event(event) => write!(f, "{}", event),
            Trigger::Semantic(alias) => write!(f, "::{alias}"),
        }
    }
}

impl ser::Serialize for Trigger {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: ser::Serializer,
    {
        serializer.serialize_str(&self.to_string())
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
        // try semantic
        if let Some(s) = value.strip_prefix("::") {
            return Ok(Trigger::Semantic(s.to_string()));
        }

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
                    unknown => return Err(format!("Unknown modifier: {}", unknown)),
                }
            }

            return Ok(Trigger::Mouse(SimpleMouseEvent { kind, modifiers }));
        }

        if let Ok(event) = value.parse::<Event>() {
            return Ok(Trigger::Event(event));
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

use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};

pub fn display_binds<A: ActionExt + Display>(
    binds: &BindMap<A>,
    cfg: Option<&HelpColorConfig>,
) -> Text<'static> {
    // Collect trigger and action strings
    let mut entries: Vec<(String, String)> = binds
        .iter()
        .map(|(trigger, actions)| {
            let value_str = if actions.len() == 1 {
                actions[0].to_string()
            } else {
                let inner = actions
                    .iter()
                    .map(|a| a.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{inner}]")
            };
            (trigger.to_string(), value_str)
        })
        .collect();

    // Sort by trigger string
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    // Build output
    let Some(cfg) = cfg else {
        // fallback plain text
        let mut text = Text::default();
        for (trigger, value) in entries {
            text.extend(Text::from(format!("{trigger} = {value}\n")));
        }
        return text;
    };

    let mut text = Text::default();

    for (trigger, value) in entries {
        let mut spans = vec![];

        // Trigger
        spans.push(Span::styled(trigger, Style::default().fg(cfg.key)));
        spans.push(Span::raw(" = "));

        // Value
        if value.starts_with('[') {
            // multi-action list: color each item
            spans.push(Span::raw("["));
            let inner = &value[1..value.len() - 1];
            for (i, item) in inner.split(", ").enumerate() {
                if i > 0 {
                    spans.push(Span::raw(", "));
                }
                spans.push(Span::styled(
                    item.to_string(),
                    Style::default().fg(cfg.value),
                ));
            }
            spans.push(Span::raw("]"));
        } else {
            spans.push(Span::styled(value, Style::default().fg(cfg.value)));
        }

        spans.push(Span::raw("\n"));
        text.extend(Text::from(Line::from(spans)));
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

    #[test]
    fn test_check_cycles() {
        use crate::bindmap;
        let bind_map: BindMap = bindmap!(
            Trigger::Semantic("a".into()) => Action::Semantic("b".into()),
            Trigger::Semantic("b".into()) => Action::Semantic("a".into()),
        );
        assert!(bind_map.check_cycles().is_err());

        let bind_map_no_cycle: BindMap = bindmap!(
            Trigger::Semantic("a".into()) => Action::Semantic("b".into()),
            Trigger::Semantic("b".into()) => Action::Print("ok".into()),
        );
        assert!(bind_map_no_cycle.check_cycles().is_ok());

        let bind_map_self_cycle: BindMap = bindmap!(
            Trigger::Semantic("a".into()) => Action::Semantic("a".into()),
        );
        assert!(bind_map_self_cycle.check_cycles().is_err());

        let bind_map_indirect_cycle: BindMap = bindmap!(
            key!(a) => Action::Semantic("foo".into()),
            Trigger::Semantic("foo".into()) => Action::Semantic("foo".into()),
        );
        assert!(bind_map_indirect_cycle.check_cycles().is_err());
    }
}
