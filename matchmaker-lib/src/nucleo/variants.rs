use std::{borrow::Cow, sync::Arc};

use crate::{nucleo::Indexed, RenderFn, SSS};
use ansi_to_tui::IntoText;

use super::{
    injector::{self},
    worker::{Column, Worker},
    Text,
};

impl<T: SSS, D: 'static> Worker<T, D> {
    /// Returns a function which templates a string given an item using the column functions
    pub fn default_format_fn<const QUOTE: bool>(
        &self,
        blank_format: impl Fn(&T) -> Cow<'_, str> + SSS,
    ) -> RenderFn<T> {
        let columns = self.columns.clone();
        let raw_preprocessor = self.raw_preprocessor.clone();

        Box::new(move |item: &T, template: &str| {
            let mut result = String::with_capacity(template.len());
            let chars = template.chars().peekable();
            let mut state = State::Normal;
            let mut key = String::new();

            enum State {
                Normal,
                InKey,
                Escape,
            }

            for c in chars {
                match state {
                    State::Normal => match c {
                        '\\' => state = State::Escape,
                        '{' => state = State::InKey,
                        _ => result.push(c),
                    },
                    State::Escape => {
                        result.push(c);
                        state = State::Normal;
                    }
                    State::InKey => match c {
                        '}' => {
                            let replacement = match key.as_str() {
                                "" => blank_format(item),
                                _ => columns
                                    .iter()
                                    .find(|col| &*col.name == key.as_str())
                                    .map(|col| {
                                        let d = raw_preprocessor(item);
                                        col.raw(item, &d).into_owned().into()
                                    })
                                    .unwrap_or_else(|| Cow::Borrowed("")),
                            };

                            if QUOTE {
                                result.push('\'');
                                result.push_str(&replacement);
                                result.push('\'');
                            } else {
                                result.push_str(&replacement);
                            }
                            key.clear();
                            state = State::Normal;
                        }
                        _ => key.push(c),
                    },
                }
            }

            if !key.is_empty() {
                result.push('{');
                result.push_str(&key);
            }

            result
        })
    }
}

impl<T: SSS, D> Worker<Indexed<T>, D> {
    /// A convenience method to initialize data. Items are indexed starting from the current nucleo item count.
    /// # Notes
    /// -  Not concurrent.
    /// - Subsequent use of IndexedInjector should start from the returned count.
    pub fn append(&self, items: impl IntoIterator<Item = T>) -> u32 {
        let mut index = self.nucleo.snapshot().item_count();
        for inner in items {
            let item = Indexed { index, inner };
            let d = (self.raw_preprocessor)(&item);
            injector::push_impl(&self.nucleo.injector(), &self.columns, item, &d);
            index += 1;
        }
        index
    }
}

/// You must either impl as_str or as_text
pub trait Render {
    fn as_str(&self) -> std::borrow::Cow<'_, str> {
        self.as_text().to_string().into()
    }
    fn as_text(&self) -> Text<'_> {
        Text::from(self.as_str())
    }
}
impl<T: AsRef<str>> Render for T {
    fn as_str(&self) -> std::borrow::Cow<'_, str> {
        self.as_ref().into()
    }
}

impl<T: Render + SSS> Worker<T, ()> {
    /// Create a new worker over items which are displayed in the picker as exactly their as_str representation.
    pub fn new_single_column() -> Self {
        let preprocessor = Arc::new(|_: &T| ());
        Self::new(
            [
                Column::new_boxed("_", Box::new(|item: &T, _: &()| item.as_text()))
                    .with_raw(|item: &T, _: &()| item.as_str()),
            ],
            0,
            preprocessor.clone(),
            preprocessor,
        )
    }
}

/// You must either impl as_str or as_text
pub trait ColumnIndexable {
    fn get_str(&self, i: usize) -> std::borrow::Cow<'_, str> {
        self.get_text(i).to_string().into()
    }

    fn get_text(&self, i: usize) -> Text<'_> {
        Text::from(self.get_str(i))
    }
}

