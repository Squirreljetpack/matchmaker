use matchmaker_partial::SimpleDeserializer;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use std::collections::HashMap;

fn de<T: DeserializeOwned>(input: &[&str]) -> T {
    let data: Vec<String> = input.iter().map(|s| s.to_string()).collect();
    let mut de = SimpleDeserializer::from_slice(&data);
    T::deserialize(&mut de).unwrap()
}

fn de_err<T: DeserializeOwned>(input: &[&str]) {
    let data: Vec<String> = input.iter().map(|s| s.to_string()).collect();
    let mut de = SimpleDeserializer::from_slice(&data);
    assert!(T::deserialize(&mut de).is_err());
}

#[test]
fn primitives() {
    assert_eq!(de::<i32>(&["1", "2"]), 1);
    assert_eq!(de::<bool>(&["false", "true"]), false);
    assert_eq!(de::<String>(&["first", "second"]), "first");
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
