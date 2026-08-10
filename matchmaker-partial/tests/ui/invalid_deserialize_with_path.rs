use matchmaker_partial_macros::partial;

#[partial]
struct Foo {
    #[serde(deserialize_with = "not a path!!")]
    x: Vec<i32>,
}

fn main() {}
