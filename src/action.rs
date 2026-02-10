use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

use serde::{Deserialize, Serialize, Serializer};

use crate::{MAX_ACTIONS, SSS, utils::serde::StringOrVec};

/// Bindable actions
/// # Additional
/// See [crate::render::render_loop] for the source code definitions.
#[derive(Debug, Clone, PartialEq)]
pub enum Action<A: ActionExt = NullActionExt> {
    /// Add item to selections
    Select,
    /// Remove item from selections
    Deselect,
    /// Toggle item in selections
    Toggle,
    /// Toggle all selections
    CycleAll,
    /// Clear all selections
    ClearSelections,
    /// Accept current selection
    Accept,
    /// Quit with code
    Quit(i32),

    // UI
    /// Cycle preview layouts
    CyclePreview,
    /// Show/hide preview for selection
    Preview(String),
    /// Show help in preview
    Help(String),
    /// Set preview layout;
    /// None restores the command of the current layout.
    SetPreview(Option<u8>),
    /// Switch or toggle preview;
    SwitchPreview(Option<u8>),
    /// Toggle wrap in main view
    ToggleWrap,
    /// Toggle wrap in preview
    ToggleWrapPreview,

    // Set
    /// Set input query
    SetInput(String),
    /// Set header
    SetHeader(Option<String>),
    /// Set footer
    SetFooter(Option<String>),
    /// Set prompt
    SetPrompt(Option<String>),

    // Columns
    /// Set column
    Column(usize),
    /// Cycle columns
    CycleColumn,

    // Programmable
    /// Execute command and continue
    Execute(String),
    /// Exit and become
    Become(String),
    /// Reload matcher/worker
    Reload(String),
    /// Print via handler
    Print(String),

    // Unimplemented
    /// History up (TODO)
    HistoryUp,
    /// History down (TODO)
    HistoryDown,
    /// Change prompt (TODO)
    ChangePrompt,
    /// Change query (TODO)
    ChangeQuery,

    // Edit (Input)
    /// Move cursor forward char
    ForwardChar,
    /// Move cursor backward char
    BackwardChar,
    /// Move cursor forward word
    ForwardWord,
    /// Move cursor backward word
    BackwardWord,
    /// Delete char
    DeleteChar,
    /// Delete word
    DeleteWord,
    /// Delete to start of line
    DeleteLineStart,
    /// Delete to end of line
    DeleteLineEnd,
    /// Clear input
    Cancel,
    /// Set input cursor pos
    InputPos(i32),

    // Navigation
    /// Move selection index up
    Up(u16),
    /// Move selection index down
    Down(u16),
    /// Scroll preview up
    PreviewUp(u16),
    /// Scroll preview down
    PreviewDown(u16),
    /// Scroll preview half page up
    PreviewHalfPageUp,
    /// Scroll preview half page down
    PreviewHalfPageDown,
    /// Jump to absolute position
    Pos(i32),

    // Other/Experimental/Debugging
    /// Insert char into input
    Input(char),
    /// Force redraw
    Redraw,
    /// Custom action
    Custom(A),
    /// Activate the nth overlay
    Overlay(usize),
}

// --------------- MACROS ---------------

/// # Example
/// ```rust
///     use matchmaker::{action::{Action, Actions, acs}, render::MMState};
///     pub fn fsaction_aliaser(
///         a: Action,
///         state: &MMState<'_, '_, String, String>,
///     ) -> Actions {
///         match a {
///             Action::Custom(_) => {
///               log::debug!("Ignoring custom action");
///               acs![]
///             }
///             _ => acs![a], // no change
///         }
///     }
/// ```
#[macro_export]
macro_rules! acs {
    ( $( $x:expr ),* $(,)? ) => {
        {
            $crate::action::Actions::from([$($x),*])
        }
    };
}
pub use crate::acs;

/// # Example
/// ```rust
///     use matchmaker::{binds::{BindMap, bindmap, key}, action::Action};
///     let default_config: BindMap = bindmap!(
///        key!(alt-enter) => Action::Print("".into())
///        // custom actions can be specified directly: key!(ctrl-c) => FsAction::Enter
///    );
/// ```
#[macro_export]
macro_rules! bindmap {
    ( $( $( $k:expr ),+ => $v:expr ),* $(,)? ) => {{
        let mut map = $crate::binds::BindMap::new();
        $(
            let action = $crate::action::Actions::from($v);
            $(
                map.insert($k.into(), action.clone());
            )+
        )*
        map
    }};
} // btw, Can't figure out if its possible to support optional meta over inserts

// --------------- ACTION_EXT ---------------
pub trait ActionExt: Debug + Clone + FromStr + Display + PartialEq + SSS {}

impl<T> From<T> for Action<T>
where
    T: ActionExt,
{
    fn from(value: T) -> Self {
        Self::Custom(value)
    }
}
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NullActionExt {}

impl ActionExt for NullActionExt {}

impl fmt::Display for NullActionExt {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Ok(())
    }
}

impl std::str::FromStr for NullActionExt {
    type Err = ();

    fn from_str(_: &str) -> Result<Self, Self::Err> {
        Err(())
    }
}

// --------------- ACTIONS ---------------
pub use arrayvec::ArrayVec;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Actions<A: ActionExt = NullActionExt>(ArrayVec<Action<A>, MAX_ACTIONS>);

macro_rules! repeat_impl {
    ($($len:expr),*) => {
        $(
            impl<A: ActionExt> From<[Action<A>; $len]> for Actions<A> {
                fn from(arr: [Action<A>; $len]) -> Self {
                    Actions(ArrayVec::from_iter(arr))
                }
            }

            impl<A: ActionExt> From<[A; $len]> for Actions<A> {
                fn from(arr: [A; $len]) -> Self {
                    Actions(arr.into_iter().map(Action::Custom).collect())
                }
            }
        )*
    }
}
impl<A: ActionExt> From<[Action<A>; 0]> for Actions<A> {
    fn from(empty: [Action<A>; 0]) -> Self {
        Actions(ArrayVec::from_iter(empty))
    }
}
repeat_impl!(1, 2, 3, 4, 5, 6);

