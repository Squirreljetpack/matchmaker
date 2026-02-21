use std::borrow::Cow;

use cli_boilerplate_automation::{define_either, define_when};
use ratatui::text::Text;

use crate::utils::text::text_to_string;

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
            Either::Right(t) => Cow::Owned(text_to_string(t)),
        }
    }

    pub fn to_text(self) -> Text<'static> {
        match self {
            Either::Left(s) => Text::from(s),
            Either::Right(t) => t,
        }
    }
}
