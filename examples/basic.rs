use matchmaker::nucleo::{Worker, Indexed};
use matchmaker::{Matchmaker, MatchError, Result};

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];

    let worker = Worker::new_single_column();
    worker.append(items);
    let identifier = Indexed::identifier;

    let mm = Matchmaker::new(worker, identifier);

    match mm.pick_default().await {
        Ok(v) => {
            println!("{}", v[0]);
        }
        Err(err) => {
            match err {
                MatchError::Abort(1) => {
                    eprintln!("cancelled");
                }
                _ => {
                    eprintln!("Error: {err}");
                }
            }
        }
    }

    Ok(())
}
