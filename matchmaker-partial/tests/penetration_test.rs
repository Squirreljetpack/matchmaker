use matchmaker_partial::*;
use matchmaker_partial_macros::partial;
use serde::{Deserialize, Serialize};

#[partial(path, merge)]
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Inner {
    pub x: i32,
}

#[partial(recurse, merge, path)]
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct Outer {
    pub opt_inner: Option<Inner>,
    pub opt_vec: Option<Vec<Inner>>,
    pub o_vec: Vec<Inner>,
}

#[partial(unwrap)]
#[derive(Default, Debug, PartialEq, Clone, Serialize, Deserialize)]
pub struct UnwrappedOuter {
    pub opt_inner: Option<Inner>,
    pub opt_vec: Option<Vec<Inner>>,
}

#[test]
fn test_penetration() {
    // Should compile if types are correct
    let p = PartialOuter {
        opt_inner: Some(PartialInner { x: Some(10) }),
        opt_vec: Some(vec![PartialInner { x: Some(20) }]),
        o_vec: Some(vec![PartialInner { x: Some(20) }]),
    };

    let mut base = Outer::default();
    base.apply(p);
    assert_eq!(base.opt_inner.unwrap().x, 10);
    assert_eq!(base.opt_vec.unwrap()[0].x, 20);
}

#[test]
fn test_unwrap_option() {
    // Should compile if types are correct
    // These should NOT be Options in PartialUnwrappedOuter
    let p = PartialUnwrappedOuter {
        opt_inner: Inner { x: 10 },
        opt_vec: vec![Inner { x: 20 }],
    };

    let mut base = UnwrappedOuter::default();
    base.apply(p);
    assert_eq!(base.opt_inner.unwrap().x, 10);
    assert_eq!(base.opt_vec.unwrap()[0].x, 20);
}
