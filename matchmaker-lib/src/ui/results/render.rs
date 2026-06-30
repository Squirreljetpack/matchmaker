use super::*;
use crate::{
    SSS,
    config::AutoscrollSettings,
    nucleo::{Style, Text, Worker, render_item::render_cell},
    ui::ResultsUI,
    utils::text::{to_static, truncation_indicator, wrap_text_static},
};

/// Renders a single item into styled table cells.
///
/// Formats, highlights, wraps, clips, and scrolls the visible columns of a matched
/// nucleo item, producing one `Text` cell per visible column.
///
/// ## Parameters
///
/// ### Item
/// - `item`: Matched item to render.
/// - `snapshot`: Nucleo snapshot containing match information.
/// - `columns`: Column definitions.
///
/// ### Layout
/// - `width_limits`: Maximum width for each column. A width of `0` hides that column.
///   Columns beyond `width_limits.len()` are treated as having unlimited width.
/// - `max_height`: Maximum lines per cell (`0` = unlimited).
/// - `hidden_cols`: Explicit visibility mask for columns.
/// - `stacked`: Whether columns should be rendered vertically instead of side-by-side.
///
/// ### Rendering
/// - `wrap`: Whether text exceeding a width limit should wrap.
/// - `highlight_style`: Style applied to matched query characters.
/// - `autoscroll`: Horizontal autoscroll configuration.
/// - `hscroll_offset`: Manual horizontal scroll offset.
/// - `vscroll_offset`: Number of rendered lines to skip from the top.
///
/// ### Scratch buffers
/// - `matcher`: Matcher used to compute highlight indices.
/// - `col_indices_buffer`: Temporary buffer reused for match indices.
///
/// ### Statistics
/// - `raw_widths`: Receives the unwrapped widths of visible columns.
/// - `max_widths`: Updated with the maximum rendered width seen for each column.
///
/// ## Mutations
///
/// - `matcher` is mutated while computing match indices.
/// - `col_indices_buffer` is cleared and reused as scratch space.
/// - `raw_widths` is appended with raw column widths.
/// - `max_widths` is updated with rendered column widths.
///
/// ## Returns
///
/// - `None` if the item does not exist.
/// - `Some((cells, data))`, where:
///   - `cells` contains one rendered cell per visible column.
///   - `cells` may be empty if vertical scrolling removes every visible line.
///   - `data` is a reference to the item's underlying data.
///
/// ## Guarantees
///
/// - `cells` is either empty or contains exactly one entry for every visible column.
/// - Cells preserve the order of visible columns.
/// - Every cell has been formatted, highlighted, and wrapped or clipped as requested.
///
/// ## Caller responsibilities
///
/// - Refresh the snapshot before calling.
/// - Decide whether to display empty rows.
/// - Apply row-level styling (selection, cursor, connection modes).
/// - Assemble rendered rows into the final table.
pub fn render_row<T: SSS, D>(
    item: &nucleo::Item<T>,
    worker: &Worker<T, D>,

    width_limits: &[u16],
    hidden_cols: &[bool],
    stacked: bool,
    max_height: usize,

    wrap: bool,
    highlight_style: Style,
    autoscroll: AutoscrollSettings,
    hscroll_offset: i8,
    vscroll_offset: usize,

    matcher: &mut nucleo::Matcher,
    col_indices_buffer: &mut Vec<u32>,

    mut width_callback: impl FnMut(usize, usize), // col, width
) -> Vec<Text<'static>> {
    // Section 2: Vertical scroll offset line skipping and max height truncation
    // For each column, format the text and apply vertical scrolling:
    // - `to_skip` tracks how many lines to skip from the top (vscroll_offset)
    // - `skip` tracks whether any visible lines remain after scrolling
    // - Columns where width_limits[i] == 0 are hidden entirely
    // - Columns where i >= width_limits.len() are rendered with no width constraint
    let mut to_skip = vscroll_offset;
    let mut skip = true; // assume no visible lines until proven otherwise
    let mut row_candidates = vec![];
    let columns = &worker.columns;
    let snapshot = worker.nucleo.snapshot();
    let d = (worker.text_preprocessor)(item.data);

    for (i, c) in columns.iter().enumerate() {
        if hidden_cols.get(i).is_some_and(|x| *x) {
            continue;
        }

        let mut t = c.format(item.data, &d);
        let w = t.width();
        width_callback(i, w);

        if stacked {
            // In stacked mode, columns are rendered vertically one after another.
            // Skip entire columns until we've consumed the vscroll_offset.
            if to_skip >= t.height() {
                to_skip -= t.height();
                t.lines.clear(); // column is entirely scrolled away
            } else {
                skip = false; // found visible content
                t.lines.drain(..to_skip); // skip partial lines in this column
                to_skip = 0;
                // Apply max_height truncation if configured
                if max_height > 0 && t.height() > max_height {
                    t.lines.truncate(max_height);
                    if let Some(last_line) = t.lines.last_mut() {
                        last_line.spans.push(truncation_indicator());
                    }
                }
            }
        } else {
            // In normal mode, all columns are rendered side-by-side.
            // Apply vscroll_offset to skip lines from the top of each column.
            if t.height() > to_skip {
                skip = false; // found visible content
                t.lines.drain(..to_skip);

                // Apply max_height truncation if configured
                if max_height > 0 && t.height() > max_height {
                    t.lines.truncate(max_height);
                    if let Some(last_line) = t.lines.last_mut() {
                        last_line.spans.push(truncation_indicator());
                    }
                }
            } else {
                t.lines.clear();
            }
        }
        row_candidates.push(t);
    }

    // If skip is true, no visible lines were found after applying vscroll.
    // Return an empty cells vector so the caller can decide whether to show
    // an empty row (if config.show_skipped) or skip entirely.
    if skip {
        return vec![];
    }

    // Section 3: Cell rendering, match highlighting, wrapping, and width collection
    // For each visible column, apply match highlighting, wrapping, and horizontal scrolling.
    //
    // Width limits:
    // - Columns where width_limits[i] != 0 use that width limit
    // - Columns where i >= width_limits.len() use u16::MAX (no wrapping)
    // - The width limit controls wrapping and/or clipping behavior

    // Determine which columns are visible and their width limits
    let mut visible_cols = vec![];
    for (i, _) in columns.iter().enumerate() {
        if i < width_limits.len() {
            if width_limits[i] != 0 {
                visible_cols.push((i, width_limits[i]));
            }
        } else {
            // Columns beyond width_limits are rendered with no constraint
            visible_cols.push((i, u16::MAX));
        }
    }

    let row: Vec<Text<'static>> = row_candidates
        .into_iter()
        .zip(visible_cols.iter())
        .map(|(cell, &(col_idx, width_limit))| {
            let column = &columns[col_idx];

            // Apply rendering based on column type and settings
            let cell = if column.filter() {
                // Filterable columns get match highlighting
                let (t, _) = render_cell(
                    cell,
                    col_idx,
                    snapshot,
                    item,
                    matcher,
                    highlight_style,
                    wrap,
                    width_limit,
                    col_indices_buffer,
                    autoscroll,
                    hscroll_offset,
                );
                t
            } else if wrap && width_limit != u16::MAX {
                // Non-filter columns with wrapping enabled
                let (cell, _) = wrap_text_static(&cell, width_limit);
                cell
            } else {
                // Non-filter columns without wrapping - just use as-is
                to_static(&cell)
            };
            #[cfg(debug_assertions)]
            if col_idx == 0 {
                log::trace!("new row col 1: {:?}, limit: {}", &cell, width_limit);
            }

            cell
        })
        .collect();

    row
}

