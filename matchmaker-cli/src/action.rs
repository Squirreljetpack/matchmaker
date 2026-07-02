use std::{cmp::Ordering, process::Command, str::FromStr, sync::Arc};

use atoi::FromRadix10;
use cba::{
    StringError, bait::ResultExt, bring::split::split_on_unescaped_delimiter, broc::CommandExt,
    unwrap,
};
use log::{debug, error};
use matchmaker::{
    Action, Actions,
    binds::Trigger,
    config::PartialRenderConfig,
    config_mm::{ConfigPreprocessedData, RangesFactory},
    event::BindSender,
    message::{BindDirective, Interrupt, RenderCommand},
    nucleo::Line,
    ui::StatusUI,
};
use matchmaker_partial::{Apply, Set};

/// Sort function type accepted by `nucleo.sort_with` over `String` items.
type StringSortFn = Arc<dyn Fn((u32, &String), (u32, &String)) -> Ordering + Send + Sync>;

pub type MMState<'a, 'b> = matchmaker::render::MMState<'a, 'b, String, ConfigPreprocessedData>;

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

    // mode
    /// Replace the entire mode stack with the given comma-separated tags.
    SetMode(String),
    /// Push a single mode tag onto the mode stack.
    PushMode(String),
    /// Pop the top mode tag from the mode stack.
    PopMode,

    // state
    /// Toggle refiltering of results by query.
    Filtering(Option<bool>),
    /// Cycle result sorting between None, Partial, and Full
    CycleSort,
    ReloadNext(Option<usize>),
    ReloadPrev,
    /// Lexicographic sort ascending by the active or given column.
    SortAscending(Option<usize>),
    /// Lexicographic sort descending.
    SortDescending(Option<usize>),
    /// Numeric sort ascending.
    SortNumericAscending(Option<usize>),
    /// Numeric sort descending.
    SortNumericDescending(Option<usize>),

    // set
    /// Set header
    SetHeader(Option<String>),
    /// Push header
    PushHeader(String),
    /// Set footer
    SetFooter(Option<String>),
    /// Push footer
    PushFooter(String),
    /// Set status without interpreting style braces
    SetPrompt(Option<String>),
    /// Set prompt
    SetStyledPrompt(String),
    /// Set status without interpreting style braces
    SetStatus(Option<String>),
    /// Set status
    SetStyledStatus(String),
    /// Run a command and display output in preview window (TODO)
    RunPreview(String),

    // Unimplemented
    /// History up (TODO)
    HistoryUp,
    /// History down (TODO)
    HistoryDown,
    /// [`matchmaker::Action::Execute`], confirm on error
    ExecuteOrConfirm(String),
    /// [`matchmaker::Action::Execute`], quit on success
    ExecuteAndQuit(String),
    /// [`matchmaker::Action::Execute`], quit on success, confirm on error, resume on signal
    BecomeOrConfirm(String),
    /// [`matchmaker::Action::Execute`], quit on success, resume on error, exit on signal
    BecomeOrResume(String),
    /// Execute command and parse output as actions
    Transform(String),
    /// Execute command and parse output as configuration
    TransformConfig(String),
}

pub struct ActionContext {
    pub bind_tx: BindSender<MMAction>,
    pub render_tx: matchmaker::event::RenderSender<MMAction>,
    pub additional_commands: (Vec<String>, usize),
    /// Factory producing per-column range lookups. See [`matchmaker::config_mm::RangesFactory`].
    pub ranges_fn: RangesFactory<String>,
    /// Active custom sort, if any. Set by sort actions and used to toggle
    /// the sort off when the same variant is re-applied.
    pub sort_discriminant: Option<(SortMode, bool)>,
    // pub output_template: Option<String>,
    // pub print_handle: AppendOnly<String>,
    // pub output_separator: String,
}

