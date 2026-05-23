use std::{
    cmp::Ordering,
    collections::HashMap,
    fmt::{self, Display},
    str::FromStr,
};

use cba::bring::StrExt;
use serde::{
    Deserializer,
    de::{self, Visitor},
    ser,
};

use crate::{
    action::{Action, ActionExt, Actions, NullActionExt},
    config::HelpDisplayConfig,
    message::Event,
    utils::string::allowed_semantic_char,
};

pub use crate::bindmap;
pub use crokey::{KeyCombination, key};
pub use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

#[allow(type_alias_bounds)]
pub type BindMap<A: ActionExt = NullActionExt> = HashMap<Trigger, Actions<A>>;

#[easy_ext::ext(BindMapExt)]
impl<A: ActionExt> BindMap<A> {
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
            key!(PageDown) => Action::HalfPageDown,
            key!(PageUp) => Action::HalfPageUp,
            key!(Home) => Action::Pos(0),
            key!(End) => Action::Pos(-1),
            key!(shift-PageDown) => Action::PreviewHalfPageDown,
            key!(shift-PageUp) => Action::PreviewHalfPageUp,
            key!(shift-Home) => Action::PreviewJump,
            key!(shift-End) => Action::PreviewJump,
            key!('?') => Action::SwitchPreview(None),
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
        if let Some(actions) = self.get(&Trigger {
            kind: TriggerKind::Semantic(current.to_string()),
            mode: String::new(),
        }) {
            for action in actions {
                if let Action::Semantic(next) = action {
                    self.dfs_semantic(next, path)?;
                }
            }
        }
        path.pop();

        Ok(())
    }

    /// Simplifies semantic trigger chains and removes unbound triggers.
    ///
    /// - `key -> @s1 -> @s2 -> concrete` becomes `key -> @s2`.
    /// - `key -> @s1 -> unbound` results in `key` being removed.
    ///
    /// # Note
    /// This method assumes that [Self::check_cycles] has been called and succeeded.
    pub fn resolve_semantics(&mut self) {
        let mut triggers_to_update = Vec::new();

        // Step 1: Build semantic resolution mapping strictly for mode-less fallbacks
        let mut semantic_map: std::collections::HashMap<String, Option<String>> =
            std::collections::HashMap::new();

        for (trigger, actions) in self.iter().filter(|(t, _)| t.mode.is_empty()) {
            if let TriggerKind::Semantic(ref s) = trigger.kind {
                if actions.len() == 1
                    && let Action::Semantic(next_s) = &actions[0]
                {
                    semantic_map
                        .entry(s.clone())
                        .and_modify(|v| *v = None)
                        .or_insert_with(|| Some(next_s.clone()));
                } else {
                    // Ambiguous or non-semantic terminal
                    semantic_map
                        .entry(s.clone())
                        .and_modify(|v| *v = None)
                        .or_insert(None);
                }
            }
        }

        // Step 2: Iterate over ALL triggers and prune/chain semantic actions
        for (trigger, actions) in self.iter() {
            let mut updated_actions = Actions::default();
            let mut changed = false;

            for action in actions.iter() {
                if let Action::Semantic(start_s) = action {
                    let mut current_s = start_s.clone();
                    let mut is_unbound = false;

                    // Trace the chain of single semantic actions
                    loop {
                        // 1. Try finding the semantic trigger specific to the current mode first
                        if !trigger.mode.is_empty() {
                            let next_trigger = Trigger {
                                kind: TriggerKind::Semantic(current_s.clone()),
                                mode: trigger.mode.clone(),
                            };

                            if let Some(next_actions) = self.get(&next_trigger) {
                                if next_actions.len() == 1
                                    && let Action::Semantic(next_s) = &next_actions[0]
                                {
                                    current_s = next_s.clone();
                                    continue;
                                }
                                break; // Found a terminal for this specific mode
                            }
                        }

                        // 2. Fallback to the global mapping (mode = "")
                        match semantic_map.get(&current_s) {
                            Some(Some(next_s)) => {
                                current_s = next_s.clone(); // Chain further
                            }
                            Some(None) => {
                                break; // Do nothing (resolves to concrete or multiple actions)
                            }
                            None => {
                                is_unbound = true; // Not in mapping, prune
                                break;
                            }
                        }
                    }

                    if !is_unbound {
                        if &current_s != start_s {
                            changed = true
                        }
                        updated_actions.push(Action::Semantic(current_s));
                    } else {
                        changed = true
                    }
                } else {
                    updated_actions.push(action.clone());
                }
            }

            if changed {
                triggers_to_update.push((trigger.clone(), updated_actions));
            }
        }

        for (t, a) in triggers_to_update {
            if a.is_empty() {
                self.remove(&t);
            } else {
                self.insert(t, a);
            }
        }
    }
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
/// A trigger kind that activates a binding.
///
/// Supported variants:
/// - `Key`: A keyboard combination (e.g., `ctrl-c`, `enter`, `a`). Parsed using `crokey`.
/// - `Mouse`: A mouse event with optional modifiers (e.g., `left`, `ctrl+scrollup`).
/// - `Event`: A lifecycle or UI event (e.g., `Start`, `QueryChange`).
/// - `Semantic`: A (nonempty) named alias prefixed with `@` (e.g., `@open`). See [`is_valid_semantic_char`].
pub enum TriggerKind {
    Key(KeyCombination),
    Mouse(SimpleMouseEvent),
    Event(Event),
    /// A "semantic" trigger, such as `Open`, which should be resolved or rejected before starting the picker.
    /// This is serialized/deserialized with a `@` prefix, such as "@Open" = "Execute(open {})"
    Semantic(String),
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub mode: String,
}

