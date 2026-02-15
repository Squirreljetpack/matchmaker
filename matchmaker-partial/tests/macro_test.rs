#![allow(unused)]

macro_rules! vec_ {
    ($($elem:expr),* $(,)?) => {
        vec![$($elem.into()),*]
    };
    (| $($elem:expr),*) => {
        vec![$($elem.to_string()),*]
    };
    ($t:ty | $($elem:expr),*) => {
        vec![$($t::from($elem)),*]
    };
    ($f:ident | $($elem:expr),*) => {
        vec![$($f($elem)),*]
    };
}

#[cfg(test)]
mod tests {
    use matchmaker_partial::PartialSetError;
    use matchmaker_partial_macros::partial;
    use serde::Serialize;
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
        impl Nested {
            fn apply(&mut self, partial: CustomPartialNested) {
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

    use super::*; // Import the macro
    // --- Defining the Structs ---

    #[partial(path)]
    #[derive(Debug, PartialEq, Default, Clone)]
    pub struct Nested {
        pub d: usize,
    }

    #[partial(path)]
    #[derive(Debug, PartialEq, Default)]
    pub struct Ex {
        pub a: usize,
        pub b: Option<usize>,
        #[partial(recurse)]
        pub c: Nested,
    }

    // --- The Tests ---

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
            c: Nested { d: 10 },
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

    #[test]
    fn test_collections_recurse_extend() {
        #[partial(recurse)]
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
}
