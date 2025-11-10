use matchmaker::Result;
use matchmaker::nucleo::worker::Worker;

fn main() -> Result<()> {
    let worker = Worker::new_single();
    let items = vec!["item1", "item2", "item3"];
    worker.append(items);

    Ok(())
}
