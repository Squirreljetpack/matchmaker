use std::{process::Command, str::FromStr};

use cba::{
    StringError, bait::ResultExt, bring::split::split_on_unescaped_delimiter, broc::CommandExt,
    unwrap,
};
use log::{debug, error};
use matchmaker::{
    Action, Actions, ConfigMMInnerItem, ConfigMMItem,
    binds::Trigger,
    event::BindSender,
    message::{BindDirective, Interrupt, RenderCommand},
    nucleo::Line,
    ui::StatusUI,
};

use matchmaker::preview::AppendOnly;

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
    /// Cycle result sorting between None, Partial, and Full
    CycleSort,
    ReloadNext(Option<usize>),

    // set
    /// Set header
    SetHeader(Option<String>),
    /// Set footer
    SetFooter(Option<String>),
    /// Set status without interpreting style braces
    SetPrompt(Option<String>),
    /// Set prompt
    SetStyledPrompt(String),
    /// Set status without interpreting style braces
    SetStatus(Option<String>),
    /// Set status
    SetStyledStatus(String),

    /// Accept current selection and print using output_template
    Accept,

    // Unimplemented
    /// History up (TODO)
    HistoryUp,
    /// History down (TODO)
    HistoryDown,
    /// [`matchmaker::Action::Execute`] but silent (TODO)
    ExecuteAsync(String),
    /// Execute command and parse output as actions
    Transform(String),
}

pub struct ActionContext {
    pub bind_tx: BindSender<MMAction>,
    pub render_tx: matchmaker::event::RenderSender<MMAction>,
    pub additional_commands: (Vec<String>, usize),
    pub output_template: Option<String>,
    pub print_handle: AppendOnly<String>,
    pub output_separator: String,
}

