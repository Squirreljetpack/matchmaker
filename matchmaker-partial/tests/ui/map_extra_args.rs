use matchmaker_partial_macros::partial;

#[partial]
struct Foo {
    x: HashMap<String, i32, std::collections::hash_map::RandomState>,
}

fn main() {}
