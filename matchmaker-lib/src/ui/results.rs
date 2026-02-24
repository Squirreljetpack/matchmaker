use std::str::FromStr;

use cli_boilerplate_automation::bring::split::split_on_nesting;
use ratatui::{
    layout::{Alignment, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Row, Table},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    SSS, Selection, Selector,
    config::{HorizontalSeparator, ResultsConfig, RowConnectionStyle, StatusConfig},
    nucleo::{Status, Worker},
    render::Click,
    utils::{
        string::{fit_width, substitute_escaped},
        text::{clip_text_lines, expand_indents, prefix_text},
    },
};

#[derive(Debug)]
pub struct ResultsUI {
    cursor: u16,
    bottom: u16,
    col: Option<usize>,

    /// available height
    height: u16,
    /// available width
    width: u16,
    // column widths.
    // Note that the first width includes the indentation.
    widths: Vec<u16>,

    pub hidden_columns: Vec<bool>,

    pub status: Status,
    status_template: Line<'static>,
    pub status_config: StatusConfig,

    pub config: ResultsConfig,

    bottom_clip: Option<u16>,
    cursor_above: u16,

    pub cursor_disabled: bool,
}

impl ResultsUI {
    pub fn new(config: ResultsConfig, status_config: StatusConfig) -> Self {
        Self {
            cursor: 0,
            bottom: 0,
            col: None,

            widths: Vec::new(),
            height: 0, // uninitialized, so be sure to call update_dimensions
            width: 0,
            hidden_columns: Default::default(),

            status: Default::default(),
            status_template: Span::from(status_config.template.clone())
                .style(status_config.fg)
                .add_modifier(status_config.modifier)
                .into(),
            status_config,
            config,

            cursor_disabled: false,
            bottom_clip: None,
            cursor_above: 0,
        }
    }

    pub fn with_hidden_columns(mut self, hidden_columns: Vec<bool>) -> Self {
        self.hidden_columns = hidden_columns;
        self
    }

    // as given by ratatui area
    pub fn update_dimensions(&mut self, area: &Rect) {
        let [bw, bh] = [self.config.border.height(), self.config.border.width()];
        self.width = area.width.saturating_sub(bw);
        self.height = area.height.saturating_sub(bh);
        log::debug!("Updated results dimensions: {}x{}", self.width, self.height);
    }

    pub fn table_width(&self) -> u16 {
        self.config.column_spacing.0 * self.widths().len().saturating_sub(1) as u16
            + self.widths.iter().sum::<u16>()
            + self.config.border.width()
    }

    // ------ config -------
    pub fn reverse(&self) -> bool {
        self.config.reverse == Some(true)
    }
    pub fn is_wrap(&self) -> bool {
        self.config.wrap
    }
    pub fn wrap(&mut self, wrap: bool) {
        self.config.wrap = wrap;
    }

    // ----- columns --------
    // todo: support cooler things like only showing/outputting a specific column/cycling columns
    pub fn toggle_col(&mut self, col_idx: usize) -> bool {
        if self.col == Some(col_idx) {
            self.col = None
        } else {
            self.col = Some(col_idx);
        }
        self.col.is_some()
    }
    pub fn cycle_col(&mut self) {
        self.col = match self.col {
            None => self.widths.is_empty().then_some(0),
            Some(c) => {
                let next = c + 1;
                if next < self.widths.len() {
                    Some(next)
                } else {
                    None
                }
            }
        };
    }

    // ------- NAVIGATION ---------
    fn scroll_padding(&self) -> u16 {
        self.config.scroll_padding.min(self.height / 2)
    }
    pub fn end(&self) -> u32 {
        self.status.matched_count.saturating_sub(1)
    }

    /// Index in worker snapshot of current item.
    /// Use with worker.get_nth().
    //  Equivalently, the cursor progress in the match list
    pub fn index(&self) -> u32 {
        if self.cursor_disabled {
            u32::MAX
        } else {
            (self.cursor + self.bottom) as u32
        }
    }
    // pub fn cursor(&self) -> Option<u16> {
    //     if self.cursor_disabled {
    //         None
    //     } else {
    //         Some(self.cursor)
    //     }
    // }
    pub fn cursor_prev(&mut self) {
        if self.cursor_above <= self.scroll_padding() && self.bottom > 0 {
            self.bottom -= 1;
            self.bottom_clip = None;
        } else if self.cursor > 0 {
            self.cursor -= 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(self.end());
        }
    }
    pub fn cursor_next(&mut self) {
        if self.cursor_disabled {
            self.cursor_disabled = false
        }

        // log::trace!(
        //     "Cursor {} @ index {}. Status: {:?}.",
        //     self.cursor,
        //     self.index(),
        //     self.status
        // );
        if self.cursor + 1 + self.scroll_padding() >= self.height
            && self.bottom + self.height < self.status.matched_count as u16
        {
            self.bottom += 1; // 
        } else if self.index() < self.end() {
            self.cursor += 1;
        } else if self.config.scroll_wrap {
            self.cursor_jump(0)
        }
    }

