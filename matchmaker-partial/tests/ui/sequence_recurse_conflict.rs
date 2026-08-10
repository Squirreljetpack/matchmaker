use matchmaker_partial_macros::partial;

struct Bar {
    y: i32,
}

#[partial]
struct Foo {
    #[partial(recurse = "PartialBar", set = "sequence")]
    x: Vec<Bar>,
}

fn main() {}
