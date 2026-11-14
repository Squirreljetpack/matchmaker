use std::marker::PhantomData;

use cba::wbog;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{
        self, MapAccess, SeqAccess, Visitor,
        value::{
            MapAccessDeserializer, SeqAccessDeserializer, StrDeserializer, StringDeserializer,
        },
    },
};

use crate::utils::string::resolve_escapes;

/// Deserialize a value represented as either a string or a struct/table without
/// masking inner deserialization errors behind `#[serde(untagged)]`.
pub fn deserialize_string_or_struct<'de, D, S, M, T>(
    deserializer: D,
    expecting: &'static str,
    from_str: fn(S) -> T,
    from_map: fn(M) -> T,
) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    S: Deserialize<'de>,
    M: Deserialize<'de>,
{
    struct StringOrStructVisitor<S, M, T> {
        expecting: &'static str,
        from_str: fn(S) -> T,
        from_map: fn(M) -> T,
        _marker: PhantomData<(S, M)>,
    }

    impl<'de, S, M, T> Visitor<'de> for StringOrStructVisitor<S, M, T>
    where
        S: Deserialize<'de>,
        M: Deserialize<'de>,
    {
        type Value = T;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str(self.expecting)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            S::deserialize(StrDeserializer::new(v)).map(self.from_str)
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            S::deserialize(StringDeserializer::new(v)).map(self.from_str)
        }

        fn visit_map<A>(self, map: A) -> Result<Self::Value, A::Error>
        where
            A: MapAccess<'de>,
        {
            M::deserialize(MapAccessDeserializer::new(map)).map(self.from_map)
        }
    }

    deserializer.deserialize_any(StringOrStructVisitor {
        expecting,
        from_str,
        from_map,
        _marker: PhantomData,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum StringOrVec {
    String(String),
    Vec(Vec<String>),
}

impl Serialize for StringOrVec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            StringOrVec::String(s) => serializer.serialize_str(s),
            StringOrVec::Vec(v) => v.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for StringOrVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct StringOrVecVisitor;

        impl<'de> Visitor<'de> for StringOrVecVisitor {
            type Value = StringOrVec;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a string or a sequence of strings")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StringOrVec::String(v.to_owned()))
            }

            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(StringOrVec::String(v))
            }

            fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                Vec::<String>::deserialize(SeqAccessDeserializer::new(seq)).map(StringOrVec::Vec)
            }
        }

        deserializer.deserialize_any(StringOrVecVisitor)
    }
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
