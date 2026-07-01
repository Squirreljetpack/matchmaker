use super::{
    Text, injector,
    worker::{Column, Worker},
};
use crate::{RenderFn, SSS};
use std::{borrow::Cow, sync::Arc};

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
                                    .and_then(|col| {
                                        let d = raw_preprocessor(item)?;
                                        Some(col.raw(item, &d).into_owned().into())
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
            if let Some(d) = (self.raw_preprocessor)(&item) {
                injector::push_impl(&self.nucleo.injector(), &self.columns, item, &d);
                count += 1;
            }
        }
        count
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
        let raw_preprocessor = Arc::new(|_: &T| Some(()));
        let text_preprocessor = Arc::new(|_: &T| ());
        Self::new(
            [
                Column::new_boxed("_", Box::new(|item: &T, _: &()| item.as_text()))
                    .with_raw(|item: &T, _: &()| item.as_str()),
            ],
            0,
            raw_preprocessor,
            text_preprocessor,
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
    ///     let worker = Worker::new_indexable(["name", "alias", "desc"], Some(matchmaker::config::StringOrInt::String("name".to_string())));
    ///     worker.append(items);
    ///     Matchmaker::new_on_cloneable(worker)
    /// }
    /// ```
    pub fn new_indexable<I, S>(column_names: I, default_column: Option<crate::config_types::StringOrInt>) -> Self
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
        let default_index = match default_column {
            Some(crate::config_types::StringOrInt::String(ref s)) => {
                columns_vec
                    .iter()
                    .position(|name| name.as_ref() == s)
                    .unwrap_or(0)
            }
            Some(crate::config_types::StringOrInt::Int(i)) => {
                if i >= 0 && (i as usize) < columns_vec.len() {
                    i as usize
                } else {
                    0
                }
            }
            None => 0,
        };

        let raw_preprocessor = Arc::new(|_: &T| Some(()));
        let text_preprocessor = Arc::new(|_: &T| ());
        Self::new(columns, default_index, raw_preprocessor, text_preprocessor)
    }
}