pub fn action_handler(
    a: MMAction,
    state: &mut MMState<'_, '_>,
    ActionContext {
        bind_tx,
        render_tx,
        additional_commands,
        ranges_fn,
        sort_discriminant,
    }: &mut ActionContext,
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

        // history
        MMAction::HistoryUp => {
            // todo
        }
        MMAction::HistoryDown => {
            // todo
        }

        MMAction::ReloadNext(x) => {
            if additional_commands.0.is_empty() {
                return;
            }

            let index = match x {
                None => {
                    additional_commands.1 =
                        (additional_commands.1 + 1) % additional_commands.0.len();
                    additional_commands.1
                }
                Some(x) => {
                    if x < additional_commands.0.len() {
                        x
                    } else {
                        error!("Index {x} is out of bounds for ReloadNext");
                        return;
                    }
                }
            };
            let payload = &additional_commands.0[index];
            state.envs.set("MM_INDEX", index);
            state.set_interrupt(Interrupt::Reload, payload.clone());
        }

        MMAction::ReloadPrev => {
            if additional_commands.0.is_empty() {
                return;
            }

            additional_commands.1 = (additional_commands.1 + additional_commands.0.len() - 1)
                % additional_commands.0.len();

            let index = additional_commands.1;

            let payload = &additional_commands.0[index];

            state.envs.set("MM_INDEX", index);

            state.set_interrupt(Interrupt::Reload, payload.clone());
        }

        // sort
        MMAction::SortAscending(idx) => {
            let n = expand_maybe_column(state, idx);
            handle_sort(
                state,
                ranges_fn,
                n,
                SortMode::Lexicographic,
                false,
                sort_discriminant,
            );
        }
        MMAction::SortDescending(idx) => {
            let n = expand_maybe_column(state, idx);
            handle_sort(
                state,
                ranges_fn,
                n,
                SortMode::Lexicographic,
                true,
                sort_discriminant,
            );
        }
        MMAction::SortNumericAscending(idx) => {
            let n = expand_maybe_column(state, idx);
            handle_sort(
                state,
                ranges_fn,
                n,
                SortMode::Numeric,
                false,
                sort_discriminant,
            );
        }
        MMAction::SortNumericDescending(idx) => {
            let n = expand_maybe_column(state, idx);
            handle_sort(
                state,
                ranges_fn,
                n,
                SortMode::Numeric,
                true,
                sort_discriminant,
            );
        }

        MMAction::RunPreview(cmd) => {
            if let Some(p) = state.preview_ui {
                p.show(true);
                state.update_preview_set(Ok(cmd));
            }
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

        // mode
        MMAction::SetMode(s) => {
            let _ = bind_tx.send(BindDirective::SetMode(s));
        }
        MMAction::PushMode(s) => {
            let _ = bind_tx.send(BindDirective::PushMode(s));
        }
        MMAction::PopMode => {
            let _ = bind_tx.send(BindDirective::PopMode);
        }

        // set
        MMAction::SetHeader(context) => {
            if let Some(s) = context {
                state.picker_ui.header.set(s);
            } else {
                state.picker_ui.header.clear(true);
            }
        }
        MMAction::PushHeader(s) => {
            state.picker_ui.header.push(s);
        }
        MMAction::SetFooter(context) => {
            if let Some(s) = context {
                state.footer_ui.set(s);
            } else {
                state.footer_ui.clear(false);
            }
        }
        MMAction::PushFooter(s) => {
            state.footer_ui.push(s);
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
                .status
                .set(Some(StatusUI::parse_template_to_status_line(&s)));
        }
        MMAction::SetStatus(s) => {
            state.picker_ui.status.set(s.map(Line::raw));
        }
        MMAction::SetPrompt(s) => {
            state.picker_ui.query.set_prompt(s.map(Line::raw));
        }
        MMAction::ExecuteOrConfirm(s) => {
            state.discriminant_payload = Some(0);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::ExecuteAndQuit(s) => {
            state.discriminant_payload = Some(1);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::BecomeOrConfirm(s) => {
            state.discriminant_payload = Some(2);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::BecomeOrResume(s) => {
            state.discriminant_payload = Some(3);
            state.set_interrupt(Interrupt::Execute, s);
        }
        MMAction::Transform(payload) => {
            let cmd = format_cli(state, &payload, None);
            if cmd.is_empty() {
                error!("Failed to format transform command: {payload}");
                return;
            }
            let vars = state.make_env_vars();

            let render_tx = render_tx.clone();
            if let Some(contents) = Command::from_script(&cmd)
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
        MMAction::TransformConfig(payload) => {
            let cmd = format_cli(state, &payload, None);
            if cmd.is_empty() {
                error!("Failed to format transform-config command: {payload}");
                return;
            }
            let vars = state.make_env_vars();

            if let Some(contents) = Command::from_script(&cmd)
                .envs(vars)
                .read_to_string()
                ._elog()
            {
                debug!("TransformConfig output:\n{}", contents);

                let words: Vec<String> = contents.lines().map(|s| s.to_string()).collect();
                match crate::parse::get_pairs(words) {
                    Ok(pairs) => {
                        let mut partial = PartialRenderConfig::default();
                        for (path, val) in pairs {
                            let mut parts = split_on_unescaped_delimiter(&val, "|||");
                            if let Err(e) = crate::parse::try_split_kv(&mut parts, false) {
                                error!("Failed to split KV for {}: {e}", path.join("."));
                                continue;
                            }

                            if let Err(e) = partial.set(path.as_slice(), &parts) {
                                error!("Failed to set partial for {}: {e}", path.join("."));
                            }
                        }

                        log::debug!("Parsed config update: {partial:?}");

                        // Apply the partial to UI components
                        state.ui.config.apply(partial.ui);
                        state.picker_ui.query.config.apply(partial.query);
                        state.picker_ui.results.config.apply(partial.results);
                        state.picker_ui.status.status_config.apply(partial.status);
                        state.footer_ui.config.apply(partial.footer);
                        state.picker_ui.header.config.apply(partial.header);

                        if let Some(preview_ui) = state.preview_ui.as_mut() {
                            preview_ui.config.apply(partial.preview);
                        }

                        let _ = render_tx.send(RenderCommand::Refresh);
                    }
                    Err(e) => {
                        error!("Failed to parse pairs from TransformConfig output: {e}");
                    }
                }
            }
        }
    }
}

impl MMAction {
    /// Validate Bind/PushBind/Unbind/PopBind instructions
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
    CycleSort, HistoryUp, HistoryDown, ReloadPrev, PopMode;


    tuples:
    Bind, Unbind, PushBind, PopBind, SetMode, PushMode, ExecuteOrConfirm, ExecuteAndQuit, BecomeOrConfirm, BecomeOrResume, Transform, TransformConfig, SetStyledPrompt, SetStyledStatus, PushHeader, PushFooter, RunPreview;

    defaults:
    ;

    options:
    SetPrompt, SetHeader, SetFooter, SetStatus, Filtering, ReloadNext, SortAscending, SortDescending, SortNumericAscending, SortNumericDescending;

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

                    _ => Err("".to_string()),
                }
            }
        }
    };
}
use enum_from_str_display;

