use crate::ui::ResultsUI;

impl ResultsUI {
    /// Hides column `i`, then invalidates the widths affected by the change.
    pub fn hc_set(&mut self, i: usize) {
        self.config.hidden_columns.set(i);
        self.on_hidden_cols_change();
    }

    /// Unhides column `i`, then invalidates the widths affected by the change.
    pub fn hc_unset(&mut self, i: usize) {
        self.config.hidden_columns.unset(i);
        self.on_hidden_cols_change();
    }

    /// Unhides the most recently hidden column, then invalidates the widths
    /// affected by the change. Returns the unhidden column index, or `None`
    /// if no column was hidden.
    pub fn hc_pop(&mut self) -> Option<usize> {
        let popped = self.config.hidden_columns.pop();
        if popped.is_some() {
            self.on_hidden_cols_change();
        }
        popped
    }

    /// Unhides all columns, then invalidates the widths affected by the change.
    pub fn hc_clear(&mut self) {
        self.config.hidden_columns.clear();
        self.on_hidden_cols_change();
    }

    /// Number of visible (non-hidden) columns.
    pub fn vcols(&self) -> usize {
        self.config.hidden_columns.visible_count()
    }

    /// Width_overrides and other arrays only index into the visible cols of self.hidden_cols, while self.width_limits maps to the all the columns. This converts the first to the second.
    pub fn expand_idx(&self, idx: usize) -> usize {
        self.config.hidden_columns.nth_gap(idx)
    }
    pub fn shrink_idx(&self, idx: usize) -> Option<usize> {
        self.config.hidden_columns.gap_index(idx)
    }

    /// Recomputes the per-column state that depends on which columns are
    /// hidden: cached widths, preferred widths, and width overrides.
    pub fn on_hidden_cols_change(&mut self) {
        self.invalidate_widths();
        self.preferred_widths.resize(self.vcols(), 0);
        self.config.width_overrides.resize(self.vcols(), 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ResultsConfig;

    #[test]
    fn test_get_col_by_index() {
        use crate::collections::HiddenColumns;
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        let mut hc = HiddenColumns::new_with_size(4);
        hc.set(1); // hide column 1
        results.config.hidden_columns = hc;
        // widths: column 0 and 2 displayed, column 3 not (width 0)
        results.width_limits = vec![10, 0, 20, 0];

        assert_eq!(results.get_col_by_display_index(0), Some(0));
        assert_eq!(results.get_col_by_display_index(1), Some(2));
        assert_eq!(results.get_col_by_display_index(2), None); // out of bounds
        assert_eq!(results.get_col_by_display_index(3), None); // out of bounds

        // Uninitialized widths -> None.
        results.width_limits = Vec::new();
        assert_eq!(results.get_col_by_display_index(0), None);
    }

    #[test]
    fn test_shrink_idx() {
        use crate::collections::HiddenColumns;
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        let mut hc = HiddenColumns::new_with_size(4);
        hc.set(1);
        results.config.hidden_columns = hc;

        // Columns:
        // 0: visible (shrink_idx should map it to 0)
        // 1: hidden (shrink_idx should return None)
        // 2: visible (shrink_idx should map it to 1, because 0 is visible and 1 is hidden)
        // 3: visible (shrink_idx should map it to 2, because 0 and 2 are visible, 1 is hidden)

        assert_eq!(results.shrink_idx(0), Some(0));
        assert_eq!(results.shrink_idx(1), None);
        assert_eq!(results.shrink_idx(2), Some(1));
        assert_eq!(results.shrink_idx(3), Some(2)); // makes equal sense to allow oob or not
    }

    #[test]
    fn test_single_column_preferred_width_is_median() {
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.config.hidden_columns = crate::collections::HiddenColumns::new_with_size(1);
        results.column_name_widths = vec![0];

        // 3 rows for 1 visible column: widths 10, 50, 20
        results.row_cache[0] = vec![
            (0, vec![], vec![10]),
            (1, vec![], vec![50]),
            (2, vec![], vec![20]),
        ];

        let updated = results.update_preferred_widths();
        assert!(updated);
        // For single column, preferred_width is max width (50)
        assert_eq!(results.preferred_widths, vec![50]);
    }

    #[test]
    fn test_try_apply_max_widths() {
        let config = ResultsConfig::default();

        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.config.hidden_columns = crate::collections::HiddenColumns::new_with_size(2);
        results.column_name_widths = vec![0, 0];
        // Populate raw widths in row_cache[0]
        results.row_cache[0] = vec![(0, vec![], vec![12, 20]), (1, vec![], vec![15, 10])];

        // Max widths: col 0 = 15, col 1 = 20. Sum = 35 <= available_width (97).
        let applied = results.try_apply_max_widths_into_width_buffer();
        assert_eq!(applied, Some(true));
        assert_eq!(results.preferred_widths[0], 15);
        assert_eq!(results.preferred_widths[1], 20);
    }
}
