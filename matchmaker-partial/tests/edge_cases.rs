//! Edge-case coverage for the `#[partial]` macro: paths that the main test
//! suite doesn't exercise — raw identifiers, `serde(with)` wrapping, skipped
//! fields, generics filtering (const/lifetime), collection merge/clear,
//! `BTree*` collections, and the various `attr`/`derive` option forms.

#![allow(unused)]

use matchmaker_partial::*;
use matchmaker_partial_macros::partial;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

// ---------------------------------------------------------------------------
// Raw identifiers
// ---------------------------------------------------------------------------

#[partial(path, merge)]
#[derive(Debug, PartialEq, Default)]
struct RawIdents {
    #[partial(alias = "ty")]
    pub r#type: i32,
    pub r#loop: Vec<i32>,
}

#[test]
fn test_raw_identifiers() {
    let mut p = PartialRawIdents::default();

    // Set arm uses the unraw'd name...
    p.set(&["type".into()], &["1".into()]).unwrap();
    assert_eq!(p.r#type, Some(1));
    // ...and any aliases.
    p.set(&["ty".into()], &["2".into()]).unwrap();
    assert_eq!(p.r#type, Some(2));
    p.set(&["loop".into()], &["3".into()]).unwrap();
    assert_eq!(p.r#loop, Some(vec![3]));

    // Apply + merge use the raw ident on the original struct.
    let mut base = RawIdents::default();
    base.apply(p);
    assert_eq!(base.r#type, 2);
    assert_eq!(base.r#loop, vec![3]);
}

// ---------------------------------------------------------------------------
// serde attribute handling
// ---------------------------------------------------------------------------

#[test]
fn test_serde_with_wrap_on_collection() {
    use serde::Deserializer;

    mod my_with {
        use super::*;
        use serde::Deserializer;
        pub fn deserialize<'de, D>(d: D) -> Result<Vec<i32>, D::Error>
        where
            D: Deserializer<'de>,
        {
            let v = Vec::<i32>::deserialize(d)?;
            Ok(v.into_iter().map(|x| x * 10).collect())
        }
    }

    // Non-Option field with `with = "mod"`: the partial gets a rewritten
    // `deserialize_with = "wrapper"` pointing at a generated fn that calls
    // `mod::deserialize`; the original keeps its `with` attribute.
    #[partial(derive(Debug, PartialEq, Deserialize))]
    #[derive(Deserialize, Debug, PartialEq)]
    struct Wrapped {
        #[serde(with = "my_with")]
        nums: Vec<i32>,
    }

    let orig: Wrapped = toml::from_str("nums = [1, 2]").unwrap();
    assert_eq!(orig.nums, vec![10, 20], "original should keep `with`");

    let p: PartialWrapped = toml::from_str("nums = [1, 2]").unwrap();
    assert_eq!(p.nums, Some(vec![10, 20]), "partial should use the wrapper");
}

#[test]
fn test_option_field_custom_deserializer() {
    use serde::Deserializer;

    fn opt_char_upper<'de, D>(d: D) -> Result<Option<char>, D::Error>
    where
        D: Deserializer<'de>,
    {
        Ok(Option::<char>::deserialize(d)?.map(|c| c.to_ascii_uppercase()))
    }

    // `Option<T>` field + custom deserializer: the field type is unchanged in
    // the partial, so the attribute is mirrored verbatim (no wrapper), and the
    // leaf path-setter goes through the custom fn too.
    #[partial(path, derive(Debug, PartialEq, Deserialize))]
    #[derive(Deserialize, Debug, PartialEq)]
    struct Cfg {
        #[serde(deserialize_with = "opt_char_upper")]
        sep: Option<char>,
    }

    let orig: Cfg = toml::from_str("sep = 'x'").unwrap();
    assert_eq!(orig.sep, Some('X'));

    let p: PartialCfg = toml::from_str("sep = 'x'").unwrap();
    assert_eq!(p.sep, Some('X'));

    let mut p2 = PartialCfg::default();
    p2.set(&["sep".into()], &["y".into()]).unwrap();
    assert_eq!(p2.sep, Some('Y'));
}

#[test]
fn test_serialize_with_same_type() {
    use serde::Serializer;

    fn double_char<S: Serializer>(v: &Option<char>, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(&v.map(|c| format!("{c}{c}")).unwrap_or_default())
    }

    // Same-type field: `serialize_with` is kept on the original and mirrored.
    #[partial(derive(Debug, PartialEq, Deserialize, Serialize))]
    #[derive(Deserialize, Debug, PartialEq, Serialize)]
    struct S {
        #[serde(serialize_with = "double_char")]
        c: Option<char>,
    }

    let p = PartialS { c: Some('q') };
    let toml = toml::to_string(&p).unwrap();
    assert_eq!(toml.trim(), "c = \"qq\"");
}

#[test]
fn test_skipped_field_with_custom_deserializer() {
    use serde::Deserializer;

    fn custom<'de, D>(d: D) -> Result<Vec<i32>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let v = Vec::<i32>::deserialize(d)?;
        Ok(v.into_iter().map(|x| x + 1).collect())
    }

    #[partial(derive(Debug, PartialEq, Deserialize))]
    #[derive(Deserialize, Debug, PartialEq)]
    struct S {
        #[partial(skip)]
        #[serde(deserialize_with = "custom")]
        skipped: Vec<i32>,
        kept: i32,
    }

    // The skipped field is absent from the partial (this literal only compiles
    // if `PartialS` has exactly one field).
    let p = PartialS { kept: Some(1) };
    assert_eq!(p.kept, Some(1));

    // The original still has (and uses) its custom deserializer.
    let orig: S = toml::from_str("skipped = [1]\nkept = 0").unwrap();
    assert_eq!(orig.skipped, vec![2]);
    assert_eq!(orig.kept, 0);
}

// ---------------------------------------------------------------------------
// Struct-level option forms
// ---------------------------------------------------------------------------

#[test]
fn test_derive_without_parens_suppresses_derives() {
    #[partial(derive)]
    #[derive(Debug, PartialEq)]
    struct D {
        x: i32,
    }

    // Only the auto `#[derive(Default)]` is emitted — no Debug/Clone/etc.
    let p = PartialD::default();
    assert_eq!(p.x, None);
}

#[test]
fn test_attr_clear_stops_field_attr_mirroring() {
    #[partial(attr(clear), derive(Debug, PartialEq, Deserialize))]
    #[derive(Deserialize, Debug, PartialEq)]
    struct S {
        #[serde(rename = "renamed")]
        x: i32,
    }

    // The partial doesn't get the `rename`, so it reads the field's own name...
    let p: PartialS = toml::from_str("x = 1").unwrap();
    assert_eq!(p.x, Some(1));
    // ...and the renamed key is ignored (serde skips unknown fields).
    let p: PartialS = toml::from_str("renamed = 1").unwrap();
    assert_eq!(p.x, None, "rename must not be mirrored to the partial");

    // The original keeps the rename.
    let orig: S = toml::from_str("renamed = 1").unwrap();
    assert_eq!(orig.x, 1);
}

#[test]
fn test_generic_field_with_custom_deserializer_not_mirrored() {
    use serde::Deserializer;

    fn custom<'de, D, T>(d: D) -> Result<T, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        T::deserialize(d)
    }

    // The field type references a struct generic, so no wrapper fn can be
    // generated: the custom deserializer stays on the original only, and the
    // partial's field parses as a plain `Option<T>`.
    #[partial(derive(Debug, PartialEq, Deserialize))]
    #[derive(Deserialize, Debug, PartialEq)]
    #[serde(bound(deserialize = "T: Deserialize<'de>"))]
    struct Foo<T: Default> {
        #[serde(deserialize_with = "custom")]
        v: T,
    }

    let p: PartialFoo<i32> = toml::from_str("v = 5").unwrap();
    assert_eq!(p.v, Some(5));
}

// ---------------------------------------------------------------------------
// Generics filtering: const and lifetime params
// ---------------------------------------------------------------------------

// `std` only implements `Default` for `[T; N]` per concrete size, so the
// field must be a `Vec` to keep the auto-generated `#[derive(Default)]` on
// the partial valid. The point is that the const param survives into
// `PartialArr<const N: usize>`.
#[partial]
#[derive(Debug, PartialEq)]
struct Arr<const N: usize> {
    data: Vec<[i32; N]>,
}

#[test]
fn test_const_generic() {
    let mut base = Arr::<3> {
        data: vec![[1, 2, 3]],
    };
    let p = PartialArr::<3> {
        data: Some(vec![[4, 5, 6]]),
    };
    base.apply(p);
    assert_eq!(base.data, vec![[4, 5, 6]]);
}

#[partial]
struct Ref<'a> {
    s: &'a str,
}

#[test]
fn test_lifetime_generic() {
    let mut base = Ref { s: "a" };
    let p = PartialRef { s: Some("b") };
    base.apply(p);
    assert_eq!(base.s, "b");
}

#[test]
fn test_empty_struct() {
    #[partial]
    #[derive(Debug, PartialEq)]
    struct Empty {}

    let p = PartialEmpty {};
    let mut base = Empty {};
    base.apply(p);
    assert_eq!(base, Empty {});
}

// ---------------------------------------------------------------------------
// Collections: BTree* and merge/clear
// ---------------------------------------------------------------------------

#[partial]
#[derive(Debug, PartialEq, Default)]
struct BTreeStruct {
    #[partial(recurse)]
    map: BTreeMap<String, Val>,
    set: BTreeSet<i32>,
}

#[partial]
#[derive(Debug, PartialEq, Default)]
struct Val {
    x: i32,
}

#[test]
fn test_btree_collections() {
    let mut base = BTreeStruct::default();
    base.map.insert("a".into(), Val { x: 1 });
    base.set.insert(10);

    let p = PartialBTreeStruct {
        map: Some(BTreeMap::from([
            ("a".into(), PartialVal { x: Some(2) }),
            ("b".into(), PartialVal { x: Some(3) }),
        ])),
        set: Some(BTreeSet::from([20])),
    };

    base.apply(p);

    assert_eq!(base.map.get("a").unwrap().x, 2, "recursive map merges");
    assert_eq!(base.map.get("b").unwrap().x, 3);
    assert!(base.set.contains(&20), "set overwrites");
    assert!(!base.set.contains(&10));
}

#[partial(merge)]
#[derive(Debug, PartialEq, Default)]
struct MergeColl {
    #[partial(unwrap)]
    list: Vec<i32>,
    map: HashMap<String, i32>,
}

#[test]
fn test_collection_merge_and_clear() {
    let mut p1 = PartialMergeColl {
        list: vec![1, 2],
        map: Some(HashMap::from([("a".into(), 10)])),
    };

    let p2 = PartialMergeColl {
        list: vec![3],
        map: Some(HashMap::from([("b".into(), 20)])),
    };

    p1.merge(p2);
    assert_eq!(p1.list, vec![1, 2, 3]);
    let map = p1.map.as_ref().unwrap();
    assert_eq!(map.get("a"), Some(&10));
    assert_eq!(map.get("b"), Some(&20));

    p1.clear();
    assert!(p1.list.is_empty(), "unwrap collections are cleared");
    assert_eq!(p1.map, None, "wrapped collections reset to None");
}

// Recursive-option merging requires the inner partial to implement `Merge`
// (the generated code calls `Merge::merge` on the two inner partials).
#[partial(merge)]
#[derive(Debug, PartialEq, Default)]
struct InnerVal {
    x: i32,
}

#[partial(recurse, merge)]
#[derive(Debug, PartialEq, Default)]
struct MergeOpt {
    inner: Option<InnerVal>,
}

#[test]
fn test_recursive_option_merge() {
    // Both sides present: merge recursively.
    let mut p1 = PartialMergeOpt {
        inner: Some(PartialInnerVal { x: Some(1) }),
    };
    let p2 = PartialMergeOpt {
        inner: Some(PartialInnerVal { x: Some(2) }),
    };
    p1.merge(p2);
    assert_eq!(p1.inner.unwrap().x, Some(2));

    // One side missing: adopt the other.
    let mut p3 = PartialMergeOpt::default();
    let p4 = PartialMergeOpt {
        inner: Some(PartialInnerVal { x: Some(5) }),
    };
    p3.merge(p4);
    assert_eq!(p3.inner.unwrap().x, Some(5));
}

// ---------------------------------------------------------------------------
// Multiple aliases on one field
// ---------------------------------------------------------------------------

#[partial(path)]
#[derive(Debug, PartialEq, Default)]
struct Aliased {
    #[partial(alias = "a", alias = "b")]
    value: i32,
}

#[test]
fn test_multiple_aliases() {
    let mut p = PartialAliased::default();
    for key in ["value", "a", "b"] {
        p.set(&[key.into()], &["7".into()]).unwrap();
        assert_eq!(p.value, Some(7), "alias {key:?} should work");
    }
}
