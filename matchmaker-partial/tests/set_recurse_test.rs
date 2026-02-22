use matchmaker_partial::{Apply, Set};
use matchmaker_partial_macros::partial;
use serde::Deserialize;

#[partial(path)]
#[derive(Debug, PartialEq, Default, Clone, Deserialize)]
struct Nested {
    pub name: String,
    pub kind: usize,
}

#[allow(unused)]
#[partial(path)]
#[derive(Debug, PartialEq, Default, Deserialize)]
struct Config {
    #[partial(set = "recurse")]
    pub tags: Vec<Nested>,
}

#[test]
fn test_set_recurse_on_vec() {
    let mut partial = PartialConfig::default();
    partial
        .set(&["tags".into(), "name".into()], &["alpha".into()])
        .unwrap();

    let expected_tags = vec![Nested {
        name: "alpha".into(),
        kind: 0,
    }];

    assert_eq!(partial.tags, Some(expected_tags));
}

#[partial(path, recurse)]
#[derive(Debug, PartialEq, Default)]
struct RecurseConfig {
    #[partial(set = "recurse")]
    pub tags: Vec<Nested>,
}

#[test]
fn test_set_recurse_on_recursive_vec() {
    let mut partial = PartialRecurseConfig::default();
    partial
        .set(&["tags".into(), "name".into()], &["beta".into()])
        .unwrap();
    // This will add a new element to the original vector
    partial
        .set(&["tags".into(), "name".into()], &["gamma".into()])
        .unwrap();

    let mut original = RecurseConfig {
        tags: vec![Nested {
            name: "alpha".into(),
            kind: 1,
        }],
    };

    original.apply(partial);

    let expected_tags = vec![
        Nested {
            name: "beta".into(),
            kind: 1,
        }, // first item is modified
        Nested {
            name: "gamma".into(),
            kind: 0,
        }, // new item is created from default
    ];
    assert_eq!(original.tags, expected_tags);
}
