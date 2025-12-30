use std::{time::{Duration, Instant}};

use crate::{MMItem, nucleo::{Column, Render, Worker, injector::Injector}};

/// Map f on matches without starting the interface.
pub fn get_matches<T: MMItem + Render>(
    items: impl IntoIterator<Item = T>,
    query: &str,
    timeout: Duration,
    mut f: impl FnMut(&T) -> bool
) {
    let mut worker = Worker::new(
        [Column::new("", |item: &T| item.as_text())]
        , 0
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
        let (_, status) = Worker::new_snapshot(&mut worker.nucleo);
        
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
            break
        }
    }
}