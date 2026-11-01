// Original code from https://github.com/helix-editor/helix (MPL 2.0)
// Modified by Squirreljetpack, 2025

use std::{collections::HashMap, mem, ops::Range, sync::Arc};

pub struct PickerQuery {
    /// The column names of the picker.
    column_names: Box<[Arc<str>]>,
    /// The index of the primary column in `column_names`.
    /// The primary column is selected by default unless another
    /// field is specified explicitly with `%fieldname`.
    primary_column: usize,
    /// The mapping between column names and input in the query
    /// for those columns.
    inner: HashMap<Arc<str>, Arc<str>>,
    /// The byte ranges of the input text which are used as input for each column.
    /// This is calculated at parsing time for use in [Self::active_column].
    /// This Vec is naturally sorted in ascending order and ranges do not overlap.
    column_ranges: Vec<(Range<usize>, Option<Arc<str>>)>,

    empty_column: Option<usize>,
}

impl PartialEq<HashMap<Arc<str>, Arc<str>>> for PickerQuery {
    fn eq(&self, other: &HashMap<Arc<str>, Arc<str>>) -> bool {
        self.inner.eq(other)
    }
}

impl PickerQuery {
    pub fn new<I: Iterator<Item = Arc<str>>>(column_names: I, primary_column: usize) -> Self {
        let column_names: Box<[_]> = column_names.collect();
        let inner = HashMap::with_capacity(column_names.len());
        let column_ranges = vec![(0..usize::MAX, Some(column_names[primary_column].clone()))];
        let empty_column = column_names.iter().position(|c| c.is_empty());

        Self {
            column_names,
            primary_column,
            inner,
            column_ranges,
            empty_column,
        }
    }

    /// Sets the default column index.
    ///
    /// # Warning
    /// This method does not check if the index exists.
    pub fn with_default_column(mut self, index: usize) -> Self {
        self.primary_column = index;
        let primary_field = &self.column_names[self.primary_column];
        self.column_ranges.clear();
        self.column_ranges
            .push((0..usize::MAX, Some(primary_field.clone())));
        self
    }

    // ----- getters -----

    pub fn get(&self, column: &str) -> Option<&Arc<str>> {
        self.inner.get(column)
    }

    pub fn primary_column_query(&self) -> Option<&str> {
        let name = self.column_names.get(self.primary_column)?;
        self.inner.get(name).map(|s| &**s)
    }

    pub fn primary_column_name(&self) -> Option<&str> {
        self.column_names.get(self.primary_column).map(|s| &**s)
    }

    pub fn primary_column_index(&self) -> usize {
        self.primary_column
    }
    pub fn empty_column_index(&self) -> Option<usize> {
        self.empty_column
    }

    /// Finds the column which the cursor is 'within' in the last parse.
    ///
    /// The cursor is considered to be within a column when it is placed within any
    /// of a column's text. See the `active_column_test` unit test below for examples.
    ///
    /// `cursor` is a byte index that represents the location of the prompt's cursor.
    pub fn current_column(&self, cursor: usize) -> Option<&Arc<str>> {
        let point = self
            .column_ranges
            .partition_point(|(range, _field)| cursor > range.end);

        self.column_ranges
            .get(point)
            .filter(|(range, _field)| cursor >= range.start)
            .and_then(|(_range, field)| field.as_ref())
    }

    pub fn active_column_name(&self, input: &str) -> String {
        let (_, ranges) = self.parse_impl(input);
        ranges
            .last()
            .and_then(|x| x.1.as_deref())
            .unwrap_or(self.column_names[self.primary_column].as_ref())
            .to_string()
    }

    /// Finds the index of the column which the cursor is 'within' in the last parse.
    /// Returns the primary column index if no specific column is active at the cursor.
    pub fn active_column_index(&self, cursor: usize) -> usize {
        self.current_column(cursor)
            .and_then(|name| self.column_names.iter().position(|c| c == name))
            .unwrap_or(self.primary_column)
    }

    // ----- parse -----

    pub fn parse(&mut self, input: &str) -> HashMap<Arc<str>, Arc<str>> {
        let (new_inner, new_ranges) = self.parse_impl(input);
        self.column_ranges = new_ranges;
        mem::replace(&mut self.inner, new_inner)
    }

    fn parse_impl(
        &self,
        input: &str,
    ) -> (
        HashMap<Arc<str>, Arc<str>>,
        Vec<(std::ops::Range<usize>, Option<Arc<str>>)>,
    ) {
        let mut fields: std::collections::HashMap<Arc<str>, String> =
            std::collections::HashMap::new();
        let primary_field = &self.column_names[self.primary_column];
        let mut column_ranges: Vec<(std::ops::Range<usize>, Option<Arc<str>>)> =
            vec![(0..usize::MAX, Some(primary_field.clone()))];

        // Column the current word feeds. A `%word` naming no column is pattern
        // text, and stays in this column.
        let mut field: Option<&Arc<str>> = None;
        // Pattern text of the current word. Words are atoms, so the join
        // re-inserts the separator between them.
        let mut text = String::new();
        // Input offset of the `%` when the current word is a field selector.
        let mut marker = None;
        let mut word_start = true;
        let mut escaped = false;

        macro_rules! end_word {
            () => {{
                // A `%word` selects a column only if its name matches one;
                // otherwise the word is pattern text.
                if let Some(idx) = marker.take() {
                    let name = &text[1..];
                    let col = if name.is_empty() {
                        self.empty_column
                            .and_then(|index| self.column_names.get(index))
                    } else {
                        self.column_names
                            .iter()
                            .filter(|col| col.starts_with(name))
                            .min_by_key(|col| col.len())
                    };

                    if let Some(col) = col {
                        text.clear();

                        // The marker is syntax, so it owns no pattern text: the
                        // previous column ends where the `%` starts.
                        if let Some((range, _)) = column_ranges.last_mut() {
                            range.end = idx;
                        }
                        column_ranges.push((idx + 1..usize::MAX, Some(col.clone())));
                        field = Some(col);
                    }
                }

                if !text.is_empty() {
                    // Chunks of the same column are space-joined so they stay
                    // separate atoms (terms) in the pattern.
                    let pattern = fields
                        .entry(field.unwrap_or(primary_field).clone())
                        .or_default();
                    if !pattern.is_empty() {
                        pattern.push(' ');
                    }
                    pattern.push_str(&text);
                    text.clear();
                }
            }};
        }

        for (idx, ch) in input.char_indices() {
            match ch {
                // Backslash escaping
                _ if escaped => {
                    // `\%` searches for a literal `%`; a backslash before any
                    // other character is kept as written.
                    if ch != '%' {
                        text.push('\\');
                    }
                    text.push(ch);
                    escaped = false;
                    word_start = false;
                }
                '\\' => escaped = true,
                // A `%` opens a field name only at the start of a word.
                '%' if word_start => {
                    marker = Some(idx);
                    text.push(ch);
                    word_start = false;
                }
                c if c.is_whitespace() => {
                    end_word!();
                    word_start = true;
                }
                c => {
                    text.push(c);
                    word_start = false;
                }
            }
        }
        // A trailing backslash is search text of its own.
        if escaped {
            text.push('\\');
        }
        end_word!();

        let new_fields: HashMap<Arc<str>, Arc<str>> = fields
            .into_iter()
            .map(|(field, query)| (field, query.into()))
            .collect();

        (new_fields, column_ranges)
    }
}