impl<A: ActionExt> From<Action<A>> for Actions<A> {
    fn from(action: Action<A>) -> Self {
        acs![action]
    }
}
// no conflict because Action is local type
impl<A: ActionExt> From<A> for Actions<A> {
    fn from(action: A) -> Self {
        acs![Action::Custom(action)]
    }
}

// ---------- SERDE ----------------

impl<A: ActionExt> serde::Serialize for Action<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de, A: ActionExt> Deserialize<'de> for Actions<A> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let helper = StringOrVec::deserialize(deserializer)?;
        let strings = match helper {
            StringOrVec::String(s) => vec![s],
            StringOrVec::Vec(v) => v,
        };

        if strings.len() > MAX_ACTIONS {
            return Err(serde::de::Error::custom(format!(
                "Too many actions, max is {MAX_ACTIONS}."
            )));
        }

        let mut actions = ArrayVec::new();
        for s in strings {
            let action = Action::from_str(&s).map_err(serde::de::Error::custom)?;
            actions.push(action);
        }

        Ok(Actions(actions))
    }
}

impl<A: ActionExt> Serialize for Actions<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self.0.len() {
            1 => serializer.serialize_str(&self.0[0].to_string()),
            _ => {
                let strings: Vec<String> = self.0.iter().map(|a| a.to_string()).collect();
                strings.serialize(serializer)
            }
        }
    }
}

// ----- action serde
enum_from_str_display!(
    units:
    Select, Deselect, Toggle, CycleAll, ClearSelections, Accept, CyclePreview, CycleColumn,
    PreviewHalfPageUp, PreviewHalfPageDown, HistoryUp, HistoryDown,
    ChangePrompt, ChangeQuery, ToggleWrap, ToggleWrapPreview, ForwardChar,
    BackwardChar, ForwardWord, BackwardWord, DeleteChar, DeleteWord,
    DeleteLineStart, DeleteLineEnd, Cancel, Redraw;

    tuples:
    Execute, Become, Reload, Preview, SetInput, Column, Pos, InputPos;

    defaults:
    (Up, 1), (Down, 1), (PreviewUp, 1), (PreviewDown, 1), (Quit, 1), (Overlay, 0), (Print, String::new()), (Help, String::new());

    options:
    SwitchPreview, SetPreview, SetPrompt, SetHeader, SetFooter
);

macro_rules! enum_from_str_display {
    (
        units: $($unit:ident),*;
        tuples: $($tuple:ident),*;
        defaults: $(($default:ident, $default_value:expr)),*;
        options: $($optional:ident),*
    ) => {
        impl<A: ActionExt> std::fmt::Display for Action<A> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $( Self::$unit => write!(f, stringify!($unit)), )*

                    $( Self::$tuple(inner) => write!(f, concat!(stringify!($tuple), "({})"), inner), )*

                    $( Self::$default(inner) => {
                        if *inner == $default_value {
                            write!(f, stringify!($default))
                        } else {
                            write!(f, concat!(stringify!($default), "({})"), inner)
                        }
                    }, )*

                    $( Self::$optional(opt) => {
                        if let Some(inner) = opt {
                            write!(f, concat!(stringify!($optional), "({})"), inner)
                        } else {
                            write!(f, stringify!($optional))
                        }
                    }, )*

                    Self::Custom(inner) => {
                        write!(f, "{}", inner.to_string())
                    }
                    Self::Input(c) => {
                        write!(f, "{c}")
                    }
                }
            }
        }

        impl<A: ActionExt>  std::str::FromStr for Action<A> {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let (name, data) = if let Some(pos) = s.find('(') {
                    if s.ends_with(')') {
                        (&s[..pos], Some(&s[pos + 1..s.len() - 1]))
                    } else {
                        (s, None)
                    }
                } else {
                    (s, None)
                };

                if let Ok(x) = name.parse::<A>() {
                    return Ok(Self::Custom(x))
                }
                match name {
                    $( stringify!($unit) => {
                        if data.is_some() {
                            Err(format!("Unexpected data for unit variant {}", name))
                        } else
                        {
                            Ok(Self::$unit)
                        }
                    }, )*

                    $( stringify!($tuple) => {
                        let d = data
                        .ok_or_else(|| format!("Missing data for {}", stringify!($tuple)))?
                        .parse()
                        .map_err(|_| format!("Invalid data for {}", stringify!($tuple)))?;
                        Ok(Self::$tuple(d))
                    }, )*

                    $( stringify!($default) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($default)))?,
                            None => $default_value,
                        };
                        Ok(Self::$default(d))
                    }, )*

                    $( stringify!($optional) => {
                        let d = match data {
                            Some(val) if !val.is_empty() => {
                                Some(val.parse().map_err(|_| format!("Invalid data for {}", stringify!($optional)))?)
                            }
                            _ => None,
                        };
                        Ok(Self::$optional(d))
                    }, )*

                    _ => Err(format!("Unknown variant {}", s))
                }
            }
        }
    };
}
use enum_from_str_display;

impl<A: ActionExt> IntoIterator for Actions<A> {
    type Item = Action<A>;
    type IntoIter = <ArrayVec<Action<A>, MAX_ACTIONS> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, A: ActionExt> IntoIterator for &'a Actions<A> {
    type Item = &'a Action<A>;
    type IntoIter = <&'a ArrayVec<Action<A>, MAX_ACTIONS> as IntoIterator>::IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
