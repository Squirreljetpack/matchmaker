use cba::{_info, _trace};

use crate::ui::ResultsUI;
impl ResultsUI {
    /// Try to directly set preferred_widths, width_limits, and widths to exact max column widths
    /// if the total sum of max widths fits within available_width.
    /// Returns `true` if max widths were applied immediately.
    pub(super) fn try_apply_max_widths(&mut self) -> bool {
        if self.row_cache[1].is_empty() || self.config.stacked_columns {
            return false;
        }

        let v_cols = self.hidden_columns.visible_count();
        let mut max_widths = vec![0u16; v_cols];
        for (_, _, row_widths) in &self.row_cache[1] {
            for (i, &w) in row_widths.iter().enumerate() {
                if i < v_cols {
                    max_widths[i] = max_widths[i].max(w);
                }
            }
        }

        let mut vi = 0;
        for (i, name_w) in self.column_name_widths.iter().enumerate() {
            if self.hidden_columns.contains(i) {
                continue;
            }
            let mut lower = 0;
            if max_widths[vi] > 0 {
                lower = lower.max(if self.config.min_width_from_cols {
                    *name_w
                } else {
                    self.config.min_width
                });
            }
            max_widths[vi] = max_widths[vi].max(lower);
            vi += 1;
        }

        let available_width = self.available_width();
        let sum: u16 = max_widths.iter().sum();

        if sum > available_width {
            return false;
        }

        let overrides = &self.config.width_overrides;
        let remaining = available_width - sum;
        if remaining > 0 && self.config.right_align_last {
            let unoverridden: Vec<usize> = (0..v_cols)
                .filter(|&i| max_widths[i] > 0 && overrides.get(i).copied().unwrap_or(0) == 0)
                .collect();

            if let Some(&last_i) = unoverridden.last() {
                max_widths[last_i] += remaining;
            }
        }

        self.preferred_widths = max_widths;

        let n_cols = self.hidden_columns.mask_len();
        self.width_limits.clear();
        self.width_limits.reserve(n_cols);
        let mut v_idx = 0;
        for idx in 0..n_cols {
            if self.hidden_columns.contains(idx) {
                self.width_limits.push(0);
            } else {
                self.width_limits.push(self.preferred_widths[v_idx]);
                v_idx += 1;
            }
        }

        if self.config.stacked_columns {
            self.widths = vec![self.width];
        } else {
            self.widths = self
                .width_limits
                .iter()
                .cloned()
                .filter(|x| *x != 0)
                .collect();

            if !self.widths.is_empty() {
                self.widths[0] += self.indentation() as u16;
            }
        }

        _info!(
            "[try_apply_max_widths]";
            self.preferred_widths;
            self.width_limits;
            self.widths;
            self.hidden_columns;
        );

        true
    }

    /// Update self.preferred_widths from collected raw_widths and max_widths, then clear them. Additionally, swap the read/write row caches.
    /// Every nonempty column is assigned a nonzero width.
    /// Noop if row_cache is empty or stacked_columns
    pub(super) fn update_preferred_widths(&mut self) -> bool {
        if self.row_cache[1].is_empty() || self.config.stacked_columns {
            return false;
        }
        // _info!(self.row_cache[1]);

        let v_cols = self.hidden_columns.visible_count();
        self.widths_buffer.clear();
        self.widths_buffer.reserve(v_cols);
        self.preferred_widths.resize(v_cols, 0);

        // Compute max_widths on the fly for the adjustment phase
        let mut max_widths = vec![0u16; v_cols];
        for (_, _, row_widths) in &self.row_cache[1] {
            for (i, &w) in row_widths.iter().enumerate() {
                if i < v_cols {
                    max_widths[i] = max_widths[i].max(w);
                }
            }
        }

        for col_idx in 0..v_cols {
            let mut v: Vec<u16> = self.row_cache[1]
                .iter()
                .map(|(_, _, row_widths)| row_widths.get(col_idx).copied().unwrap_or(0))
                .collect();

            let median = if !v.is_empty() {
                v.sort_unstable();
                v[v.len() / 2]
            } else {
                0
            };
            self.widths_buffer.push(median);
        }

        // 2. Adjust the values in place based on config.min_width and v_max_widths
        let mut vi = 0;

        for (i, name_w) in self.column_name_widths.iter().enumerate() {
            if self.hidden_columns.contains(i) {
                continue;
            }

            let mut lower = max_widths[vi];
            if lower > 0 {
                lower = lower.max(if self.config.min_width_from_cols {
                    *name_w
                } else {
                    self.config.min_width
                })
            }

            self.widths_buffer[vi] = self.widths_buffer[vi].max(lower);

            vi += 1;
        }

        let grew = self
            .preferred_widths
            .iter()
            .zip(&self.widths_buffer)
            .any(|(old, new)| new > old);

        let shrank = self
            .preferred_widths
            .iter()
            .zip(&self.widths_buffer)
            .any(|(old, new)| new < old);

        let sum: u16 = self.preferred_widths.iter().sum();

        let condition = sum <= self.width && !(grew && shrank);

        // 3.
        if self.preferred_widths.is_empty() || condition {
            self.preferred_widths = std::mem::take(&mut self.widths_buffer);
            grew || shrank
        } else {
            Self::apply_width_thresholds(
                &mut self.preferred_widths,
                &self.widths_buffer,
                self.config.resize_col_thresholds,
                false,
            )
        }
    }

