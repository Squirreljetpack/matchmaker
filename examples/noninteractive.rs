use std::time::Duration;

use matchmaker::noninteractive::get_matches;
use matchmaker::{SSS, Result};
use matchmaker::nucleo::Render;

pub fn mm_get_match<T: SSS + Clone + Render>(
    items: impl IntoIterator<Item = T>,
    query: &str,
) -> Option<T> {
    let mut ret = None;
    get_matches(
        items,
        query,
        Duration::from_millis(10),
        |x| {
            ret = Some(x.clone()); true
        }
    );
    ret
}

fn main() -> Result<()> {
    let items = std::fs::read_dir(".").unwrap().filter_map(|e| {
        match e {
            Ok(e) => Some(e.file_name().to_string_lossy().to_string()),
            _ => None
        }
    });

    if let Some(m) = mm_get_match(items, ".") {
        println!("Found: {m}")
    };

    Ok(())
}
