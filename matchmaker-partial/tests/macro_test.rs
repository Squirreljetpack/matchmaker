#![allow(unused)]

macro_rules! vec_ {
    ($($elem:expr),* $(,)?) => {
        vec![$($elem.to_string()),*]
    };
}

#[cfg(test)]
mod tests {
    use matchmaker_partial::*;
    use matchmaker_partial_macros::partial;
    use serde::{Deserialize, Serialize};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn test_make_partial_macro() {
        #[partial]
        struct MyStruct {
            field_i32: i32,
            field_string: String,
            field_option_bool: Option<bool>, // This remains Option<bool> in PartialMyStruct
        }

        // Test if the generated struct exists and has the correct fields
        let _partial_instance = PartialMyStruct {
            field_i32: Some(10),
            field_string: Some("hello".to_string()),
            field_option_bool: Some(true),
        };

        // Test default implementation
        let default_partial = PartialMyStruct::default();
        assert_eq!(default_partial.field_i32, None);
        assert_eq!(default_partial.field_string, None);
        assert_eq!(default_partial.field_option_bool, None);
    }

    #[test]
    fn test_make_partial_macro_with_generics() {
        #[partial]
        struct GenericStruct<T, U>
        where
            T: Default + 'static, // Added 'static for the test with String
            U: Clone + 'static,   // Added 'static for the test with String
        {
            data_t: T,
            data_option_u: Option<U>, // This remains Option<U> in PartialGenericStruct
            data_vec: Vec<T>,
        }

        let partial_instance = PartialGenericStruct {
            data_t: Some(String::default()),
            data_option_u: Some("test".to_string()), // Assign Some(U) or None
            data_vec: Some(vec![String::default()]),
        };

        assert!(partial_instance.data_t.is_some());
        assert!(partial_instance.data_option_u.is_some());
        assert!(partial_instance.data_vec.is_some());

        let default_partial = PartialGenericStruct::<String, String>::default();
        assert!(default_partial.data_t.is_none());
        assert!(default_partial.data_option_u.is_none());
        assert!(default_partial.data_vec.is_none());
    }

    #[test]
    fn test_apply_and_recurse() {
        #[partial]
        #[derive(Debug, PartialEq, Serialize)]
        struct Nested {
            a: i32,
            b: String,
        }

        #[partial]
        #[derive(Debug, PartialEq)]
        struct Test {
            x: i32,
            #[partial(recurse)]
            nested: Nested,
        }

        let mut a = Test {
            x: 1,
            nested: Nested {
                a: 10,
                b: "hello".into(),
            },
        };

        let p = PartialTest {
            x: Some(2),
            nested: PartialNested {
                a: Some(20),
                b: None,
            },
        };

        a.apply(p);

        assert_eq!(
            a,
            Test {
                x: 2,
                nested: Nested {
                    a: 20,
                    b: "hello".into()
                }
            }
        );
    }

    #[test]
    fn test_recurse_with_type_override() {
        // This is a mock partial type.
        #[derive(Default, Debug, PartialEq, Clone, Serialize)]
        struct CustomPartialNested {
            b: Option<String>,
        }

        #[derive(Debug, PartialEq, Serialize)]
        struct Nested {
            a: i32,
            b: String,
        }

        // We need to implement `apply` manually for this test.
        impl Apply for Nested {
            type Partial = CustomPartialNested;
            fn apply(&mut self, partial: Self::Partial) {
                if let Some(b) = partial.b {
                    self.b = b;
                }
            }
        }

        #[partial]
        #[derive(Debug, PartialEq, Serialize)]
        struct TestRecurseOverride {
            #[partial(recurse = "CustomPartialNested")]
            nested: Nested,
        }

        let mut a = TestRecurseOverride {
            nested: Nested {
                a: 10,
                b: "hello".into(),
            },
        };

        let p = PartialTestRecurseOverride {
            nested: CustomPartialNested {
                b: Some("world".into()),
            },
        };

        a.apply(p);

        assert_eq!(
            a.nested,
            Nested {
                a: 10, // unchanged
                b: "world".into(),
            }
        );
    }

