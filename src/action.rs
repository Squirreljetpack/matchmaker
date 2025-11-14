use std::{mem::discriminant, str::FromStr};

use serde::{Deserialize, Serialize, Serializer};

#[derive(Debug, Clone, Deserialize, Default)]
pub enum Action {
    #[default] // used to satisfy enumstring
    Select,
    Deselect,
    Toggle,
    CycleAll,
    Accept,
    Quit(Exit),

    // UI
    CyclePreview,
    Preview(String), // if match: hide, else match
    Help(String), // content is shown in preview, empty for default help display
    SwitchPreview(Option<u8>), // n => ^ but with layout + layout_cmd, 0 => just toggle visibility
    SetPreview(Option<u8>), // n => set layout, 0 => set current layout cmd

    ToggleWrap,
    ToggleWrapPreview,

    // Programmable
    Execute(String),
    Become(String),
    Reload(String),
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
    Cancel,

    // Navigation
    Up(Count),
    Down(Count),
    PreviewUp(Count),
    PreviewDown(Count),
    PreviewHalfPageUp,
    PreviewHalfPageDown,
    Pos(i32),

    // Experimental/Debugging
    Redraw,
}

impl serde::Serialize for Action {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
    S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

// -----------------------------------------------------------------------------------------------------------------------


impl PartialEq for Action {
    fn eq(&self, other: &Self) -> bool {
        discriminant(self) == discriminant(other)
    }
}

impl Eq for Action {}

impl std::hash::Hash for Action {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        discriminant(self).hash(state);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Actions(pub Vec<Action>);

impl<const N: usize> From<[Action; N]> for Actions {
    fn from(arr: [Action; N]) -> Self {
        Actions(arr.into())
    }
}

impl<'de> Deserialize<'de> for Actions {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
    D: serde::Deserializer<'de>,
    {
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Single(String),
            Multiple(Vec<String>),
        }

        let helper = Helper::deserialize(deserializer)?;
        let strings = match helper {
            Helper::Single(s) => vec![s],
            Helper::Multiple(v) => v,
        };

        let mut actions = Vec::with_capacity(strings.len());
        for s in strings {
            let action = Action::from_str(&s).map_err(serde::de::Error::custom)?;
            actions.push(action);
        }

        Ok(Actions(actions))
    }
}

impl Serialize for Actions {
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

macro_rules! impl_display_and_from_str_enum {
    ($enum:ident,
        $($unit:ident),*;
        $($tuple:ident),*;
        $($tuple_default:ident),*;
        $($tuple_option:ident),*;
        $($tuple_string_default:ident),*
    ) => {
        impl std::fmt::Display for $enum {
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
                }
            }
        }

        impl std::str::FromStr for $enum {
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

                match name {
                    $( stringify!($unit) => Ok(Self::$unit), )*

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

                    _ => Err(format!("Unknown variant {}", name)),
                }
            }
        }
    };
}

// call it like:
impl_display_and_from_str_enum!(
    Action,
    Select, Deselect, Toggle, CycleAll, Accept, CyclePreview, CycleColumn,
    PreviewHalfPageUp, PreviewHalfPageDown, HistoryUp, HistoryDown,
    ChangePrompt, ChangeQuery, ToggleWrap, ToggleWrapPreview, ForwardChar,
    BackwardChar, ForwardWord, BackwardWord, DeleteChar, DeleteWord,
    DeleteLineStart, DeleteLineEnd, Cancel, Redraw;
    // tuple variants
    Execute, Become, Reload, Print, Preview, SetInput, Column, Pos;
    // tuple with default
    Up, Down, PreviewUp, PreviewDown, Quit;
    // tuple with option
    SwitchPreview, SetPreview, SetPrompt, SetHeader, SetFooter;
    //
    Help
);

use crate::{impl_int_wrapper};
impl_int_wrapper!(Exit, i32, 1);
impl_int_wrapper!(Count, u16, 1);

