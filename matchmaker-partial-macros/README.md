# matchmaker-partial-macros

This crate provides the `#[partial]` attribute macro for the `matchmaker-partial` crate.

It is used to automatically generate "partial" versions of structs where fields are wrapped in `Option`, along with implementations for `Apply`, `Set`, `Merge`, and `Clear` traits.

Please refer to the [matchmaker-partial](https://crates.io/crates/matchmaker-partial) documentation for usage examples.

> [!NOTE]
> This code is partially AI generated, with a few features I haven't gotten around to rounding out so the behavior may be a bit spotty in places. Nevertheless, for the needs of the main binary it works well enough, and there are tests in `matchmaker-partial` which should give a good idea the situations it can be relied on to work correctly.