    /// Applies threshold-based width updates to `old` based on `new` values.
    /// Returns `true` if any widths were modified or if `old` was fully replaced.
    ///
    /// - If lengths differ, `old` is replaced with `new`.
    /// - If **any** column becomes visible (`old == 0` and `new > 0`), or disappears
    ///   (`old > 0` and `new == 0`) while `immediate_prune` is `true`, `old` is
    ///   fully replaced with `new`.
    /// - Otherwise, individual column widths are updated only if their size change
    ///   meets or exceeds the respective `grow` or `shrink` thresholds.
    fn apply_width_thresholds(
        old: &mut Vec<u16>,
        new: &[u16],
        [grow, shrink]: [u16; 2],
        immediate_prune: bool,
    ) -> bool {
        // If lengths differ, replace immediately
        if old.len() != new.len() {
            *old = new.to_vec();
            return true;
        }

        // Check if any column appeared (0 -> >0) or dropped to zero (if immediate_prune)
        let needs_full_update = old.iter().zip(new.iter()).any(|(&o, &n)| {
            let appeared = o == 0 && n > 0;
            let disappeared = immediate_prune && o > 0 && n == 0;
            appeared || disappeared
        });

        if needs_full_update {
            old.clone_from_slice(new);
            return true;
        }

        // Standard threshold logic for hysteresis resizing
        let mut changed = false;

        for (old_val, &new_val) in old.iter_mut().zip(new.iter()) {
            if new_val > *old_val {
                // Growing: update if change meets/exceeds threshold
                if new_val - *old_val >= grow {
                    *old_val = new_val;
                    changed = true;
                }
            } else if *old_val > new_val && *old_val - new_val >= shrink {
                // Shrinking: update if change meets/exceeds threshold
                *old_val = new_val;
                changed = true;
            }
        }

        changed
    }

    /// Set self.width_limits using self.preferred_widths.
    /// Also sets self.widths: the rendered table column widths
    /// no-op: if row_cache[1] or preferred_widths are not populated
    pub(super) fn update_width_limits(&mut self) {
        if self.config.stacked_columns {
            let default = self.width.saturating_sub(self.indentation() as u16);

            self.widths_buffer = (0..self.hidden_columns.mask_len())
                .map(|i| {
                    if self.hidden_columns.contains(i) {
                        0
                    } else {
                        default
                    }
                })
                .collect();
        } else {
            self.update_width_limits_into_width_buffer();
            if self.widths_buffer.is_empty() {
                return;
            }
            self.expand_width_limits_in_buffer();

            _trace!(
                "[update_width_limits]";
                self.preferred_widths
            );
        }

        if self.width_limits != self.widths_buffer {
            _info!("applying width buffer"; self.width_limits; self.widths_buffer);
        }

        // using apply_width_thresholds has unexpected effect of transitioning instead of preventing small resizes
        if Self::apply_width_thresholds(
            &mut self.width_limits,
            &self.widths_buffer,
            self.config.resize_col_thresholds,
            true,
        ) {
            self.row_cache[0].clear();
            _trace!(self.width_limits);

            if self.config.stacked_columns {
                self.widths = vec![self.width];
            } else {
                self.widths = self
                    .width_limits
                    .iter()
                    .cloned()
                    .filter(|x| *x != 0)
                    .collect();

                if !self.widths.is_empty() {
                    self.widths[0] += self.indentation() as u16;
                }
                _info!(self.widths);
            }
        }
    }

