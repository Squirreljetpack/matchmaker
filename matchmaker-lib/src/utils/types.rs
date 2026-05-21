use std::borrow::Cow;

use cba::{broc::EnvVars, define_either};

pub trait EnvVarsExt {
    fn env_set(&mut self, key: impl Into<String>, value: impl ToString);
    fn env_get(&self, key: &str) -> Option<&String>;
}

impl EnvVarsExt for EnvVars {
    fn env_set(&mut self, key: impl Into<String>, value: impl ToString) {
        let key = key.into();
        let val = value.to_string();
        if let Some(pos) = self.iter().position(|(k, _)| k == &key) {
            self[pos].1 = val;
        } else {
            self.push((key, val));
        }
    }

    fn env_get(&self, key: &str) -> Option<&String> {
        self.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}
pub use ratatui::text::Text;

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

impl Either<Box<str>, Text<'static>> {
    pub fn to_cow(&self) -> Cow<'_, str> {
        match self {
            Either::Left(s) => Cow::Borrowed(s),
            Either::Right(t) => Cow::Owned(t.to_string()),
        }
    }

    pub fn to_text(self) -> Text<'static> {
        match self {
            Either::Left(s) => Text::from(s.into_string()),
            Either::Right(t) => t,
        }
    }

    pub fn as_text(&self) -> Text<'_> {
        match self {
            Either::Left(s) => Text::from(s.as_ref()),
            Either::Right(t) => t.clone(),
        }
    }
}
