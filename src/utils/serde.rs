
#![allow(unused)]
use cli_boilerplate_automation::wbog;
use serde::{Deserialize, Deserializer, Serialize};

pub mod fromstr {
    use std::fmt::Display;
    use std::str::FromStr;
    use serde::{Deserialize, Deserializer, Serializer, de};

    pub fn serialize<T, S>(value: &T, serializer: S) -> Result<S::Ok, S::Error>
    where
    T: Display,
    S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }
    
    pub fn deserialize<'de, T, D>(deserializer: D) -> Result<T, D::Error>
    where
    T: FromStr,
    T::Err: Display,
    D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        T::from_str(&s).map_err(de::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>)
}
impl Default for StringOrVec {
    fn default() -> Self {
        StringOrVec::String(String::new())
    }
}

pub fn bounded_usize<'de, const MAX: usize, D>(d: D) -> Result<usize, D::Error>
where
D: Deserializer<'de>,
{
    let v = usize::deserialize(d)?;
    if v > MAX {
        wbog!("{} exceeded the the limit of {} and was clamped.", v, MAX);
        Ok(MAX)
    } else {
        Ok(v)
        // return Err(serde::de::Error::custom(format!"{} exceeds the maximum of {MAX}"));
    }
}