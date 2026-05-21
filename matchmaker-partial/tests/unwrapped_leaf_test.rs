use matchmaker_partial::Set;
use matchmaker_partial_macros::partial;

#[allow(unused)]
#[partial(path)]
#[derive(Default, Debug, PartialEq)]
struct UnwrappedLeaf {
    #[partial(unwrap)]
    pub x: i32,
}

#[test]
fn test_unwrapped_leaf_set() {
    let mut p = PartialUnwrappedLeaf::default();
    p.set(&["x".to_string()], &["42".to_string()]).unwrap();
    assert_eq!(p.x, 42);
}
