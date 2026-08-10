use matchmaker_partial_macros::partial;

#[partial]
struct Foo {
    #[partial(set = "bogus")]
    x: Vec<i32>,
}

fn main() {}
