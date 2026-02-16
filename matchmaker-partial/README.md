# matchmaker-partial

Support for partial updates and configuration in matchmaker. This crate provides traits and logic for merging partial configuration structures, which is useful for overriding default settings with user-defined values.

## Features

- **Derive Macros**: Automatically generate partial versions of your structs where all fields are wrapped in `Option`.
- **Apply Updates**: Easily apply a partial struct to a full struct.
- **Dynamic Setting**: Update partial structs using string paths and values (ideal for CLI/environment overrides).
- **Nested Recursion**: Support for recursive partial updates in nested struct hierarchies.
- **Merging**: Merge multiple partial structs together.

## Example: Basic Usage

Using the `#[partial]` macro to generate a partial version of a struct and applying updates.

```rust
use matchmaker_partial::Apply;
use matchmaker_partial_macros::partial;

#[partial]
#[derive(Debug, PartialEq, Default)]
struct Config {
    pub name: String,
    pub threads: i32,
}

fn main() {
    let mut config = Config {
        name: "default".into(),
        threads: 4,
    };

    // The macro generates PartialConfig where all fields are Option
    let partial = PartialConfig {
        name: Some("custom".into()),
        threads: None, // This field won't be updated
    };

    // Apply the partial updates to the original struct
    config.apply(partial);

    assert_eq!(config.name, "custom");
    assert_eq!(config.threads, 4);
}
```

## Example: Dynamic Updates with `set`

The `set` method allows updating a partial struct using string paths and values. This requires the `#[partial(path)]` attribute.

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

## Example: Nested Structs with `recurse`

You can use `#[partial(recurse)]` to handle nested structures.

```rust
use matchmaker_partial::Apply;
use matchmaker_partial_macros::partial;

#[partial]
#[derive(Debug, Default, PartialEq, Clone)]
struct UIConfig {
    pub width: u32,
    pub height: u32,
}

#[partial]
#[derive(Debug, Default, PartialEq)]
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
