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

pub use super::mode_filter::PrefixFilter;
pub use crate::bindmap;
pub use crokey::{KeyCombination, key};
pub use crossterm::event::{KeyModifiers, MouseButton, MouseEventKind};

#[allow(type_alias_bounds)]
pub type BindMap<A: ActionExt = NullActionExt> = HashMap<Trigger, Actions<A>>;

/// A mode-specific resolved bind map that uses `TriggerKind` for O(1) lookups.
///
/// Produced by [`BindMapExt::resolve_semantics`]. All semantic aliases have been
/// resolved, and the map only contains triggers that match the given mode.
pub type ResolvedBindMap<A = NullActionExt> = HashMap<TriggerKind, Actions<A>>;

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
            key!(ctrl-u) => Action::ClearQuery,
            key!(alt-a) => Action::QueryPos(0),

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

    /// non-"universal" keybinds, some requiring keyboard enhancements, mostly for copying sections from
    pub fn with_extras(mut self) -> Self {
        let ext = bindmap!(
            // keyboard enhancement
            key!(ctrl-'[') => Action::ToggleWrap,
            key!(alt-']') => Action::TogglePreviewWrap,
            key!(alt-'{') => Action::ToggleWrap,
            key!(alt-'}') => Action::TogglePreviewWrap,

            key!(ctrl-shift-right) => Action::HScroll(1),
            key!(ctrl-shift-left) => Action::HScroll(-1),
            key!(ctrl-shift-up) => Action::VScroll(1),
            key!(ctrl-shift-down) => Action::VScroll(-1),
            key!(alt-right) => Action::HScroll(1),
            key!(alt-left) => Action::HScroll(-1),
            key!(alt-up) => Action::VScroll(1),
            key!(alt-down) => Action::VScroll(-1),
            key!(alt-'/') => Action::NextPreview,
            key!(alt-shift-'/') => Action::PrevPreview,
            key!(ctrl-'/') => Action::NextPreview,
            key!(ctrl-shift-'/') => Action::PrevPreview,
            key!(alt-h) => Action::Help("".to_string()),

            key!(tab) => [Action::ToggleSelection, Action::Down(1)],
            key!(shift-backtab) => [Action::ToggleSelection, Action::Up(1)],
            key!(ctrl-a) => Action::CycleSelections,
            key!(ctrl-shift-a) => Action::ClearSelections

            // not currently supported by crossterm
            // "shift+scrollup" = "PreviewUp"
            // "shift+scrolldown" = "PreviewDown"

        );
        self.extend(ext);
        self
    }

    pub fn extend_from(&mut self, mut others: Self) {
        others.extend(std::mem::take(self));
        *self = others;
    }

    /// Check for infinite loops in semantic actions.
    pub fn check_cycles(&self) -> Result<(), String> {
        for actions in self.values() {
            for action in actions {
                if let Action::Semantic(s) = action {
                    let mut path = Vec::new();
                    dfs_semantic(self, s, &mut path)?;
                }
            }
        }
        Ok(())
    }

    /// Resolves all semantic aliases into concrete actions for the given mode.
    ///
    /// This filters the bind map to only include triggers whose `mode` PrefixFilter
    /// matches the given `mode` string, then fully resolves all semantic aliases.
    /// Triggers that don't resolve successfully (e.g., a semantic trigger with no
    /// actions) are filtered out.
    /// Resolves all semantic aliases into concrete actions for the given mode.
    ///
    /// Returns a [`ResolvedBindMap`] keyed by [`TriggerKind`] for O(1) lookups.
    pub fn resolve_semantics(&self, mode: &[Box<str>]) -> ResolvedBindMap<A> {
        let mut resolved: ResolvedBindMap<A> = HashMap::new();

        // Iterate through all triggers and resolve those matching the current mode
        for (trigger, actions) in self.iter() {
            // Only include triggers whose mode filter matches the current mode
            if !trigger.mode.matches(mode) {
                continue;
            }

            // Resolve the actions (replaces semantic aliases with concrete actions)
            if let Some(resolved_actions) = self.resolve_actions(actions, mode) {
                resolved.insert(trigger.kind.clone(), resolved_actions);
            }
            // If resolve_actions returns None, the trigger is dropped (e.g., unbound alias)
        }

        resolved
    }

    pub fn resolve_actions(&self, actions: &Actions<A>, mode: &[Box<str>]) -> Option<Actions<A>> {
        let mut resolved = Vec::new();
        let mut in_trace = false;

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

                    // inner alias names replace outer alias display in chains
                    let already_traced = matches!(flat_actions.first(), Some(Action::Trace(_)));
                    if !already_traced && !in_trace {
                        resolved.push(Action::Trace(format!("@@{alias}")));
                        resolved.extend(flat_actions);
                        resolved.push(Action::Trace(String::new()));
                    } else {
                        resolved.extend(flat_actions);
                    }
                } else {
                    // If any alias is unresolvable, the whole sequence is invalid
                    return None;
                }
            } else {
                if let Action::Trace(s) = action {
                    in_trace = !s.is_empty();
                }
                resolved.push(action.clone());
            }
        }

        if resolved.is_empty() {
            None
        } else {
            Some(Actions(resolved))
        }
    }

    /// Find a semantic trigger with the given alias whose mode filter matches the current mode.
    /// Prefer the most specific match (one with non-empty mode that matches) over a fallback
    /// (empty mode filter that matches everything).
    /// O(N) but we use the resolved bindmap at runtime.
    pub fn resolve_alias(&self, alias: &str, mode: &[Box<str>]) -> Option<&Actions<A>> {
        let mut fallback = None;

        for (trigger, actions) in self.iter() {
            if let TriggerKind::Semantic(name) = &trigger.kind
                && name == alias
            {
                if !trigger.mode.is_empty() && trigger.mode.matches(mode) {
                    // Most specific match found
                    return Some(actions);
                }
                if trigger.mode.is_empty() {
                    fallback = Some(actions);
                }
            }
        }

        fallback
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

fn dfs_semantic<A: ActionExt>(
    binds: &BindMap<A>,
    current: &str,
    path: &mut Vec<String>,
) -> Result<(), String> {
    if path.contains(&current.to_string()) {
        return Err(format!(
            "Infinite loop detected in semantic actions: {} -> {}",
            path.join(" -> "),
            current
        ));
    }

    path.push(current.to_string());
    if let Some(actions) = binds.get(&Trigger {
        kind: TriggerKind::Semantic(current.to_string()),
        mode: PrefixFilter::default(),
    }) {
        for action in actions {
            if let Action::Semantic(next) = action {
                dfs_semantic(binds, next, path)?;
            }
        }
    }
    path.pop();

    Ok(())
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
    /// This is serialized/deserialized with a `@` prefix, such as "@open" = "ExecuteOrConfirm(open {})"
    Semantic(String),
}

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct Trigger {
    pub kind: TriggerKind,
    pub mode: PrefixFilter,
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
            mode: PrefixFilter::default(),
        }
    }
}

