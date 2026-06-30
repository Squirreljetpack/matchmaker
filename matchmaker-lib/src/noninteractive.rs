use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::{
    nucleo::{injector::Injector, new_snapshot, Column, Render, Worker},
    SSS,
};

/// Map f on matches without starting the interface.
pub fn get_matches<T: SSS + Render>(
    items: impl IntoIterator<Item = T>,
    query: &str,
    timeout: Duration,
    mut f: impl FnMut(&T) -> bool,
) {
    let preprocessor = Arc::new(|_: &T| ());
    let mut worker = Worker::new(
        [Column::new("", |item: &T, _: &()| item.as_text())],
        0,
        preprocessor.clone(),
        preprocessor,
    );
    let mut total = 0;

    let injector = worker.injector();
    for i in items {
        total += 1;
        let _ = injector.push(i);
    }

    worker.find(query);

    let start = Instant::now();
    loop {
        let (_, status) = new_snapshot(&mut worker.nucleo);

        if status.item_count == total && !status.running {
            break;
        }

        if start.elapsed() >= timeout {
            break;
        }
        // new_snapshot already waits
    }

    for t in worker.raw_results() {
        if f(t) {
            break;
        }
    }
}
