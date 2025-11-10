use matchmaker::nucleo::worker::Worker;
use matchmaker::{Matchmaker, MatchmakerError, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];

    let worker = Worker::new_single();
    worker.append(items);
    let identifier = Worker::clone_identifier;

    let mm = Matchmaker::new(worker, identifier);

    match mm.pick().await {
        Ok(iter) => {
            for s in iter {
                println!("{s}");
            }
        }
        Err(err) => {
            if let Some(e) = err.downcast_ref::<MatchmakerError>()
                && matches!(e, MatchmakerError::Abort(1))
            {
                eprintln!("cancelled");
            } else {
                eprintln!("Error: {err}");
            }
        }
    }

    Ok(())
}