#[allow(unused)]
pub fn action_handler(
    a: MMAction,
    state: &mut MMState<'_, '_>,
    ActionContext {
        bind_tx,
        render_tx,
        additional_commands,
        output_template,
        print_handle,
        output_separator,
    }: &mut ActionContext,
) {
    match a {
        MMAction::Accept => {
            let repeat = |s: String| {
                if atty::is(atty::Stream::Stdout) {
                    print_handle.push(s);
                } else {
                    print!("{}{}", s, output_separator);
                }
            };

            if let Some(template) = output_template {
                crate::formatter::format_cli(state, template, Some(&repeat));
            } else {
                state.map_selected_to_vec(|x| repeat(x.to_cow().to_string()));
            }

            state.should_quit_nomatch = true;
        }
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

        // history
        MMAction::HistoryUp => {
            // todo
        }
        MMAction::HistoryDown => {
            // todo
        }

        MMAction::ReloadNext(x) => {
            let payload = match x {
                None => {
                    additional_commands.1 =
                        (additional_commands.1 + 1) % additional_commands.0.len();
                    &additional_commands.0[additional_commands.1]
                }
                Some(x) => {
                    if x < additional_commands.0.len() {
                        &additional_commands.0[x]
                    } else {
                        error!("Index {x} is out of bounds for ReloadNext");
                        return;
                    }
                }
            };
            state.set_interrupt(Interrupt::Reload, payload.clone());
        }

        // binds
        MMAction::Bind(s) => {
            let (trigger, values) = unwrap!(parse_bind_parts(&s)._elog());
            let _ = bind_tx.send(BindDirective::Bind(trigger, values));
        }
        MMAction::Unbind(s) => {
            let trigger = unwrap!(s.parse()._elog());
            let _ = bind_tx.send(BindDirective::Unbind(trigger));
        }
        MMAction::PushBind(s) => {
            let (trigger, action) = unwrap!(parse_push_bind_parts(&s)._elog());
            let _ = bind_tx.send(BindDirective::PushBind(trigger, action));
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
        MMAction::SetStyledPrompt(s) => {
            state
                .picker_ui
                .query
                .set_prompt(Some(StatusUI::parse_template_to_status_line(&s)));
        }
        MMAction::SetStyledStatus(s) => {
            state
                .picker_ui
                .results
                .set_status_line(Some(StatusUI::parse_template_to_status_line(&s)));
        }
        MMAction::SetStatus(s) => {
            state.picker_ui.results.set_status_line(s.map(Line::raw));
        }
        MMAction::SetPrompt(s) => {
            state.picker_ui.query.set_prompt(s.map(Line::raw));
        }
        MMAction::ExecuteAsync(s) => {
            state.set_interrupt(Interrupt::ExecuteSilent, s);
        }
        MMAction::Transform(payload) => {
            let cmd = format_cli(state, &payload, None);
            if cmd.is_empty() {
                error!("Failed to format transform command: {payload}");
                return;
            }
            let vars = state.make_env_vars();
            let render_tx = render_tx.clone();
            if let Some(mut contents) = Command::from_script(&cmd)
                .envs(vars)
                .read_to_string()
                ._elog()
            {
                debug!("Transform output:\n{}", contents);

                for line in contents.lines() {
                    match Action::<MMAction>::from_str(line) {
                        Ok(action) => {
                            let _ = render_tx.send(RenderCommand::Action(action));
                        }
                        Err(_) => {
                            error!("Failed to parse action from transform output: {}", line);
                        }
                    }
                }
            }
        }
    }
}

impl MMAction {
    pub fn validate(&self) -> Result<(), StringError> {
        match self {
            MMAction::Bind(s) => {
                let (_trigger, actions) = crate::action::parse_bind_parts(s)?;
                for a in &actions {
                    if let Action::Custom(mm) = a {
                        mm.validate()?;
                    }
                }
            }
            MMAction::PushBind(s) => {
                let (_trigger, a) = crate::action::parse_push_bind_parts(s)?;
                if let Action::Custom(mm) = &a {
                    mm.validate()?;
                }
            }
            MMAction::Unbind(s) | MMAction::PopBind(s) => {
                s.parse::<Trigger>()?;
            }
            _ => {}
        }
        Ok(())
    }
}

pub fn parse_bind_parts(s: &str) -> Result<(Trigger, Actions<MMAction>), StringError> {
    let (trigger, values) = s
        .split_once('=')
        .ok_or_else(|| format!("Expected '=' in Bind({s})"))?;

    let trigger = trigger.trim().parse()?;

    let parts = split_on_unescaped_delimiter(values, "|||");

    let actions = parts
        .iter()
        .map(|p| Action::<MMAction>::from_str(p.trim()))
        .collect::<Result<Vec<_>, _>>()?;

    Ok((trigger, Actions::from_iter(actions)))
}

pub fn parse_push_bind_parts(s: &str) -> Result<(Trigger, Action<MMAction>), StringError> {
    let s = s.trim();
    let (trigger, values) = s
        .split_once('=')
        .ok_or_else(|| format!("Expected '=' in PushBind({s})"))?;

    let trigger = trigger.trim().parse()?;
    let action = Action::<MMAction>::from_str(values.trim())?;

    Ok((trigger, action))
}

enum_from_str_display! {
    MMAction;

    units:
    CycleSort, HistoryUp, HistoryDown, Accept;


    tuples:
    Bind, Unbind, PushBind, PopBind, ExecuteAsync, Transform, SetStyledPrompt, SetStyledStatus;

    defaults:
    ;

    options:
    SetPrompt, SetHeader, SetFooter, SetStatus, Filtering, ReloadNext;

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

use crate::formatter::format_cli;

#[cfg(test)]
mod tests {
    use super::*;
    use matchmaker::Action;

    #[test]
    fn test_parse_actions() {
        assert!(Action::<MMAction>::from_str("Unbind(QueryChange)").is_ok());
        assert!(Action::<MMAction>::from_str("Filtering(false)").is_ok());
        assert!(Action::<MMAction>::from_str("SetPrompt(rg> )").is_ok());
        assert!(Action::<MMAction>::from_str("Reload").is_ok());

        let bind_inner = match Action::<MMAction>::from_str(
            "Bind(QueryChange = Reload(rg --column --line-number --no-heading --color=always --smart-case \"$FZF_QUERY\"))",
        )
        .unwrap()
        {
            Action::Custom(MMAction::Bind(s)) => s,
            _ => panic!(),
        };

        let (_trigger, actions) = parse_bind_parts(&bind_inner).unwrap();
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            Action::Reload(cmd) => assert_eq!(
                cmd,
                "rg --column --line-number --no-heading --color=always --smart-case \"$FZF_QUERY\""
            ),
            _ => panic!(),
        }

        let push_inner = match Action::<MMAction>::from_str("PushBind(ctrl-r = @enter_mm)").unwrap()
        {
            Action::Custom(MMAction::PushBind(s)) => s,
            _ => panic!(),
        };

        let (_trigger, action) = parse_push_bind_parts(&push_inner).unwrap();
        assert_eq!(action, Action::Semantic("enter_mm".into()));
    }
}
