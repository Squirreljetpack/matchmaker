use std::{mem::discriminant, str::FromStr};

use serde::Deserialize;
use strum_macros::Display;

#[derive(Debug, Display, Clone, Deserialize, Default)]
pub enum Action {
    #[default] // used to satisfy enumstring
    Select,
    Deselect,
    Toggle,
    CycleAll,
    Accept,
    Quit(Exit),
    
    // UI
    ChangeHeader(String),
    CyclePreview,
    Preview(String), // if match: hide, else match
    SwitchPreview(Option<u8>), // n => ^ but with layout + layout_cmd, 0 => just toggle visibility
    SetPreview(Option<u8>), // n => set layout, 0 => set current layout cmd
    
    // Programmable
    Execute(String),
    Become(String),
    Reload(String),
    Print(String),
    
    SetInput(String),
    SetHeader(String),
    SetPrompt(String),
    Column(usize),
    // Unimplemented
    HistoryUp,
    HistoryDown,
    ChangePrompt,
    ChangeQuery,
    ToggleWrap,
    
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
    Redraw
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

#[derive(Debug, Clone)]
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

macro_rules! impl_from_str_enum {
    ($enum:ident,
        $($unit:ident),*;
        $($tuple:ident),*;
        $($tuple_default:ident),*;
        $($tuple_option:ident),*
    ) => {
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
                    // Unit variants
                    $( stringify!($unit) => Ok($enum::$unit), )*
                    
                    // Tuple variants (data required)
                    $( stringify!($tuple) => {
                        let d = data
                        .ok_or_else(|| format!("Missing data for {}", stringify!($tuple)))?
                        .parse()
                        .map_err(|_| format!("Invalid data for {}", stringify!($tuple)))?;
                        Ok($enum::$tuple(d))
                    }, )*
                    
                    // Tuple variants with default fallback
                    $( stringify!($tuple_default) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($tuple_default)))?,
                            None => Default::default(),
                        };
                        Ok($enum::$tuple_default(d))
                    }, )*
                    
                    // Tuple variants that produce Option<T>
                    $( stringify!($tuple_option) => {
                        let d = match data {
                            Some(val) if !val.is_empty() => {
                                Some(val.parse().map_err(|_| format!("Invalid data for {}", stringify!($tuple_option)))?)
                            },
                            _ => None,
                        };
                        Ok($enum::$tuple_option(d))
                    }, )*
                    
                    _ => Err(format!("Unknown variant {}", name)),
                }
            }
        }
    };
}

impl_from_str_enum!(
    Action,
    Select,
    Deselect,
    Toggle,
    CycleAll,
    Accept,
    CyclePreview,
    
    PreviewHalfPageUp,
    PreviewHalfPageDown,
    HistoryUp,
    HistoryDown,
    ChangePrompt,
    ChangeQuery,
    ToggleWrap,
    ForwardChar,
    BackwardChar,
    ForwardWord,
    BackwardWord,
    DeleteChar,
    DeleteWord,
    DeleteLineStart,
    DeleteLineEnd,
    Cancel;
    // tuple variants
    Execute,
    Become,
    Reload,
    Print,
    ChangeHeader,
    Preview,
    
    SetInput,
    Column,
    Pos;
    // tuples with defaults
    Up,
    Down,
    PreviewUp,
    PreviewDown,
    Quit;
    
    // tuple options
    SwitchPreview,
    SetPreview
);

use crate::{impl_int_wrapper};
impl_int_wrapper!(Exit, i32, 1);
impl_int_wrapper!(Count, u16, 1);