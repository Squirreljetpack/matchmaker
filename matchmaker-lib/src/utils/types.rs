use std::borrow::Cow;

use cba::define_either;
pub use ratatui::text::{Line, Text};

define_either! {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord)]
    pub enum Either<L, R = L> {
        Left,
        Right
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SimpleText(pub Box<[Line<'static>]>);

impl From<String> for SimpleText {
    fn from(s: String) -> Self {
        if s.contains('\n') {
            Self(
                s.lines()
                    .map(|l| Line::from(l.to_string()))
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )
        } else {
            Self(vec![Line::from(s)].into_boxed_slice())
        }
    }
}

impl From<Text<'static>> for SimpleText {
    fn from(t: Text<'static>) -> Self {
        Self(t.lines.into_boxed_slice())
    }
}

impl SimpleText {
    pub fn to_cow(&self) -> Cow<'_, str> {
        if self.0.is_empty() {
            return Cow::Borrowed("");
        }
        if self.0.len() == 1 {
            return Cow::Owned(self.0[0].to_string());
        }
        let mut s = String::new();
        for (i, line) in self.0.iter().enumerate() {
            if i > 0 {
                s.push('\n');
            }
            s.push_str(&line.to_string());
        }
        Cow::Owned(s)
    }

    // pub fn to_text(self) -> Text<'static> {
    //     Text::from(self.0.into_vec())
    // }

    // pub fn as_text(&self) -> Text<'_> {
    //     Text::from(self.0.iter().cloned().collect::<Vec<_>>())
    // }
}