    /// Calculate column width limits that fit within the available content width.
    ///
    /// This method implements a constraint-satisfaction algorithm to allocate column widths:
    ///
    /// ### Algorithm Overview:
    /// Given a fixed available width and columns with preferred/max widths, distribute space
    /// while respecting user overrides, minimum widths, and content preferences.
    ///
    /// ### Returns:
    /// A vector where result[i] is the width limit for column i. Hidden columns have
    /// width 0 (will be skipped by render_row). The sum is guaranteed <= available_width.
    ///
    /// ### Requires:
    /// self.preferred_widths is non-empty.
    ///
    /// ### Invariants:
    /// - sum(result) <= content_width()
    /// - Hidden columns have width 0
    /// - Non-hidden columns have width >= min_width (when feasible)
    /// - User overrides are respected when feasible
    fn update_width_limits_into_width_buffer(&mut self) {
        if self.row_cache[0].is_empty() || self.preferred_widths.is_empty() {
            _info!(
                "skipped width_limits update, either is empty: row cache or preferred":
                self.preferred_widths
            );
            self.widths_buffer.clear();
            return;
        }

        let v_cols = self.preferred_widths.len();
        let mut max_widths = vec![0u16; v_cols];
        for (_, _, row_widths) in &self.row_cache[0] {
            for (i, &w) in row_widths.iter().enumerate() {
                if i < v_cols {
                    max_widths[i] = max_widths[i].max(w);
                }
            }
        }

        _info!(max_widths; self.preferred_widths);

        // Identify only the columns that have a preferred width > 0
        let active_cols: Vec<usize> = (0..v_cols)
            .filter(|&i| self.preferred_widths[i] > 0)
            .collect();

        // statistics are available iff max_widths is populated (which mirrors active_cols)
        if active_cols.is_empty() {
            self.widths_buffer.clear();
            return;
        }

        // update temporarily for accurate available_width
        let new: Vec<_> = max_widths.iter().cloned().filter(|x| *x != 0).collect();
        if new.len() != self.widths.len() {
            self.widths = new;
        }
        let available_width = self.available_width();

        // Prepare width buffers
        let overrides = &mut self.config.width_overrides;
        overrides.resize(v_cols, 0); // it should already be

        // We clear and resize to ensure any inactive columns are initialized to 0
        self.widths_buffer.clear();
        self.widths_buffer.resize(v_cols, 0);

        // Step 2: Validate width overrides fit within available space
        // Constraint: sum(overrides) + count(unoverridden) * min_width <= available_width
        // If violated, drop overrides from right-to-left until satisfied
        let mut current_override_sum: u16 = active_cols.iter().map(|&i| overrides[i]).sum();
        let mut unoverridden_count =
            active_cols.iter().filter(|&&i| overrides[i] == 0).count() as u16;

        while current_override_sum + unoverridden_count * self.config.min_width > available_width {
            // Find the rightmost active column with an override
            let Some(&i) = active_cols.iter().rev().find(|&&i| overrides[i] > 0) else {
                break;
            };

            current_override_sum -= overrides[i];
            overrides[i] = 0;
            unoverridden_count += 1;
        }

        // Step 3: Fallback to even distribution if overrides still infeasible
        if current_override_sum + unoverridden_count * self.config.min_width > available_width {
            let avg = available_width / active_cols.len() as u16;
            let rem = available_width % active_cols.len() as u16;

            for &i in &active_cols {
                self.widths_buffer[i] = avg;
            }

            if let Some(&last_i) = active_cols.last() {
                self.widths_buffer[last_i] += rem;
            }

            return;
        }

        // Step 4: Lock in validated overrides
        let mut remaining_width = available_width;
        let mut unassigned_cols = vec![];
        for &i in &active_cols {
            if overrides[i] > 0 {
                self.widths_buffer[i] = overrides[i];
                remaining_width = remaining_width.saturating_sub(overrides[i]);
            } else {
                unassigned_cols.push(i);
            }
        }

        // Step 5: Iterative preferred-width allocation
        while !unassigned_cols.is_empty() {
            let avg = remaining_width / unassigned_cols.len() as u16;
            let mut newly_assigned = false;
            let mut next = Vec::with_capacity(unassigned_cols.len());

            for &i in &unassigned_cols {
                if self.preferred_widths[i] <= avg {
                    self.widths_buffer[i] = self.preferred_widths[i];
                    remaining_width -= self.preferred_widths[i];
                    newly_assigned = true;
                } else {
                    next.push(i);
                }
            }
            unassigned_cols = next;

            if !newly_assigned {
                break;
            }
        }

        // Step 6: Equal distribution for oversized columns
        if !unassigned_cols.is_empty() {
            let avg = remaining_width / unassigned_cols.len() as u16;
            let rem = remaining_width % unassigned_cols.len() as u16;
            let last_unassigned = *unassigned_cols.last().unwrap();

            for &i in &unassigned_cols {
                self.widths_buffer[i] = avg;
            }
            self.widths_buffer[last_unassigned] += rem;
        }

        // Step 7: Final expansion pass
        let current_sum: u16 = self.widths_buffer.iter().sum();
        if current_sum < available_width {
            let mut gaps: Vec<(usize, u16)> = active_cols
                .iter()
                .filter_map(|&i| {
                    if overrides[i] > 0 {
                        None
                    } else {
                        let max_w = max_widths.get(i).copied().unwrap_or(0);
                        let gap = max_w.saturating_sub(self.widths_buffer[i]);
                        (gap > 0).then_some((i, gap))
                    }
                })
                .collect();

            let mut remaining = available_width - current_sum;

            while remaining > 0 && !gaps.is_empty() {
                let per = (remaining / gaps.len() as u16).max(1);

                gaps.retain_mut(|(i, gap)| {
                    let add = per.min(*gap).min(remaining);
                    self.widths_buffer[*i] += add;
                    *gap -= add;
                    remaining -= add;
                    *gap > 0
                });
            }

            // we only grow up to max widths which might < sum
            if remaining > 0 && self.config.right_align_last {
                let unoverridden: Vec<usize> = active_cols
                    .iter()
                    .copied()
                    .filter(|&i| overrides[i] == 0)
                    .collect();

                if let Some(&last_i) = unoverridden.last() {
                    self.widths_buffer[last_i] += remaining;
                }
            }
        }

        let final_sum: u16 = self.widths_buffer.iter().sum();
        debug_assert!(
            final_sum <= available_width,
            "max_widths: sum of widths {} exceeds content_width {}",
            final_sum,
            available_width
        );
    }