// impl Ord for TriggerKind {
//     fn cmp(&self, other: &Self) -> Ordering {
//         use TriggerKind::*;

//         match (self, other) {
//             (Key(a), Key(b)) => a.to_string().cmp(&b.to_string()),
//             (Mouse(a), Mouse(b)) => a.cmp(b),
//             (Event(a), Event(b)) => a.cmp(b),
//             (Semantic(a), Semantic(b)) => a.cmp(b),

//             // define variant order
//             (Key(_), _) => Ordering::Less,
//             (Mouse(_), Key(_)) => Ordering::Greater,
//             (Mouse(_), Event(_)) => Ordering::Less,
//             (Mouse(_), Semantic(_)) => Ordering::Less,
//             (Event(_), Key(_)) => Ordering::Greater,
//             (Event(_), Mouse(_)) => Ordering::Greater,
//             (Event(_), Semantic(_)) => Ordering::Less,
//             (Semantic(_), _) => Ordering::Greater,
//         }
//     }
// }

// impl PartialOrd for TriggerKind {
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
        Trigger {
            kind: TriggerKind::Mouse(SimpleMouseEvent {
                kind: e.kind,
                modifiers: e.modifiers,
            }),
            mode: String::new(),
        }
    }
}

impl From<KeyCombination> for Trigger {
    fn from(key: KeyCombination) -> Self {
        Trigger {
            kind: TriggerKind::Key(key),
            mode: String::new(),
        }
    }
}

impl From<Event> for Trigger {
    fn from(event: Event) -> Self {
        Trigger {
            kind: TriggerKind::Event(event),
            mode: String::new(),
        }
    }
}
// ------------ SERDE

impl Display for TriggerKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TriggerKind::Key(key) => write!(f, "{}", key),
            TriggerKind::Mouse(event) => {
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
            TriggerKind::Event(event) => write!(f, "{}", event),
            TriggerKind::Semantic(alias) => write!(f, "@{alias}"),
        }
    }
}

impl Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.mode.is_empty() {
            write!(f, "{} ({})", self.kind, self.mode)?;
        }
        write!(f, "{}", self.kind)
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

impl FromStr for TriggerKind {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        // try semantic
        if let Some(s) = value.strip_prefix("@") {
            if s.chars().all(allowed_semantic_char) && !s.is_empty() {
                return Ok(TriggerKind::Semantic(s.to_string()));
            } else if !s.is_empty() {
                return Err(format!(
                    "Invalid semantic trigger name: @{s}. Allowed characters are alphanumeric, space, and -_.:/+$@"
                ));
            }
        }

        // 1. Try KeyCombination
        if let Ok(key) = KeyCombination::from_str(value) {
            return Ok(TriggerKind::Key(key));
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

            return Ok(TriggerKind::Mouse(SimpleMouseEvent { kind, modifiers }));
        }

        if let Ok(event) = value.parse::<Event>() {
            return Ok(TriggerKind::Event(event));
        }

        Err(format!("failed to parse trigger kind from '{}'", value))
    }
}

