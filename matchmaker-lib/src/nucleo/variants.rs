use std::{borrow::Cow, sync::Arc};

use crate::{RenderFn, SSS, nucleo::Indexed};

use super::{
    Text,
    injector::{self},
    worker::{Column, Worker},
};

impl<T: SSS> Worker<T> {
    /// Returns a function which templates a string given an item using the column functions
    pub fn default_format_fn<const QUOTE: bool>(
        &self,
        blank_format: impl Fn(&T) -> Cow<'_, str> + SSS,
    ) -> RenderFn<T> {
        let columns = self.columns.clone();

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
                                    .map(|col| col.format_text(item))
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

impl<T: SSS> Worker<Indexed<T>> {
    /// A convenience method to initialize data. Items are indexed starting from the current nucleo item count.
    /// # Notes
    /// -  Not concurrent.
    /// - Subsequent use of IndexedInjector should start from the returned count.
    pub fn append(&self, items: impl IntoIterator<Item = T>) -> u32 {
        let mut index = self.nucleo.snapshot().item_count();
        for inner in items {
            injector::push_impl(
                &self.nucleo.injector(),
                &self.columns,
                Indexed { index, inner },
            );
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

impl<T: Render + SSS> Worker<T> {
    /// Create a new worker over items which are displayed in the picker as exactly their as_str representation.
    pub fn new_single_column() -> Self {
        Self::new([Column::new("_", |item: &T| item.as_text())], 0)
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

impl<T> Worker<T>
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
    /// ) -> Matchmaker<Indexed<RunAction>, RunAction> {
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

        let columns = columns_vec
            .iter()
            .enumerate()
            .map(|(i, name)| Column::new(name.clone(), move |item: &T| item.get_text(i)));

        // Find the index of the default column
        let default_index = if let Some(default_column) = default_column {
            columns_vec
                .iter()
                .position(|name| name.as_ref() == default_column)
                .unwrap_or(0) // fallback to 0 if not found
        } else {
            0
        };

        Self::new(columns, default_index)
    }
}
