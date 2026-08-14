use cba::{_info, _trace};

use crate::ui::ResultsUI;
impl ResultsUI {
    /// Try to directly set preferred_widths, width_limits, and widths to exact max column widths
    /// if the total sum of max widths fits within available_width.
    /// Returns:
    /// Some(true) if max widths were applied immediately,
    /// Some(false) if they don't fit,
    /// None if updates should be skipped.
    pub(super) fn try_apply_max_widths_into_width_buffer(&mut self) -> Option<bool> {
        if self.row_cache[0].is_empty() || self.config.stacked_columns {
            self.widths_buffer.clear();
            return None;
        }

        self.max_widths.clear();
        self.prepare_max_widths();
        if self.preferred_widths == self.max_widths {
            return None;
        }

        self.widths_buffer = self.max_widths.clone();
        let mut vi = 0;
        for (i, name_w) in self.column_name_widths.iter().enumerate() {
            if self.config.hidden_columns.contains(i) {
                continue;
            }
            let mut lower = 0;
            if self.widths_buffer[vi] > 0 {
                lower = lower.max(if self.config.min_width_from_cols {
                    *name_w
                } else {
                    self.config.min_width
                });
            }
            self.widths_buffer[vi] = self.widths_buffer[vi].max(lower);
            vi += 1;
        }

        // Apply column width overrides directly (1:1 mapping with widths_buffer)
        for (w, &override_w) in self
            .widths_buffer
            .iter_mut()
            .zip(&self.config.width_overrides)
        {
            if override_w > 0 {
                *w = override_w;
            }
        }

        let sum: u16 = self.widths_buffer.iter().sum();
        if sum > self.available_width() {
            self.widths_buffer.clear();
            Some(false)
        } else {
            _info!(self.max_widths);
            self.preferred_widths = self.max_widths.clone();
            _info!(self.widths_buffer);
            Some(true)
        }
    }

    fn prepare_max_widths(&mut self) {
        if !self.max_widths.is_empty() {
            return;
        }

        self.max_widths.resize(self.vcols(), 0);
        for (_, _, row_widths) in &self.row_cache[0] {
            // guaranteed row len == vcols
            for (i, &w) in row_widths.iter().enumerate() {
                self.max_widths[i] = self.max_widths[i].max(w);
            }
        }
    }

