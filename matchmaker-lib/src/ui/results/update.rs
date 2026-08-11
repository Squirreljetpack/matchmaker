use crate::{config::RowConnectionStyle, ui::ResultsUI};
use cba::_info;
use ratatui::widgets::{Row, Table};

use crate::{
    SSS, Selector,
    nucleo::{Worker, new_snapshot},
};

impl ResultsUI {
    // this is supposed to cover all invalidations
    // Requirements: new snapshots are requested only by update_table
    fn is_clean(&mut self) -> bool {
        let dirty = self.changed.iter().any(|x| *x)
            || self.cursor_moved.is_some()
            || self.row_cache[0].is_empty()
            || self.needs_new_width_limits();

        self.cursor_moved = None;
        self.changed[0] = false;
        self.changed[2] = false;
        self.changed[3] = false;

        !dirty
    }

    pub(crate) fn needs_new_width_limits(&self) -> bool {
        self.changed[2] || self.width_limits.is_empty()
    }

    pub fn update_table<T: SSS, D: 'static>(
        &mut self,
        worker: &mut Worker<T, D>,
        selector: &Selector,
        matcher: &mut nucleo::Matcher,
    ) {
        debug_assert!(
            !worker.columns.is_empty() && (self.hidden_columns.mask_len() == worker.columns.len())
        );
        // Step 0: Refresh the nucleo snapshot and status before rendering
        let (_snapshot, status) = new_snapshot(&mut worker.nucleo);

        let mc = status.matched_count;
        if status.changed || mc != self.status.matched_count {
            // this can trigger during ingestion or resort.
            self.changed[0] = true;
        }
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

        if self.needs_new_width_limits() {
            self.update_width_limits();
        }

        if self.cursor_moved.is_some() {
            self.bottom_clip = 0;
        }
        if self.is_clean() {
            return;
        }

        _info!(
            "[update_table]";
            mc;
            self.bottom;
            self.cursor;
            self.height;
            self.width;
            self.available_width();
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
        if let Some((h, _truncated)) = self.get_row(
            self.bottom + idx,
            matcher,
            worker,
            selector,
            !self.cursor_disabled,
            (self.height, false),
            &mut rows,
            None,
        ) {
            total_height = h;
        } else {
            log::error!("Unreachable: failed to render cursor row");
        }
        _info!("RENDER: AFTER ROWS");

        // Step 2: Build after_rows to ensure bottom scroll padding
        let mut after_rows: Vec<Row<'static>> = Vec::new();
        let mut after_row_data: Vec<(u32, u16)> = Vec::new();
        let mut after_height = 0u16;
        let mut after_idx = idx + 1;
        let mut after_truncated = false;

        if scroll_padding > 0 && total_height < self.height {
            while after_height < scroll_padding && idx + self.bottom < mc {
                // Add separator if needed
                if let Some(cells) = self.hr() {
                    after_rows.push(Row::new(cells).height(1));
                    after_row_data.push((u32::MAX, 1));
                    after_height += 1;
                }

                // Add item
                if let Some((h, truncated)) = self.get_row(
                    self.bottom + after_idx,
                    matcher,
                    worker,
                    selector,
                    false,
                    (scroll_padding.saturating_sub(after_height), self.reverse()),
                    &mut after_rows,
                    Some(&mut after_row_data),
                ) {
                    after_height += h;
                    after_truncated = truncated;
                } else {
                    break;
                }

                after_idx += 1;
            }
        }

        // Step 3: Fill before-cursor items
        let mut before_height = 0;
        let mut remaining_height = self.height.saturating_sub(total_height + after_height);
        _info!("RENDER: BEFORE ROWS with remaining height": remaining_height);

        while remaining_height > 0 {
            let mut max_h = remaining_height;

            if idx > 0 {
                idx -= 1;
                if idx == 0 {
                    if self.bottom_clip > 0 {
                        _info!(self.bottom_clip);
                        max_h = self.bottom_clip
                    }
                }
            } else if before_height < scroll_padding && self.bottom > 0 {
                self.bottom -= 1;
                self.cursor += 1;
                after_idx += 1;
                max_h = max_h.min(scroll_padding.saturating_sub(before_height))
                // keep adding
            } else {
                break;
            }

            // Add separator if needed
            if let Some(cells) = self.hr() {
                rows.push(Row::new(cells));
                self.row_data.push((u32::MAX, 1));
                before_height += 1;
                remaining_height = remaining_height.saturating_sub(1);

                if remaining_height == 0 {
                    break;
                }
            }

            // Add item
            if let Some((h, truncated)) = self.get_row(
                self.bottom + idx,
                matcher,
                worker,
                selector,
                false,
                (max_h, !self.reverse()),
                &mut rows,
                None,
            ) {
                if truncated && before_height < scroll_padding {
                    self.bottom_clip = h;
                }

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
            // pop last truncated row, leaving the maybe_separator
            if after_truncated {
                let last_item_idx = after_row_data.last().unwrap().0;
                let mut removed = false;

                while after_row_data
                    .last()
                    .is_some_and(|(i, _)| *i == last_item_idx)
                {
                    removed = true;
                    after_rows.pop();
                    let popped_height = after_row_data.pop().unwrap().1;
                    remaining_height += popped_height;
                }

                if removed {
                    after_idx -= 1;
                }
            } else {
                // ensure after_row_data ends with maybe_separator
                if let Some(cells) = self.hr() {
                    rows.push(Row::new(cells).height(1));
                    self.row_data.push((u32::MAX, 1));
                }
            }

            // Find the next idx after after_row_data
            idx = after_idx;

            _info!(
                "RENDER: FILLING ROWS AFTER": rows.len();
                " + after": after_rows.len();
                "from INDEX ": idx;
            );

            // Append after_rows to rows
            rows.extend(after_rows);
            self.row_data.extend(after_row_data);

            while remaining_height > 0 && self.bottom + idx < mc {
                // Check if we need to truncate
                let max_h = (remaining_height, self.reverse());

                // Add item
                if let Some((h, _truncated)) = self.get_row(
                    self.bottom + idx,
                    matcher,
                    worker,
                    selector,
                    false,
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
                    && let Some(cells) = self.hr()
                {
                    rows.push(Row::new(cells).height(1));
                    self.row_data.push((u32::MAX, 1));
                    remaining_height = remaining_height.saturating_sub(1);
                }

                idx += 1;
            }
        }

        _info!(rows.len());

        // Section 5.5: Compute preferred widths for next pass from collected data

        let wrap_condition = if self.is_wrap() {
            self.row_cache[0].iter().any(|(_, _, widths)| {
                widths
                    .iter()
                    .zip(&self.preferred_widths)
                    .any(|(&w, &pref_w)| pref_w == 0 && w > 0)
            })
        } else {
            self.changed[3]
        };
        // Recompute preferred widths when the row layout is known to have
        // changed or when we don't have valid
        // width limits yet (first pass after a resize). Returns `true` if
        // the new preferred widths differ from the current ones, in which
        // case the width limits need to be recomputed.
        if self.changed[1] || self.preferred_widths.is_empty() || wrap_condition {
            if self.try_apply_max_widths() {
                // Applied exact max widths directly to preferred_widths, width_limits, and widths
            } else if self.update_preferred_widths() {
                _info!(
                    "[update_preferred]";
                    self.preferred_widths;
                    self.width_limits;
                    self.hidden_columns;
                    self.changed[1];
                    self.changed[3];
                );
                self.changed[2] = true;
            }
        };
        self.changed[1] = false;

        // if we needed redraw table, its because row changed
        self.row_cache.swap(0, 1);
        self.row_cache[1].clear();

        if rows.is_empty() || self.needs_new_width_limits() {
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
        let mut table = Table::new(final_rows, self.widths.clone())
            .column_spacing(self.config.column_spacing.0);

        table = table.block(self.config.border.as_static_block());

        if matches!(self.config.row_connection, RowConnectionStyle::Full) {
            table = table.style(self.config.style)
        }
        self.table = table;
    }
}
