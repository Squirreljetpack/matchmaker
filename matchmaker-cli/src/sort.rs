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
            SortMode::Numeric => compare_numeric(a, b),
        }
    }
}

/// Compare two values by the numbers they contain: the first numeric segment
/// decides, ties fall through to the next segment, and a value without a
/// leading number sorts after those with one. So `0d0h9m` sorts before
/// `0d1h0m`, and `12abc3` before `12abc10`.
fn compare_numeric(a: &str, b: &str) -> Ordering {
    match (numeric_segments(a), numeric_segments(b)) {
        (Some(xs), Some(ys)) => xs
            .iter()
            .zip(ys.iter())
            .map(|(x, y)| x.partial_cmp(y).unwrap_or(Ordering::Equal))
            .find(|o| !o.is_eq())
            .unwrap_or_else(|| xs.len().cmp(&ys.len())),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => a.cmp(b),
    }
}

/// Split a value into the numbers it contains, e.g. `0d0h26m5s` becomes
/// `[0, 0, 26, 5]`. Returns `None` when the value does not start with a digit,
/// so non-numeric values sort last.
fn numeric_segments(s: &str) -> Option<Vec<f64>> {
    let mut segments = Vec::new();
    let mut rest = s.as_bytes();
    while let Some((value, used)) = parse_float(rest) {
        segments.push(value);
        rest = &rest[used..];
        match rest.iter().position(u8::is_ascii_digit) {
            Some(i) => rest = &rest[i..],
            None => break,
        }
    }
    (!segments.is_empty()).then_some(segments)
}

/// Parse the leading number of a byte slice, returning it with the number of
/// bytes it covers. `n` is the integer part, and a trailing `.` triggers decimal
/// parsing. Returns `None` if the input does not start with a digit.
fn parse_float(input: &[u8]) -> Option<(f64, usize)> {
    let (n, used) = u64::from_radix_10(input);
    if used == 0 {
        return None;
    }
    let rest = &input[used..];
    if rest.first() == Some(&b'.') {
        let (d, used2) = u64::from_radix_10(&rest[1..]);
        if used2 == 0 {
            // "3." with no decimal digits — treat as integer.
            return Some((n as f64, used));
        }
        // Build "<n>.<d>" and let `f64::from_str` handle the float math.
        let mut buf = String::with_capacity(used + 1 + used2);
        use std::fmt::Write;
        let _ = write!(&mut buf, "{n}.{d}");
        buf.parse().ok().map(|f| (f, used + 1 + used2))
    } else {
        Some((n as f64, used))
    }
}

/// Helper: resolve an optional column index from a `Sort`/`SortNumeric`
/// action into the worker-column index to sort by.
///
/// `None` resolves to the active column. A given index is mapped through
/// [`ResultsUI::get_col_by_index`]; when it does not correspond to a
/// displayed column, an error is logged and `None` is returned.
pub fn expand_maybe_column(state: &MMState<'_>, idx: Option<usize>) -> Option<usize> {
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
    state: &mut MMState<'_>,
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
pub fn handle_sort_reverse(state: &mut MMState<'_>, dir: Option<bool>, sort: &mut SortSetting) {
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
        assert_eq!(parse_float(b"3"), Some((3.0, 1)));
        assert_eq!(parse_float(b"0"), Some((0.0, 1)));
        assert_eq!(parse_float(b"42abc"), Some((42.0, 2)));
        assert_eq!(parse_float(b"3.5"), Some((3.5, 3)));
        assert_eq!(parse_float(b"0.5"), Some((0.5, 3)));
        assert_eq!(parse_float(b"100.25"), Some((100.25, 6)));
        assert_eq!(parse_float(b"3."), Some((3.0, 1)));
        assert_eq!(parse_float(b""), None);
        assert_eq!(parse_float(b"abc"), None);
        assert_eq!(parse_float(b".5"), None); // no leading digit
    }

    #[test]
    fn test_numeric_segments() {
        assert_eq!(
            numeric_segments("0d0h26m5s"),
            Some(vec![0.0, 0.0, 26.0, 5.0])
        );
        assert_eq!(
            numeric_segments("14d11h26m5s"),
            Some(vec![14.0, 11.0, 26.0, 5.0])
        );
        assert_eq!(numeric_segments("3.5-2"), Some(vec![3.5, 2.0]));
        assert_eq!(numeric_segments("-"), None);
        assert_eq!(numeric_segments("abc1"), None);
    }

    #[test]
    fn test_sort_mode_numeric_orders_correctly() {
        let mode = SortMode::Numeric;
        assert_eq!(mode.compare("2", "10"), Ordering::Less);
        assert_eq!(mode.compare("10", "2"), Ordering::Greater);
        assert_eq!(mode.compare("3.14", "3.2"), Ordering::Less);

        // Duration cells compare segment by segment.
        assert_eq!(mode.compare("0d0h26m5s", "0d0h26m9s"), Ordering::Less);
        assert_eq!(mode.compare("0d0h59m9s", "0d1h0m0s"), Ordering::Less);
        assert_eq!(mode.compare("0d9h0m0s", "14d11h26m5s"), Ordering::Less);
        assert_eq!(mode.compare("14d11h26m5s", "14d11h26m5s"), Ordering::Equal);
        assert_eq!(mode.compare("0d0h0m26s", "-"), Ordering::Less);

        let lex = SortMode::Lexicographic;
        assert_eq!(lex.compare("2", "10"), Ordering::Greater);
        assert_eq!(mode.compare("abc", "abd"), Ordering::Less);
        assert_eq!(mode.compare("10", "abc"), Ordering::Less);
        assert_eq!(mode.compare("abc", "10"), Ordering::Greater);
    }
}