impl ResultsUI {
    /// Formats, styles, and prefixes the row cells for a single item, then pushes to flat_rows.
    ///
    /// ### Parameters:
    /// - `max_height`: If provided, truncates the row to this height. Uses `!self.reverse()` to truncate from the appropriate end.
    /// - `rows`: Rows are pushed directly here
    ///
    /// ### Requires:
    ///   self.width_limits are updated.
    ///
    /// ### Returns:
    /// - `Some(height)` if rows were successfully pushed (height is the total height added)
    /// - `None` if the item couldn't be rendered
    pub(super) fn get_row<T: SSS, D>(
        &mut self,
        idx: u32,
        matcher: &mut nucleo::Matcher,
        worker: &Worker<T, D>,
        // post_render styling options
        selector: &mut Selector<T, impl Selection>,
        is_current: bool,
        active_column: usize,
        max_height: Option<(u16, bool)>,
        // output
        rows: &mut Vec<Row<'static>>,
        row_data: &mut Vec<(u32, u16)>,
    ) -> Option<u16> {
        let vscroll_offset = self.vscroll_to_skip(is_current);
        let stacked = self.config.stacked_columns;

        let item = worker.nucleo.snapshot().get_item(idx)?;
        let id = selector.id(item.data);
        let mut row_widths = vec![0u16; self.v_cols()];

        // check cache
        let cached = if id == u32::MAX {
            None
        } else {
            self.row_cache[0]
                .iter()
                .find(|(idx, _, _)| *idx == id)
                .cloned()
        };

        let texts = if let Some(cached) = cached {
            self.row_cache[1].push(cached.clone());

            cached.1
        } else {
            let mut non_hidden_idx = 0;
            let width_callback = |_: usize, w: usize| {
                if non_hidden_idx < row_widths.len() {
                    row_widths[non_hidden_idx] = w as u16;
                    non_hidden_idx += 1;
                }
            };
            let texts = render_row(
                &item,
                worker,
                &self.width_limits,
                &self.hidden_columns,
                stacked,
                self.config.max_height,
                self.config.wrap,
                self.config.match_style.into(),
                self.config.autoscroll,
                self.hscroll,
                vscroll_offset,
                matcher,
                &mut self.col_indices_buffer,
                width_callback,
            );

            if texts.is_empty() {
                if self.config.show_skipped {
                    rows.push(Row::default().height(1));
                    row_data.push((idx, 1));
                    return Some(1);
                } else {
                    return None;
                }
            }

            if id != u32::MAX {
                self.row_cache[1].push((id, texts.clone(), row_widths));
            }

            texts
        };

        let is_selected = selector.contains(item.data);
        let prefix = if is_selected {
            self.config.multi_prefix.clone()
        } else {
            self.default_prefix((idx - self.bottom) as usize)
        };

        let mut row_texts = vec![];
        if self.width_limits.is_empty() {
            return Some(0); // wait for update
        } else {
            for (i, (col_idx, mut col)) in self
                .hidden_columns
                .iter()
                .cloned()
                .enumerate()
                .filter(|h| !h.1)
                .map(|h| h.0)
                .zip(texts)
                .enumerate()
            {
                if i == 0 || stacked {
                    prefix_span(
                        &mut col,
                        prefix.clone(),
                        self.config.prefix_style,
                        self.config.prefix_inactive_style,
                        is_current,
                    );
                }

                let col = style_text(col, active_column == col_idx, is_current, &self.config);
                row_texts.push(col);
            }
        }

        if !stacked && self.config.right_align_last && row_texts.len() > 1 {
            row_texts.last_mut().unwrap().alignment = Some(ratatui::layout::Alignment::Right);
        }

        // Apply truncation if max_height is specified
        if let Some((max_h, from_end)) = max_height {
            for text in &mut row_texts {
                crate::utils::text::take_lines(text, max_h, from_end);
            }
        }

        // Determine row-level styling based on connection style and current row state
        let row_style = match (is_current, self.config.row_connection) {
            (true, RowConnectionStyle::Full) => self.config.current_style,
            (true, RowConnectionStyle::Capped) => self.config.inactive_current_style,
            _ => StyleSetting::DEFAULT,
        }
        .into_style_no_submodifiers();

        if !stacked {
            // Non-stacked mode: single row with all cells
            let height = row_texts
                .iter()
                .map(|t| t.height() as u16)
                .max()
                .unwrap_or_default();

            debug_row(&row_texts);

            rows.push(Row::new(row_texts).height(height).style(row_style));
            row_data.push((idx, height));
            Some(height)
        } else {
            // Stacked mode: split into multiple rows, one per cell
            let mut total_height = 0u16;
            for cell in row_texts {
                let h = cell.height() as u16;
                rows.push(Row::new(vec![cell]).height(h).style(row_style));
                row_data.push((idx, h));
                total_height += h;
            }
            Some(total_height)
        }
    }
}