    pub fn cursor_jump(&mut self, index: u32) {
        self.cursor_disabled = false;
        self.bottom_clip = None;

        let end = self.end();
        let index = index.min(end) as u16;

        if index < self.bottom || index >= self.bottom + self.height {
            self.bottom = (end as u16 + 1)
                .saturating_sub(self.height) // don't exceed the first item of the last self.height items
                .min(index);
            self.cursor = index - self.bottom;
        } else {
            self.cursor = index - self.bottom;
        }
    }

    // ------- RENDERING ----------
    pub fn indentation(&self) -> usize {
        self.config.multi_prefix.width()
    }
    pub fn col(&self) -> Option<usize> {
        self.col
    }

    /// Column widths.
    /// Note that the first width includes the indentation.
    pub fn widths(&self) -> &Vec<u16> {
        &self.widths
    }
    // results width
    pub fn width(&self) -> u16 {
        self.width.saturating_sub(self.indentation() as u16)
    }

    /// Adapt the stored widths (initialized by [`Worker::results`]) to the fit within the available width (self.width)
    pub fn max_widths(&self) -> Vec<u16> {
        if !self.config.wrap {
            return vec![];
        }

        let mut widths = vec![u16::MAX; self.widths.len()];

        let total: u16 = self.widths.iter().sum();
        if total <= self.width() {
            return vec![];
        }

        let mut available = self.width();
        let mut scale_total = 0;
        let mut scalable_indices = Vec::new();

        for (i, &w) in self.widths.iter().enumerate() {
            if w <= self.config.min_wrap_width {
                available = available.saturating_sub(w);
            } else if i < self.hidden_columns.len() && self.hidden_columns[i] {
                widths[i] = 0;
            } else {
                scale_total += w;
                scalable_indices.push(i);
            }
        }

        for &i in &scalable_indices {
            let old = self.widths[i];
            let new_w = old * available / scale_total;
            widths[i] = new_w.max(self.config.min_wrap_width);
        }

        // give remainder to the last scalable column
        if let Some(&last_idx) = scalable_indices.last() {
            let used_total: u16 = widths.iter().sum();
            if used_total < self.width() {
                widths[last_idx] += self.width() - used_total;
            }
        }

        widths
    }

