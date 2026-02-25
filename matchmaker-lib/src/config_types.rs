use std::fmt;

use cli_boilerplate_automation::{
    bird::one_or_many, define_restricted_wrapper, define_transparent_wrapper,
};
use ratatui::widgets::Borders;

use regex::Regex;

use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, Visitor},
    ser::SerializeSeq,
};

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

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum RowConnectionStyle {
    #[default]
    Disjoint,
    Capped,
    Full,
}

define_transparent_wrapper!(
    #[derive(Copy, Clone, serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    Count: u16 = 1
);
use ratatui::widgets::Padding as rPadding;

define_transparent_wrapper!(
    #[derive(Copy, Clone, Default)]
    Padding: rPadding
);

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone, Copy)]
#[serde(untagged)]
pub enum ShowCondition {
    Bool(bool),
    Free(u16),
}
impl Default for ShowCondition {
    fn default() -> Self {
        Self::Bool(false)
    }
}
impl From<bool> for ShowCondition {
    fn from(value: bool) -> Self {
        ShowCondition::Bool(value)
    }
}

impl From<u16> for ShowCondition {
    fn from(value: u16) -> Self {
        ShowCondition::Free(value)
    }
}

// -----------------------------------------------------------------------------------------

impl Serialize for Padding {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let padding = self;
        if padding.top == padding.bottom
            && padding.left == padding.right
            && padding.top == padding.left
        {
            serializer.serialize_u16(padding.top)
        } else if padding.top == padding.bottom && padding.left == padding.right {
            let mut seq = serializer.serialize_seq(Some(2))?;
            seq.serialize_element(&padding.left)?;
            seq.serialize_element(&padding.top)?;
            seq.end()
        } else {
            let mut seq = serializer.serialize_seq(Some(4))?;
            seq.serialize_element(&padding.top)?;
            seq.serialize_element(&padding.right)?;
            seq.serialize_element(&padding.bottom)?;
            seq.serialize_element(&padding.left)?;
            seq.end()
        }
    }
}

impl<'de> Deserialize<'de> for Padding {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        use serde::de::Error;

        let repr: Vec<u16> = one_or_many::deserialize(deserializer)?;

        let inner = match repr.len() {
            1 => {
                let v = repr[0];
                rPadding {
                    top: v,
                    right: v,
                    bottom: v,
                    left: v,
                }
            }
            2 => {
                let lr = repr[0];
                let tb = repr[1];
                rPadding {
                    top: tb,
                    right: lr,
                    bottom: tb,
                    left: lr,
                }
            }
            4 => rPadding {
                top: repr[0],
                right: repr[1],
                bottom: repr[2],
                left: repr[3],
            },
            _ => {
                return Err(D::Error::custom(
                    "a number or an array of 1, 2, or 4 numbers",
                ));
            }
        };

        Ok(inner.into())
    }
}

// ---------------------------------------------------------------------------------

define_restricted_wrapper!(
    #[derive(Clone, serde::Serialize, serde::Deserialize)]
    #[serde(transparent)]
    FormatString: String
);

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Top,
    Bottom,
    Left,
    #[default]
    Right,
}

impl Side {
    pub fn opposite(&self) -> Borders {
        match self {
            Side::Top => Borders::BOTTOM,
            Side::Bottom => Borders::TOP,
            Side::Left => Borders::RIGHT,
            Side::Right => Borders::LEFT,
        }
    }
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorSetting {
    None,
    #[default]
    Default,
}

#[derive(Default, Debug, Clone, PartialEq, serde::Serialize)]
pub struct ColumnSetting {
    pub filter: bool,
    pub hidden: bool,
    pub name: String,
}

#[derive(Default, Debug, Clone)]
pub enum Split {
    /// Split by delimiter. Supports regex.
    Delimiter(Regex),
    /// A sequence of regexes.
    Regexes(Vec<Regex>),
    /// No splitting.
    #[default]
    None,
}

impl PartialEq for Split {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Split::Delimiter(r1), Split::Delimiter(r2)) => r1.as_str() == r2.as_str(),
            (Split::Regexes(v1), Split::Regexes(v2)) => {
                if v1.len() != v2.len() {
                    return false;
                }
                v1.iter()
                    .zip(v2.iter())
                    .all(|(r1, r2)| r1.as_str() == r2.as_str())
            }
            (Split::None, Split::None) => true,
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------------

impl serde::Serialize for Split {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Split::Delimiter(r) => serializer.serialize_str(r.as_str()),
            Split::Regexes(rs) => {
                let mut seq = serializer.serialize_seq(Some(rs.len()))?;
                for r in rs {
                    seq.serialize_element(r.as_str())?;
                }
                seq.end()
            }
            Split::None => serializer.serialize_none(),
        }
    }
}

impl<'de> Deserialize<'de> for Split {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct SplitVisitor;

        impl<'de> Visitor<'de> for SplitVisitor {
            type Value = Split;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("string for delimiter or array of strings for regexes")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                // Try to compile single regex
                Regex::new(value)
                    .map(Split::Delimiter)
                    .map_err(|e| E::custom(format!("Invalid regex: {}", e)))
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut regexes = Vec::new();
                while let Some(s) = seq.next_element::<String>()? {
                    let r = Regex::new(&s)
                        .map_err(|e| de::Error::custom(format!("Invalid regex: {}", e)))?;
                    regexes.push(r);
                }
                Ok(Split::Regexes(regexes))
            }
        }

        deserializer.deserialize_any(SplitVisitor)
    }
}

impl<'de> Deserialize<'de> for ColumnSetting {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct ColumnStruct {
            #[serde(default = "default_true")]
            filter: bool,
            #[serde(default)]
            hidden: bool,
            name: String,
        }

        fn default_true() -> bool {
            true
        }

        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Input {
            Str(String),
            Obj(ColumnStruct),
        }

        match Input::deserialize(deserializer)? {
            Input::Str(name) => Ok(ColumnSetting {
                filter: true,
                hidden: false,
                name,
            }),
            Input::Obj(obj) => Ok(ColumnSetting {
                filter: obj.filter,
                hidden: obj.hidden,
                name: obj.name,
            }),
        }
    }
}
