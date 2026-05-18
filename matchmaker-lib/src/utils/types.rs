use std::borrow::Cow;

use cba::define_either;
use ratatui::text::Text;

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

impl Either<String, Text<'static>> {
    pub fn to_cow(&self) -> Cow<'_, str> {
        match self {
            Either::Left(s) => Cow::Borrowed(s),
            Either::Right(t) => Cow::Owned(t.to_string()),
        }
    }

    pub fn to_text(self) -> Text<'static> {
        match self {
            Either::Left(s) => Text::from(s),
            Either::Right(t) => t,
        }
    }
}
