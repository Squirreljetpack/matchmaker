use std::{
    fmt::{self, Debug, Display},
    mem::discriminant,
    str::FromStr,
};

use cli_boilerplate_automation::impl_transparent_wrapper;
use serde::{Deserialize, Serialize, Serializer};

use crate::{
    MAX_ACTIONS, SSS,
    render::{Effects, MMState},
    utils::serde::StringOrVec,
};

#[derive(Debug, Clone, Default)]
pub enum Action<A: ActionExt = NullActionExt> {
    #[default] // used to satisfy enumstring
    /// Add item to selections
    Select,
    /// Remove item from selections
    Deselect,
    /// Toggle item in selections
    Toggle,
    CycleAll,
    ClearAll,
    Accept,
    // Returns MatchError::Abort
    Quit(Exit),

    // UI
    CyclePreview,
    Preview(String),           // if match: hide, else match
    Help(String),              // content is shown in preview, empty for default help display
    SwitchPreview(Option<u8>), // n => ^ but with layout + layout_cmd, None => just toggle visibility
    SetPreview(Option<u8>),    // n => set layout, None => set current layout cmd

    ToggleWrap,
    ToggleWrapPreview,

    // Programmable
    /// Pauses the tui display and the event loop, and invokes the handler for [`crate::message::Interrupt::Execute`]
    /// The remaining actions in the buffer are still processed
    Execute(String),
    /// Exits the tui and invokes the handler for [`crate::message::Interrupt::Become`]
    Become(String),
    /// Restarts the matcher-worker and invokes the handler for [`crate::message::Interrupt::Reload`]
    Reload(String),

    /// Invokes the handler for [`crate::message::Interrupt::Print`]
    /// See also: [`crate::Matchmaker::register_print_handler`]
    Print(String),

    SetInput(String),
    SetHeader(Option<String>),
    SetFooter(Option<String>),
    SetPrompt(Option<String>),
    Column(usize),
    CycleColumn,

    // Unimplemented
    HistoryUp,
    HistoryDown,
    ChangePrompt,
    ChangeQuery,

    // Edit
    ForwardChar,
    BackwardChar,
    ForwardWord,
    BackwardWord,
    DeleteChar,
    DeleteWord,
    DeleteLineStart,
    DeleteLineEnd,
    Cancel, // clear input
    InputPos(i32),

    // Navigation
    Up(Count),
    Down(Count),
    PreviewUp(Count),
    PreviewDown(Count),
    PreviewHalfPageUp,
    PreviewHalfPageDown,
    Pos(i32),

    // Other/Experimental/Debugging
    Input(char),
    Redraw,
    Custom(A),
    Overlay(usize),
}

impl<A: ActionExt> serde::Serialize for Action<A> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<A: ActionExt> PartialEq for Action<A> {
    fn eq(&self, other: &Self) -> bool {
        discriminant(self) == discriminant(other)
    }
}
impl<A: ActionExt> Eq for Action<A> {}

// --------- ACTION_EXT ------------------
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

pub trait ActionExt: Debug + Clone + FromStr + Display + PartialEq + SSS {}

pub type ActionExtHandler<T, S, A> = fn(A, &MMState<'_, T, S>) -> Effects;
pub type ActionAliaser<T, S, A> = fn(Action<A>, &MMState<'_, T, S>) -> Actions<A>;
pub use arrayvec::ArrayVec;
/// # Example
/// ```rust
///     use matchmaker::{action::{Action, Actions, acs}, render::MMState};
///     pub fn fsaction_aliaser(
///         a: Action,
///         state: &MMState<'_, String, String>,
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
    ( $( $k:expr => $v1:expr ),* $(,)? ) => {{
        let mut map = $crate::binds::BindMap::new();
        $(
            map.insert($k.into(), $crate::action::Actions::from($v1));
        )*
        map
    }};
}
// ----------- ACTIONS ---------------
#[derive(Debug, Clone, PartialEq)]
pub struct Actions<A: ActionExt = NullActionExt>(pub ArrayVec<Action<A>, MAX_ACTIONS>);
impl<A: ActionExt> Default for Actions<A> {
    fn default() -> Self {
        Self(ArrayVec::new())
    }
}

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

macro_rules! impl_display_and_from_str_enum {
    (
        $($unit:ident),*;
        $($tuple:ident),*;
        $($tuple_default:ident),*;
        $($tuple_option:ident),*;
        $($tuple_string_default:ident),*
    ) => {
        impl<A: ActionExt> std::fmt::Display for Action<A> {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    // Unit variants
                    $( Self::$unit => write!(f, stringify!($unit)), )*

                    // Tuple variants (always show inner)
                    $( Self::$tuple(inner) => write!(f, concat!(stringify!($tuple), "({})"), inner), )*

                    // Tuple variants with generic default fallback
                    $( Self::$tuple_default(inner) => {
                        if *inner == core::default::Default::default() {
                            write!(f, stringify!($tuple_default))
                        } else {
                            write!(f, concat!(stringify!($tuple_default), "({})"), inner)
                        }
                    }, )*

                    // Tuple variants with Option<T>
                    $( Self::$tuple_option(opt) => {
                        if let Some(inner) = opt {
                            write!(f, concat!(stringify!($tuple_option), "({})"), inner)
                        } else {
                            write!(f, stringify!($tuple_option))
                        }
                    }, )*

                    $( Self::$tuple_string_default(inner) => {
                        if inner.is_empty() {
                            write!(f, stringify!($tuple_string_default))
                        } else {
                            write!(f, concat!(stringify!($tuple_string_default), "({})"), inner)
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

                    $( stringify!($tuple_default) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($tuple_default)))?,
                            None => Default::default(),
                        };
                        Ok(Self::$tuple_default(d))
                    }, )*

                    $( stringify!($tuple_option) => {
                        let d = match data {
                            Some(val) if !val.is_empty() => {
                                Some(val.parse().map_err(|_| format!("Invalid data for {}", stringify!($tuple_option)))?)
                            }
                            _ => None,
                        };
                        Ok(Self::$tuple_option(d))
                    }, )*

                    $( stringify!($tuple_string_default) => {
                        let d = match data {
                            Some(val) if !val.is_empty() => val.to_string(),
                            _ => String::new(),
                        };
                        Ok(Self::$tuple_string_default(d))
                    }, )*

                    _ => Err(format!("Unknown variant {}", s))
                }
            }
        }
    };
}

// call it like:
impl_display_and_from_str_enum!(
    Select, Deselect, Toggle, CycleAll, ClearAll, Accept, CyclePreview, CycleColumn,
    PreviewHalfPageUp, PreviewHalfPageDown, HistoryUp, HistoryDown,
    ChangePrompt, ChangeQuery, ToggleWrap, ToggleWrapPreview, ForwardChar,
    BackwardChar, ForwardWord, BackwardWord, DeleteChar, DeleteWord,
    DeleteLineStart, DeleteLineEnd, Cancel, Redraw;
    // tuple variants
    Execute, Become, Reload, Preview, SetInput, Column, Pos, InputPos;
    // tuple with default
    Up, Down, PreviewUp, PreviewDown, Quit, Overlay;
    // tuple with option
    SwitchPreview, SetPreview, SetPrompt, SetHeader, SetFooter;
    // tuple_string_default
    Print, Help
);

impl_transparent_wrapper!(Exit, i32, 1);
impl_transparent_wrapper!(Count, u16, 1; derive(Copy));

// --------------------------------------
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
