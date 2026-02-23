use std::str::FromStr;

use cli_boilerplate_automation::{bait::ResultExt, bring::split::split_nesting, unwrap};
use matchmaker::{
    Action, ConfigMMInnerItem, ConfigMMItem, event::BindSender, message::BindDirective,
    nucleo::Span,
};

pub type MMState<'a, 'b> = matchmaker::render::MMState<'a, 'b, ConfigMMItem, ConfigMMInnerItem>;

#[derive(Debug, Clone, PartialEq)]
pub enum MMAction {
    // binds
    /// define a bind
    Bind(String),
    /// unset a bind
    Unbind(String),
    /// append actions to a bind
    PushBind(String),
    /// pop an action from a bind
    PopBind(String),

    // state
    /// Toggle refiltering of results by query.
    Filtering(Option<bool>),
    /// Toggle visibility of column (TODO)
    ToggleColumn(Option<usize>),
    /// Cycle result sorting between None, Partial, and Full
    CycleSort,

    // set
    /// Set header
    SetHeader(Option<String>),
    /// Set footer
    SetFooter(Option<String>),
    /// Set prompt (TODO)
    SetPrompt(Option<String>),
    /// Set status (TODO)
    SetStatus(Option<String>),
    /// Set prompt (TODO)
    SetQuery(String),

    // Unimplemented
    /// History up (TODO)
    HistoryUp,
    /// History down (TODO)
    HistoryDown,
    /// [`matchmaker::Action::Execute`] but silent (TODO)
    ExecuteSilent(String),
}

pub struct ActionContext {
    pub bind_tx: BindSender<MMAction>,
}

#[allow(unused)]
pub fn action_handler(
    a: MMAction,
    state: &mut MMState<'_, '_>,
    ActionContext { bind_tx }: &mut ActionContext,
) {
    match a {
        // state
        MMAction::CycleSort => {
            #[cfg(feature = "experimental")]
            {
                let threshold = match state.picker_ui.worker.get_stability() {
                    0 => 6,
                    u32::MAX => 0,
                    _ => u32::MAX,
                };
                state.picker_ui.worker.set_stability(threshold);
            }
        }
        MMAction::Filtering(s) => {
            if let Some(s) = s {
                state.filtering = s
            } else {
                state.filtering = !state.filtering
            }
        }
        MMAction::ToggleColumn(s) => {
            // todo
        }

        // history
        MMAction::HistoryUp => {
            // todo
        }
        MMAction::HistoryDown => {
            // todo
        }

        // binds
        MMAction::Bind(s) => {
            let s = s.trim();
            let (trigger, values) = if let Some(s) = s.strip_prefix("==") {
                ("=", s)
            } else {
                unwrap!(s.split_once('='))
            };

            let trigger = unwrap!(trigger.parse()._elog());
            let parts = unwrap!(
                split_nesting(&values, ['(', ')'], ['[', ']']);
                |n: i32| if n > 0 {
                    log::error!("Encountered {} unclosed parentheses", n)
                } else {
                    log::error!("Extra closing parenthesis at index {}", -n)
                }
            );
            let values = unwrap!(
                parts
                    .iter()
                    .map(|p| Action::<MMAction>::from_str(&s).map_err(|e| e.to_string()))
                    .collect::<Result<_, _>>()
                    ._elog()
            );

            let _ = bind_tx.send(BindDirective::Bind(trigger, values));
        }
        MMAction::Unbind(s) => {
            let trigger = unwrap!(s.parse()._elog());
            let _ = bind_tx.send(BindDirective::Unbind(trigger));
        }
        MMAction::PushBind(s) => {
            // todo
        }
        MMAction::PopBind(s) => {
            let trigger = unwrap!(s.parse()._elog());
            let _ = bind_tx.send(BindDirective::PopBind(trigger));
        }

        // set
        MMAction::SetHeader(context) => {
            if let Some(s) = context {
                state.picker_ui.header.set(s);
            } else {
                state.picker_ui.header.clear(true);
            }
        }
        MMAction::SetFooter(context) => {
            if let Some(s) = context {
                state.footer_ui.set(s);
            } else {
                state.footer_ui.clear(false);
            }
        }
        MMAction::SetPrompt(s) => {
            if let Some(s) = s {
                state.picker_ui.input.prompt = Span::from(s);
            } else {
                state.picker_ui.input.reset_prompt();
            }
        }
        MMAction::SetStatus(s) => {
            state.picker_ui.results.set_status_line(s);
        }
        MMAction::SetQuery(s) => {
            state.picker_ui.input.set(s, u16::MAX);
        }

        MMAction::ExecuteSilent(s) => {
            // todo
        }
    }
}

enum_from_str_display! {
    MMAction;

    units:
    CycleSort, HistoryUp, HistoryDown;


    tuples:
    SetQuery, Bind, Unbind, PushBind, PopBind, ExecuteSilent;

    defaults:
    ;

    options:
    SetPrompt, SetHeader, SetFooter, SetStatus, ToggleColumn, Filtering;

    lossy:
    ;
}

//------------------------------------------------
macro_rules! enum_from_str_display {
    (
        $enum:ty;
        units: $( $unit:ident ),* $(,)?;
        tuples: $( $tuple:ident ),* $(,)?;
        defaults: $(($default:ident, $default_value:expr)),*;
        options: $($optional:ident),*;
        lossy: $( $lossy:ident ),* ;
    ) => {
        impl std::fmt::Display for $enum {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                use $enum::*;
                match self {
                    $( $unit => write!(f, stringify!($unit)), )*

                    $( $tuple(inner) => write!(f, concat!(stringify!($tuple), "({})"), inner), )*

                    $( $default(inner) => {
                        if *inner == $default_value {
                            write!(f, stringify!($default))
                        } else {
                            write!(f, concat!(stringify!($default), "({})"), inner)
                        }
                    }, )*

                    $( $optional(opt) => {
                        if let Some(inner) = opt {
                            write!(f, concat!(stringify!($optional), "({})"), inner)
                        } else {
                            write!(f, stringify!($optional))
                        }
                    }, )*

                    $( $lossy(inner) => {
                        if inner.is_empty() {
                            write!(f, stringify!($pathbuf))
                        } else {
                            write!(f, concat!(stringify!($lossy), "({})"), std::ffi::OsString::from(inner).to_string_lossy())
                        }
                    }, )*

                    /* ---------- Manually parsed ---------- */

                    /* ------------------------------------- */

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
                    $( stringify!($unit) => {
                        if data.is_some() {
                            Err(format!("Unexpected data for {}", name))
                        } else {
                            Ok(Self::$unit)
                        }
                    }, )*

                    $( stringify!($tuple) => {
                        let val = data
                        .ok_or_else(|| format!("Missing data for {}", name))?
                        .parse()
                        .map_err(|_| format!("Invalid data for {}", name))?;
                        Ok(Self::$tuple(val))
                    }, )*

                    $( stringify!($lossy) => {
                        let d = match data {
                            Some(val) => val.parse()
                            .map_err(|_| format!("Invalid data for {}", stringify!($lossy)))?,
                            None => Default::default(),
                        };
                        Ok(Self::$lossy(d))
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

                    /* ---------- Manually parsed ---------- */

                    /* ------------------------------------- */

                    _ => Err(format!("Unknown action {}", s)),
                }
            }
        }
    };
}
use enum_from_str_display;