impl<T> Worker<T, ()>
where
    T: ColumnIndexable + SSS,
{
    /// Create a new worker over indexable items, whose columns correspond to indices according to the relative order of the column names given to this function.
    /// # Example
    /// ```rust
    /// #[derive(Clone)]
    /// pub struct RunAction {
    ///     name: String,
    ///     alias: String,
    ///     desc: String
    /// };
    ///
    /// use matchmaker::{Matchmaker, Selector};
    /// use matchmaker::nucleo::{Indexed, Worker, ColumnIndexable};
    ///
    /// impl ColumnIndexable for RunAction {
    ///     fn get_str(&self, i: usize) -> std::borrow::Cow<'_, str> {
    ///         if i == 0 {
    ///             &self.name
    ///         } else if i == 1 {
    ///             &self.alias
    ///         } else {
    ///             &self.desc
    ///         }.into()
    ///     }
    /// }
    ///
    /// pub fn make_mm(
    ///     items: impl Iterator<Item = RunAction>,
    /// ) -> Matchmaker<Indexed<RunAction>, (), RunAction> {
    ///     let worker = Worker::new_indexable(["name", "alias", "desc"], Some("name"));
    ///     worker.append(items);
    ///     let selector = Selector::new(Indexed::identifier);
    ///     Matchmaker::new(worker, selector)
    /// }
    /// ```
    pub fn new_indexable<I, S>(column_names: I, default_column: Option<&str>) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        let columns_vec: Vec<Arc<str>> = column_names.into_iter().map(Into::into).collect();

        let columns = columns_vec.iter().enumerate().map(|(i, name)| {
            Column::new(name.clone(), move |item: &T, _: &()| item.get_text(i))
                .with_raw(move |item: &T, _: &()| item.get_str(i))
        });

        // Find the index of the default column
        let default_index = if let Some(default_column) = default_column {
            columns_vec
                .iter()
                .position(|name| name.as_ref() == default_column)
                .unwrap_or(0) // fallback to 0 if not found
        } else {
            0
        };

        let preprocessor = Arc::new(|_: &T| ());
        Self::new(columns, default_index, preprocessor.clone(), preprocessor)
    }
}

