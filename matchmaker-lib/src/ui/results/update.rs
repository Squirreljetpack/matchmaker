use crate::ui::ResultsUI;
use ratatui::widgets::{Row, Table};

use crate::{
    SSS, Selection, Selector,
    nucleo::{Worker, new_snapshot},
    render::Click,
};

impl ResultsUI {
    pub fn update_table<T: SSS, D>(
        &mut self,
        active_column: usize,
        worker: &mut Worker<T, D>,
        selector: &mut Selector<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
        click: &mut Click,
    ) {
        // Step 0: Refresh the nucleo snapshot and status before rendering
        let (_snapshot, status) = new_snapshot(&mut worker.nucleo);
        let old_mc = self.matched_count;
        let mc = status.matched_count;
        self.matched_count = mc;
        self.status = status;

        log::debug!(
            "[update_table] start: match_count={}, bottom={}, cursor={}, height={}, width={}, available_width={}",
            mc,
            self.bottom,
            self.cursor,
            self.height,
            self.width,
            self.available_width(),
        );

        // Section 1: Boundaries alignment, update width limits, early returns
        // Ensure cursor is within matched bounds, and update scroll position if bounds changed.
        if mc == 0 {
            // todo: or clear?
            return;
        }
        if mc < self.bottom + self.cursor as u32 && !self.cursor_disabled {
            self.cursor_jump(mc);
        } else {
            self.cursor = self.cursor.min(mc.saturating_sub(1) as u16);
        }

        if !self.preferred_widths.is_empty() && self.width_limits.is_empty() {
            self.update_width_limits();
        }
        // minimum widths from header
        // for (w, c) in max_widths.iter_mut().zip(self.columns.iter()) {
        //     let name_width = c.name.width() as u16;
        //     if *w != 0 {
        //         *w = (*w).max(name_width);
        //     }
        // }

        // todo: This is supposed to cover all invalidations but I'm not so certain
        let cursor_moved = self.cursor_moved;
        if cursor_moved.is_none() && !self.status.changed && !self.row_cache[0].is_empty() {
            return;
        }
        self.cursor_moved = None;

        // Section 3: Row-building algorithm

        // rows: Vec<Row<'static>> - actual row data for rendering
        // row_data: Vec<(u32, u16)> - metadata (item_idx, height)
        // item_idx is u32::MAX for separator rows
        let mut rows: Vec<Row<'static>> = Vec::new();
        let mut row_data: Vec<(u32, u16)> = Vec::new();

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
            &mut row_data,
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
                    &mut after_row_data,
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
                row_data.push((u32::MAX, 1));
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
                &mut row_data,
            ) {
                before_height += h;
                remaining_height = remaining_height.saturating_sub(h);
            } else {
                break;
            }
        }

        rows.reverse();
        row_data.reverse();

        // Step 5: Set bottom to new screen bottom
        if remaining_height == 0 {
            // Screen full: find lowest index in rows and adjust bottom/cursor
            if let Some(lowest_idx) = row_data
                .iter()
                .filter_map(|(i, _)| (*i != u32::MAX).then_some(*i))
                .next()
                && lowest_idx > self.bottom
            {
                let delta = lowest_idx - self.bottom;
                if delta < self.height as u32 {
                    self.bottom += delta;
                    self.cursor -= delta as u16;
                } else {
                    log::error!(
                        "Unexpected large delta: bottom={} cursor={} lowest={}",
                        self.bottom,
                        self.cursor,
                        lowest_idx
                    );
                }
            }

            // Append after_rows
            rows.extend(after_rows);
            row_data.extend(after_row_data);
        } else {
            // pop possibly truncated rows, leaving the maybe_separator
            if after_height == scroll_padding && scroll_padding > 0 {
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
                    row_data.push((u32::MAX, 1));
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
            row_data.extend(after_row_data);

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
                    &mut row_data,
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
                    row_data.push((u32::MAX, 1));
                    remaining_height = remaining_height.saturating_sub(1);
                }

                idx += 1;
            }
        }

        #[cfg(debug_assertions)]
        log::debug!("RENDER: FILLED AFTER ROWS to {} TOTAL", rows.len());

        // Section 5.5: Compute preferred widths for next pass from collected data
        self.row_cache.swap(0, 1);
        self.row_cache[1].clear();

        if self.cursor_moved.is_some() || mc != old_mc || self.width_limits.is_empty() {
            self.update_preferred_widths();
        }

        // Section 6: Click mapping.
        // Map visual mouse clicks back to absolute item indices by accumulating row heights.
        if let Click::ResultPos(c) = click {
            let c = if self.reverse() {
                self.height.saturating_sub(*c).saturating_sub(1)
            } else {
                *c
            };

            let mut acc_height = 0;
            for &(idx, h) in &row_data {
                if idx != u32::MAX && acc_height <= c && c < acc_height + h {
                    //log::debug!("Mapped click position to index: {c} -> {idx}");
                    *click = Click::ResultIdx(idx);
                    break;
                }
                acc_height += h;
            }
        }

        if let Click::ResultPos(_c) = click {
            *click = Click::ResultIdx(idx);
        }

        // Section 7: Table assembly & reversing.
        // Convert collected items into the final flattened row list, reversing row ordering
        // if `reverse = true`. All styling is already applied to rows inside `get_row`.
        let mut final_rows: Vec<Row> = rows;

        //log::debug!(
        //    "[update_table] assembled final_rows len={}",
        //    final_rows.len()
        //);

        if self.reverse() {
            final_rows.reverse();
            let remaining_space = self.height.saturating_sub(total_height);
            // log::debug!(
            //     "[update_table] reverse mode remaining_space={}",
            //     remaining_space
            // );
            if remaining_space > 0 {
                final_rows.insert(0, Row::new(vec![vec![]]).height(remaining_space));
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

        let mut table = Table::new(final_rows, self.widths.clone())
            .column_spacing(self.config.column_spacing.0);

        table = table.block(self.config.border.as_static_block());
        self.table = table;
    }
}
