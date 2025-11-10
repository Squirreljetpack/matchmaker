use std::{borrow::Cow, sync::Arc};

use crate::{
    PickerItem,
};

use super::{injector::{self, Indexed}, Text, worker::{Column, Worker}};

// C is not generic because not sure about how C should be used/passed
impl<T: PickerItem> Worker<T> {
    /// Returns a function which templates a string given an item using the column functions
    pub fn make_format_fn<const QUOTE: bool>(
        &self,
        blank_format: impl Fn(&T) -> &str + Send + Sync + 'static,
    ) -> Box<dyn Fn(&T, &str) -> String + Send + Sync> {
        let columns = self.columns.clone();
        
        Box::new(move |item: &T, template: &str| {
            let mut result = String::with_capacity(template.len());
            let mut chars = template.chars().peekable();
            let mut state = State::Normal;
            let mut key = String::new();
            
            enum State {
                Normal,
                InKey,
                Escape,
            }
            
            while let Some(c) = chars.next() {
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
                                "" => Cow::Borrowed(blank_format(item)),
                                _ => columns
                                .iter()
                                .find(|col| &*col.name == key.as_str())
                                .map(|col| col.format_text(item, &()))
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

pub trait Render {
    fn as_str(&self) -> &str;
}
impl<T: AsRef<str>> Render for T {
    fn as_str(&self) -> &str {
        self.as_ref()
    }
}

impl<T: Render + PickerItem> Worker<Indexed<T>> {
    pub fn new_single() -> Self {
        Self::new(
            vec![Column::new("_", |item: &Indexed<T>, _context: &()| {
                Text::from(item.inner.as_str())
            })],
            0,
            (),
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
                &(),
            );
            index += 1;
        }
        index
    }

    pub fn clone_identifier(item: &Indexed<T>) -> (u32, T) 
    where T: Clone {
        (item.index, item.inner.clone())
    }
}

pub trait ColumnIndexable {
    fn index(&self, i: usize) -> &str;
}

impl<T> Worker<T>
where
T: ColumnIndexable + PickerItem,
{
    pub fn new_indexable<I, S>(column_names: I) -> Self
    where
    I: IntoIterator<Item = S>,
    S: Into<Arc<str>>,
    {
        let columns = column_names.into_iter().enumerate().map(|(i, name)| {
            let name = name.into();
            
            Column::new(name, move |item: &T, _| {
                let text = item.index(i);
                Text::from(text)
            })
        });
        
        Self::new(columns, 0, ())
    }
}
