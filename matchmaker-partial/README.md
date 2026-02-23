# matchmaker-partial

Support for partial updates and configuration in matchmaker. This crate provides traits and logic for merging partial configuration structures, which is useful for overriding default settings with user-defined values.

## Features

- **Derive Macros**: Automatically generate partial versions of your structs where all fields are wrapped in `Option`.
- **Apply Updates**: Easily apply a partial struct to a full struct.
- **Dynamic Setting**: Update partial structs using string paths and values (ideal for CLI/environment overrides).
- **Nested Recursion**: Support for recursive partial updates in nested struct hierarchies.
- **Merging**: Merge multiple partial structs together.

## Basic Usage

Using the `#[partial]` macro to generate a partial version of a struct and applying updates.

```rust
use matchmaker_partial::Apply;
use matchmaker_partial_macros::partial;

#[partial]
#[derive(Debug, PartialEq)]
struct Config {
    pub name: String,
    pub threads: Option<i32>,
}

fn main() {
    let mut config = Config {
        name: "default".into(),
        threads: Some(4),
    };

    // The macro generates PartialConfig where all fields are Option
    // Note that fields which were already optional are not re-wrapped.
    let partial = PartialConfig {
        name: Some("custom".into()),
        threads: None, // This field won't be updated
    };

    // Apply the partial updates to the original struct
    config.apply(partial);

    assert_eq!(config.name, "custom");
    assert_eq!(config.threads, Some(4));
}
```

## Dynamic Updates with `set`

The `set` method allows updating a partial struct using string paths and values. This requires the `#[partial(path)]` attribute.
The actual (typed) value is produced from the input list using a custom data deserializer which reads `&[String]`.

```rust
use matchmaker_partial::{Set, Apply};
use matchmaker_partial_macros::partial;

#[partial(path)]
#[derive(Debug, Default)]
struct Config {
    pub name: String,
    pub threads: i32,
}

fn main() {
    let mut config = Config::default();
    let mut partial = PartialConfig::default();

    // Dynamically set values using string paths (e.g., from CLI flags)
    partial.set(&["name".to_string()], &["my-app".to_string()]).unwrap();
    partial.set(&["threads".to_string()], &["8".to_string()]).unwrap();

    config.apply(partial);

    assert_eq!(config.name, "my-app");
    assert_eq!(config.threads, 8);
}
```

> [!NOTE]
> Items in keyed collections can be also referenced either by `path.to.collection.key`, or in the provided input value (So that the provided input slice is the concatenation of `List deserializing to key` + `List deserializing to value`).

## Nested Structs with `recurse`

You can use `#[partial(recurse)]` to handle nested structures.

```rust
use matchmaker_partial::Apply;
use matchmaker_partial_macros::partial;

#[partial]
#[derive(Debug, PartialEq, Clone)]
struct UIConfig {
    pub width: u32,
    pub height: u32,
}

#[partial]
#[derive(Debug, PartialEq)]
struct AppConfig {
    pub name: String,
    #[partial(recurse)]
    pub ui: UIConfig,
}

fn main() {
    let mut config = AppConfig::default();
    
    let partial = PartialAppConfig {
        name: Some("Nested Example".into()),
        ui: PartialUIConfig {
            width: Some(1024),
            height: None,
        },
    };

    config.apply(partial);
    
    assert_eq!(config.ui.width, 1024);
    assert_eq!(config.ui.height, 0); // Original/Default value preserved
}
```

## Collections

When `#[partial(unwrap)]`is applied to a collection (`HashMap, Vec, BTreeMap, BTreeSet`), the corresponding field omits the wrapping `Option`. This holds even for collections wrapped in Option.

When `#[partial(recurse)]` is applied to a collection, the nesting propogates to the internal type: `Vec<Inner>` becomes `Vec<PartialInner>`.

Otherwise if the mirror collection type is wrapped in Option, apply overwrites. If the inner type is also partial, corresponding values are applied to, and any extra values are applied to the default prototype and inserted. Otherwise, the base data is extended.

The behavior is summarized in the following table:

#### Type Transformation

