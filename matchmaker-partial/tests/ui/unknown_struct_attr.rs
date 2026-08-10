use matchmaker_partial_macros::partial;

#[partial(bogus)]
struct Foo {
    x: i32,
}

fn main() {}
