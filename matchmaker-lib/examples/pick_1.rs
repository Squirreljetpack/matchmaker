use matchmaker::nucleo::{Render, Worker};
use matchmaker::{MatchResultExt, Matchmaker, Result, SSS};

pub async fn mm_get<T: SSS + Render + Clone>(items: impl IntoIterator<Item = T>) -> Result<T> {
    let worker = Worker::new_single_column();
    worker.append(items);
    let mm = Matchmaker::new(worker, |_state| {
        // TODO: extract Vec<T> from state.picker_ui.selector
        vec![]
    });

    mm.pick_default().await.first()
}

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];
    println!("{}", mm_get(items).await?);

    Ok(())
}
