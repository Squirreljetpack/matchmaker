use crate::ui::ResultsUI;
impl ResultsUI {
    /// Update self.preferred_widths from collected raw_widths and max_widths, then clear them. Additionally, swap the read/write row caches.
    pub(super) fn update_preferred_widths(&mut self) {
        if self.config.stacked_columns {
            return;
        }

        self.row_cache.swap(0, 1);
        self.row_cache[1].clear();

        if self.row_cache[0].is_empty() {
            return;
        }

        let v_cols = self.v_cols();
        self.preferred_widths.clear();
        self.preferred_widths.reserve(v_cols);

        // Compute max_widths on the fly for the adjustment phase
        let mut max_widths = vec![0u16; v_cols];
        for (_, _, row_widths) in &self.row_cache[0] {
            for (i, &w) in row_widths.iter().enumerate() {
                if i < v_cols {
                    max_widths[i] = max_widths[i].max(w);
                }
            }
        }

        for col_idx in 0..v_cols {
            let mut v: Vec<u16> = self.row_cache[0]
                .iter()
                .map(|(_, _, row_widths)| row_widths.get(col_idx).copied().unwrap_or(0))
                .filter(|&w| w > 0)
                .collect();

            let median = if !v.is_empty() {
                v.sort_unstable();
                v[v.len() / 2]
            } else {
                0
            };
            self.preferred_widths.push(median);
        }

        // 2. Adjust the values in place based on config.min_width and v_max_widths
        for (i, pref) in self.preferred_widths.iter_mut().enumerate() {
            let max_w = max_widths.get(i).copied().unwrap_or(0);

            if *pref <= self.config.min_width {
                *pref = max_w.min(self.config.min_width);
            }
        }
    }

