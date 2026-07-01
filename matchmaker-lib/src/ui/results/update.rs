use crate::{config::RowConnectionStyle, ui::ResultsUI};
use ratatui::widgets::{Row, Table};

use crate::{
    SSS, Selector,
    nucleo::{Worker, new_snapshot},
};

impl ResultsUI {
    pub fn update_table<T: SSS, D: 'static>(
        &mut self,
        active_column: usize,
        worker: &mut Worker<T, D>,
        selector: &Selector,
        matcher: &mut nucleo::Matcher,
    ) {
        // Step 0: Refresh the nucleo snapshot and status before rendering
        let (_snapshot, status) = new_snapshot(&mut worker.nucleo);
        let mc = status.matched_count;
        // safely covers all invalidation events. We still keep some savings when matcher is running by caching rows.
        let dirty = (self.matched_count != mc || status.changed)
            || self.row_cache[0].is_empty()
            || self.width_limits.is_empty(); // this last one is cleared in update_dimensions as that doesn't change rows, querychange is more likely to change
        self.matched_count = mc;
        self.status = status;

        // Section 1: Boundaries alignment, update width limits, early returns
        // Ensure cursor is within matched bounds, and update scroll position if bounds changed.
        if mc == 0 {
            self.table = Table::default(); // todo: maybe delay this, like waiting for a signal to reduce flicker?
            self.row_data.clear();
            return;
        }
        if mc < self.bottom + self.cursor as u32 && !self.cursor_disabled {
            self.cursor_jump(mc);
        } else {
            self.cursor = self.cursor.min(mc.saturating_sub(1) as u16);
        }

        // for (w, c) in max_widths.iter_mut().zip(self.columns.iter()) {                        ..
        //     let name_width = c.name.width() as u16;                                           ..
        //     if *w != 0 {                                                                      ..
        //         *w = (*w).max(name_width);                                                    ..
        //     }                                                                                 ..
        // }

        // todo: This is supposed to cover all invalidations but I'm not so certain
        let cursor_moved = self.cursor_moved;
        self.cursor_moved = None;
        if cursor_moved.is_none() && !dirty {
            return;
        }

        // needs: preferred_widths
        if dirty {
            self.update_width_limits();
        }
        #[cfg(debug_assertions)]
        log::debug!(
            "[update_table]: match_count={}, bottom={}, cursor={}, height={}, width={}, available_width={}",
            mc,
            self.bottom,
            self.cursor,
            self.height,
            self.width,
            self.available_width(),
        );

        // Section 3: Row-building algorithm

        // rows: Vec<Row<'static>> - actual row data for rendering
        // row_data lives on self (ResultsUI::row_data) and is written by
        // get_row via `row_data: None`.
        let mut rows: Vec<Row<'static>> = Vec::new();
        self.row_data.clear();

        let scroll_padding = self.scroll_padding();

        let mut idx = self.cursor as u32;

        // Step 1: Render cursor item
        let mut total_height = 0;
        if let Some(h) = self.get_row(
            self.bottom + idx,
            matcher,
            worker,
            selector,
            !self.cursor_disabled,
            active_column,
            Some((self.height, false)),
            &mut rows,
            None,
        ) {
            total_height = h;
        } else {
            log::error!("Unreachable: failed to render cursor row");
        }
        #[cfg(debug_assertions)]
        log::debug!("RENDER: AFTER ROWS");

        // Step 2: Build after_rows to ensure bottom scroll padding
        let mut after_rows: Vec<Row<'static>> = Vec::new();
        let mut after_row_data: Vec<(u32, u16)> = Vec::new();
        let mut after_height = 0u16;

        if scroll_padding > 0 {
            let mut idx = idx + 1;
            while after_height < scroll_padding && idx + self.bottom < mc {
                // Add separator if needed
                if let Some(cells) = self.hr_cells() {
                    after_rows.push(Row::new(cells).height(1));
                    after_row_data.push((u32::MAX, 1));
                    after_height += 1;
                }

                // Add item
                if let Some(h) = self.get_row(
                    self.bottom + idx,
                    matcher,
                    worker,
                    selector,
                    false,
                    active_column,
                    Some((scroll_padding.saturating_sub(after_height), self.reverse())),
                    &mut after_rows,
                    Some(&mut after_row_data),
                ) {
                    after_height += h;
                } else {
                    break;
                }

                idx += 1;
            }
        }

        #[cfg(debug_assertions)]
        log::debug!("RENDER: BEFORE ROWS");
        // Step 3: Fill before-cursor items
        let mut before_height = 0;
        let mut remaining_height = self.height.saturating_sub(total_height + after_height);

        while remaining_height > 0 {
            if idx > 0 {
                idx -= 1;
            } else if before_height < scroll_padding && self.bottom > 0 {
                self.bottom -= 1;
                self.cursor += 1;
                // keep adding
            } else {
                break;
            }

            // Check if we need to truncate
            let max_h =
                (remaining_height <= scroll_padding).then_some((remaining_height, !self.reverse()));

            // Add separator if needed
            if let Some(cells) = self.hr_cells() {
                rows.push(Row::new(cells));
                self.row_data.push((u32::MAX, 1));
                before_height += 1;
                remaining_height = remaining_height.saturating_sub(1);

                if remaining_height == 0 {
                    break;
                }
            }

            // Add item
            if let Some(h) = self.get_row(
                self.bottom + idx,
                matcher,
                worker,
                selector,
                false,
                active_column,
                max_h,
                &mut rows,
                None,
            ) {
                before_height += h;
                remaining_height = remaining_height.saturating_sub(h);
            } else {
                break;
            }
        }

        rows.reverse();
        self.row_data.reverse();

        // Step 5: Set bottom to new screen bottom
        if remaining_height == 0 {
            // Screen full: find lowest index in rows and adjust bottom/cursor
            if let Some(lowest_idx) = self
                .row_data
                .iter()
                .filter_map(|(i, _)| (*i != u32::MAX).then_some(*i))
                .next()
                && lowest_idx > self.bottom
            {
                let delta = lowest_idx - self.bottom;
                self.bottom += delta;
                self.cursor -= delta as u16;
            }

            // Append after_rows
            rows.extend(after_rows);
            self.row_data.extend(after_row_data);
        } else {
            // pop possibly truncated rows, leaving the maybe_separator
            if after_height == scroll_padding && scroll_padding > 0 && !self.width_limits.is_empty()
            {
                let last_item_idx = after_row_data.last().unwrap().0;
                while after_row_data
                    .last()
                    .is_some_and(|(i, _)| *i == last_item_idx)
                {
                    after_rows.pop();
                    let popped_height = after_row_data.pop().unwrap().1;
                    remaining_height += popped_height;
                }
            } else {
                // ensure after_row_data ends with maybe_separator
                if let Some(cells) = self.hr_cells() {
                    rows.push(Row::new(cells).height(1));
                    self.row_data.push((u32::MAX, 1));
                }
            }

            // Compute after_idx
            idx = after_row_data
                .iter()
                .rev()
                .find(|(idx, _)| *idx != u32::MAX)
                .map(|(idx, _)| idx + 1 - self.bottom)
                .unwrap_or(self.cursor as u32 + 1);

            #[cfg(debug_assertions)]
            log::debug!(
                "RENDER: FILLING ROWS AFTER {} + {} (after) items, from INDEX {}",
                rows.len(),
                after_rows.len(),
                idx,
            );

            // Append after_rows to rows
            rows.extend(after_rows);
            self.row_data.extend(after_row_data);

            while remaining_height > 0 && self.bottom + idx < mc {
                // Check if we need to truncate
                let max_h = if remaining_height <= scroll_padding {
                    Some((remaining_height, self.reverse()))
                } else {
                    None
                };

                // Add item
                if let Some(h) = self.get_row(
                    self.bottom + idx,
                    matcher,
                    worker,
                    selector,
                    false,
                    active_column,
                    max_h,
                    &mut rows,
                    None,
                ) {
                    remaining_height = remaining_height.saturating_sub(h);
                } else {
                    break;
                }

                // Add separator if needed and we have more items
                if remaining_height > 0
                    && idx + 1 < mc
                    && let Some(cells) = self.hr_cells()
                {
                    rows.push(Row::new(cells).height(1));
                    self.row_data.push((u32::MAX, 1));
                    remaining_height = remaining_height.saturating_sub(1);
                }

                idx += 1;
            }
        }

        #[cfg(debug_assertions)]
        log::debug!("RENDER: FILLED AFTER ROWS to {} TOTAL", rows.len());

        // Section 5.5: Compute preferred widths for next pass from collected data

        // if we needed redraw table, its because row changed
        self.row_cache.swap(0, 1);
        self.row_cache[1].clear();

        // Recompute preferred widths when the row layout is known to have
        // changed (cursor moved, fresh table) or when we don't have valid
        // width limits yet (first pass after a resize). Returns `true` if
        // the new preferred widths differ from the current ones, in which
        // case the width limits need to be recomputed.
        let preferred_widths_changed = if cursor_moved.is_some()
            || self.preferred_widths.is_empty()
            || self.width_limits.is_empty()
        {
            self.update_preferred_widths()
        } else {
            false
        };

        #[cfg(debug_assertions)]
        log::debug!(
            "[update_table]: recomputed preferred={:?}, current width_limits={:?}",
            self.preferred_widths,
            self.width_limits
        );

        if rows.is_empty() {
            // update rendered table next pass using preferred widths gathered this pass
            return;
        }

        // Section 7: Table assembly & reversing.
        // Convert collected items into the final flattened row list, reversing row ordering
        // if `reverse = true`. All styling is already applied to rows inside `get_row`.
        let mut final_rows: Vec<Row> = rows;

        if self.reverse() {
            final_rows.reverse();
            if remaining_height > 0 {
                final_rows.insert(0, Row::new(vec![vec![]]).height(remaining_height));
            }
        }

        // Section 8: Final Table layout construction.
        // Update self.widths to new_width_limits, incrementing the first nonzero column
        // with the indentation, and build the `Table` widget.
        if !self.config.stacked_columns {
            self.widths = self
                .width_limits
                .iter()
                .cloned()
                .filter(|x| *x != 0)
                .collect();
            if !self.widths.is_empty() {
                self.widths[0] += self.indentation() as u16;
            }
        } else {
            self.widths = vec![self.width];
        }

        if preferred_widths_changed {
            self.width_limits.clear();
        }

        let mut table = Table::new(final_rows, self.widths.clone())
            .column_spacing(self.config.column_spacing.0);

        table = table.block(self.config.border.as_static_block());

        if matches!(self.config.row_connection, RowConnectionStyle::Full) {
            table = table.style(self.config.style)
        }
        self.table = table;
    }
}
