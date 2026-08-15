use cba::wbog;
use serde::{Deserialize, Deserializer, Serialize};

use crate::utils::string::resolve_escapes;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}
impl Default for StringOrVec {
    fn default() -> Self {
        StringOrVec::String(String::new())
    }
}

pub fn bounded_usize<'de, D, const MIN: usize, const MAX: usize>(d: D) -> Result<usize, D::Error>
where
    D: Deserializer<'de>,
{
    let v = usize::deserialize(d)?;
    if v < MIN {
        wbog!("{} exceeded the the limit of {} and was clamped.", v, MIN);
        Ok(MIN)
    } else if v > MAX {
        wbog!("{} exceeded the the limit of {} and was clamped.", v, MAX);
        Ok(MAX)
    } else {
        Ok(v)
    }
}

/// Serde helpers for `Vec<OsString>` config fields.
///
/// Serde's own `OsString` impls only accept a platform-variant enum form
/// (`[{ Unix = "..." }]`), so shell/command config is deserialized from plain
/// strings instead: `["bash", "-c"]`.
pub mod os_strings {
    use std::ffi::OsString;

    use serde::{Deserialize, Deserializer};

    pub fn deserialize<'de, D>(d: D) -> Result<Vec<OsString>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Vec::<String>::deserialize(d).map(|v| v.into_iter().map(OsString::from).collect())
    }
}

pub fn escaped_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(|s| resolve_escapes(&s)))
}

pub fn escaped_opt_char<'de, D>(deserializer: D) -> Result<Option<char>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let parsed = resolve_escapes(&s);
            let mut chars = parsed.chars();
            let first = chars
                .next()
                .ok_or_else(|| serde::de::Error::custom("escaped string is empty"))?;
            if chars.next().is_some() {
                return Err(serde::de::Error::custom(
                    "escaped string must be exactly one character",
                ));
            }
            Ok(Some(first))
        }
        None => Ok(None),
    }
}