    // this updates the internal status, so be sure to call make_status afterward
    // some janky wrapping is implemented, dunno whats causing flickering, padding is fixed going down only
    pub fn make_table<'a, T: SSS>(
        &mut self,
        worker: &'a mut Worker<T>,
        selector: &mut Selector<T, impl Selection>,
        matcher: &mut nucleo::Matcher,
        click: &mut Click,
    ) -> Table<'a> {
        let offset = self.bottom as u32;
        let end = (self.bottom + self.height) as u32;
        let hz = !self.config.stacked_columns;

        let width_limits = if hz {
            self.max_widths()
        } else {
            vec![
                if self.config.wrap {
                    self.width
                } else {
                    u16::MAX
                };
                worker.columns.len()
            ]
        };

        let (mut results, mut widths, status) =
            worker.results(offset, end, &width_limits, self.match_style(), matcher);

        let match_count = status.matched_count;
        self.status = status;

        if match_count < (self.bottom + self.cursor) as u32 && !self.cursor_disabled {
            self.cursor_jump(match_count);
        } else {
            self.cursor = self.cursor.min(results.len().saturating_sub(1) as u16)
        }

        widths[0] += self.indentation() as u16;

        let mut rows = vec![];
        let mut total_height = 0;

        if results.is_empty() {
            return Table::new(rows, widths);
        }

        let height_of = |t: &(Vec<ratatui::text::Text<'a>>, _)| {
            self._hr()
                + if hz {
                    t.0.iter()
                        .map(|t| t.height() as u16)
                        .max()
                        .unwrap_or_default()
                } else {
                    t.0.iter().map(|t| t.height() as u16).sum::<u16>()
                }
        };

        // log::debug!("results initial: {}, {}, {}, {}, {}", self.bottom, self.cursor, total_height, self.height, results.len());
        let h_at_cursor = height_of(&results[self.cursor as usize]);
        let h_after_cursor = results[self.cursor as usize + 1..]
            .iter()
            .map(height_of)
            .sum();
        let h_to_cursor = results[0..self.cursor as usize]
            .iter()
            .map(height_of)
            .sum::<u16>();
        let cursor_end_should_lt = self.height - self.scroll_padding().min(h_after_cursor);
        // let cursor_start_should_gt = self.scroll_padding().min(h_to_cursor);

        // log::debug!(
        //     "Computed heights: {h_at_cursor}, {h_to_cursor}, {h_after_cursor}, {cursor_end_should_lt}",
        // );
        // begin adjustment
        let mut start_index = 0; // the index in results of the first complete item

        if h_at_cursor >= cursor_end_should_lt {
            start_index = self.cursor;
            self.bottom += self.cursor;
            self.cursor = 0;
            self.cursor_above = 0;
            self.bottom_clip = None;
        } else
        // increase the bottom index so that cursor_should_above is maintained
        if let h_to_cursor_end = h_to_cursor + h_at_cursor
            && h_to_cursor_end > cursor_end_should_lt
        {
            let mut trunc_height = h_to_cursor_end - cursor_end_should_lt;
            // note that there is a funny side effect that scrolling up near the bottom can scroll up a bit, but it seems fine to me

            for r in results[start_index as usize..self.cursor as usize].iter_mut() {
                let h = height_of(r);
                let (row, item) = r;
                start_index += 1; // we always skip at least the first item

                if trunc_height < h {
                    let mut remaining_height = h - trunc_height;
                    let prefix = if selector.contains(item) {
                        self.config.multi_prefix.clone().to_string()
                    } else {
                        self.default_prefix(0)
                    };

                    total_height += remaining_height;

                    log::debug!("r: {remaining_height}");
                    if hz {
                        if h - self._hr() < remaining_height {
                            for (_, t) in
                                row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0)
                            {
                                clip_text_lines(t, h - remaining_height, !self.reverse());
                            }
                        }

                        prefix_text(&mut row[0], prefix);

                        let last_visible = widths
                            .iter()
                            .enumerate()
                            .rev()
                            .find_map(|(i, w)| (*w != 0).then_some(i));

                        let mut row_texts: Vec<_> = row
                            .iter()
                            .take(last_visible.map(|x| x + 1).unwrap_or(0))
                            .cloned()
                            .collect();

                        if self.config.right_align_last && row_texts.len() > 1 {
                            row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                        }

                        let row = Row::new(row_texts).height(remaining_height);
                        rows.push(row);
                    } else {
                        let mut push = vec![];

                        for col in row.into_iter().rev() {
                            let mut height = col.height() as u16;
                            if remaining_height == 0 {
                                break;
                            } else if remaining_height < height {
                                clip_text_lines(col, remaining_height, !self.reverse());
                                height = remaining_height;
                            }
                            remaining_height -= height;
                            prefix_text(col, prefix.clone());
                            push.push(Row::new(vec![col.clone()]).height(height));
                        }
                        rows.extend(push.into_iter().rev());
                    }

                    self.bottom += start_index - 1;
                    self.cursor -= start_index - 1;
                    self.bottom_clip = Some(remaining_height);
                    break;
                } else if trunc_height == h {
                    self.bottom += start_index;
                    self.cursor -= start_index;
                    self.bottom_clip = None;
                    break;
                }

                trunc_height -= h;
            }
        } else if let Some(mut remaining_height) = self.bottom_clip {
            start_index += 1;
            // same as above
            let h = height_of(&results[0]);
            let (row, item) = &mut results[0];
            let prefix = if selector.contains(item) {
                self.config.multi_prefix.clone().to_string()
            } else {
                self.default_prefix(0)
            };

            total_height += remaining_height;

            if hz {
                if self._hr() + remaining_height != h {
                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, remaining_height, !self.reverse());
                    }
                }

                prefix_text(&mut row[0], prefix);

                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                let row = Row::new(row_texts).height(remaining_height);
                rows.push(row);
            } else {
                let mut push = vec![];

                for col in row.into_iter().rev() {
                    let mut height = col.height() as u16;
                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        clip_text_lines(col, remaining_height, !self.reverse());
                        height = remaining_height;
                    }
                    remaining_height -= height;
                    prefix_text(col, prefix.clone());
                    push.push(Row::new(vec![col.clone()]).height(height));
                }
                rows.extend(push.into_iter().rev());
            }
        }

        // topside padding is non-flexible, and does its best to stay at 2 full items without obscuring cursor.
        // One option is we move enforcement from cursor_prev to

        let mut remaining_height = self.height.saturating_sub(total_height);

        for (mut i, (mut row, item)) in results.drain(start_index as usize..).enumerate() {
            i += self.bottom_clip.is_some() as usize;

            // this is technically one step out of sync but idc
            if let Click::ResultPos(c) = click
                && self.height - remaining_height > *c
            {
                let idx = self.bottom as u32 + i as u32 - 1;
                log::debug!("Mapped click position to index: {c} -> {idx}",);
                *click = Click::ResultIdx(idx);
            }
            if self.is_current(i) {
                self.cursor_above = self.height - remaining_height;
            }

            // insert hr
            if let Some(hr) = self.hr()
                && remaining_height > 0
            {
                rows.push(hr);
                remaining_height -= 1;
            }
            if remaining_height == 0 {
                break;
            }

            // set prefix
            let prefix = if selector.contains(item) {
                self.config.multi_prefix.clone().to_string()
            } else {
                self.default_prefix(i)
            };

            if hz {
                let mut height = row
                    .iter()
                    .map(|t| t.height() as u16)
                    .max()
                    .unwrap_or_default();

                if remaining_height < height {
                    height = remaining_height;

                    for (_, t) in row.iter_mut().enumerate().filter(|(i, _)| widths[*i] != 0) {
                        clip_text_lines(t, height, self.reverse());
                    }
                }
                remaining_height -= height;

                prefix_text(&mut row[0], prefix);

                // same as above
                let last_visible = widths
                    .iter()
                    .enumerate()
                    .rev()
                    .find_map(|(i, w)| (*w != 0).then_some(i));

                let mut row_texts: Vec<_> = row
                    .iter()
                    .take(last_visible.map(|x| x + 1).unwrap_or(0))
                    .cloned()
                    // highlight
                    .enumerate()
                    .map(|(x, t)| {
                        if self.is_current(i)
                            && (self.col.is_none()
                                && matches!(
                                    self.config.row_connection_style,
                                    RowConnectionStyle::Disjoint
                                )
                                || self.col == Some(x))
                        {
                            t.style(self.current_style())
                        } else {
                            t
                        }
                    })
                    .collect();

                if self.config.right_align_last && row_texts.len() > 1 {
                    row_texts.last_mut().unwrap().alignment = Some(Alignment::Right)
                }

                // push
                let mut row = Row::new(row_texts).height(height);

                if self.is_current(i)
                    && self.col.is_none()
                    && !matches!(
                        self.config.row_connection_style,
                        RowConnectionStyle::Disjoint
                    )
                {
                    row = row.style(self.current_style())
                }

                rows.push(row);
            } else {
                let mut push = vec![];

                for (x, mut col) in row.into_iter().enumerate() {
                    let mut height = col.height() as u16;

                    if remaining_height == 0 {
                        break;
                    } else if remaining_height < height {
                        height = remaining_height;
                        clip_text_lines(&mut col, remaining_height, self.reverse());
                    }
                    remaining_height -= height;

                    prefix_text(&mut col, prefix.clone());

                    // push
                    let mut row = Row::new(vec![col]).height(height);

                    if self.is_current(i) && (self.col.is_none() || self.col == Some(x)) {
                        row = row.style(self.current_style())
                    }

                    push.push(row);
                }
                rows.extend(push);
            }
        }

        if self.reverse() {
            rows.reverse();
            if remaining_height > 0 {
                rows.insert(0, Row::new(vec![vec![]]).height(remaining_height));
            }
        }

        // up to the last nonempty row position

        if hz {
            self.widths = {
                let pos = widths.iter().rposition(|&x| x != 0).map_or(0, |p| p + 1);
                let mut widths = widths[..pos].to_vec();
                if pos > 2 && self.config.right_align_last {
                    let used = widths.iter().take(widths.len() - 1).sum();
                    widths[pos - 1] = self.width().saturating_sub(used);
                }
                widths
            };
        }

        // why does the row highlight apply beyond the table width?
        let mut table = Table::new(
            rows,
            if hz {
                self.widths.clone()
            } else {
                vec![self.width]
            },
        )
        .column_spacing(self.config.column_spacing.0)
        .style(self.config.fg)
        .add_modifier(self.config.modifier);

        table = table.block(self.config.border.as_static_block());
        table
    }
}

