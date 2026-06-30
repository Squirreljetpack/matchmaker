use std::{borrow::Cow, sync::Arc};

use crate::{RenderFn, SSS};
use ansi_to_tui::IntoText;

use super::{
    Text, injector,
    worker::{Column, Worker},
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

    /// Push items into the matcher.
    ///
    /// The item type `T` is the worker's element type. No `Indexed` wrapping is required;
    /// nucleo assigns each pushed item a unique internal `u32` index which can later be
    /// retrieved via [`Self::get_nth_indexed`].
    pub fn append(&self, items: impl IntoIterator<Item = T>) -> u32 {
        let mut count = 0;
        for item in items {
            let d = (self.raw_preprocessor)(&item);
            injector::push_impl(&self.nucleo.injector(), &self.columns, item, &d);
            count += 1;
        }
        count
    }

    /// Return the nucleo index and a reference to the data of the n-th matched item, if any.
    ///
    /// The returned `u32` is the stable nucleo item index (see [`nucleo::Match::idx`]).
    /// Callers can use this as a key into [`crate::Selector`] or as a row-cache key.
    pub fn get_nth_indexed(&self, n: u32) -> Option<(u32, &T)> {
        let snapshot = self.nucleo.snapshot();
        let m = snapshot.matches().get(n as usize)?;
        let idx = m.idx;
        // SAFETY: `idx` is taken from a match in the snapshot we just took, so it
        // points to an initialized item in that snapshot.
        let item = unsafe { snapshot.get_item_unchecked(idx) };
        Some((idx, item.data))
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
    /// use matchmaker::nucleo::{Worker, ColumnIndexable};
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
    /// ) -> Matchmaker<RunAction, RunAction> {
    ///     let worker = Worker::new_indexable(["name", "alias", "desc"], Some("name"));
    ///     worker.append(items);
    ///     Matchmaker::new_on_cloneable(worker)
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
    Vec<Column<String, ConfigPreprocessedData>>,
    Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync>,
    Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync>,
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
            .unwrap_or_else(|| {
                cba::wbog!("Default column '{default_column}' not found, defaulting to first.");
                0
            })
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
    let raw_preprocessor: Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync> = {
        let split_fn = split_fn.clone();
        let (_parse_ansi, trim) = preprocess;
        Arc::new(move |item: &String| {
            let s = if trim {
                item.trim().to_string()
            } else {
                item.clone()
            };

            let ranges = split_fn(&s);
            (Err(s), ranges)
        })
    };

    // Build text preprocessor (returns parsed text if ANSI enabled)
    let text_preprocessor: Arc<dyn Fn(&String) -> ConfigPreprocessedData + Send + Sync> = {
        let split_fn = split_fn.clone();
        let (parse_ansi, trim) = preprocess;
        Arc::new(move |item: &String| {
            let s = if trim {
                item.trim().to_string()
            } else {
                item.clone()
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
    let columns: Vec<Column<String, ConfigPreprocessedData>> = column_names
        .iter()
        .enumerate()
        .map(|(i, name)| {
            Column::new(
                name.clone(),
                move |_item: &String, d: &ConfigPreprocessedData| {
                    let (text_result, ranges) = d;
                    let range = ranges.get(i).copied().unwrap_or((0, 0));

                    match text_result {
                        Ok(text) => crate::utils::text::slice_ratatui_text(
                            text,
                            range.0 as usize..range.1 as usize,
                        ),
                        Err(s) => Text::from(&s[range.0 as usize..range.1 as usize]),
                    }
                },
            )
            .with_raw(move |_item: &String, d: &ConfigPreprocessedData| {
                let (_text_result, ranges) = d;
                let range = ranges.get(i).copied().unwrap_or((0, 0));

                match _text_result {
                    Err(s) => Cow::Borrowed(&s[range.0 as usize..range.1 as usize]),
                    Ok(text) => {
                        let s = text.to_string();
                        Cow::Owned(s[range.0 as usize..range.1 as usize].to_string())
                    }
                }
            })
        })
        .collect();

    (columns, raw_preprocessor, text_preprocessor, default_index)
}
