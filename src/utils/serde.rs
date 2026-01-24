#![allow(unused)]
use cli_boilerplate_automation::wbog;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::utils::text::parse_escapes;

pub mod fromstr {
    use serde::{Deserialize, Deserializer, Serializer, de};
    use std::fmt::Display;
    use std::str::FromStr;

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
    Vec(Vec<String>),
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

pub fn escaped_opt_string<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    Ok(opt.map(|s| parse_escapes(&s)))
}

pub fn escaped_opt_char<'de, D>(deserializer: D) -> Result<Option<char>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        Some(s) => {
            let parsed = parse_escapes(&s);
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

pub mod serde_duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ms = duration.as_millis() as u64;
        serializer.serialize_u64(ms)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ms = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(ms))
    }
}

pub mod modifier {
    use ratatui::style::Modifier;

    use serde::{
        Deserialize, Deserializer, Serialize, Serializer,
        de::{self},
    };

    use crate::utils::serde::StringOrVec;

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Modifier, D::Error>
    where
        D: Deserializer<'de>,
    {
        let input = StringOrVec::deserialize(deserializer)?;
        let mut modifier = Modifier::empty();

        let add_modifier = |name: &str, m: &mut Modifier| -> Result<(), D::Error> {
            match name.to_lowercase().as_str() {
                "bold" => {
                    *m |= Modifier::BOLD;
                    Ok(())
                }
                "italic" => {
                    *m |= Modifier::ITALIC;
                    Ok(())
                }
                "underlined" => {
                    *m |= Modifier::UNDERLINED;
                    Ok(())
                }
                // "slow_blink" => {
                //     *m |= Modifier::SLOW_BLINK;
                //     Ok(())
                // }
                // "rapid_blink" => {
                //     *m |= Modifier::RAPID_BLINK;
                //     Ok(())
                // }
                // "reversed" => {
                //     *m |= Modifier::REVERSED;
                //     Ok(())
                // }
                // "dim" => {
                //     *m |= Modifier::DIM;
                //     Ok(())
                // }
                // "crossed_out" => {
                //     *m |= Modifier::CROSSED_OUT;
                //     Ok(())
                // }
                "none" => {
                    *m = Modifier::empty();
                    Ok(())
                } // reset all modifiers
                other => Err(de::Error::custom(format!("invalid modifier '{}'", other))),
            }
        };

        match input {
            StringOrVec::String(s) => add_modifier(&s, &mut modifier)?,
            StringOrVec::Vec(list) => {
                for item in list {
                    add_modifier(&item, &mut modifier)?;
                }
            }
        }

        Ok(modifier)
    }

    pub fn serialize<S>(modifier: &Modifier, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut mods = Vec::new();

        if modifier.contains(Modifier::BOLD) {
            mods.push("bold");
        }
        if modifier.contains(Modifier::ITALIC) {
            mods.push("italic");
        }
        if modifier.contains(Modifier::UNDERLINED) {
            mods.push("underlined");
        }
        // add other flags if needed
        // if modifier.contains(Modifier::DIM) { mods.push("dim"); }

        match mods.len() {
            0 => serializer.serialize_str("none"),
            1 => serializer.serialize_str(mods[0]),
            _ => mods.serialize(serializer),
        }
    }
}
