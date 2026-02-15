use serde::{
    de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor},
    forward_to_deserialize_any,
};

use crate::SimpleError;

pub struct SimpleDeserializer<'de> {
    input: &'de [String],
}

impl<'de> SimpleDeserializer<'de> {
    pub fn from_slice(input: &'de [String]) -> Self {
        Self { input }
    }

    fn expect_single(&self) -> Result<&'de str, SimpleError> {
        if self.input.is_empty() {
            return Err(SimpleError::ExpectedSingle);
        }
        Ok(&self.input[self.input.len() - 1])
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
            visitor.$visit(v)
        }
    };
}

impl<'de> Deserializer<'de> for SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.input.len() {
            0 => visitor.visit_unit(),
            1 => {
                let s = &self.input[self.input.len() - 1];

                if s == "true" {
                    return visitor.visit_bool(true);
                }
                if s == "false" {
                    return visitor.visit_bool(false);
                }
                if s.is_empty() || s == "()" {
                    return visitor.visit_unit();
                }
                if let Ok(i) = s.parse::<i64>() {
                    return visitor.visit_i64(i);
                }
                if let Ok(f) = s.parse::<f64>() {
                    return visitor.visit_f64(f);
                }

                visitor.visit_str(s)
            }
            _ => self.deserialize_seq(visitor),
        }
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        match s {
            "true" => visitor.visit_bool(true),
            "false" => visitor.visit_bool(false),
            _ => Err(SimpleError::InvalidType {
                expected: "a boolean",
                found: s.to_string(),
            }),
        }
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        let mut chars = s.chars();
        let c = chars.next().ok_or_else(|| SimpleError::InvalidType {
            expected: "a char",
            found: s.to_string(),
        })?;
        if chars.next().is_some() {
            return Err(SimpleError::InvalidType {
                expected: "a single character",
                found: s.to_string(),
            });
        }
        visitor.visit_char(c)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_str(self.expect_single()?)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_string(self.expect_single()?.to_string())
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let s = self.expect_single()?;
        if s.is_empty() || s == "()" {
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
        if self.input.is_empty() || (self.input.len() == 1 && self.input[0] == "null") {
            visitor.visit_none()
        } else {
            visitor.visit_some(self)
        }
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SimpleSeqAccess {
            iter: self.input.iter(),
        })
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

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.input.len() != len {
            return Err(SimpleError::InvalidType {
                expected: "tuple of specified length",
                found: format!("{} elements", self.input.len()),
            });
        }
        self.deserialize_seq(visitor)
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
        visitor.visit_map(SimpleMapAccess {
            iter: self.input.iter(),
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        if self.input.is_empty() {
            return Err(SimpleError::InvalidType {
                expected: "enum variant",
                found: "empty input".to_string(),
            });
        }

        let variant = &self.input[0..1];
        let rest = &self.input[1..];
        visitor.visit_enum(SimpleEnumAccess { variant, rest })
    }

    forward_to_deserialize_any! {
        bytes byte_buf identifier ignored_any
    }
}

struct SimpleSeqAccess<'de> {
    iter: std::slice::Iter<'de, String>,
}

impl<'de> SeqAccess<'de> for SimpleSeqAccess<'de> {
    type Error = SimpleError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => {
                let de = SimpleDeserializer {
                    input: std::slice::from_ref(value),
                };
                seed.deserialize(de).map(Some)
            }
            None => Ok(None),
        }
    }
}

struct SimpleMapAccess<'de> {
    iter: std::slice::Iter<'de, String>,
}

impl<'de> MapAccess<'de> for SimpleMapAccess<'de> {
    type Error = SimpleError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(key) => {
                let de = SimpleDeserializer {
                    input: std::slice::from_ref(key),
                };
                seed.deserialize(de).map(Some)
            }
            None => Ok(None),
        }
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        match self.iter.next() {
            Some(value) => {
                let de = SimpleDeserializer {
                    input: std::slice::from_ref(value),
                };
                seed.deserialize(de)
            }
            None => Err(SimpleError::ExpectedSingle),
        }
    }
}

struct SimpleEnumAccess<'de> {
    variant: &'de [String],
    rest: &'de [String],
}

impl<'de> de::EnumAccess<'de> for SimpleEnumAccess<'de> {
    type Error = SimpleError;
    type Variant = SimpleDeserializer<'de>;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let val = seed.deserialize(SimpleDeserializer::from_slice(&self.variant))?;
        Ok((val, SimpleDeserializer::from_slice(self.rest)))
    }
}

impl<'de> de::VariantAccess<'de> for SimpleDeserializer<'de> {
    type Error = SimpleError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        if !self.input.is_empty() {
            return Err(SimpleError::InvalidType {
                expected: "unit variant",
                found: format!("{} elements", self.input.len()),
            });
        }
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(self)
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