    #[test]
    fn test_struct_level_recurse_with_overrides() {
        #[partial]
        #[derive(Debug, PartialEq, Clone)]
        struct Inner {
            pub count: i32,
        }

        #[partial(recurse)] // All fields will attempt to use Partial counterparts by default
        #[derive(Debug, PartialEq)]
        struct Outer {
            pub nested: Inner, // Will be PartialInner

            #[partial(skip)]
            pub sensitive_data: String, // Will be omitted from PartialOuter

            #[partial(recurse = "")]
            pub simple_override: i32, // Will be Option<i32> despite struct-level recurse
        }

        let mut root = Outer {
            nested: Inner { count: 10 },
            sensitive_data: "original".into(),
            simple_override: 100,
        };

        // PartialOuter is generated based on the attributes:
        // 1. 'nested' is PartialInner because of struct-level #[partial(recurse)]
        // 2. 'sensitive_data' is missing because of #[partial(skip)]
        // 3. 'simple_override' is Option<i32> because of #[partial(recurse = "")]
        let p = PartialOuter {
            nested: PartialInner { count: Some(20) },
            simple_override: Some(200),
            // sensitive_data: Some("hacker".into()) // This would fail to compile
        };

        root.apply(p);

        // Verify recursion worked
        assert_eq!(root.nested.count, 20);

        // Verify override worked (applied as Option)
        assert_eq!(root.simple_override, 200);

        // Verify skip worked (original value preserved as it wasn't in the partial)
        assert_eq!(root.sensitive_data, "original");
    }

    #[test]
    fn test_partial_derives() {
        use serde::{Deserialize, Serialize};

        // Case 1: Clone all original derives
        #[partial]
        #[derive(Default, Clone, PartialEq, Debug, Deserialize, Serialize)]
        struct Original {
            name: String,
        }

        let p = PartialOriginal {
            name: Some("test".into()),
        };
        let original = Original {
            name: String::new(),
        };
        // Verify Serialize/Deserialize were cloned (by checking if they compile/work)
        let toml = toml::to_string(&p).unwrap();
        assert!(toml.contains("name"));

        // Case 2: Explicit override
        // We don't include Default here to prove it only emits what we asked
        #[partial]
        #[partial(derive(Clone, PartialEq, Debug))]
        struct Explicit {
            id: i32,
        }

        let p1 = PartialExplicit { id: Some(1) };
        let p2 = p1.clone();
        assert_eq!(p1, p2);
        // let toml = toml::to_string(&p1).unwrap(); // compile error
    }

    #[test]
    fn test_partial_merge_and_clear() {
        #[partial(merge)]
        #[derive(Debug, PartialEq, Clone)]
        struct Stats {
            hp: i32,
            mana: i32,
        }

        #[partial(recurse, merge)]
        #[derive(Debug, PartialEq, Clone)]
        struct Character {
            #[partial(recurse = "")]
            name: String,
            stats: Stats,
        }

        // 1. Setup base character
        let mut hero = Character {
            name: "Arthur".into(),
            stats: Stats { hp: 100, mana: 50 },
        };

        // 2. Create first partial (name update)
        let mut p1 = PartialCharacter::default();
        p1.name = Some("King Arthur".into());

        // 3. Create second partial (stats update)
        let mut p2 = PartialCharacter::default();
        p2.stats.hp = Some(150);

        // 4. Merge p2 into p1
        // After this, p1 should have both the new name and the new HP
        p1.merge(p2);

        assert_eq!(p1.name, Some("King Arthur".into()));
        assert_eq!(p1.stats.hp, Some(150));
        assert_eq!(p1.stats.mana, None); // Mana was never touched

        // 5. Apply the merged partial to the hero
        hero.apply(p1);

        assert_eq!(hero.name, "King Arthur");
        assert_eq!(hero.stats.hp, 150);
        assert_eq!(hero.stats.mana, 50); // Mana preserved from original

        // 6. Test clear
        let mut p3 = PartialCharacter::default();
        p3.name = Some("Temporary".into());
        p3.stats.mana = Some(100);

        p3.clear();

        assert_eq!(p3.name, None);
        assert_eq!(p3.stats.mana, None);
        assert_eq!(p3.stats.hp, None);
    }