// helpers
impl ResultsUI {
    pub(super) fn default_prefix(&self, i: usize) -> String {
        let substituted = substitute_escaped(
            &self.config.default_prefix,
            &[
                ('d', &(i + 1).to_string()),                        // cursor index
                ('r', &(i + 1 + self.bottom as usize).to_string()), // absolute index
            ],
        );

        fit_width(&substituted, self.indentation())
    }

    pub(super) fn hr_cells(&self) -> Option<Vec<ratatui::text::Text<'static>>> {
        let sep = self.config.separator;

        if matches!(sep, HorizontalSeparator::None) {
            return None;
        }

        let unit = sep.as_str();
        let line = unit.repeat(self.width as usize);

        if !self.config.stacked_columns && self.widths.len() > 1 {
            Some(vec![ratatui::text::Text::raw(line); self.widths().len()])
        } else {
            Some(vec![ratatui::text::Text::raw(line)])
        }
    }

    pub(super) fn _hr(&self) -> u16 {
        !matches!(self.config.separator, HorizontalSeparator::None) as u16
    }

    pub(super) fn vscroll_to_skip(&self, is_current: bool) -> usize {
        if !self.config.vscroll_current_only || is_current {
            self.vscroll as usize
        } else {
            0
        }
    }
}
fn style_text<'a>(
    mut t: ratatui::text::Text<'a>,
    is_active_col: bool,
    is_current_row: bool,
    config: &ResultsConfig,
) -> ratatui::text::Text<'a> {
    match config.row_connection {
        RowConnectionStyle::Disjoint => {
            if is_active_col {
                t = t.patch_style(
                    if is_current_row {
                        config.current_style
                    } else {
                        config.style
                    }
                    .into_style_no_submodifiers(),
                );
            } else {
                t = t.patch_style(
                    if is_current_row {
                        config.inactive_current_style
                    } else {
                        config.inactive_style
                    }
                    .into_style_no_submodifiers(),
                );
            }
        }
        RowConnectionStyle::Capped => {
            if is_active_col {
                t = t.patch_style(
                    if is_current_row {
                        config.current_style
                    } else {
                        config.style
                    }
                    .into_style_no_submodifiers(),
                );
            }
        }
        RowConnectionStyle::Full => {}
    }
    t
}