impl From<KeyCombination> for Trigger {
    fn from(key: KeyCombination) -> Self {
        Trigger {
            kind: TriggerKind::Key(key),
            mode: PrefixFilter::default(),
        }
    }
}

impl From<Event> for Trigger {
    fn from(event: Event) -> Self {
        Trigger {
            kind: TriggerKind::Event(event),
            mode: PrefixFilter::default(),
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
            write!(f, "{}^^{}", self.kind, self.mode)
        } else {
            write!(f, "{}", self.kind)
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
        if let Some((mode, kind_str)) = value.split_once("^^")
            && !mode.is_empty()
        {
            let mode_filter = PrefixFilter::from_str(mode)?;
            let kind = TriggerKind::from_str(kind_str)?;
            return Ok(Trigger {
                kind,
                mode: mode_filter,
            });
        }

        let kind = TriggerKind::from_str(value)?;
        Ok(Trigger {
            kind,
            mode: PrefixFilter::default(),
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
    resolved: &ResolvedBindMap<A>,
    config: &HelpDisplayConfig,
) -> Text<'static> {
    let mut entries: Vec<(String, Vec<Action<A>>, Option<u32>)> = Vec::new();

    for (kind, actions) in resolved.iter() {
        if config.hide_semantic && matches!(kind, TriggerKind::Semantic(_)) {
            continue;
        }
        if !config.show_events && matches!(kind, TriggerKind::Event(_)) {
            continue;
        }

        let trigger_str = if let TriggerKind::Event(_) = kind {
            format!("{}{}", config.event_trigger_prefix, kind)
        } else {
            kind.to_string()
        };

        let f_key_num = if matches!(kind, TriggerKind::Key(_))
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

    // Process all bindings into their final visible string sequences. Items between trace delimiters are replaced by the trace message
    let mut entries_processed: Vec<(String, Vec<String>)> = entries
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

    // 2. Sort directly on the final string representations
    entries_processed.sort_by(|a, b| {
        // Handle F-key positioning
        if config.sort_fn_last {
            let get_f_key = |trigger: &str| -> Option<u32> {
                if trigger.starts_with('F')
                    && trigger.len() > 1
                    && trigger[1..].chars().all(|c| c.is_ascii_digit())
                {
                    trigger[1..].parse::<u32>().ok()
                } else {
                    None
                }
            };

            let a_fkey = get_f_key(&a.0);
            let b_fkey = get_f_key(&b.0);

            if a_fkey.is_some() != b_fkey.is_some() {
                return a_fkey.is_some().cmp(&b_fkey.is_some());
            }
            if a_fkey.is_some() {
                return a_fkey.cmp(&b_fkey);
            }
        }

        // Handle Trace prioritization purely from the strings
        if config.quote_traces {
            let is_trace_str = |items: &[String]| -> bool {
                // Every item in the sequence must look like a trace
                items.iter().all(
                    |s| s.starts_with('"') && s.ends_with('"'), // || s.starts_with('@')
                )
            };

            let a_trace = is_trace_str(&a.1);
            let b_trace = is_trace_str(&b.1);

            if a_trace != b_trace {
                return b_trace.cmp(&a_trace); // Prioritize true over false
            }
        }

        // Fallback to alphabetical sorting
        a.1.cmp(&b.1)
    });

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

    /// Helper to convert a mode string like `"0,1"` to a `Vec<Box<str>>` for tests.
    fn mode_vec(s: &str) -> Vec<Box<str>> {
        s.split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| s.into())
            .collect()
    }

    #[test]
    fn test_prefix_filter() {
        // Empty filter matches everything
        let empty = PrefixFilter::default();
        assert!(empty.matches(&[]));
        assert!(empty.matches(&["0".into()]));
        assert!(empty.matches(&["0".into(), "1".into()]));
        assert!(empty.matches(&["vim".into()]));

        // Single positive prefix
        let f = PrefixFilter::from_str("0").unwrap();
        assert!(!f.matches(&[]));
        assert!(f.matches(&["0".into()]));
        assert!(f.matches(&["0".into(), "1".into()]));
        assert!(f.matches(&["0".into(), "vim".into()]));
        assert!(!f.matches(&["1".into()]));
        assert!(!f.matches(&["vim".into()]));

        // Multiple positive prefixes (AND)
        let f = PrefixFilter::from_str("0,1").unwrap();
        assert!(!f.matches(&[]));
        assert!(!f.matches(&["0".into()]));
        assert!(!f.matches(&["1".into()]));
        assert!(f.matches(&["0".into(), "1".into()]));
        assert!(f.matches(&["0".into(), "1".into(), "vim".into()]));
        assert!(f.matches(&["1".into(), "0".into()])); // order doesn't matter, both tags present
        assert!(!f.matches(&["vim".into()]));

        // Negative prefix
        let f = PrefixFilter::from_str("!0").unwrap();
        assert!(f.matches(&[]));
        assert!(!f.matches(&["0".into()]));
        assert!(!f.matches(&["0".into(), "1".into()]));
        assert!(f.matches(&["1".into()]));
        assert!(f.matches(&["vim".into()]));

        // Combined positive and negative
        let f = PrefixFilter::from_str("0,!1").unwrap();
        assert!(!f.matches(&[]));
        assert!(f.matches(&["0".into()]));
        assert!(!f.matches(&["0".into(), "1".into()]));
        assert!(f.matches(&["0".into(), "vim".into()]));
        assert!(!f.matches(&["1".into()]));
        assert!(!f.matches(&["vim".into()]));

        // Prefix matching (not just exact)
        let f = PrefixFilter::from_str("vim").unwrap();
        assert!(f.matches(&["vim".into()]));
        assert!(f.matches(&["vim_insert".into()])); // matches because starts with "vim"
        assert!(!f.matches(&["vi".into()]));

        // from() with patterns
        let f = PrefixFilter::from(vec!["0", "!1"]).unwrap();
        assert_eq!(f.positive_prefixes, vec!["0".to_string()]);
        assert_eq!(f.negative_prefixes, vec!["1".to_string()]);

        // is_empty
        assert!(PrefixFilter::default().is_empty());
        assert!(!PrefixFilter::from_str("0").unwrap().is_empty());

        // Display
        assert_eq!(PrefixFilter::from_str("0,1").unwrap().to_string(), "0,1");
        assert_eq!(PrefixFilter::from_str("0,!1").unwrap().to_string(), "0,!1");
        assert_eq!(PrefixFilter::default().to_string(), "");
    }

    #[test]
    fn test_bindmap_trigger() {
        let mut bind_map: BindMap = BindMap::new();

        // Insert trigger with default actions
        let trigger0 = Trigger {
            kind: TriggerKind::Mouse(SimpleMouseEvent {
                kind: MouseEventKind::ScrollDown,
                modifiers: KeyModifiers::empty(),
            }),
            mode: PrefixFilter::default(),
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
            mode: PrefixFilter::default(),
        };
        assert!(!bind_map.contains_key(&shift_trigger));
    }

    #[test]
    fn test_semantic_parsing() {
        assert_eq!(
            Trigger::from_str("@foo").unwrap(),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: PrefixFilter::default()
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
        assert_eq!(t.mode, PrefixFilter::from_str("vim").unwrap());
        assert_eq!(t.kind, TriggerKind::Key(key!(a)));
        assert_eq!(t.to_string(), "a^^vim");

        let t2 = Trigger::from_str("a").unwrap();
        assert_eq!(t2.mode, PrefixFilter::default());
        assert_eq!(t2.kind, TriggerKind::Key(key!(a)));
        assert_eq!(t2.to_string(), "a");

        // Comma-separated prefix patterns
        let t3 = Trigger::from_str("0,1^^enter").unwrap();
        assert_eq!(t3.mode, PrefixFilter::from_str("0,1").unwrap());

        // Negative prefix
        let t4 = Trigger::from_str("!0^^enter").unwrap();
        assert_eq!(t4.mode, PrefixFilter::from_str("!0").unwrap());

        // Empty mode -> whole string parsed as TriggerKind, which fails here
        assert!(Trigger::from_str("^^a").is_err());
    }

    #[test]
    fn test_check_cycles() {
        use crate::bindmap;
        let bind_map: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("b".into()),
            Trigger {
                kind: TriggerKind::Semantic("b".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("a".into()),
        );
        assert!(bind_map.check_cycles().is_err());

        let bind_map_no_cycle: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("b".into()),
            Trigger {
                kind: TriggerKind::Semantic("b".into()),
                mode: PrefixFilter::default()
            } => Action::Print("ok".into()),
        );
        assert!(bind_map_no_cycle.check_cycles().is_ok());

        let bind_map_self_cycle: BindMap = bindmap!(
            Trigger {
                kind: TriggerKind::Semantic("a".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("a".into()),
        );
        assert!(bind_map_self_cycle.check_cycles().is_err());

        let bind_map_indirect_cycle: BindMap = bindmap!(
            key!(a) => Action::Semantic("foo".into()),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("foo".into()),
        );
        assert!(bind_map_indirect_cycle.check_cycles().is_err());
    }

    #[test]
    fn test_resolve_semantics_basic_chains() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Chain: key(a) -> @s1 -> @s2 -> concrete
        let bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("s2".into()),
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: PrefixFilter::default()
            } => Action::Accept,
        );

        let resolved = bind_map.resolve_semantics(&mode_vec(""));

        // key(a) resolves directly to Accept (wrapped in Trace for the innermost alias)
        let actions_a = resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(
            actions_a.0,
            vec![
                Action::Trace("@@s2".into()),
                Action::Accept,
                Action::Trace(String::new())
            ]
        );

        // The intermediate alias @s1 also exists in the resolved map and resolves to Accept
        let actions_s1 = resolved.get(&TriggerKind::Semantic("s1".into())).unwrap();
        assert_eq!(
            actions_s1.0,
            vec![
                Action::Trace("@@s2".into()),
                Action::Accept,
                Action::Trace(String::new())
            ]
        );

        // The final alias @s2 also exists in the resolved map and resolves to Accept
        let actions_s2 = resolved.get(&TriggerKind::Semantic("s2".into())).unwrap();
        assert_eq!(actions_s2.0, vec![Action::Accept]);
    }

    #[test]
    fn test_resolve_semantics_unbound_and_partial() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Entirely unbound: key(b) -> @s3 -> @s4 -> unbound
        let bind_map_unbound: BindMap<NullActionExt> = bindmap!(
            key!(b) => Action::Semantic("s3".into()),
            Trigger {
                kind: TriggerKind::Semantic("s3".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("s4".into()),
        );
        let resolved_unbound = bind_map_unbound.resolve_semantics(&mode_vec(""));
        assert!(!resolved_unbound.contains_key(&TriggerKind::Key(key!(b))));

        // Partially unbound multi-action: key(c) -> [@s5, Accept], where @s5 resolves but @s6 does not
        let bind_map_partial: BindMap<NullActionExt> = bindmap!(
            key!(c) => [Action::Semantic("s5".into()), Action::Accept],
            Trigger {
                kind: TriggerKind::Semantic("s5".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("s6".into()),
        );
        let resolved_partial = bind_map_partial.resolve_semantics(&mode_vec(""));
        assert!(!resolved_partial.contains_key(&TriggerKind::Key(key!(c))));
    }

    #[test]
    fn test_resolve_semantics_multi_action_chain() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Multi-action chain: key(c) -> @s5 -> [@s6, Accept]
        let bind_map_multi: BindMap<NullActionExt> = bindmap!(
            key!(c) => Action::Semantic("s5".into()),
            Trigger {
                kind: TriggerKind::Semantic("s5".into()),
                mode: PrefixFilter::default()
            } => [Action::Semantic("s6".into()), Action::Accept],
            Trigger {
                kind: TriggerKind::Semantic("s6".into()),
                mode: PrefixFilter::default()
            } => Action::ClearQuery,
        );
        let resolved_multi = bind_map_multi.resolve_semantics(&mode_vec(""));
        let actions = resolved_multi.get(&TriggerKind::Key(key!(c))).unwrap();
        assert_eq!(
            actions.0,
            vec![
                Action::Trace("@@s6".into()),
                Action::ClearQuery,
                Action::Trace(String::new()),
                Action::Accept
            ]
        );
    }

    #[test]
    fn test_resolve_semantics_with_trace() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // Chain with Trace: key(a) -> @s1 -> @s2 -> [Trace("desc"), Accept]
        let bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::default()
            } => Action::Semantic("s2".into()),
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: PrefixFilter::default()
            } => [Action::Trace("desc".into()), Action::Accept],
        );
        let resolved = bind_map.resolve_semantics(&mode_vec(""));

        // key(a) should resolve directly to [Trace("desc"), Accept] without any extra @@s2 trace wrapped around it
        let actions = resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(
            actions.0,
            vec![Action::Trace("desc".into()), Action::Accept]
        );
    }

    #[test]
    fn test_resolve_semantics_modes() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        // key(a) -> @s1
        // @s1 is Accept in default mode
        // @s1 is ClearQuery in "mode1"
        let bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Semantic("s1".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::default()
            } => Action::Accept,
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::from_str("mode1").unwrap()
            } => Action::ClearQuery,
        );

        // Resolve for default mode
        let default_resolved = bind_map.resolve_semantics(&mode_vec(""));
        let a_default = default_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(
            a_default.0,
            vec![
                Action::Trace("@@s1".into()),
                Action::Accept,
                Action::Trace(String::new())
            ]
        );

        // Resolve for mode1
        let mode1_resolved = bind_map.resolve_semantics(&mode_vec("mode1"));
        let a_mode1 = mode1_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(
            a_mode1.0,
            vec![
                Action::Trace("@@s1".into()),
                Action::ClearQuery,
                Action::Trace(String::new())
            ]
        );

        // Resolve for mode2 (falls back to default Accept)
        let mode2_resolved = bind_map.resolve_semantics(&mode_vec("mode2"));
        let a_mode2 = mode2_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(
            a_mode2.0,
            vec![
                Action::Trace("@@s1".into()),
                Action::Accept,
                Action::Trace(String::new())
            ]
        );
    }

    #[test]
    fn test_resolve_semantics_key_modes() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        let bind_map: BindMap<NullActionExt> = bindmap!(
            Trigger {
                kind: TriggerKind::Key(key!(a)),
                mode: PrefixFilter::from_str("mode1").unwrap()
            } => Action::Accept,
            Trigger {
                kind: TriggerKind::Key(key!(b)),
                mode: PrefixFilter::default()
            } => Action::Accept,
        );

        // In default mode, key(a) is filtered out because it is mode1-specific,
        // but key(b) is included because its mode matches default.
        let default_resolved = bind_map.resolve_semantics(&mode_vec(""));
        assert!(!default_resolved.contains_key(&TriggerKind::Key(key!(a))));
        assert!(default_resolved.contains_key(&TriggerKind::Key(key!(b))));

        // In mode1, both key(a) and key(b) match.
        let mode1_resolved = bind_map.resolve_semantics(&mode_vec("mode1"));
        assert!(mode1_resolved.contains_key(&TriggerKind::Key(key!(a))));
        assert!(mode1_resolved.contains_key(&TriggerKind::Key(key!(b))));

        // In mode2, key(a) is filtered out, key(b) is included.
        let mode2_resolved = bind_map.resolve_semantics(&mode_vec("mode2"));
        assert!(!mode2_resolved.contains_key(&TriggerKind::Key(key!(a))));
        assert!(mode2_resolved.contains_key(&TriggerKind::Key(key!(b))));
    }

    #[test]
    fn test_resolve_semantics_intersection() {
        use crate::action::NullActionExt;
        use crate::bindmap;

        let bind_map: BindMap<NullActionExt> = bindmap!(
            key!(a) => [Action::Semantic("s1".into()), Action::Semantic("s2".into())],
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::default()
            } => Action::Print("s1_default".into()),
            Trigger {
                kind: TriggerKind::Semantic("s1".into()),
                mode: PrefixFilter::from_str("m1").unwrap()
            } => Action::Print("s1_m1".into()),

            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: PrefixFilter::default()
            } => Action::Print("s2_default".into()),
            Trigger {
                kind: TriggerKind::Semantic("s2".into()),
                mode: PrefixFilter::from_str("m1").unwrap()
            } => Action::Print("s2_m1".into()),
        );

        // Resolve for default mode: both fall back to default
        let default_resolved = bind_map.resolve_semantics(&mode_vec(""));
        let actions = default_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(actions.0.len(), 6);
        assert_eq!(actions.0[1], Action::Print("s1_default".into()));
        assert_eq!(actions.0[4], Action::Print("s2_default".into()));

        // Resolve for m1: both use m1 definition
        let m1_resolved = bind_map.resolve_semantics(&mode_vec("m1"));
        let actions_m1 = m1_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(actions_m1.0[1], Action::Print("s1_m1".into()));
        assert_eq!(actions_m1.0[4], Action::Print("s2_m1".into()));

        // Resolve for m2: s1 falls back to default, s2 falls back to default
        let m2_resolved = bind_map.resolve_semantics(&mode_vec("m2"));
        let actions_m2 = m2_resolved.get(&TriggerKind::Key(key!(a))).unwrap();
        assert_eq!(actions_m2.0[1], Action::Print("s1_default".into()));
        assert_eq!(actions_m2.0[4], Action::Print("s2_default".into()));
    }

    #[test]
    fn test_display_help_semantic_help() {
        let binds: BindMap<NullActionExt> = bindmap!(
            key!(a) => Action::Print("a".into()),
            Trigger {
                kind: TriggerKind::Semantic("foo".into()),
                mode: PrefixFilter::default()
            } => Action::Print("foo".into()),
        );

        // With semantic help
        let mut cfg = HelpDisplayConfig::default();
        cfg.hide_semantic = false;

        let help_show = display_help(&binds.resolve_semantics(&[]), &cfg);
        let help_show_str = help_show.to_string();
        assert!(help_show_str.contains("a = Print(a)"));
        assert!(help_show_str.contains("@foo = Print(foo)"));

        // Without semantic help
        let mut cfg_hide = HelpDisplayConfig::default();
        cfg_hide.hide_semantic = true;
        let help_hide = display_help(&binds.resolve_semantics(&[]), &cfg_hide);
        let help_hide_str = help_hide.to_string();
        assert!(help_hide_str.contains("a = Print(a)"));
        assert!(!help_hide_str.contains("@foo"));
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
        let help = display_help(&binds.resolve_semantics(&[]), &cfg);
        let help_str = help.to_string();

        let lines: Vec<_> = help_str.lines().filter(|l| !l.is_empty()).collect();
        assert!(lines[0].contains("a = Print(a)"));
        assert!(lines[1].contains("b = Print(b)"));
        assert!(lines[2].contains("F1 = Print(f1)"));
        assert!(lines[3].contains("F2 = Print(f2)"));

        // Disable sort_fn_last
        cfg.sort_fn_last = false;
        let help_no_sort = display_help(&binds.resolve_semantics(&[]), &cfg);
        let help_no_sort_str = help_no_sort.to_string();
        let _lines_no_sort: Vec<_> = help_no_sort_str.lines().filter(|l| !l.is_empty()).collect();

        let binds_diff: BindMap<NullActionExt> = bindmap!(
            key!(F1) => Action::Print("aaa".into()),
            key!(b) => Action::Print("bbb".into()),
        );

        // sort_fn_last = true -> b (non-F) then F1
        cfg.sort_fn_last = true;
        let help_diff = display_help(&binds_diff.resolve_semantics(&[]), &cfg);
        let help_diff_str = help_diff.to_string();
        let lines_diff: Vec<_> = help_diff_str.lines().filter(|l| !l.is_empty()).collect();
        assert!(lines_diff[0].contains("b = Print(bbb)"));
        assert!(lines_diff[1].contains("F1 = Print(aaa)"));

        // sort_fn_last = false -> F1 (Print(aaa)) then b (Print(bbb))
        cfg.sort_fn_last = false;
        let help_diff_no = display_help(&binds_diff.resolve_semantics(&[]), &cfg);
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
                mode: PrefixFilter::default()
            } => Action::Print("start".into()),
        );

        let mut cfg = HelpDisplayConfig::default();
        cfg.event_trigger_prefix = "EV:".to_string();
        cfg.show_events = true;
        let help_show = display_help(&binds.resolve_semantics(&[]), &cfg);
        let help_str = help_show.to_string();
        assert!(help_str.contains("EV:Start"));

        cfg.show_events = false;
        let help_hide = display_help(&binds.resolve_semantics(&[]), &cfg);
        let help_str_hide = help_hide.to_string();
        assert!(!help_str_hide.contains("Start"));
    }
}