    // ----------------------------------------
    #[partial(path)]
    #[derive(Debug, PartialEq, Default, Clone, Deserialize)]
    pub struct Nested {
        pub d: usize,
        pub e: String,
    }

    #[partial(path)]
    #[derive(Debug, PartialEq, Default)]
    pub struct Ex {
        pub a: usize,
        pub b: Option<usize>,
        #[partial(recurse)]
        pub c: Nested,
    }

    #[test]
    fn test_path_setting_success() {
        let mut p_ex = PartialEx::default();

        // 1. Test setting a top-level leaf
        let path_a = vec_!["a"];
        p_ex.set(&path_a, &vec_!["42"]).expect("Should set a");
        assert_eq!(p_ex.a, Some(42));

        // 2. Test setting a nested leaf
        let path_c_d = vec_!["c", "d"];
        p_ex.set(&path_c_d, &vec_!["100"]).expect("Should set c.d");
        assert_eq!(p_ex.c.d, Some(100));
    }

    #[test]
    fn test_path_setting_errors() {
        let mut p_ex = PartialEx::default();

        // 1. Missing Field
        let path_err = vec_!["unknown"];
        let res = p_ex.set(&path_err, &vec_!["1"]);
        assert_eq!(res, Err(PartialSetError::Missing("unknown".to_string())));

        // 2. Extra Paths (trying to go deeper than 'a' allows)
        let path_extra = vec_!["a", "too_deep"];
        let res_extra = p_ex.set(&path_extra, &vec_!["1"]);
        assert_eq!(
            res_extra,
            Err(PartialSetError::ExtraPaths(vec_!["too_deep"]))
        );

        // 3. Missing Paths (stopping at 'c' which is recursive)
        let path_short = vec_!["c"];
        let res_short = p_ex.set(&path_short, &vec_!["1"]);
        assert_eq!(res_short, Err(PartialSetError::EarlyEnd("c".to_string())));
    }

    #[test]
    fn test_full_workflow() {
        let mut original = Ex {
            a: 1,
            b: None,
            c: Nested {
                d: 10,
                ..Default::default()
            },
        };

        let mut p_ex = PartialEx::default();

        // Update partial via string paths (e.g., from a CLI or API)
        p_ex.set(&vec_!["a"], &vec_!["2"]).unwrap();
        p_ex.set(&vec_!["c", "d"], &vec_!["20"]).unwrap();

        // Apply partial to original
        original.apply(p_ex);

        assert_eq!(original.a, 2);
        assert_eq!(original.c.d, 20);
        assert_eq!(original.b, None); // Untouched
    }

    // ----------------------------------------

