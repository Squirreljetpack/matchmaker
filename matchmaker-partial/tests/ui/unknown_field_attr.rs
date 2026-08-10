use matchmaker_partial_macros::partial;

#[partial]
struct Foo {
    #[partial(bogus)]
    x: i32,
}

fn main() {}
