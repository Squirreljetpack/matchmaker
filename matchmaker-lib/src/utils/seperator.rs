#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum HorizontalSeparator {
    #[default]
    None,
    Empty,
    Light,
    Normal,
    Heavy,
    Dashed,
}

impl HorizontalSeparator {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => unreachable!(),
            Self::Empty => " ",
            Self::Light => "─", // U+2500
            Self::Normal => "─",
            Self::Heavy => "━",  // U+2501
            Self::Dashed => "╌", // U+254C (box drawings light double dash)
        }
    }
}