    #[test]
    fn test_collections_unwrap() {
        #[partial(recurse, path)]
        #[derive(Debug, PartialEq, Default, Clone)]
        struct CollectionStruct {
            pub recurse: Vec<Nested>, // Option<Vec<PartialNested>>
            #[partial(recurse = "", unwrap)]
            pub unwrap: Vec<Nested>, // Vec<Nested>
            #[partial(unwrap)]
            pub recurse_unwrap: Vec<Nested>, // Vec<PartialNested>
            #[partial(recurse = "")]
            pub neither: Vec<Nested>, // Option<Vec<Nested>>
            #[partial(unwrap, set = "sequence")]
            pub unwrap_seq: Vec<Nested>, // Vec<Nested>, extends on set
            // #[partial(recurse)]
            // #[partial(set = "sequence")]
            // pub recurse_seq: Vec<Nested>, // should compiler error and it does
            #[partial(set = "sequence")]
            pub unrecursed_seq: Vec<Nested>, // Option<Vec<Nested>>, overwrites on set
        }

        let initial = vec![
            Nested {
                d: 3,
                e: "hi".into(),
            },
            Nested {
                d: 1,
                e: "hi".into(),
            },
        ];

        let mut base = CollectionStruct {
            recurse: initial.clone(),
            unwrap: initial.clone(),
            recurse_unwrap: initial.clone(),
            neither: initial.clone(),
            unwrap_seq: initial.clone(),
            unrecursed_seq: initial.clone(),
        };

        let p = PartialCollectionStruct {
            recurse: Some(vec![
                PartialNested {
                    d: Some(10),
                    e: None,
                },
                PartialNested {
                    d: Some(10),
                    e: None,
                },
                PartialNested {
                    d: Some(10),
                    e: None,
                },
            ]),
            unwrap: vec![Nested {
                d: 20,
                e: "B".into(),
            }],
            recurse_unwrap: vec![PartialNested {
                d: Some(30),
                e: None,
            }],
            neither: Some(vec![Nested {
                d: 40,
                e: "D".into(),
            }]),
            unwrap_seq: vec![Nested {
                d: 50,
                e: "E".into(),
            }],
            unrecursed_seq: Some(vec![Nested {
                d: 70,
                e: "G".into(),
            }]),
        };

        base.apply(p);

        // 1. recurse: Option<Vec<P>>. apply does zip-apply then extend.
        assert_eq!(base.recurse.len(), 3);
        assert_eq!(base.recurse[0].d, 10);
        assert_eq!(base.recurse[0].e, "hi");
        assert_eq!(base.recurse[1].d, 10);
        assert_eq!(base.recurse[2].d, 10);

        // 2. unwrap: Vec<T>. apply does extend.
        assert_eq!(base.unwrap.len(), 3);
        assert_eq!(base.unwrap[2].d, 20);

        // 3. recurse_unwrap: Vec<P>. apply does extend-by-applying-to-default.
        assert_eq!(base.recurse_unwrap.len(), 3);
        assert_eq!(base.recurse_unwrap[2].d, 30);
        assert_eq!(base.recurse_unwrap[2].e, "");

        // 4. neither: Option<Vec<T>>. apply does overwrite.
        assert_eq!(base.neither.len(), 1);
        assert_eq!(base.neither[0].d, 40);

        // 5. unwrap_seq: Vec<T>, sequence. apply does extend.
        assert_eq!(base.unwrap_seq.len(), 3);
        assert_eq!(base.unwrap_seq[2].d, 50);

        // 6. unrecursed_seq: Option<Vec<T>>, sequence. apply does overwrite.
        assert_eq!(base.unrecursed_seq.len(), 1);
        assert_eq!(base.unrecursed_seq[0].d, 70);

        // --- Test Path Set ---
        let mut p_path = PartialCollectionStruct::default();

        // set singleton on unwrapped Vec (unwrap) -> extend
        p_path
            .set(&["unwrap".into()], &vec_!["d", "99", "e", ""])
            .unwrap();
        assert_eq!(p_path.unwrap.len(), 1);
        assert_eq!(p_path.unwrap[0].d, 99);

        // set singleton on wrapped Vec (neither) -> initialize then push
        p_path
            .set(&["neither".into()], &vec_!["d", "88", "e", ""])
            .unwrap();
        p_path
            .set(&["neither".into()], &vec_!["d", "88", "e", "88"])
            .unwrap();
        assert_eq!(
            p_path.neither,
            Some(vec![
                Nested {
                    d: 88,
                    e: "".into()
                },
                Nested {
                    d: 88,
                    e: "88".into()
                }
            ])
        );

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

        // todo: fix deserializer to pass this
        // set sequence on unrecursed_seq (Option<Vec>) -> overwrite
        // p_path
        //     .set(
        //         &["unrecursed_seq".into()],
        //         &vec_!["d", "1", "e", "", "d", "2", "e", ""],
        //     )
        //     .unwrap();
        // assert_eq!(p_path.unrecursed_seq.as_ref().unwrap().len(), 2);
        // assert_eq!(p_path.unrecursed_seq.as_ref().unwrap()[0].d, 1);
        // assert_eq!(p_path.unrecursed_seq.as_ref().unwrap()[1].d, 2);
    }

