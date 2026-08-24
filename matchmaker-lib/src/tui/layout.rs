use crate::config::ScrollStrategy;

/// Rows to scroll up for the given strategy, given the rows currently
/// available below the cursor.
///
/// Callers assume the cursor position is the worst case (`height - 1`) when it
/// cannot be measured, so every strategy still produces an in-view placement.
pub(super) fn scroll_amount(strategy: ScrollStrategy, request: u16, min: u16, available: u16) -> u16 {
    match strategy {
        ScrollStrategy::Compact => min.saturating_sub(available),
        ScrollStrategy::Lazy if available >= min => 0,
        ScrollStrategy::Aggressive | ScrollStrategy::Lazy | ScrollStrategy::Expansive => {
            request.saturating_sub(available)
        }
    }
}

/// Final viewport height for the given strategy.
pub(super) fn viewport_height(
    strategy: ScrollStrategy,
    request: u16,
    min: u16,
    max: u16,
    available: u16,
) -> u16 {
    match strategy {
        // occupy every row below the cursor, capped by max
        ScrollStrategy::Expansive => available
            .min(if max == 0 { available } else { max })
            .max(min),
        _ => available.min(request).max(min),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUEST: u16 = 20;
    const MIN: u16 = 10;

    fn amount(strategy: ScrollStrategy, available: u16) -> u16 {
        scroll_amount(strategy, REQUEST, MIN, available)
    }

    #[test]
    fn aggressive_always_reaches_request() {
        assert_eq!(amount(ScrollStrategy::Aggressive, 24), 0);
        assert_eq!(amount(ScrollStrategy::Aggressive, 15), 5);
        assert_eq!(amount(ScrollStrategy::Aggressive, 1), 19);
    }

    #[test]
    fn lazy_scrolls_only_below_min() {
        assert_eq!(amount(ScrollStrategy::Lazy, 24), 0);
        // between min and request: no disturbance
        assert_eq!(amount(ScrollStrategy::Lazy, 15), 0);
        // below min: goes all the way to the request
        assert_eq!(amount(ScrollStrategy::Lazy, 9), 11);
    }

    #[test]
    fn compact_scrolls_only_to_min() {
        assert_eq!(amount(ScrollStrategy::Compact, 24), 0);
        assert_eq!(amount(ScrollStrategy::Compact, 15), 0);
        assert_eq!(amount(ScrollStrategy::Compact, 9), 1);
    }

    #[test]
    fn expansive_scrolls_like_aggressive_but_uses_all_rows() {
        assert_eq!(amount(ScrollStrategy::Expansive, 24), 0);
        assert_eq!(amount(ScrollStrategy::Expansive, 15), 5);
        assert_eq!(amount(ScrollStrategy::Expansive, 9), 11);
    }

    #[test]
    fn viewport_height_per_strategy() {
        let vh = |strategy, available| viewport_height(strategy, REQUEST, MIN, 0, available);
        assert_eq!(vh(ScrollStrategy::Aggressive, 17), 17);
        assert_eq!(vh(ScrollStrategy::Lazy, 17), 17);
        // capped by request
        assert_eq!(vh(ScrollStrategy::Aggressive, 25), REQUEST);
        // expansive ignores the percentage but respects max (0 = uncapped)
        assert_eq!(
            viewport_height(ScrollStrategy::Expansive, REQUEST, MIN, 0, 25),
            25
        );
        assert_eq!(
            viewport_height(ScrollStrategy::Expansive, REQUEST, MIN, 18, 25),
            18
        );
    }

    #[test]
    fn worst_case_assumption_stays_in_view() {
        let available = 1; // synthesized when the cursor cannot be measured
        for strategy in [
            ScrollStrategy::Aggressive,
            ScrollStrategy::Lazy,
            ScrollStrategy::Compact,
        ] {
            let scroll = scroll_amount(strategy, REQUEST, MIN, available);
            // viewport must end within the screen
            assert!(available + scroll <= REQUEST.max(MIN));
            let target = if strategy == ScrollStrategy::Compact {
                MIN
            } else {
                REQUEST
            };
            assert_eq!(scroll + available, target);
        }
    }
}