use crate::formatter::format_cli;

/// Sort mode used by `apply_sort`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SortMode {
    Lexicographic,
    Numeric,
}

impl SortMode {
    fn compare(self, a: &str, b: &str) -> Ordering {
        match self {
            SortMode::Lexicographic => a.cmp(b),
            SortMode::Numeric => {
                let fa = parse_float(a.as_bytes());
                let fb = parse_float(b.as_bytes());
                match (fa, fb) {
                    (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
                    (Some(_), None) => Ordering::Less,
                    (None, Some(_)) => Ordering::Greater,
                    (None, None) => a.cmp(b),
                }
            }
        }
    }
}

/// Parse a `f64` from a byte slice using `atoi::FromRadix10` for the integer
/// part. Mirrors the spec: `n` is the integer part, and a trailing `.` triggers
/// decimal parsing. Returns `None` if the input does not start with a digit.
fn parse_float(input: &[u8]) -> Option<f64> {
    let (n, used) = u64::from_radix_10(input);
    if used == 0 {
        return None;
    }
    let rest = &input[used..];
    if rest.first() == Some(&b'.') {
        let (d, used2) = u64::from_radix_10(&rest[1..]);
        if used2 == 0 {
            // "3." with no decimal digits — treat as integer.
            return Some(n as f64);
        }
        // Build "<n>.<d>" and let `f64::from_str` handle the float math.
        let mut buf = String::with_capacity(used + 1 + used2);
        use std::fmt::Write;
        let _ = write!(&mut buf, "{n}.{d}");
        buf.parse().ok()
    } else {
        Some(n as f64)
    }
}

fn expand_maybe_column(state: &MMState<'_, '_>, idx: Option<usize>) -> usize {
    match idx {
        None => state.picker_ui.active_column_index(),
        Some(i) => state.picker_ui.results.expand_idx(i),
    }
}

fn apply_sort(
    state: &mut MMState<'_, '_>,
    ranges_fn: &RangesFactory<String>,
    n: usize,
    mode: SortMode,
    descending: bool,
) {
    let lookup = ranges_fn(n);
    let lookup_for_closure = lookup.clone();
    let sort_fn: StringSortFn =
        Arc::new(move |(_ia, a): (u32, &String), (_ib, b): (u32, &String)| {
            let sub_a: &str = &lookup_for_closure(a);
            let sub_b: &str = &lookup_for_closure(b);
            let ord = mode.compare(sub_a, sub_b);
            if descending { ord.reverse() } else { ord }
        });

    state.picker_ui.worker.nucleo.sort_with(Some(sort_fn));
    state.picker_ui.worker.nucleo.resort();
}

fn handle_sort(
    state: &mut MMState<'_, '_>,
    ranges_fn: &RangesFactory<String>,
    n: usize,
    mode: SortMode,
    descending: bool,
    sort_discriminant: &mut Option<(SortMode, bool)>,
) {
    if *sort_discriminant == Some((mode, descending)) {
        state.picker_ui.worker.nucleo.sort_with(None);
        state.picker_ui.worker.nucleo.resort();
        *sort_discriminant = None;
    } else {
        apply_sort(state, ranges_fn, n, mode, descending);
        *sort_discriminant = Some((mode, descending));
    }
}

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