    /// Set self.width_limits using self.preferred_widths
    pub(super) fn update_width_limits(&mut self) {
        if self.config.stacked_columns {
            let default = self.width.saturating_sub(self.indentation() as u16);

            self.widths_buffer = (0..self.hidden_columns.len())
                .map(|i| {
                    if self.hidden_columns.get(i).is_some_and(|x| *x) {
                        0
                    } else {
                        default
                    }
                })
                .collect();
        } else {
            self.update_width_limits_into_width_buffer();
            log::debug!(
                "[update_table] preferred={:?}, limits={:?}",
                self.preferred_widths,
                self.width_limits,
            );
        }
        if self.width_limits != self.widths_buffer {
            self.set_dirty();
            self.width_limits = std::mem::take(&mut self.widths_buffer);
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
        if self.row_cache[0].is_empty() {
            self.widths_buffer.clear();
            return;
        }

        let v_cols = self.v_cols();
        let mut max_widths = vec![0u16; v_cols];
        for (_, _, row_widths) in &self.row_cache[0] {
            for (i, &w) in row_widths.iter().enumerate() {
                if i < v_cols {
                    max_widths[i] = max_widths[i].max(w);
                }
            }
        }

        // statistics are available iff max_widths is populated
        if max_widths.iter().all(|x| *x == 0) {
            self.widths_buffer.clear();
            return;
        }

        let available_width = self.available_width();

        // Extract overrides for non-hidden columns
        let n_cols = self.hidden_columns.len();
        self.config.width_overrides.resize(n_cols, 0);
        let mut v_overrides = Vec::with_capacity(v_cols);
        let mut ov_iter = self.config.width_overrides.iter().copied();
        for &hidden in &self.hidden_columns {
            let ov = ov_iter.next().unwrap_or(0);
            if !hidden {
                v_overrides.push(ov);
            }
        }
        self.config.width_overrides = v_overrides;

        // Prepare width buffers
        let overrides = &mut self.config.width_overrides;
        overrides.resize(self.preferred_widths.len(), 0);
        self.widths_buffer.resize(self.preferred_widths.len(), 0);

        // Step 2: Validate width overrides fit within available space
        // Constraint: sum(overrides) + count(unoverridden) * min_width <= available_width
        // If violated, drop overrides from right-to-left until satisfied
        loop {
            let mut unoverridden_count = 0;
            let mut current_override_sum = 0;
            for i in 0..v_cols {
                if overrides[i] > 0 {
                    current_override_sum += overrides[i];
                } else {
                    unoverridden_count += 1;
                }
            }
            // Check if constraint is satisfied
            if current_override_sum + unoverridden_count * self.config.min_width <= available_width
            {
                break;
            }
            // Drop rightmost override and retry
            let mut dropped = false;
            for i in (0..v_cols).rev() {
                if overrides[i] > 0 {
                    overrides[i] = 0;
                    dropped = true;
                    break;
                }
            }
            if !dropped {
                break;
            }
        }

        // Step 3: Fallback to even distribution if overrides still infeasible
        // This happens when even minimum widths can't fit for all columns
        let mut unoverridden_count = 0;
        let mut sum_overrides = 0;
        for i in 0..v_cols {
            if overrides[i] > 0 {
                sum_overrides += overrides[i];
            } else {
                unoverridden_count += 1;
            }
        }
        if sum_overrides + unoverridden_count * self.config.min_width > available_width {
            // Distribute available_width evenly, remainder to last column
            let avg = available_width / v_cols as u16;
            let rem = available_width % v_cols as u16;
            let last_visible = v_cols.saturating_sub(1);
            for i in 0..v_cols {
                self.widths_buffer[i] = avg;
            }
            self.widths_buffer[last_visible] += rem;
        }

        // Step 4: Lock in validated overrides
        // Apply overrides to their columns and track remaining unassigned width
        let mut remaining_width = available_width;
        let mut unassigned_cols = vec![];
        for i in 0..v_cols {
            if overrides[i] > 0 {
                self.widths_buffer[i] = overrides[i];
                remaining_width = remaining_width.saturating_sub(overrides[i]);
            } else {
                unassigned_cols.push(i);
            }
        }

        // Step 5: Iterative preferred-width allocation
        // Greedily assign preferred widths to columns that fit within the average.
        // Columns that fit get their ideal width, freeing space for others.
        loop {
            if unassigned_cols.is_empty() {
                break;
            }
            let avg = remaining_width / unassigned_cols.len() as u16;
            let mut newly_assigned = false;
            let mut new_unassigned = vec![];
            for &i in &unassigned_cols {
                if self.preferred_widths[i] <= avg {
                    // Column fits comfortably, assign preferred width
                    self.widths_buffer[i] = self.preferred_widths[i];
                    remaining_width = remaining_width.saturating_sub(self.preferred_widths[i]);
                    newly_assigned = true;
                } else {
                    // Column wants more than average, defer to later
                    new_unassigned.push(i);
                }
            }
            unassigned_cols = new_unassigned;
            if !newly_assigned {
                break;
            }
        }

        // Step 6: Equal distribution for oversized columns
        // Columns that wanted more than average are constrained. Divide remaining
        // space equally among them, with remainder going to the last column.
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
        // If we have leftover space, expand columns toward their max_width.
        let current_sum: u16 = self.widths_buffer.iter().sum();
        if current_sum < available_width {
            let remainder = available_width - current_sum;

            // Calculate gaps for visible columns
            let mut gaps: Vec<(usize, u16)> = (0..v_cols)
                .filter_map(|i| {
                    let max_w = max_widths.get(i).copied().unwrap_or(0);
                    let current_w = self.widths_buffer[i];
                    let gap = max_w.saturating_sub(current_w);
                    if gap > 0 { Some((i, gap)) } else { None }
                })
                .collect();

            if !gaps.is_empty() {
                // Sort by gap ascending (smallest gaps first)
                gaps.sort_by_key(|&(_, gap)| gap);

                // Check if remainder is smaller than smallest gap
                let smallest_gap = gaps[0].1;
                if remainder < smallest_gap {
                    // Distribute equally among all columns with gaps
                    let per_col = remainder / gaps.len() as u16;
                    let rem = remainder % gaps.len() as u16;
                    for (idx, (i, _gap)) in gaps.iter().enumerate() {
                        self.widths_buffer[*i] += per_col;
                        if (idx as u16) < rem {
                            self.widths_buffer[*i] += 1;
                        }
                    }
                } else {
                    // Distribute remainder in gap order
                    let mut remaining = remainder;
                    for (i, gap) in gaps {
                        if remaining == 0 {
                            break;
                        }
                        let expand = gap.min(remaining);
                        self.widths_buffer[i] += expand;
                        remaining -= expand;
                    }
                }
            }
        }

        let expand_width_limits_impl = |this: &mut Self| {
            let n_cols = this.hidden_columns.len();

            let mut new_limits = Vec::with_capacity(n_cols);
            let mut i = 0;
            for &hidden in &this.hidden_columns {
                if hidden {
                    new_limits.push(0);
                } else {
                    new_limits.push(this.widths_buffer[i]);
                    i += 1;
                }
            }
            this.widths_buffer = new_limits;

            // Map overrides back to original indices
            let mut new_overrides = Vec::with_capacity(n_cols);
            let mut j = 0;
            for &hidden in &this.hidden_columns {
                if hidden {
                    new_overrides.push(0);
                } else {
                    new_overrides.push(this.config.width_overrides[j]);
                    j += 1;
                }
            }
            this.config.width_overrides = new_overrides;
        };

        expand_width_limits_impl(self);

        let final_sum: u16 = self.widths_buffer.iter().sum();
        debug_assert!(
            final_sum <= available_width,
            "max_widths: sum of widths {} exceeds content_width {}",
            final_sum,
            available_width
        );
    }
}