impl ResultsUI {
    pub fn make_status(&self, full_width: u16) -> Paragraph<'_> {
        let status_config = &self.status_config;
        let replacements = [
            ('r', self.index().to_string()),
            ('m', self.status.matched_count.to_string()),
            ('t', self.status.item_count.to_string()),
        ];

        // sub replacements into line
        let mut new_spans = Vec::new();

        if status_config.match_indent {
            new_spans.push(Span::raw(" ".repeat(self.indentation())));
        }

        for span in &self.status_template {
            let subbed = substitute_escaped(&span.content, &replacements);
            new_spans.push(Span::styled(subbed, span.style));
        }

        let substituted_line = Line::from(new_spans);

        // sub whitespace expansions
        let effective_width = match self.status_config.row_connection_style {
            RowConnectionStyle::Full => full_width,
            _ => self.width,
        } as usize;
        let expanded = expand_indents(substituted_line, r"\s", effective_width)
            .style(status_config.fg)
            .add_modifier(status_config.modifier);

        Paragraph::new(expanded)
    }

    pub fn set_status_line(&mut self, template: Option<Line<'static>>) {
        let status_config = &self.status_config;

        self.status_template = template
            .unwrap_or(status_config.template.clone().into())
            .style(status_config.fg)
            .add_modifier(status_config.modifier)
            .into()
    }
}

