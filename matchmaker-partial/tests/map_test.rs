#![allow(unused)]

use matchmaker_partial::*;
use matchmaker_partial_macros::partial;
use std::collections::BTreeMap;

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
        pub binds: BTreeMap<String, String>,
    }

    let mut p = PartialMapStruct::default();

    // Test 1: Setting a key via path (head="binds", tail=["ctrl-c"])
    let res = p.set(&vec_!["binds", "ctrl-c"], &vec_!["Quit"]);
    assert!(
        res.is_ok(),
        "Setting map key via path should succeed, got {:?}",
        res
    );
    assert_eq!(p.binds.get("ctrl-c"), Some(&"Quit".to_string()));

    // Test 2: Setting whole map via field (head="binds", tail=[])
    let mut p2 = PartialMapStruct::default();
    let res2 = p2.set(&vec_!["binds"], &vec_!["ctrl-q", "Exit"]);
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
        pub map: BTreeMap<String, Val>,
    }

    #[partial]
    #[derive(Debug, PartialEq, Default, Clone)]
    struct OverwriteStruct {
        #[partial(recurse)]
        pub map: BTreeMap<String, Val>,
    }

    // 1. Test Extend (unwrapped)
    let mut ext = ExtendStruct::default();
    ext.map.insert("a".to_string(), Val { x: 1 });

    let mut p_ext = PartialExtendStruct::default();
    let mut p_val = PartialVal::default();
    p_val.x = Some(2);
    p_ext.map.insert("a".to_string(), p_val);
    p_ext.map.insert("b".to_string(), {
        let mut v = PartialVal::default();
        v.x = Some(3);
        v
    });

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
    let mut p_map = BTreeMap::new();
    let mut p_val = PartialVal::default();
    p_val.x = Some(2);
    p_map.insert("b".to_string(), p_val);
    p_ovr.map = Some(p_map);

    ovr.apply(p_ovr);
    // If merge-apply, "a" should be preserved, "b" should be added
    assert_eq!(ovr.map.get("a").unwrap().x, 1);
    assert_eq!(ovr.map.get("b").unwrap().x, 2);
    assert_eq!(ovr.map.len(), 2);
}