/// Preprocessed data type for config-based columns
/// Contains: (Result<Text, raw_string>, split_ranges)
pub type ConfigPreprocessedData = (Result<Text<'static>, String>, Vec<(u32, u32)>);

/// Build columns for config-based matchmaker with preprocessing support.
/// Returns (columns, raw_preprocessor, text_preprocessor, default_column_index)
pub fn build_columns_for_config(
    preprocess: (bool, bool), // (parse_ansi, trim)
    split: crate::config::Split,
    column_names: Vec<Arc<str>>,
    default_column: Option<&str>,
) -> (
    Vec<Column<Indexed<String>, ConfigPreprocessedData>>,
    Arc<dyn Fn(&Indexed<String>) -> ConfigPreprocessedData + Send + Sync>,
    Arc<dyn Fn(&Indexed<String>) -> ConfigPreprocessedData + Send + Sync>,
    usize,
) {
    use crate::config::Split;
    use regex::Regex;

    let col_count = column_names.len();

    // Find default column index
    let default_index = if let Some(default_column) = default_column {
        column_names
            .iter()
            .position(|name| name.as_ref() == default_column)
            .unwrap_or(0)
    } else {
        0
    };

    // Build split function based on config
    let split_fn: Arc<dyn Fn(&str) -> Vec<(u32, u32)> + Send + Sync> = match split {
        Split::Delimiter(ref rg) => {
            let rg = rg.clone();
            Arc::new(move |s: &str| {
                let mut ranges = Vec::with_capacity(col_count);
                let mut last_end = 0;

                for m in rg.find_iter(s).take(col_count - 1) {
                    ranges.push((last_end as u32, m.start() as u32));
                    last_end = m.end();
                }

                ranges.push((last_end as u32, s.len() as u32));
                ranges
            })
        }
        Split::Regexes(ref rgs) => {
            let rgs: Vec<Regex> = rgs.clone();
            Arc::new(move |s: &str| {
                let mut ranges = Vec::with_capacity(col_count);

                for re in rgs.iter().take(col_count) {
                    if let Some(m) = re.find(s) {
                        ranges.push((m.start() as u32, m.end() as u32));
                    } else {
                        ranges.push((0, 0));
                    }
                }
                ranges
            })
        }
        Split::None => Arc::new(move |s: &str| vec![(0u32, s.len() as u32)]),
    };

    // Build raw preprocessor (returns string representation)
    let raw_preprocessor: Arc<dyn Fn(&Indexed<String>) -> ConfigPreprocessedData + Send + Sync> = {
        let split_fn = split_fn.clone();
        let (_parse_ansi, trim) = preprocess;
        Arc::new(move |item: &Indexed<String>| {
            let s = if trim {
                item.inner.trim().to_string()
            } else {
                item.inner.clone()
            };

            let ranges = split_fn(&s);
            (Err(s), ranges)
        })
    };

    // Build text preprocessor (returns parsed text if ANSI enabled)
    let text_preprocessor: Arc<dyn Fn(&Indexed<String>) -> ConfigPreprocessedData + Send + Sync> = {
        let split_fn = split_fn.clone();
        let (parse_ansi, trim) = preprocess;
        Arc::new(move |item: &Indexed<String>| {
            let s = if trim {
                item.inner.trim().to_string()
            } else {
                item.inner.clone()
            };

            let text_result = if parse_ansi {
                match s.as_bytes().into_text() {
                    Ok(text) => Ok(text),
                    Err(_) => Err(s.clone()),
                }
            } else {
                Err(s.clone())
            };

            let ranges = split_fn(&s);
            (text_result, ranges)
        })
    };

    // Build columns
    let columns: Vec<Column<Indexed<String>, ConfigPreprocessedData>> = column_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            Column::new(
                name.clone(),
                move |_item: &Indexed<String>, d: &ConfigPreprocessedData| {
                    let (text_result, ranges) = d;
                    let range = ranges.get(i).copied().unwrap_or((0, 0));

                    match text_result {
                        Ok(text) => crate::utils::text::slice_ratatui_text(
                            text,
                            range.0 as usize..range.1 as usize,
                        ),
                        Err(s) => {
                            let slice = if range.0 == 0 && range.1 == 0 {
                                ""
                            } else {
                                &s[range.0 as usize..range.1 as usize]
                            };
                            Text::from(slice)
                        }
                    }
                },
            )
            .with_raw(move |_item: &Indexed<String>, d: &ConfigPreprocessedData| {
                let (_text_result, ranges) = d;
                let range = ranges.get(i).copied().unwrap_or((0, 0));

                // For raw, we need the string representation
                // If text_result is Err, we have the string directly
                // If text_result is Ok, we need to extract from the original string
                // We'll reconstruct from ranges - but we need the original string
                // This is a bit tricky - let's store the string in the data
                match _text_result {
                    Err(s) => {
                        let slice = if range.0 == 0 && range.1 == 0 {
                            ""
                        } else {
                            &s[range.0 as usize..range.1 as usize]
                        };
                        Cow::Borrowed(slice)
                    }
                    Ok(text) => {
                        // Convert text back to string and slice
                        let s = text.to_string();
                        let slice = if range.0 == 0 && range.1 == 0 {
                            String::new()
                        } else {
                            s[range.0 as usize..range.1 as usize].to_string()
                        };
                        Cow::Owned(slice)
                    }
                }
            })
        })
        .collect();

    (columns, raw_preprocessor, text_preprocessor, default_index)
}