// helpers
impl ResultsUI {
    fn default_prefix(&self, i: usize) -> String {
        let substituted = substitute_escaped(
            &self.config.default_prefix,
            &[
                ('d', &(i + 1).to_string()),                        // cursor index
                ('r', &(i + 1 + self.bottom as usize).to_string()), // absolute index
            ],
        );

        fit_width(&substituted, self.indentation())
    }

    fn current_style(&self) -> Style {
        Style::from(self.config.current_fg)
            .bg(self.config.current_bg)
            .add_modifier(self.config.current_modifier)
    }

    fn is_current(&self, i: usize) -> bool {
        !self.cursor_disabled && self.cursor == i as u16
    }

    pub fn match_style(&self) -> Style {
        Style::default()
            .fg(self.config.match_fg)
            .add_modifier(self.config.match_modifier)
    }

    fn hr(&self) -> Option<Row<'static>> {
        let sep = self.config.horizontal_separator;

        if matches!(sep, HorizontalSeparator::None) {
            return None;
        }

        // todo: support non_stacked by doing a seperate rendering pass
        if !self.config.stacked_columns && self.widths.len() > 1 {
            return Some(Row::new(vec![vec![]]));
        }

        let unit = sep.as_str();
        let line = unit.repeat(self.width as usize);

        Some(Row::new(vec![line]))
    }

    fn _hr(&self) -> u16 {
        !matches!(self.config.horizontal_separator, HorizontalSeparator::None) as u16
    }
}

pub struct StatusUI {}

impl StatusUI {
    pub fn parse_template_to_status_line(s: &str) -> Line<'static> {
        let parts = match split_on_nesting(&s, ['{', '}']) {
            Ok(x) => x,
            Err(n) => {
                if n > 0 {
                    log::error!("Encountered {} unclosed parentheses", n)
                } else {
                    log::error!("Extra closing parenthesis at index {}", -n)
                }
                return Line::from(s.to_string());
            }
        };

        let mut spans = Vec::new();
        let mut in_nested = !s.starts_with('{');
        for part in parts {
            in_nested = !in_nested;
            let content = part.as_str();

            if in_nested {
                let inner = &content[1..content.len() - 1];

                if let Some((color_name, text)) = inner.split_once(':') {
                    if let Ok(color) = Color::from_str(color_name) {
                        spans.push(Span::styled(text.to_string(), Style::default().fg(color)));
                        continue;
                    }
                }
            }

            spans.push(Span::raw(content.to_string()));
        }

        Line::from(spans)
    }
}