    #[test]
    fn test_parse_float() {
        // Integer inputs.
        assert_eq!(parse_float(b"3"), Some(3.0));
        assert_eq!(parse_float(b"0"), Some(0.0));
        assert_eq!(parse_float(b"42"), Some(42.0));

        // Float inputs (using values exactly representable in f64 to avoid
        // rounding artifacts; the parser is `f64::from_str` under the hood).
        assert_eq!(parse_float(b"3.5"), Some(3.5));
        assert_eq!(parse_float(b"0.5"), Some(0.5));
        assert_eq!(parse_float(b"100.25"), Some(100.25));

        // Trailing dot with no decimals -> integer.
        assert_eq!(parse_float(b"3."), Some(3.0));

        // Extra text after the number is ignored by atoi.
        assert_eq!(parse_float(b"42abc"), Some(42.0));

        // Unparseable inputs.
        assert_eq!(parse_float(b""), None);
        assert_eq!(parse_float(b"abc"), None);
        assert_eq!(parse_float(b".5"), None); // no leading digit
    }

    #[test]
    fn test_sort_mode_numeric_orders_correctly() {
        // Numeric mode must put "2" before "10".
        let mode = SortMode::Numeric;
        assert_eq!(mode.compare("2", "10"), Ordering::Less);
        assert_eq!(mode.compare("10", "2"), Ordering::Greater);
        assert_eq!(mode.compare("3.14", "3.2"), Ordering::Less);

        // Lexicographic mode keeps the wrong order.
        let lex = SortMode::Lexicographic;
        assert_eq!(lex.compare("2", "10"), Ordering::Greater);

        // Unparseable falls back to lexicographic.
        assert_eq!(mode.compare("abc", "abd"), Ordering::Less);
        // One parseable, one not: parseable sorts first.
        assert_eq!(mode.compare("10", "abc"), Ordering::Less);
        assert_eq!(mode.compare("abc", "10"), Ordering::Greater);
    }
}