pub fn deserialize<'de, T>(input: &'de [String]) -> Result<T, SimpleError>
where
    T: de::Deserialize<'de>,
{
    let de = SimpleDeserializer::from_slice(input);
    T::deserialize(de)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde::de::DeserializeOwned;
    use std::collections::HashMap;

    fn de<T: DeserializeOwned>(input: &[&str]) -> T {
        let data: Vec<String> = input.iter().map(|s| s.to_string()).collect();
        let de = SimpleDeserializer::from_slice(&data);
        T::deserialize(de).unwrap()
    }

    fn de_err<T: DeserializeOwned>(input: &[&str]) {
        let data: Vec<String> = input.iter().map(|s| s.to_string()).collect();
        let de = SimpleDeserializer::from_slice(&data);
        assert!(T::deserialize(de).is_err());
    }

    #[test]
    fn primitives_last() {
        assert_eq!(de::<i32>(&["1", "2"]), 2);
        assert_eq!(de::<bool>(&["false", "true"]), true);
        assert_eq!(de::<String>(&["first", "second"]), "second");
    }

    #[test]
    fn bool_ok() {
        assert_eq!(de::<bool>(&["true"]), true);
        assert_eq!(de::<bool>(&["false"]), false);
    }

    #[test]
    fn bool_err() {
        de_err::<bool>(&["not_bool"]);
    }

    #[test]
    fn integers() {
        assert_eq!(de::<i32>(&["42"]), 42);
        assert_eq!(de::<i8>(&["-5"]), -5);
        assert_eq!(de::<u16>(&["10"]), 10);
    }

    #[test]
    fn floats() {
        assert_eq!(de::<f32>(&["1.5"]), 1.5);
        assert_eq!(de::<f64>(&["2.25"]), 2.25);
    }

    #[test]
    fn char_ok() {
        assert_eq!(de::<char>(&["a"]), 'a');
    }

    #[test]
    fn char_err() {
        de_err::<char>(&["ab"]);
        de_err::<char>(&[""]);
    }

    #[test]
    fn string_ok() {
        assert_eq!(de::<String>(&["hello"]), "hello");
    }

    #[test]
    fn unit_ok() {
        assert_eq!(de::<()>(&[""]), ());
        assert_eq!(de::<()>(&["()"]), ());
    }

    #[test]
    fn unit_err() {
        de_err::<()>(&["not_unit"]);
    }

    #[test]
    fn option_none() {
        assert_eq!(de::<Option<i32>>(&[]), None);
        assert_eq!(de::<Option<i32>>(&["null"]), None);
    }

    #[test]
    fn option_some() {
        assert_eq!(de::<Option<i32>>(&["5"]), Some(5));
    }

    #[test]
    fn vec_of_ints() {
        let v: Vec<i32> = de(&["1", "2", "3"]);
        assert_eq!(v, vec![1, 2, 3]);
    }

    #[test]
    fn vec_of_strings() {
        let v: Vec<String> = de(&["a", "b", "c"]);
        assert_eq!(v, vec!["a", "b", "c"]);
    }

    #[test]
    fn vec_of_options() {
        let v: Vec<Option<i32>> = de(&["1", "null", "3"]);
        assert_eq!(v, vec![Some(1), None, Some(3)]);
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Newtype(i32);

    #[test]
    fn newtype_struct() {
        let n: Newtype = de(&["99"]);
        assert_eq!(n, Newtype(99));
    }

    #[test]
    fn error_on_multiple_scalars() {
        de_err::<i32>(&[]);
    }

    #[test]
    fn struct_map() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct S {
            a: i32,
            b: String,
        }

        let s: S = de(&["a", "10", "b", "hello"]);
        assert_eq!(
            s,
            S {
                a: 10,
                b: "hello".to_string()
            }
        );
    }

    #[test]
    fn hashmap_ok() {
        let m: HashMap<String, i32> = de(&["x", "1", "y", "2"]);
        let mut expected = HashMap::new();
        expected.insert("x".to_string(), 1);
        expected.insert("y".to_string(), 2);
        assert_eq!(m, expected);
    }

    #[test]
    fn deserialize_struct_tuple_enum() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct MyStruct {
            a: i32,
            b: String,
        }

        #[derive(Debug, Deserialize, PartialEq)]
        struct MyTupleStruct(i32, String);

        #[derive(Debug, Deserialize, PartialEq)]
        enum MyEnum {
            Unit,
            Newtype(i32),
            Tuple(i32, i32),
            Struct { x: i32, y: i32 },
        }

        // Struct
        let s: MyStruct = de(&["a", "42", "b", "hello"]);
        assert_eq!(
            s,
            MyStruct {
                a: 42,
                b: "hello".to_string()
            }
        );

        // Tuple struct
        let t: MyTupleStruct = de(&["7", "world"]);
        assert_eq!(t, MyTupleStruct(7, "world".to_string()));

        // Enum unit
        let e: MyEnum = de(&["Unit"]);
        assert_eq!(e, MyEnum::Unit);

        // Enum newtype
        let e: MyEnum = de(&["Newtype", "123"]);
        assert_eq!(e, MyEnum::Newtype(123));

        // Enum tuple
        let e: MyEnum = de(&["Tuple", "1", "2"]);
        assert_eq!(e, MyEnum::Tuple(1, 2));

        // Enum struct
        let e: MyEnum = de(&["Struct", "y", "20", "x", "10"]);
        assert_eq!(e, MyEnum::Struct { x: 10, y: 20 });
    }
}
