//! Build the `state` table handed to the lua engine from matchmaker state.
//!
//! Column values are resolved exactly like the template keys `{1}`, `{2}`, …:
//! numeric keys by index, any configured column name by name. Payloads can
//! therefore address columns without going through the shell-quoting template
//! formatter (which produces strings unsafe to embed in lua source).

use matchmaker::{nucleo::Render, render::MMState, SSS};

/// Snapshot of matchmaker state for one lua run. Pure data — the lua table is
/// materialized from this in [`crate::lua::new_vm`].
#[derive(Clone, Debug, Default)]
pub(crate) struct LuaState {
    /// The input (same as `env.MM_QUERY`).
    pub query: String,
    /// The whole current item line, unsplit.
    pub raw: Option<String>,
    /// `(column name, value)` pairs of the current item; `None` when no item
    /// is selected.
    pub current: Option<Vec<(String, String)>>,
    /// `(column name, value)` pairs of every selected item (the current item
    /// when the selection is empty, mirroring `{+0}`).
    pub selected: Vec<Vec<(String, String)>>,
    /// Index of the current item, if any.
    pub position: Option<u32>,
    pub total: u32,
    pub matched: u32,
    pub selected_count: usize,
    /// 0-based index of the active column (for `{!}`).
    pub active: usize,
    pub mode: String,
    /// The [`crate::start::COMMAND_ARGS`] of this run (the `$0`/`$1` keys).
    pub args: Vec<String>,
}

impl LuaState {
    /// Empty state, used before the picker is up (`[envs]`, `start.directory`).
    /// The mode and command args are still populated.
    pub(crate) fn empty() -> Self {
        Self {
            mode: current_mode(),
            args: current_args(),
            ..Self::default()
        }
    }

    /// Snapshot the current matchmaker state.
    pub(crate) fn from_mm<T: SSS + Render, D: 'static>(state: &MMState<'_, T, D>) -> Self {
        let columns = &state.picker_ui.worker.columns;
        let preprocess = &state.picker_ui.worker.raw_preprocessor;

        let item_columns = |item: &T| -> Option<Vec<(String, String)>> {
            let d = preprocess(item)?;
            Some(
                columns
                    .iter()
                    .map(|c| (c.name.to_string(), c.raw(item, &d).into_owned()))
                    .collect(),
            )
        };

        let (position, raw, current) = match state.picker_ui.current_indexed() {
            Some((idx, item)) => (
                Some(idx),
                Some(item.as_str().into_owned()),
                item_columns(item),
            ),
            None => (None, None, None),
        };

        let selected = state.map_selected_to_vec(|_, item| item_columns(item).unwrap_or_default());

        Self {
            query: state.picker_ui.query.input(),
            raw,
            current,
            selected,
            position,
            total: state.status().item_count,
            matched: state.status().matched_count,
            selected_count: state.selections().len(),
            active: state.picker_ui.active_column_index(),
            mode: current_mode(),
            args: current_args(),
        }
    }
}

fn current_mode() -> String {
    matchmaker::event::MODE
        .lock()
        .map(|m| m.iter().map(|s| s.as_ref()).collect::<Vec<_>>().join(","))
        .unwrap_or_default()
}

fn current_args() -> Vec<String> {
    crate::start::COMMAND_ARGS
        .lock()
        .map(|a| a.iter().map(|s| s.to_string_lossy().into_owned()).collect())
        .unwrap_or_default()
}