| Original         | No Recurse / Not Unwrapped | No Recurse / Unwrap | Recurse / Not Unwrapped                   | Recurse / Unwrap              |
| ---------------- | -------------------------- | ------------------- | ----------------------------------------- | ----------------------------- |
| `Vec<T>`         | `Option<Vec<T>>`           | `Vec<T>`            | `Option<Vec<P>>`                          | `Vec<P>`                      |
| `Option<Vec<T>>` | `Option<Vec<T>>`           | `Vec<T>`            | `Option<Vec<P>>`                          | `Vec<P>`                      |
| Apply behavior   | Overwrite                  | Extend              | Apply, then extend from upgraded versions | Upgrade all to T, then extend |

## Set Attributes

### `set = "sequence"`

On a collection, using `#[partial(set = "sequence")]` causes it to deserialize input as a sequence rather than as the next given single value.

```rust
#[partial(path)]
struct Config {
    #[partial(set = "sequence")]
    pub tags: Vec<String>,
}

let mut partial = PartialConfig::default();
partial.set(&["tags"], &["alpha", "beta", "gamma"]).unwrap();
assert_eq!(partial.tags, Some(vec!["alpha".into(), "beta".into(), "gamma".into()]));
```

### `set = "recurse"`

On a collection, using `#[partial(set = "recurse")]` adds support for additional path segments after its own. The additional path segments are used to create a new partial item with a single field set, which is then appended to the collection.

```rust
struct Nested {
    pub name: String
    pub kind: usize
}

#[partial(path)]
struct Config {
    #[partial(set = "recurse")]
    pub tags: Vec<Nested>,
}

let mut partial = PartialConfig::default();
partial.set(&["tags", "name"], &["alpha"]).unwrap();
assert_eq!(partial.tags, Some(
    vec![Nested { name: "alpha".into(), kind: 0 }]
));
```

### `serde(alias)`

Fields with `#[serde(alias = "...")]` or `#[partial(alias = "...")]` can be updated using any of the specified aliases in addition to the original field name.

```rust
#[partial(path)]
struct Config {
    #[serde(alias = "threads_count")]
    pub threads: i32,
}

let mut partial = PartialConfig::default();
partial.set(&["threads_count"], &["8"]).unwrap();
assert_eq!(partial.threads, Some(8));
```

### `serde(flatten)`

Flattened fields allow embedding nested structs directly at the top level. When a flattened field also uses `#[partial(recurse)]`, set delegates updates to the nested partial rather than expecting a top-level field match.

`#[partial(flatten)]` is also supported.

```rust
#[partial(path)]
struct Inner {
    pub width: u32,
    pub height: u32,
}

#[partial(path)]
struct Outer {
    #[serde(flatten)]
    #[partial(recurse)]
    pub inner: Inner,
}

let mut partial = PartialOuter::default();
partial.set(&["width"], &["1024"]).unwrap();
partial.set(&["height"], &["768"]).unwrap();
assert_eq!(partial.inner.width, Some(1024));
assert_eq!(partial.inner.height, Some(768));
```

## Modify with `merge`

Adding `#[partial(merge)]` generates the `Merge` trait for the partial struct, providing:

- `merge(&mut self, other)`: Updates fields from another partial where they are `Some`.
- `clear(&mut self)`: Resets all fields to `None`.

#### Example

```rust
#[partial(merge)]
struct Stats { hp: i32, mana: i32 }

#[partial(recurse, merge)]
struct Character { name: String, stats: Stats }

let mut hero = Character { name: "Arthur".into(), stats: Stats { hp: 100, mana: 50 } };

let mut p1 = PartialCharacter::default();
p1.name = Some("King Arthur".into());

let mut p2 = PartialCharacter::default();
p2.stats.hp = Some(150);

p1.merge(p2);
hero.apply(p1);

assert_eq!(hero.name, "King Arthur");
assert_eq!(hero.stats.hp, 150);
assert_eq!(hero.stats.mana, 50);

let mut p3 = PartialCharacter::default();
p3.name = Some("Temp".into());
p3.clear();
assert_eq!(p3.name, None);
```

# Deserializer

Set fills values from `&[String]` by using a simple data deserializer.

- Most primitive types read a single word.
- Tuples and Sequences attempt to deserialize their next values from the remaining words sequentially.
- Maps deserialize keys and values alternately in sequence.
- Struct deserialization ends when the next word is not a field name.
- Tuple deserialization ends when the requisite number of values have been deserialized.
- Otherwise, maps and sequence types consume the input until exhausted.
- Options are transparent to their inner type unless the word list is exhausted, or the following word is "null". (This last behavior may be subject to change).
- Unit structs expect "" or "()".