    #[test]
    fn test_collections_recurse() {
        #[partial(unwrap)]
        #[derive(Debug, PartialEq, Default, Clone)]
        struct CollectionStruct {
            pub list: Vec<i32>,
            pub map: HashMap<String, i32>,
            pub set: HashSet<i32>,
        }

        let mut base = CollectionStruct::default();
        base.list.push(1);
        base.map.insert("old".into(), 10);
        base.set.insert(100);

        // In PartialCollectionStruct, these are the original types, not Option<T>
        let p = PartialCollectionStruct {
            list: vec![2, 3],
            map: vec![("new".to_string(), 20)].into_iter().collect(),
            set: vec![200].into_iter().collect(),
        };

        base.apply(p);

        assert_eq!(base.list, vec![1, 2, 3]);
        assert_eq!(base.map.get("old"), Some(&10));
        assert_eq!(base.map.get("new"), Some(&20));
        assert!(base.set.contains(&100));
        assert!(base.set.contains(&200));
    }

    #[test]
    fn test_collections_set_behavior() {
        #[partial(path, unwrap)]
        #[derive(Debug, PartialEq, Default, Clone)]
        struct UnwrappedColl {
            pub list: Vec<i32>,
        }

        #[partial(path)]
        #[derive(Debug, PartialEq, Default, Clone)]
        struct WrappedColl {
            pub list: Vec<i32>,
            #[partial(set = "sequence")]
            pub seq: Vec<i32>,
        }

        let mut u = PartialUnwrappedColl::default();
        // Singleton set on unwrapped Vec -> extend
        u.set(&["list".to_string()], &["1".to_string()]).unwrap();
        u.set(&["list".to_string()], &["2".to_string()]).unwrap();
        assert_eq!(u.list, vec![1, 2]);

        let mut w = PartialWrappedColl::default();
        // Singleton set on wrapped Vec (Option<Vec>) -> initialize then push
        w.set(&["list".to_string()], &["10".to_string()]).unwrap();
        w.set(&["list".to_string()], &["20".to_string()]).unwrap();
        assert_eq!(w.list, Some(vec![10, 20]));

        // Sequence set on wrapped Vec -> overwrite
        w.set(&["seq".to_string()], &vec_!["1", "2", "3"]).unwrap();
        assert_eq!(w.seq, Some(vec![1, 2, 3]));
    }

    #[test]
    fn test_set_alias_and_flatten() {
        #[partial(path)]
        #[derive(Debug, PartialEq, Default, Clone, Serialize)]
        struct InnerFlat {
            pub a: i32,
            pub b: i32,
        }

        #[partial(path)]
        #[derive(Debug, PartialEq, Default, Clone, Serialize)]
        struct Root {
            #[serde(alias = "alias_x")]
            pub x: i32,
            #[serde(flatten)]
            #[partial(recurse)]
            pub flat: InnerFlat,
            pub y: i32,
        }

        let mut p = PartialRoot::default();

        // 1. Test alias
        p.set(&["alias_x".to_string()], &["10".to_string()])
            .unwrap();
        assert_eq!(p.x, Some(10));

        // 2. Test direct name still works
        p.set(&["x".to_string()], &["20".to_string()]).unwrap();
        assert_eq!(p.x, Some(20));

        // 3. Test flatten (path "a" should go to flat.a)
        p.set(&["a".to_string()], &["100".to_string()]).unwrap();
        assert_eq!(p.flat.a, Some(100));

        // 4. Test flatten (path "b" should go to flat.b)
        p.set(&["b".to_string()], &["200".to_string()]).unwrap();
        assert_eq!(p.flat.b, Some(200));

        // 5. Test normal field
        p.set(&["y".to_string()], &["30".to_string()]).unwrap();
        assert_eq!(p.y, Some(30));

        // 6. Test missing field
        let res = p.set(&["unknown".to_string()], &["0".to_string()]);
        assert_eq!(res, Err(PartialSetError::Missing("unknown".to_string())));
    }
}
