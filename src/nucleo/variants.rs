use std::{borrow::Cow, sync::Arc};

use crate::{
    MMItem, RenderFn, nucleo::Indexed, utils::text::plain_text
};

use super::{injector::{self}, Text, worker::{Column, Worker}};

impl<T: MMItem> Worker<T> {
    /// Returns a function which templates a string given an item using the column functions
    pub fn make_format_fn<const QUOTE: bool>(
        &self,
        blank_format: impl Fn(&T) -> Cow<'_, str> + MMItem,
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

/// You must either impl as_str or as_text
pub trait Render {
    fn as_str(&self) -> Cow<'_, str> {
        plain_text(&self.as_text()).into()
    }
    fn as_text(&self) -> Text<'_> {
        Text::from(self.as_str())
    }
}
impl<T: AsRef<str>> Render for T {
    fn as_str(&self) -> Cow<'_, str> {
        self.as_ref().into()
    }
}

impl<T: Render + MMItem> Worker<Indexed<T>> {
    /// Create a new worker over items which are displayed in the picker as exactly their as_str representation.
    pub fn new_single_column() -> Self {
        Self::new(
            [Column::new("_", |item: &Indexed<T>| {
                item.inner.as_text()
            })],
            0,
        )
    }

    /// A convenience method to initialize data. Note that it is clearly unsound to use this concurrently with other workers, or to subsequently push with an IndexedInjector.
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

pub trait ColumnIndexable {
    fn index(&self, i: usize) -> &str;
}

impl<T> Worker<T>
where
T: ColumnIndexable + MMItem,
{
    /// Create a new worker over indexable items, whose columns as displayed in the picker correspond to indices according to the relative order of the column names given to this function.
    pub fn new_indexable<I, S>(column_names: I) -> Self
    where
    I: IntoIterator<Item = S>,
    S: Into<Arc<str>>,
    {
        let columns = column_names.into_iter().enumerate().map(|(i, name)| {
            let name = name.into();

            Column::new(name, move |item: &T| {
                let text = item.index(i);
                Text::from(text)
            })
        });

        Self::new(columns, 0)
    }
}
