use std::{cmp::Ordering, sync::Arc};

use atoi::FromRadix10;
use cba::wbog;
use matchmaker::config_mm::{ConfigMatchmaker, RangesFactory};
use serde::{Deserialize, Serialize};

use crate::action::MMState;
use crate::config::SortSetting;

/// Sort function type accepted by `nucleo.sort_with` over `String` items.
type StringSortFn = Arc<dyn Fn((u32, &String), (u32, &String)) -> bool + Send + Sync>;

/// Sort mode used by [`apply_sort`].
#[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortMode {
    /// No custom sort (the default).
    #[default]
    None,
    Lexicographic,
    Numeric,
}

impl SortMode {
    fn compare(self, a: &str, b: &str) -> Ordering {
        match self {
            SortMode::None | SortMode::Lexicographic => a.cmp(b),
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

/// Helper: resolve an optional column index from a `Sort`/`SortNumeric`
/// action into the worker-column index to sort by.
///
/// `None` resolves to the active column. A given index is mapped through
/// [`ResultsUI::get_col_by_index`]; when it does not correspond to a
/// displayed column, an error is logged and `None` is returned.
pub fn expand_maybe_column(state: &MMState<'_, '_>, idx: Option<usize>) -> Option<usize> {
    match idx {
        None => Some(state.picker_ui.active_column_index()),
        Some(i) => {
            let n = state.picker_ui.results.get_col_by_display_index(i);
            if n.is_none() {
                log::error!("Sort column {i} not found among displayed columns");
            }
            n
        }
    }
}

/// Sort the results by the given column, toggling the sort off when `mode` is
/// already active on that column; the configured threshold is restored in that
/// case.
pub fn apply_sort(
    state: &mut MMState<'_, '_>,
    ranges_fn: &RangesFactory<String>,
    n: usize,
    mode: SortMode,
    sort: &mut SortSetting,
) {
    let Some(column_name) = state
        .picker_ui
        .worker
        .columns
        .get(n)
        .map(|c| c.name.clone())
    else {
        log::error!("Sort column {n} not found among worker columns");
        return;
    };

    if sort.mode == mode && sort.column.as_str() == column_name.as_ref() {
        state.picker_ui.worker.nucleo.sort_with(None);
        state.picker_ui.worker.nucleo.set_stability(*sort.threshold);
        state.picker_ui.worker.nucleo.resort();
        sort.mode = SortMode::None;
    } else {
        let lookup = ranges_fn(n);
        let lookup_for_closure = lookup.clone();
        let sort_fn: StringSortFn =
            Arc::new(move |(_ia, a): (u32, &String), (_ib, b): (u32, &String)| {
                let sub_a: &str = &lookup_for_closure(a);
                let sub_b: &str = &lookup_for_closure(b);
                mode.compare(sub_a, sub_b) == Ordering::Less
            });

        state.picker_ui.worker.nucleo.sort_with(Some(sort_fn));
        state.picker_ui.worker.nucleo.set_stability(u32::MAX);
        state.worker_resort();
        sort.mode = mode;
        sort.column = column_name.to_string();
    }
}

/// Reverse the current sort direction, or set it explicitly when `dir` is given.
pub fn handle_sort_reverse(state: &mut MMState<'_, '_>, dir: Option<bool>, sort: &mut SortSetting) {
    let new_dir = match dir {
        Some(b) => b,
        None => !sort.reverse,
    };
    sort.reverse = new_dir;

    state.picker_ui.worker.nucleo.reverse_items(new_dir);
    state.picker_ui.worker.nucleo.resort();
}

/// Apply the configured sort settings to a freshly built matchmaker's worker,
/// so it starts with the configured sort state.
pub fn init_mm_sort(
    mm: &mut ConfigMatchmaker,
    ranges_fn: &RangesFactory<String>,
    sort: SortSetting,
) {
    let worker = &mut mm.worker;
    worker.reverse_items(sort.reverse);
    worker.set_stability(*sort.threshold);

    if sort.mode != SortMode::None {
        // Resolve the configured column; skip the sort mode entirely when
        // the column does not exist.
        let Some(n) = worker.get_column_index(&sort.column) else {
            wbog!("Sort column '{}' not found, skipping sort.", sort.column);
            return;
        };
        let lookup = ranges_fn(n);
        let lookup_for_closure = lookup.clone();
        let mode = sort.mode;
        let sort_fn: StringSortFn =
            Arc::new(move |(_ia, a): (u32, &String), (_ib, b): (u32, &String)| {
                let sub_a: &str = &lookup_for_closure(a);
                let sub_b: &str = &lookup_for_closure(b);
                mode.compare(sub_a, sub_b) == Ordering::Less
            });

        worker.nucleo.sort_with(Some(sort_fn));
        worker.set_stability(u32::MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_float() {
        assert_eq!(parse_float(b"3"), Some(3.0));
        assert_eq!(parse_float(b"0"), Some(0.0));
        assert_eq!(parse_float(b"42"), Some(42.0));
        assert_eq!(parse_float(b"42abc"), Some(42.0));
        assert_eq!(parse_float(b"3.5"), Some(3.5));
        assert_eq!(parse_float(b"0.5"), Some(0.5));
        assert_eq!(parse_float(b"100.25"), Some(100.25));
        assert_eq!(parse_float(b"3."), Some(3.0));
        assert_eq!(parse_float(b""), None);
        assert_eq!(parse_float(b"abc"), None);
        assert_eq!(parse_float(b".5"), None); // no leading digit
    }

    #[test]
    fn test_sort_mode_numeric_orders_correctly() {
        let mode = SortMode::Numeric;
        assert_eq!(mode.compare("2", "10"), Ordering::Less);
        assert_eq!(mode.compare("10", "2"), Ordering::Greater);
        assert_eq!(mode.compare("3.14", "3.2"), Ordering::Less);

        let lex = SortMode::Lexicographic;
        assert_eq!(lex.compare("2", "10"), Ordering::Greater);
        assert_eq!(mode.compare("abc", "abd"), Ordering::Less);
        assert_eq!(mode.compare("10", "abc"), Ordering::Less);
        assert_eq!(mode.compare("abc", "10"), Ordering::Greater);
    }
}
