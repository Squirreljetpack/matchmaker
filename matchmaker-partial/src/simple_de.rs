use crate::errors::SimpleError;
use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, IntoDeserializer, MapAccess, SeqAccess,
    VariantAccess, Visitor,
};
use serde::forward_to_deserialize_any;

#[derive(Debug)]
pub struct SimpleDeserializer<'de> {
    input: &'de [String],
    start: usize,
    // Ok(len) for tuple, Err(field_names) for struct
    consuming: Option<Result<usize, &'static [&'static str]>>,
}

pub fn deserialize<'de, T>(input: &'de [String]) -> Result<T, SimpleError>
where
    T: de::Deserialize<'de>,
{
    let mut de = SimpleDeserializer::from_slice(input);
    T::deserialize(&mut de)
}

impl<'de> SimpleDeserializer<'de> {
    pub fn from_slice(input: &'de [String]) -> Self {
        Self {
            input,
            start: 0,
            consuming: None,
        }
    }

    fn expect_single(&self) -> Result<&'de str, SimpleError> {
        self.input
            .get(self.start)
            .map(|s| s.as_str())
            .ok_or(SimpleError::ExpectedSingle)
    }

    fn with_sub<T, F>(
        &mut self,
        f: F,
        consuming: impl Into<Option<Result<usize, &'static [&'static str]>>>,
    ) -> Result<T, SimpleError>
    where
        F: FnOnce(&mut Self) -> Result<T, SimpleError>,
    {
        let mut sub = Self {
            input: &self.input[self.start..],
            start: 0,
            consuming: None,
        };
        sub.consuming = consuming.into();
        let ret = f(&mut sub)?;
        self.start += sub.start;
        Ok(ret)
    }
}

macro_rules! impl_number {
    ($name:ident, $ty:ty, $visit:ident, $expect:literal) => {
        fn $name<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            let s = self.expect_single()?;
            let v: $ty = s.parse().map_err(|_| SimpleError::InvalidType {
                expected: $expect,
                found: s.to_string(),
            })?;
            self.start += 1;
            visitor.$visit(v)
        }
    };
}

impl<'de, 'a> Deserializer<'de> for &'a mut SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.start >= self.input.len() {
            return visitor.visit_unit();
        }

        let s = &self.input[self.start];
        let val = if s == "true" {
            visitor.visit_bool(true)?
        } else if s == "false" {
            visitor.visit_bool(false)?
        } else if s.is_empty() || s == "()" {
            visitor.visit_unit()?
        } else if let Ok(i) = s.parse::<i64>() {
            visitor.visit_i64(i)?
        } else if let Ok(f) = s.parse::<f64>() {
            visitor.visit_f64(f)?
        } else {
            visitor.visit_str(s)?
        };
        self.start += 1;
        Ok(val)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        let val = match s {
            "true" | "" => visitor.visit_bool(true)?,
            "false" => visitor.visit_bool(false)?,
            _ => {
                return Err(SimpleError::InvalidType {
                    expected: "a boolean",
                    found: s.to_string(),
                });
            }
        };
        self.start += 1;
        Ok(val)
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        let mut chars = s.chars();
        let c = chars.next().ok_or(SimpleError::InvalidType {
            expected: "a char",
            found: s.to_string(),
        })?;
        if chars.next().is_some() {
            return Err(SimpleError::InvalidType {
                expected: "a single character",
                found: s.to_string(),
            });
        }
        self.start += 1;
        visitor.visit_char(c)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let val = visitor.visit_str(self.expect_single()?)?;
        self.start += 1;
        Ok(val)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let val = visitor.visit_string(self.expect_single()?.to_string())?;
        self.start += 1;
        Ok(val)
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        if s.is_empty() || s == "()" {
            self.start += 1;
            visitor.visit_unit()
        } else {
            Err(SimpleError::InvalidType {
                expected: "unit",
                found: s.to_string(),
            })
        }
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.start >= self.input.len() {
            visitor.visit_none()
        } else if self.input[self.start] == "null" {
            self.start += 1;
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.with_sub(|s| visitor.visit_seq(s), None)
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.with_sub(|s| visitor.visit_seq(s), Ok(len))
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.with_sub(|s| visitor.visit_map(s), None)
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.with_sub(|s| visitor.visit_map(s), Err(fields))
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        // we don't actually use the passed in variants
        self.with_sub(|s| visitor.visit_enum(s), Err(variants))
    }

    impl_number!(deserialize_i8, i8, visit_i8, "an i8");
    impl_number!(deserialize_i16, i16, visit_i16, "an i16");
    impl_number!(deserialize_i32, i32, visit_i32, "an i32");
    impl_number!(deserialize_i64, i64, visit_i64, "an i64");
    impl_number!(deserialize_u8, u8, visit_u8, "a u8");
    impl_number!(deserialize_u16, u16, visit_u16, "a u16");
    impl_number!(deserialize_u32, u32, visit_u32, "a u32");
    impl_number!(deserialize_u64, u64, visit_u64, "a u64");
    impl_number!(deserialize_f32, f32, visit_f32, "an f32");
    impl_number!(deserialize_f64, f64, visit_f64, "an f64");

    forward_to_deserialize_any! { bytes byte_buf identifier ignored_any }
}

// === Implement SeqAccess, MapAccess, EnumAccess, VariantAccess ===

impl<'de, 'a> SeqAccess<'de> for &'a mut SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        if let Some(Ok(len)) = self.consuming {
            if len == 0 {
                return Ok(None);
            }
        }

        if self.start >= self.input.len() {
            return Ok(None);
        }

        let val = self.with_sub(|s| seed.deserialize(s), None)?;

        if let Some(Ok(ref mut len)) = self.consuming {
            *len -= 1;
        }

        Ok(Some(val))
    }
}

impl<'de, 'a> MapAccess<'de> for &'a mut SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if self.start >= self.input.len() {
            return Ok(None);
        }

        let key = if let Some(Err(fields)) = self.consuming {
            let key = &self.input[self.start];
            if !fields.contains(&key.as_str()) {
                return Ok(None);
            } else {
                self.start += 1;
                seed.deserialize(key.clone().into_deserializer())?
            }
        } else {
            self.with_sub(|s| seed.deserialize(s), None)?
        };

        Ok(Some(key))
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let val = self.with_sub(|s| seed.deserialize(s), None)?;
        Ok(val)
    }
}

impl<'de, 'a> EnumAccess<'de> for &'a mut SimpleDeserializer<'de> {
    type Error = SimpleError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let val = self.with_sub(|s| seed.deserialize(s), None)?;
        Ok((val, self))
    }
}

impl<'de, 'a> VariantAccess<'de> for &'a mut SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.with_sub(|s| seed.deserialize(s), None)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_struct("", fields, visitor)
    }
}
