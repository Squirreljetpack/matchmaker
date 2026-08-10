//! Compile-fail tests: the `#[partial]` macro must reject invalid input with
//! a spanned error instead of panicking (or worse, silently misbehaving).

#[test]
fn ui() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