    fn expand_width_limits_in_buffer(&mut self) {
        let n_cols = self.hidden_columns.mask_len();

        let mut new_limits = Vec::with_capacity(n_cols);
        let mut i = 0;
        for idx in 0..n_cols {
            if self.hidden_columns.contains(idx) {
                new_limits.push(0);
            } else {
                new_limits.push(self.widths_buffer[i]);
                i += 1;
            }
        }
        self.widths_buffer = new_limits;
    }

    /// Adjust the user-set width override for the `col`-th non-hidden column by
    /// `expand` (positive = widen, negative = narrow). No-op if the resulting
    /// width would fall below `config.min_width`, or if `col` is out of range.
    ///
    /// `col` indexes into `self.config.width_overrides`, which is sized to
    /// the number of non-hidden columns (i.e. [`Self::v_cols`]).
    pub fn resize_col(&mut self, expand: i16, col: usize) {
        let v_idx = self.shrink_idx(col);
        if self.width_limits.len() <= col || v_idx.is_none() {
            log::warn!("Could not resize due to uninitialized width_limits, please retry");
            return;
        }
        let v_idx = v_idx.unwrap();

        let current = self.width_limits[col];
        let new = if expand > 0 {
            current
                .saturating_add(expand.unsigned_abs())
                .max(self.config.min_width)
        } else {
            current.saturating_sub(expand.unsigned_abs())
        };

        log::trace!(
            "Resizing {v_idx} -> {col}, current overrides: {:?}, new: {new:?}",
            self.config.width_overrides
        );

        self.config.width_overrides[v_idx] = new as u16;
        self.width_limits.clear();
    }

    /// Width_overrides and other arrays only index into the visible cols of self.hidden_cols, while self.width_limits maps to the all the columns. This converts the first to the second.
    pub fn expand_idx(&self, idx: usize) -> usize {
        self.hidden_columns.nth_gap(idx)
    }
    pub fn shrink_idx(&self, idx: usize) -> Option<usize> {
        self.hidden_columns.gap_index(idx)
    }

