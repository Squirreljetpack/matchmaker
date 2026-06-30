use std::time::{Duration, Instant};

use crate::{
    SSS,
    nucleo::{Render, Worker, new_snapshot},
};

/// Map f on matches without starting the interface.
pub fn get_matches<T: SSS + Render>(
    items: impl IntoIterator<Item = T>,
    query: &str,
    timeout: Duration,
    mut f: impl FnMut(&T) -> bool,
) {
    let mut worker = Worker::new_single_column();

    let total = worker.append(items);
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

    for t in worker.matched_results() {
        if f(t) {
            break;
        }
    }
}