impl FromStr for Trigger {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if let Some((mode, kind_str)) = value.split_once("^^") {
            if !mode.is_empty() && mode.chars().all(|c| c.is_alphanumeric()) {
                let kind = TriggerKind::from_str(kind_str)?;
                return Ok(Trigger {
                    kind,
                    mode: mode.to_string(),
                });
            }
        }

        let kind = TriggerKind::from_str(value)?;
        Ok(Trigger {
            kind,
            mode: String::new(),
        })
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
    config: &HelpDisplayConfig,
) -> Text<'static> {
    use fmt::Alignment::Center;
    // Collect trigger and action strings
    let mut entries: Vec<(String, Vec<String>)> = binds
        .iter()
        .filter(|(trigger, _)| {
            !config.hide_semantic || !matches!(trigger.kind, TriggerKind::Semantic(_))
        })
        .map(|(trigger, actions)| {
            (
                trigger.to_string(),
                actions.iter().map(|a| a.to_string()).collect(),
            )
        })
        .collect();

    // Sort by actions (values) instead of triggers
    entries.sort_by(|a, b| a.1.cmp(&b.1));

    // Build output
    let Some(cfg) = &config.colors else {
        // fallback plain text
        let mut text = Text::default();
        for (trigger, actions) in entries {
            let value = if actions.len() == 1 {
                actions[0].ellipsize(config.max_len, Center)
            } else {
                let inner = actions
                    .into_iter()
                    .map(|a| a.ellipsize(config.max_len, Center))
                    .collect::<Vec<_>>()
                    .join(", ");

                if let Some([open, close]) = config.seq_brackets {
                    format!("{open}{inner}{close}")
                } else {
                    inner
                }
            };
            text.extend(Text::from(format!("{trigger} = {value}\n")));
        }
        return text;
    };

    let mut text = Text::default();

    for (trigger, actions) in entries {
        let mut spans = vec![
            // Trigger
            Span::styled(trigger, Style::default().fg(cfg.key)),
            Span::raw(" = "),
        ];

        // Value
        if actions.len() > 1 {
            // multi-action list: color each item
            if let Some([open, _]) = config.seq_brackets {
                spans.push(Span::raw(open.to_string()));
            }

            for (i, item) in actions.into_iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(", "));
                }
                let truncated = item.ellipsize(config.max_len, Center);
                spans.push(Span::styled(truncated, Style::default().fg(cfg.value)));
            }

            if let Some([_, close]) = config.seq_brackets {
                spans.push(Span::raw(close.to_string()));
            }
        } else {
            // single action
            let truncated = actions[0].ellipsize(config.max_len, Center);
            spans.push(Span::styled(truncated, Style::default().fg(cfg.value)));
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
        let trigger0 = Trigger {
            kind: TriggerKind::Mouse(SimpleMouseEvent {
                kind: MouseEventKind::ScrollDown,
                modifiers: KeyModifiers::empty(),
            }),
            mode: String::new(),
        };
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
        let shift_trigger = Trigger {
            kind: TriggerKind::Mouse(SimpleMouseEvent {
                kind: MouseEventKind::ScrollDown,
                modifiers: KeyModifiers::SHIFT,
            }),
            mode: String::new(),
        };
        assert!(!bind_map.contains_key(&shift_trigger));
    }

    #[test]
    fn test_semantic_parsing() {
        assert_eq!(
            Trigger::from_str("@foo").unwrap(),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: String::new()
            }
        );
        let trigger = Trigger::from_str("@").unwrap();
        // "@" itself is a valid key, but should NOT be parsed as a Semantic trigger.
        assert!(matches!(trigger.kind, TriggerKind::Key(_)));

        assert_eq!(
            Action::<NullActionExt>::from_str("@foo").unwrap(),
            Action::Semantic("foo".into())
        );
        assert_eq!(
            Action::<NullActionExt>::from_str("@foo bar").unwrap(),
            Action::Semantic("foo bar".into())
        );
        assert!(Action::<NullActionExt>::from_str("@").is_err());

        // todo: lowpri: test invalid semantic names
    }

    #[test]
    fn test_mode_parsing() {
        let t = Trigger::from_str("vim^^a").unwrap();
        assert_eq!(t.mode, "vim".to_string());
        assert_eq!(t.kind, TriggerKind::Key(key!(a)));
        assert_eq!(t.to_string(), "vim^^a");

        let t2 = Trigger::from_str("a").unwrap();
        assert_eq!(t2.mode, String::new());
        assert_eq!(t2.kind, TriggerKind::Key(key!(a)));
        assert_eq!(t2.to_string(), "a");

        // Invalid mode (non-alphanumeric) -> whole string parsed as TriggerKind, which fails here
        assert!(Trigger::from_str("v-im^^a").is_err());

        // Empty mode -> whole string parsed as TriggerKind, which fails here
        assert!(Trigger::from_str("^^a").is_err());
    }

    #[test]
    fn test_check_cycles() {
        use crate::bindmap;
        let bind_map: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: String::new()
            } => Action::Semantic("b".into()),
            Trigger {
                kind: TriggerKind::Semantic("b".into()),
                mode: String::new()
            } => Action::Semantic("a".into()),
        );
        assert!(bind_map.check_cycles().is_err());

        let bind_map_no_cycle: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: String::new()
            } => Action::Semantic("b".into()),
            Trigger {
                kind: TriggerKind::Semantic("b".into()),
                mode: String::new()
            } => Action::Print("ok".into()),
        );
        assert!(bind_map_no_cycle.check_cycles().is_ok());

        let bind_map_self_cycle: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: String::new()
            } => Action::Semantic("a".into()),
        );
        assert!(bind_map_self_cycle.check_cycles().is_err());

        let bind_map_indirect_cycle: BindMap = bindmap!(
            key!(a) => Action::Semantic("foo".into()),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: String::new()
            } => Action::Semantic("foo".into()),
        );
        assert!(bind_map_indirect_cycle.check_cycles().is_err());
    }

    #[test]
    fn test_resolve_semantics() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Chain: key(a) -> @s1 -> @s2 -> concrete
        let mut bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: String::new()
            } => Action::Semantic("s2".into()),
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: String::new()
            } => Action::Accept,
        );
        bind_map.resolve_semantics();
        assert_eq!(
            bind_map.get(&key!(a).into()).unwrap().0[0],
            Action::Semantic("s2".into())
        );
        assert_eq!(
            bind_map
                .get(&Trigger {
                    kind: TriggerKind::Semantic("s1".into()),
                    mode: String::new()
                })
                .unwrap()
                .0[0],
            Action::Semantic("s2".into())
        );

        // Unbound: key(b) -> @s3 -> @s4 -> unbound
        let mut bind_map_unbound: BindMap<NullActionExt> = bindmap!(
            key!(b) => Action::Semantic("s3".into()),
            Trigger {
                kind: TriggerKind::Semantic("s3".into()),
                mode: String::new()
            } => Action::Semantic("s4".into()),
        );
        bind_map_unbound.resolve_semantics();
        assert!(!bind_map_unbound.contains_key(&key!(b).into()));
        assert!(!bind_map_unbound.contains_key(&Trigger {
            kind: TriggerKind::Semantic("s3".into()),
            mode: String::new()
        }));

        // Multi-action chain: key(c) -> @s5 -> [@s6, Accept]
        let mut bind_map_multi: BindMap<NullActionExt> = bindmap!(
            key!(c) => Action::Semantic("s5".into()),
            Trigger {
                kind: TriggerKind::Semantic("s5".into()),
                mode: String::new()
            } => [Action::Semantic("s6".into()), Action::Accept],
            Trigger {
                kind: TriggerKind::Semantic("s6".into()),
                mode: String::new()
            } => Action::Cancel,
        );
        bind_map_multi.resolve_semantics();
        // key(c) should still point to @s5 because @s5 has multiple actions.
        assert_eq!(
            bind_map_multi.get(&key!(c).into()).unwrap().0[0],
            Action::Semantic("s5".into())
        );
    }

    #[test]
    fn test_display_binds_semantic_help() {
        let binds: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Print("a".into()),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: String::new()
            } => Action::Print("foo".into()),
        );

        // With semantic help
        let help_show = display_binds(&binds, &Default::default());
        let help_show_str = help_show.to_string();
        assert!(help_show_str.contains("a = Print(a)"));
        assert!(help_show_str.contains("@foo = Print(foo)"));

        // Without semantic help
        let help_hide = display_binds(&binds, &Default::default());
        let help_hide_str = help_hide.to_string();
        assert!(help_hide_str.contains("a = Print(a)"));
        assert!(!help_hide_str.contains("@foo = Print(foo)"));
    }
}
