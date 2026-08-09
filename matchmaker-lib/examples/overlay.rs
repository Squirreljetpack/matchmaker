//! Demonstrates the fuzzy picker overlay ([`matchmaker::ui::PickerOverlay`]).
//!
//! Press `ctrl-o` to toggle the overlay open (the overlay itself is closed
//! with `esc`). Inside the overlay:
//!
//! - Type to filter the dummy two-column dataset (both columns are matched).
//! - `up` / `down` move the cursor.
//! - `enter` hits the placeholder `todo!()` accept path, which panics with the
//!   current item — see `PickerOverlay::current_item` for the access pattern.
//!
//! The main picker (single-column worker with 50 dummy items) remains usable
//! while the overlay is closed.

use matchmaker::{
    MatchError, Matchmaker, PickOptions, Result,
    action::{Action, NullActionExt},
    binds::{bindmap, key},
    config::{OverlayConfig, QueryConfig, ResultsConfig},
    nucleo::Worker,
    ui::PickerOverlay,
};

#[tokio::main]
async fn main() -> Result<()> {
    // The main picker.
    let worker = Worker::new_single_column();
    worker.append((0..50).map(|i| format!("item {i}")));
    let mm: Matchmaker<String, String> = Matchmaker::new_on_cloneable(worker);

    // The overlay's own two-column dataset (dummy content for now).
    let overlay_items: Vec<(String, String)> = (0..100)
        .map(|i| (format!("result {i}"), format!("detail {i}")))
        .collect();

    let opts = mm.pick::<NullActionExt>(
        PickOptions::new()
            .overlay(PickerOverlay::new(
                overlay_items,
                OverlayConfig::default(),
                ResultsConfig::default(),
                QueryConfig::default(),
            ))
            .binds(bindmap!(key!(ctrl-o) => Action::Overlay(0))),
    );

    match opts.await {
        Ok(v) => {
            if let Some(first) = v.into_iter().next() {
                println!("{first}");
            }
        }
        Err(err) => match err {
            MatchError::Abort(1) => eprintln!("cancelled"),
            _ => eprintln!("Error: {err}"),
        },
    }

    Ok(())
}
