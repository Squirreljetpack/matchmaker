use matchmaker::nucleo::{Indexed, Render, Worker};
use matchmaker::{MatchError, Matchmaker, Result, ResultExt, SSS, Selector};

pub async fn mm_get<T: SSS + Render + Clone>(
    items: impl IntoIterator<Item = T>,
) -> Result<T, MatchError> {
    let worker = Worker::new_single_column();
    worker.append(items);
    let selector = Selector::new(Indexed::identifier);
    let mm = Matchmaker::new(worker, selector);

    mm.pick_default().await.first()
}

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];
    println!("{}", mm_get(items).await?);

    Ok(())
}
