use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
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
        #[allow(unused_mut)]
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

    /// Fully resolves all aliases into concrete triggers.
    pub fn resolve_semantics(&mut self) {
        let mut alias_modes: HashMap<String, HashSet<String>> = HashMap::new();
        for trigger in self.keys() {
            if let TriggerKind::Semantic(alias) = &trigger.kind {
                alias_modes
                    .entry(alias.clone())
                    .or_default()
                    .insert(trigger.mode.clone());
            }
        }

        let mut new_binds: BindMap<A> = HashMap::new();

        // 1. Fully resolve concrete triggers that ALREADY have a mode
        for (trigger, actions) in self.iter().filter(|(t, _)| !t.mode.is_empty()) {
            if let TriggerKind::Semantic(_) = &trigger.kind {
                continue;
            }

            if let Some(resolved) = self.resolve_actions(actions, &trigger.mode) {
                new_binds.insert(trigger.clone(), resolved);
            }
        }

        // 2. Resolve concrete triggers WITHOUT a mode (mode = "")
        // and also handle mode propagation (intersection)
        for (trigger, actions) in self.iter().filter(|(t, _)| t.mode.is_empty()) {
            if let TriggerKind::Semantic(_) = &trigger.kind {
                continue;
            }

            // Resolve for the default mode
            if let Some(resolved) = self.resolve_actions(actions, "") {
                new_binds.insert(trigger.clone(), resolved);
            }

            // Find common modes for all aliases in these actions
            let mut common_modes: Option<HashSet<String>> = None;
            let mut has_aliases = false;

            for action in actions.iter() {
                if let Action::Semantic(alias) = action {
                    has_aliases = true;
                    let modes = alias_modes.get(alias).cloned().unwrap_or_default();
                    if let Some(common) = &mut common_modes {
                        common.retain(|m| modes.contains(m));
                    } else {
                        common_modes = Some(modes);
                    }
                }
            }

            if has_aliases {
                if let Some(mut modes) = common_modes {
                    // Remove default mode as it's already handled
                    modes.remove("");
                    // Remove modes that already have a definition for this trigger
                    modes.retain(|m| {
                        !self.contains_key(&Trigger {
                            kind: trigger.kind.clone(),
                            mode: m.clone(),
                        })
                    });

                    for mode in modes {
                        if let Some(resolved) = self.resolve_actions(actions, &mode) {
                            new_binds.insert(
                                Trigger {
                                    kind: trigger.kind.clone(),
                                    mode,
                                },
                                resolved,
                            );
                        }
                    }
                }
            }
        }

        *self = new_binds;
    }

    pub fn resolve_actions(&self, actions: &Actions<A>, mode: &str) -> Option<Actions<A>> {
        let mut resolved = Vec::new();

        for action in actions.iter() {
            if let Action::Semantic(alias) = action {
                if let Some(alias_actions) = self.resolve_alias(alias, mode) {
                    let has_nested_aliases = alias_actions
                        .iter()
                        .any(|a| matches!(a, Action::Semantic(_)));
                    let flat_actions = if has_nested_aliases {
                        self.resolve_actions(alias_actions, mode)?
                    } else {
                        alias_actions.clone()
                    };

                    let already_traced = matches!(flat_actions.first(), Some(Action::Trace(_)));
                    if !already_traced {
                        resolved.push(Action::Trace(format!("@@{alias}")));
                        resolved.extend(flat_actions.into_iter());
                        resolved.push(Action::Trace(String::new()));
                    } else {
                        resolved.extend(flat_actions.into_iter());
                    }
                } else {
                    // If any alias is unresolvable, the whole sequence is invalid
                    return None;
                }
            } else {
                resolved.push(action.clone());
            }
        }

        if resolved.is_empty() {
            None
        } else {
            Some(Actions(resolved))
        }
    }

    pub fn resolve_alias(&self, alias: &str, mode: &str) -> Option<&Actions<A>> {
        let specific_trigger = Trigger {
            kind: TriggerKind::Semantic(alias.to_string()),
            mode: mode.to_string(),
        };

        if let Some(actions) = self.get(&specific_trigger) {
            return Some(actions);
        }

        if !mode.is_empty() {
            let fallback_trigger = Trigger {
                kind: TriggerKind::Semantic(alias.to_string()),
                mode: String::new(),
            };
            return self.get(&fallback_trigger);
        }

        None
    }

    /// Strips all `Action::Trace` from the bindings.
    ///
    /// Traces are required to be alternating: the first trace must be nonempty,
    /// the second (if any) must be empty, the third nonempty, and so on.
    ///
    /// Returns `false` if the traces did not strictly follow this alternating
    /// pattern (nonempty, empty, nonempty...) within any action sequence.
    pub fn strip_traces(&mut self) -> bool {
        let mut valid_alternating = true;

        for actions in self.values_mut() {
            let mut i = 0;
            let mut expect_empty = false; // Starts with a required nonempty trace

            while i < actions.len() {
                if let Action::Trace(trace_content) = &actions[i] {
                    // Check if the current trace matches the expected emptiness state
                    let is_empty = trace_content.is_empty();
                    if is_empty != expect_empty {
                        valid_alternating = false;
                    }

                    // Flip the expectation for the next trace encounter
                    expect_empty = !expect_empty;

                    // Strip the trace from the actions vector
                    actions.remove(i);
                } else {
                    i += 1;
                }
            }
        }

        valid_alternating
    }

    pub fn check_traces(&self) -> bool {
        let mut valid_alternating = true;

        for actions in self.values() {
            let mut expect_empty = false;

            let mut offending = false;

            for action in actions.iter() {
                if let Action::Trace(trace_content) = action {
                    let is_empty = trace_content.is_empty();

                    if is_empty != expect_empty {
                        valid_alternating = false;

                        offending = true;
                    }

                    expect_empty = !expect_empty;
                }
            }

            if offending {
                log::warn!("Offending action list: {:?}", actions);
            }
        }

        valid_alternating
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
pub fn display_help<A: ActionExt + Display>(
    binds: &BindMap<A>,
    config: &HelpDisplayConfig,
    mode: Option<&str>,
) -> Text<'static> {
    // Filter and collect triggers based on mode
    let mut entries: Vec<(String, Vec<Action<A>>, Option<u32>)> = Vec::new();
    let mut seen_trigger_kinds = HashSet::new();

    if let Some(target_mode) = mode {
        // First, collect specifically for this mode
        for (trigger, actions) in binds.iter() {
            if trigger.mode == target_mode {
                // todo: resolve needs a option to keep semantic definitions
                if config.hide_semantic && matches!(trigger.kind, TriggerKind::Semantic(_)) {
                    continue;
                }
                if !config.show_events && matches!(trigger.kind, TriggerKind::Event(_)) {
                    continue;
                }
                seen_trigger_kinds.insert(trigger.kind.clone());

                let trigger_str = if let TriggerKind::Event(_) = trigger.kind {
                    format!("{}{}", config.event_trigger_prefix, trigger.kind)
                } else {
                    trigger.kind.to_string()
                };

                let f_key_num = if matches!(trigger.kind, TriggerKind::Key(_))
                    && trigger_str.starts_with('F')
                    && trigger_str.len() > 1
                    && trigger_str[1..].chars().all(|c| c.is_ascii_digit())
                {
                    trigger_str[1..].parse::<u32>().ok()
                } else {
                    None
                };

                entries.push((trigger_str, actions.iter().cloned().collect(), f_key_num));
            }
        }
    }

    // Then, collect for default mode (mode = "") for triggers not seen yet
    for (trigger, actions) in binds.iter() {
        if trigger.mode.is_empty() && !seen_trigger_kinds.contains(&trigger.kind) {
            if config.hide_semantic && matches!(trigger.kind, TriggerKind::Semantic(_)) {
                continue;
            }
            if !config.show_events && matches!(trigger.kind, TriggerKind::Event(_)) {
                continue;
            }

            let trigger_str = if let TriggerKind::Event(_) = trigger.kind {
                format!("{}{}", config.event_trigger_prefix, trigger.kind)
            } else {
                trigger.kind.to_string()
            };

            let f_key_num = if matches!(trigger.kind, TriggerKind::Key(_))
                && trigger_str.starts_with('F')
                && trigger_str.len() > 1
                && trigger_str[1..].chars().all(|c| c.is_ascii_digit())
            {
                trigger_str[1..].parse::<u32>().ok()
            } else {
                None
            };

            entries.push((trigger_str, actions.iter().cloned().collect(), f_key_num));
        }
    }

    // Sort by actions (values) instead of triggers
    entries.sort_by(|a, b| {
        if config.sort_fn_last && a.2.is_some() != b.2.is_some() {
            return a.2.is_some().cmp(&b.2.is_some());
        }

        if config.sort_fn_last && a.2.is_some() {
            // If sort_fn_last is true and we are in the last section (F-keys), sort by numeric value
            return a.2.cmp(&b.2);
        }

        let s1: Vec<String> = a.1.iter().map(|a| a.to_string()).collect();
        let s2: Vec<String> = b.1.iter().map(|a| a.to_string()).collect();
        s1.cmp(&s2)
    });

    // Process all bindings into their final visible string sequences. Items between trace delimiters are replaced by the trace message
    let entries_processed: Vec<(String, Vec<String>)> = entries
        .into_iter()
        .map(|(trigger, actions, _)| {
            let mut visible_items = Vec::new();
            let mut skipping = false;
            let mut last_trace = String::new();

            for action in actions {
                if let Action::Trace(s) = action {
                    if s.is_empty() {
                        if skipping {
                            // Expected empty, push the saved nonempty trace
                            visible_items.push(last_trace);
                        } // Empty signals resume

                        last_trace = String::new();
                        skipping = false;
                    } else {
                        if skipping {
                            // Expected empty, got nonempty (treat as if extra empty before)
                            visible_items.push(last_trace);
                        }

                        // Start or continue skipping
                        let (display_trace, is_alias_trace) =
                            if let Some(alias) = s.strip_prefix("@@") {
                                (format!("@{alias}"), true)
                            } else {
                                (s.clone(), false)
                            };

                        last_trace = if config.quote_traces && !is_alias_trace {
                            format!("\"{display_trace}\"")
                        } else {
                            display_trace
                        };
                        skipping = true;
                    }
                } else if !skipping {
                    visible_items.push(action.to_string().ellipsize(
                        config.max_item_len,
                        if config.ellipsize_center {
                            fmt::Alignment::Center
                        } else {
                            fmt::Alignment::Left
                        },
                    ));
                }
            }

            if skipping && !last_trace.is_empty() {
                visible_items.push(last_trace);
            }

            (trigger, visible_items)
        })
        .collect();

    // Build output
    let Some(cfg) = &config.colors else {
        // fallback plain text
        let mut text = Text::default();
        for (trigger, actions) in entries_processed {
            let value = if actions.is_empty() {
                String::new()
            } else if actions.len() == 1 {
                actions[0].clone()
            } else {
                let inner = actions.join(", ");
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

    for (trigger, actions) in entries_processed {
        let mut spans = vec![
            Span::styled(trigger, Style::default().fg(cfg.key)),
            Span::raw(" = "),
        ];

        if actions.len() > 1 {
            if let Some([open, _]) = config.seq_brackets {
                spans.push(Span::raw(open.to_string()));
            }

            for (i, item) in actions.into_iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(", "));
                }
                spans.push(Span::styled(item, Style::default().fg(cfg.value)));
            }

            if let Some([_, close]) = config.seq_brackets {
                spans.push(Span::raw(close.to_string()));
            }
        } else if let Some(single_item) = actions.first() {
            spans.push(Span::styled(
                single_item.clone(),
                Style::default().fg(cfg.value),
            ));
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
        assert_eq!(t.to_string(), "a (vim)a");

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

        // key(a) should resolve directly to Accept (with traces)
        let actions = bind_map.get(&key!(a).into()).unwrap();
        // It will be [Trace("@@s1"), Trace("@@s2"), Accept, Trace(""), Trace("")]
        // Actually, wait:
        // resolve_actions([@s1])
        //   resolve_alias(s1) -> [@s2]
        //   has_nested = true
        //   flat = resolve_actions([@s2])
        //     resolve_alias(s2) -> [Accept]
        //     has_nested = false
        //     flat = [Accept]
        //     already_traced = false
        //     returns [Trace("@@s2"), Accept, Trace("")]
        //   already_traced = true
        //   returns [Trace("@@s2"), Accept, Trace("")]
        assert_eq!(actions.len(), 3);
        assert_eq!(actions.0[0], Action::Trace("@@s2".into()));
        assert_eq!(actions.0[1], Action::Accept);
        assert_eq!(actions.0[2], Action::Trace(String::new()));

        // @s1 should be GONE
        assert!(!bind_map.contains_key(&Trigger {
            kind: TriggerKind::Semantic("s1".into()),
            mode: String::new()
        }));

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
        let actions = bind_map_multi.get(&key!(c).into()).unwrap();
        // @s5 is not traced because it has nested aliases?
        // Wait, resolve_actions([@s5]):
        //   alias_actions = [@s6, Accept]
        //   has_nested = true
        //   flat = resolve_actions([@s6, Accept])
        //     resolve_actions([@s6]) -> [Trace("@@s6"), Cancel, Trace("")]
        //     resolve_actions([Accept]) -> [Accept]
        //     returns [Trace("@@s6"), Cancel, Trace(""), Accept]
        //   already_traced = true
        //   returns [Trace("@@s6"), Cancel, Trace(""), Accept]
        assert_eq!(actions.len(), 4);
    }

    #[test]
    fn test_resolve_semantics_with_trace() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Chain with Trace: key(a) -> @s1 -> @s2 -> [Trace("desc"), Accept]
        let mut bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: String::new()
            } => Action::Semantic("s2".into()),
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: String::new()
            } => [Action::Trace("desc".into()), Action::Accept],
        );
        bind_map.resolve_semantics();

        // key(a) should resolve directly to [Trace("desc"), Accept]
        let actions = bind_map.get(&key!(a).into()).unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions.0[0], Action::Trace("desc".into()));
        assert_eq!(actions.0[1], Action::Accept);
    }

    #[test]
    fn test_resolve_semantics_modes() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // key(a) -> @s1
        // @s1 is Accept in default mode
        // @s1 is Cancel in "mode1"
        let mut bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: String::new()
            } => Action::Accept,
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: "mode1".into()
            } => Action::Cancel,
        );

        bind_map.resolve_semantics();

        // key(a) in default mode should be Accept
        let a_default = bind_map.get(&key!(a).into()).unwrap();
        assert!(a_default.iter().any(|a| matches!(a, Action::Accept)));

        // key(a) in mode1 should be Cancel
        let a_mode1 = bind_map
            .get(&Trigger {
                kind: TriggerKind::Key(key!(a)),
                mode: "mode1".into(),
            })
            .unwrap();
        assert!(a_mode1.iter().any(|a| matches!(a, Action::Cancel)));
    }

    #[test]
    fn test_resolve_semantics_intersection() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // key(a) -> [@s1, @s2]
        // @s1 defined in default, m1, m2
        // @s2 defined in default, m1, m3
        // Intersection: default, m1
        let mut bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => [Action::Semantic("s1".into()), Action::Semantic("s2".into())],
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: String::new()
            } => Action::Select,
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: "m1".into()
            } => Action::Select,
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: "m2".into()
            } => Action::Select,

            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: String::new()
            } => Action::Deselect,
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: "m1".into()
            } => Action::Deselect,
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: "m3".into()
            } => Action::Deselect,
        );

        bind_map.resolve_semantics();

        // key(a) in default
        assert!(bind_map.contains_key(&key!(a).into()));
        // key(a) in m1
        assert!(bind_map.contains_key(&Trigger {
            kind: TriggerKind::Key(key!(a)),
            mode: "m1".into()
        }));
        // key(a) in m2 - NO
        assert!(!bind_map.contains_key(&Trigger {
            kind: TriggerKind::Key(key!(a)),
            mode: "m2".into()
        }));
        // key(a) in m3 - NO
        assert!(!bind_map.contains_key(&Trigger {
            kind: TriggerKind::Key(key!(a)),
            mode: "m3".into()
        }));
    }

    #[test]
    fn test_display_help_semantic_help() {
        let binds: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Print("a".into()),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: String::new()
            } => Action::Print("foo".into()),
        );

        // With semantic help
        let mut cfg = HelpDisplayConfig::default();
        cfg.hide_semantic = false;
        let help_show = display_help(&binds, &cfg, None);
        let help_show_str = help_show.to_string();
        assert!(help_show_str.contains("a = Print(a)"));
        assert!(help_show_str.contains("@foo = Print(foo)"));

        // Without semantic help
        let mut cfg_hide = HelpDisplayConfig::default();
        cfg_hide.hide_semantic = true;
        let help_hide = display_help(&binds, &cfg_hide, None);
        let help_hide_str = help_hide.to_string();
        assert!(help_hide_str.contains("a = Print(a)"));
        assert!(!help_hide_str.contains("@foo = Print(foo)"));
    }

    #[test]
    fn test_display_help_sort_fn_last() {
        let binds: BindMap<NullActionExt> = bindmap!(
            key!(F1) => Action::Print("f1".into()),
            key!(a) => Action::Print("a".into()),
            key!(F2) => Action::Print("f2".into()),
            key!(b) => Action::Print("b".into()),
        );

        let mut cfg = HelpDisplayConfig::default();
        cfg.sort_fn_last = true;
        let help = display_help(&binds, &cfg, None);
        let help_str = help.to_string();

        let lines: Vec<_> = help_str.lines().filter(|l| !l.is_empty()).collect();
        // Sorting is by action first, then is_f_key? 
        // No, my code:
        /*
        if config.sort_fn_last && a.2 != b.2 {
            return a.2.cmp(&b.2);
        }
        */
        // so non-F (a.2=false) < F (a.2=true).
        // Then by action.
        assert!(lines[0].contains("a = Print(a)"));
        assert!(lines[1].contains("b = Print(b)"));
        assert!(lines[2].contains("F1 = Print(f1)"));
        assert!(lines[3].contains("F2 = Print(f2)"));

        // Disable sort_fn_last
        cfg.sort_fn_last = false;
        let help_no_sort = display_help(&binds, &cfg, None);
        let help_no_sort_str = help_no_sort.to_string();
        let _lines_no_sort: Vec<_> = help_no_sort_str.lines().filter(|l| !l.is_empty()).collect();
        
        let binds_diff: BindMap<NullActionExt> = bindmap!(
            key!(F1) => Action::Print("aaa".into()),
            key!(b) => Action::Print("bbb".into()),
        );
        
        // sort_fn_last = true -> b (non-F) then F1
        cfg.sort_fn_last = true;
        let help_diff = display_help(&binds_diff, &cfg, None);
        let help_diff_str = help_diff.to_string();
        let lines_diff: Vec<_> = help_diff_str.lines().filter(|l| !l.is_empty()).collect();
        assert!(lines_diff[0].contains("b = Print(bbb)"));
        assert!(lines_diff[1].contains("F1 = Print(aaa)"));

        // sort_fn_last = false -> F1 (Print(aaa)) then b (Print(bbb))
        cfg.sort_fn_last = false;
        let help_diff_no = display_help(&binds_diff, &cfg, None);
        let help_diff_no_str = help_diff_no.to_string();
        let lines_diff_no: Vec<_> = help_diff_no_str.lines().filter(|l| !l.is_empty()).collect();
        assert!(lines_diff_no[0].contains("F1 = Print(aaa)"));
        assert!(lines_diff_no[1].contains("b = Print(bbb)"));
    }

    #[test]
    fn test_display_help_events() {
        let binds: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Print("a".into()),
            Trigger {
                kind: TriggerKind::Event(Event::Start),
                mode: String::new()
            } => Action::Print("start".into()),
        );

        let mut cfg = HelpDisplayConfig::default();
        cfg.event_trigger_prefix = "EV:".to_string();
        cfg.show_events = true;
        let help_show = display_help(&binds, &cfg, None);
        let help_str = help_show.to_string();
        assert!(help_str.contains("EV:Start"));

        cfg.show_events = false;
        let help_hide = display_help(&binds, &cfg, None);
        let help_str_hide = help_hide.to_string();
        assert!(!help_str_hide.contains("Start"));
    }
}