    /// Update self.preferred_widths from collected raw_widths and max_widths, then clear them. Additionally, swap the read/write row caches.
    /// Every nonempty column is assigned a nonzero width.
    /// Noop if row_cache is empty or stacked_columns.
    /// Widths buffer is cleared if it ran.
    pub(super) fn update_preferred_widths(&mut self) -> bool {
        if self.row_cache[0].is_empty() || self.config.stacked_columns {
            return false;
        }
        // _info!(self.row_cache[1]);

        let v_cols = self.vcols();
        self.prepare_max_widths();

        let mut v = Vec::new();

        for col_idx in 0..v_cols {
            v.clear();
            v.extend(
                self.row_cache[0]
                    .iter()
                    .filter_map(|(_, _, row_widths)| row_widths.get(col_idx).copied()),
            );

            let v = if v.is_empty() {
                0
            } else {
                let mid = v.len() / 2;
                *v.select_nth_unstable(mid).1
            };
            self.widths_buffer.push(v);
        }

        // 2. Adjust the values in place based on config.min_width and v_max_widths
        let mut vi = 0;

        for (i, name_w) in self.column_name_widths.iter().enumerate() {
            if self.config.hidden_columns.contains(i) {
                continue;
            }

            let mut lower = self.max_widths[vi];
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

        let empty_before = self.preferred_widths.is_empty();
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
        _info!(condition);

        // 3.
        if empty_before || condition {
            self.preferred_widths = std::mem::take(&mut self.widths_buffer);
            empty_before || grew || shrank
        } else {
            let ret = Self::apply_width_thresholds(
                &mut self.preferred_widths,
                &self.widths_buffer,
                self.config.resize_col_thresholds,
                false,
            );
            self.widths_buffer.clear();
            ret
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
    /// no-op: if row_cache[0] or preferred_widths are not populated
    pub(super) fn update_width_limits(&mut self) {
        let skip_allocation = !self.widths_buffer.is_empty();
        if self.config.stacked_columns {
            let default = self.width.saturating_sub(self.indentation() as u16);

            self.widths_buffer = (0..self.config.hidden_columns.mask_len())
                .map(|i| {
                    if self.config.hidden_columns.contains(i) {
                        0
                    } else {
                        default
                    }
                })
                .collect();
        } else {
            self.update_width_limits_into_width_buffer(skip_allocation);
            self.max_widths.clear();
            if self.widths_buffer.is_empty() {
                return;
            }
            self.expand_width_limits_in_buffer();

            _trace!(
                "[update_width_limits]";
                self.preferred_widths
            );
        }

        let changed = if self.width_limits != self.widths_buffer {
            _info!("applying width buffer"; self.width_limits; self.widths_buffer);
            if skip_allocation {
                self.width_limits = std::mem::take(&mut self.widths_buffer);
                true
            } else {
                Self::apply_width_thresholds(
                    &mut self.width_limits,
                    &self.widths_buffer,
                    self.config.resize_col_thresholds,
                    true,
                )
            }
        } else {
            false
        };

        if changed {
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
                    // we only grow up to max widths which might < sum
                    let remaining = self
                        .available_width()
                        .saturating_sub(self.widths.iter().sum::<u16>());
                    if self.config.right_align_last && remaining > 0 {
                        *self.widths.last_mut().unwrap() += remaining;
                    }
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
    fn update_width_limits_into_width_buffer(&mut self, skip_allocation: bool) {
        // assert (when nonempty), self.vcols() = self.preferred_widths.len() = self.widths_buffer.len()
        if self.row_cache[0].is_empty() || self.preferred_widths.is_empty() {
            _info!(
                "skipped width_limits update, either is empty: row cache or preferred":
                self.preferred_widths
            );
            // invalidated, width buffer override no longer applies
            self.widths_buffer.clear();

            return;
        }

        let v_cols = self.vcols();
        self.prepare_max_widths();

        // Identify only the columns that have a preferred width > 0
        let active_cols: Vec<usize> = (0..v_cols)
            .filter(|&i| self.preferred_widths[i] > 0)
            .collect();

        if active_cols.is_empty() {
            return;
        }

        // update temporarily for accurate available_width
        let new: Vec<_> = self
            .max_widths
            .iter()
            .cloned()
            .filter(|x| *x != 0)
            .collect();
        if new.len() != self.widths.len() {
            self.widths = new;
        }
        let available_width = self.available_width();
        let overrides = &mut self.config.width_overrides;
        overrides.resize(v_cols, 0); // it should already be

        if !skip_allocation {
            self.widths_buffer.resize(v_cols, 0);

            // Step 2: Validate width overrides fit within available space
            // Constraint: sum(overrides) + count(unoverridden) * min_width <= available_width
            // If violated, drop overrides from right-to-left until satisfied
            let mut current_override_sum: u16 = active_cols.iter().map(|&i| overrides[i]).sum();
            let mut unoverridden_count =
                active_cols.iter().filter(|&&i| overrides[i] == 0).count() as u16;

            while current_override_sum + unoverridden_count * self.config.min_width
                > available_width
            {
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
                        let max_w = self.max_widths.get(i).copied().unwrap_or(0);
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
        let n_cols = self.config.hidden_columns.mask_len();

        let mut new_limits = Vec::with_capacity(n_cols);
        let mut i = 0;
        for idx in 0..n_cols {
            if self.config.hidden_columns.contains(idx) {
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

    /// Maps the `idx`-th displayed column (a column rendered with a nonzero
    /// width) to its index in the worker's columns. Displayed columns are a
    /// subset of the non-hidden columns — hidden columns always have width 0.
    /// Returns `None` when `idx` is out of bounds or widths are not computed
    /// yet.
    pub fn get_col_by_display_index(&self, idx: usize) -> Option<usize> {
        let mut n = 0;
        for (i, &w) in self.width_limits.iter().enumerate() {
            if w > 0 {
                if n == idx {
                    return Some(i);
                }
                n += 1;
            }
        }
        None
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
    fn test_try_apply_max_widths() {
        let config = ResultsConfig::default();
        let mut results = ResultsUI::new(config);
        results.width = 100;
        results.config.hidden_columns = crate::collections::HiddenColumns::new_with_size(2);
        results.column_name_widths = vec![0, 0];
        results.row_cache[0] = vec![(0, vec![], vec![12, 20]), (1, vec![], vec![15, 10])];

        let applied = results.try_apply_max_widths_into_width_buffer();
        assert_eq!(applied, Some(true));
        assert_eq!(results.preferred_widths, vec![15, 20]);
        // try_apply itself preserves row_cache
        assert!(!results.row_cache[0].is_empty());

        // When width_limits changes from [] to [15, 20], update_width_limits clears row_cache
        results.update_width_limits();
        assert_eq!(results.width_limits, vec![15, 20]);
        assert!(results.row_cache[0].is_empty());

        // When width_limits does not change on subsequent update, row_cache is preserved
        results.row_cache[0] = vec![(0, vec![], vec![15, 20])];
        results.update_width_limits();
        assert!(!results.row_cache[0].is_empty());
    }
}