    pub fn get_gutter_col_idx(&self, x: u16) -> Option<usize> {
        let mut pos = self.indentation() as u16;
        if self.config.column_spacing.0 == 0 {
            return None;
        }

        _info!("Computing gutter"; self.width_limits; x);

        for (i, &width) in self.width_limits.iter().enumerate() {
            pos += width;

            if width > 0 {
                if (pos..pos + self.config.column_spacing.0).contains(&x) {
                    return Some(i);
                }

                pos += self.config.column_spacing.0;
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ResultsConfig;

    #[test]
    fn test_get_dragged_column_gutter() {
        let mut config = ResultsConfig::default();
        config.column_spacing.0 = 2;

        let mut results = ResultsUI::new(config);
        results.width_limits = vec![10, 20, 30];

        // Indentation = 2
        // Column 0 ends at 12 -> Gutter 0 spans [12, 13]
        // Column 1 ends at 34 -> Gutter 1 spans [34, 35]
        assert_eq!(results.get_gutter_col_idx(11), None);
        assert_eq!(results.get_gutter_col_idx(12), Some(0));
        assert_eq!(results.get_gutter_col_idx(13), Some(0));
        assert_eq!(results.get_gutter_col_idx(14), None);
        assert_eq!(results.get_gutter_col_idx(34), Some(1));
    }

    #[test]
    fn test_shrink_idx() {
        use crate::collections::HiddenColumns;
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        let mut hc = HiddenColumns::new_with_size(4);
        hc.set(1);
        results.hidden_columns = hc;

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
    fn test_overrides_not_expanded() {
        let mut config = ResultsConfig::default();
        config.width_overrides = vec![10, 0, 5]; // Column 0 override 10, Column 2 override 5
        config.min_width = 2;

        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.preferred_widths = vec![8, 12, 6];
        results.row_cache[0] = vec![(0, vec![], vec![8, 12, 6])];

        results.update_width_limits();

        // The overridden columns should NOT expand in the final step.
        // Column 0 = 10
        // Column 1 = 12 (expanded to max)
        // Column 2 = 5
        assert_eq!(results.width_limits[0], 10);
        assert_eq!(results.width_limits[1], 12);
        assert_eq!(results.width_limits[2], 5);
    }

    #[test]
    fn test_single_column_preferred_width_is_median() {
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.hidden_columns = crate::collections::HiddenColumns::new_with_size(1);
        results.column_name_widths = vec![0];

        // 3 rows for 1 visible column: widths 10, 50, 20
        results.row_cache[1] = vec![
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
    fn test_right_align_last_expands_last_column() {
        let mut config = ResultsConfig::default();
        config.right_align_last = true;

        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.preferred_widths = vec![10, 15];
        results.hidden_columns = crate::collections::HiddenColumns::new_with_size(2);
        results.column_name_widths = vec![0, 0];
        results.row_cache[0] = vec![(0, vec![], vec![10, 15])];

        results.update_width_limits();

        // Available width = 100 - indentation(2) - spacing(1) = 97
        // Column 0 = 10
        // Column 1 (last column) = 15 + (97 - 25) = 87
        assert_eq!(results.width_limits[0], 10);
        assert_eq!(results.width_limits[1], 87);
    }

    #[test]
    fn test_try_apply_max_widths() {
        let mut config = ResultsConfig::default();
        config.right_align_last = true;

        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.hidden_columns = crate::collections::HiddenColumns::new_with_size(2);
        results.column_name_widths = vec![0, 0];
        // Populate raw widths in row_cache[1]
        results.row_cache[1] = vec![(0, vec![], vec![12, 20]), (1, vec![], vec![15, 10])];

        // Max widths: col 0 = 15, col 1 = 20. Sum = 35 <= available_width (97).
        let applied = results.try_apply_max_widths();
        assert!(applied);
        assert_eq!(results.preferred_widths[0], 15);
        // Column 1 absorbs remaining width (82) since right_align_last is true
        assert_eq!(results.preferred_widths[1], 82);
        assert_eq!(results.width_limits[0], 15);
        assert_eq!(results.width_limits[1], 82);
        // widths[0] includes indentation (+2)
        assert_eq!(results.widths, vec![17, 82]);
    }
}
