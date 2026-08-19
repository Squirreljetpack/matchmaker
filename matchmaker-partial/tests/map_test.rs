#![allow(unused)]

use matchmaker_partial::*;
use matchmaker_partial_macros::partial;
use std::collections::HashMap;

macro_rules! vec_ {
    ($($elem:expr),* $(,)?) => {
        vec![$($elem.into()),*]
    };
}

#[test]
fn test_map_set_path() {
    #[partial(path, unwrap)]
    #[derive(Debug, PartialEq, Default, Clone)]
    struct MapStruct {
        #[partial(recurse = "")]
        pub binds: HashMap<String, String>,
    }

    let mut p = PartialMapStruct::default();

    // Test 1: Setting a key via path (head="binds", tail=["ctrl-c"])
    let res = p.set(&["binds".into(), "ctrl-c".into()], &["Quit(1)".into()]);
    assert!(
        res.is_ok(),
        "Setting map key via path should succeed, got {:?}",
        res
    );
    assert_eq!(p.binds.get("ctrl-c"), Some(&"Quit(1)".to_string()));

    // Test 2: Setting whole map via field (head="binds", tail=[])
    let mut p2 = PartialMapStruct::default();
    let res2 = p2.set(&["binds".into()], &["ctrl-q".into(), "Exit".into()]);
    assert!(
        res2.is_ok(),
        "Setting map field directly should succeed, got {:?}",
        res2
    );
    assert_eq!(p2.binds.get("ctrl-q"), Some(&"Exit".to_string()));
}

#[test]
fn test_map_apply_overwrite_vs_extend() {
    #[partial]
    #[derive(Debug, PartialEq, Default, Clone)]
    struct Val {
        pub x: i32,
    }

    #[partial(unwrap)]
    #[derive(Debug, PartialEq, Default, Clone)]
    struct ExtendStruct {
        #[partial(recurse)]
        pub map: HashMap<String, Val>,
    }

    #[partial]
    #[derive(Debug, PartialEq, Default, Clone)]
    struct OverwriteStruct {
        #[partial(recurse)]
        pub map: HashMap<String, Val>,
    }

    // 1. Test Extend (unwrapped)
    let mut ext = ExtendStruct::default();
    ext.map.insert("a".to_string(), Val { x: 1 });

    let mut p_ext = PartialExtendStruct::default();
    let p_val = PartialVal { x: Some(2) };
    p_ext.map.insert("a".to_string(), p_val);
    p_ext.map.insert("b".to_string(), PartialVal { x: Some(3) });

    ext.apply(p_ext);
    assert_eq!(ext.map.get("a").unwrap().x, 2);
    assert_eq!(ext.map.get("b").unwrap().x, 3);
    assert_eq!(ext.map.len(), 2);

    // 2. Test Overwrite (wrapped recursive -> merge-apply)
    // Actually, user said non_unwrapped = overwrite.
    // BUT they also said recursive = zip-apply (merge-apply for maps).
    // So if it's recursive, it should merge.

    let mut ovr = OverwriteStruct::default();
    ovr.map.insert("a".to_string(), Val { x: 1 });

    let mut p_ovr = PartialOverwriteStruct::default();
    let mut p_map = HashMap::new();
    let p_val = PartialVal { x: Some(2) };
    p_map.insert("b".to_string(), p_val);
    p_ovr.map = Some(p_map);

    ovr.apply(p_ovr);
    // If merge-apply, "a" should be preserved, "b" should be added
    assert_eq!(ovr.map.get("a").unwrap().x, 1);
    assert_eq!(ovr.map.get("b").unwrap().x, 2);
    assert_eq!(ovr.map.len(), 2);
}
