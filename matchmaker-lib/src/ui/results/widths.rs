use crate::ui::ResultsUI;
impl ResultsUI {
    /// Update self.preferred_widths from collected raw_widths and max_widths, then clear them. Additionally, swap the read/write row caches.
    /// Noop if row_cache is empty or stacked_columns
    pub(super) fn update_preferred_widths(&mut self) -> bool {
        if self.row_cache[0].is_empty() || self.config.stacked_columns {
            return false;
        }

        let v_cols = self.v_cols();
        self.widths_buffer.clear();
        self.widths_buffer.reserve(v_cols);
        self.preferred_widths.resize(v_cols, 0);

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
        if self.preferred_widths.is_empty()
            || self.widths_buffer.iter().filter(|x| **x > 0).count() == 1
        {
            self.preferred_widths = std::mem::take(&mut self.widths_buffer);
            true
        } else {
            let [grow_threshold, shrink_threshold] = self.config.resize_col_thresholds;
            let mut changed = false;

            for (old, &new) in self
                .preferred_widths
                .iter_mut()
                .zip(self.widths_buffer.iter())
            {
                if new > *old {
                    if new - *old >= grow_threshold {
                        *old = new;
                        changed = true;
                    }
                } else if *old > new && *old - new >= shrink_threshold {
                    *old = new;
                    changed = true;
                }
            }
            changed
        }
    }

    /// Set self.width_limits using self.preferred_widths.
    /// no-op: if row_cache[0] or preferred_widths are not populated
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
            #[cfg(debug_assertions)]
            log::trace!(
                "limits changed: {:?} -> {:?}",
                self.width_limits,
                self.widths_buffer
            );
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
        if self.row_cache[0].is_empty() || self.preferred_widths.is_empty() {
            #[cfg(debug_assertions)]
            log::debug!(
                "skipped width update: preferred={:?} row_cache=...",
                self.preferred_widths
            );
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
        #[cfg(debug_assertions)]
        log::debug!(
            "max_widths={max_widths:?}, preferred={:?}",
            self.preferred_widths
        );

        // statistics are available iff max_widths is populated
        if max_widths.iter().all(|x| *x == 0) {
            self.widths_buffer.clear();
            return;
        }

        let available_width = self.available_width();

        // Prepare width buffers
        let overrides = &mut self.config.width_overrides;
        overrides.resize(v_cols, 0); // it should already be
        self.widths_buffer.resize(self.preferred_widths.len(), 0);

        // Step 2: Validate width overrides fit within available space
        // Constraint: sum(overrides) + count(unoverridden) * min_width <= available_width
        // If violated, drop overrides from right-to-left until satisfied
        let mut current_override_sum: u16 = overrides.iter().sum();
        let mut unoverridden_count = overrides.iter().filter(|&&w| w == 0).count() as u16;

        while current_override_sum + unoverridden_count * self.config.min_width > available_width {
            let Some(i) = overrides.iter().rposition(|&w| w > 0) else {
                break;
            };

            current_override_sum -= overrides[i];
            overrides[i] = 0;
            unoverridden_count += 1;
        }

        // Step 3: Fallback to even distribution if overrides still infeasible
        // This happens when even minimum widths can't fit for all columns
        if current_override_sum + unoverridden_count * self.config.min_width > available_width {
            let avg = available_width / v_cols as u16;
            let rem = available_width % v_cols as u16;

            self.widths_buffer.fill(avg);
            self.widths_buffer[v_cols - 1] += rem;

            return;
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
            let mut gaps: Vec<(usize, u16)> = (0..v_cols)
                .filter_map(|i| {
                    let max_w = max_widths.get(i).copied().unwrap_or(0);
                    let gap = max_w.saturating_sub(self.widths_buffer[i]);
                    (gap > 0).then_some((i, gap))
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
