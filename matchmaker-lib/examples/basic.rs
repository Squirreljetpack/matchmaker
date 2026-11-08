use matchmaker::nucleo::Worker;
use matchmaker::{MatchError, Matchmaker, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];

    let worker = Worker::new_single_column();
    worker.append(items);
    let mm = Matchmaker::new_on_cloneable(worker);

    match mm.pick_default().await {
        Ok(v) => {
            if let Some(first) = v.into_iter().next() {
                println!("{first}");
            }
        }
        Err(err) => match err {
            MatchError::Abort(1) => {
                eprintln!("cancelled");
            }
            _ => {
                eprintln!("Error: {err}");
            }
        },
    }

    Ok(())
}
