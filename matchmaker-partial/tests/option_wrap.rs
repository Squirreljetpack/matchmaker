#![allow(unused)]

use matchmaker_partial::*;
use matchmaker_partial_macros::partial;
use serde::{Deserialize, Serialize};

macro_rules! vec_ {
    ($($elem:expr),* $(,)?) => {
        vec![$($elem.into()),*]
    };
}

#[partial(path)]
#[derive(Debug, PartialEq, Default, Clone, Serialize, Deserialize)]
pub struct Nested {
    pub d: Option<usize>,
    pub e: String,
}

#[test]
fn test_collection_set_recursion() {
    #[partial(recurse, path)]
    #[derive(Debug, PartialEq, Default, Clone, Serialize, Deserialize)]
    struct CollectionStruct {
        pub recurse: Vec<Nested>, // Option<Vec<PartialNested>>
        #[partial(recurse, unwrap)]
        pub recurse_unwrap: Vec<Nested>, // Vec<PartialNested>
    }

    let mut p_path = PartialCollectionStruct::default();

    p_path
        .set(&["recurse_unwrap".into()], &vec_!["d", "99"])
        .unwrap();
    assert_eq!(p_path.recurse_unwrap.len(), 1);
    assert_eq!(p_path.recurse_unwrap[0].d.unwrap(), 99);

    // set singleton on recurse Vec -> extend
    p_path.set(&["recurse".into()], &vec_!["d", "99"]).unwrap();
    assert_eq!(
        p_path.recurse.as_ref().map(|s| s.len()).unwrap_or_default(),
        1
    );
    assert_eq!(p_path.recurse.unwrap()[0].d.unwrap(), 99);
}

#[partial(merge, path)]
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Inner {
    pub val: i32,
}

#[partial(recurse, merge, path)]
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Outer {
    pub opt_inner: Option<Inner>,
    pub opt_vec: Option<Vec<Inner>>,
}

#[test]
fn test_option_recurse_behavior() {
    let mut outer = Outer::default();
    let mut p = PartialOuter::default();

    // Test recursion into Option<Inner>
    p.opt_inner = Some(PartialInner { val: Some(10) });

    Apply::apply(&mut outer, p);

    // Should have promoted None -> Some(Inner) and applied PartialInner
    assert_eq!(outer.opt_inner, Some(Inner { val: 10 }));

    // p.opt_vec = Some(vec![PartialInner { val: Some(10) }]);
}

#[test]
fn test_from_implementation() {
    let p = PartialInner { val: Some(42) };
    let inner: Inner = from(p);
    assert_eq!(inner.val, 42);
}
