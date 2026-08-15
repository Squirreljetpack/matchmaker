//! Integration test for the `IoStream::Test` backend: run a full pick with
//! output captured in `matchmaker::test::TEST_BUFFER` instead of a real
//! terminal, then assert on the captured contents.

use std::time::Duration;

use matchmaker::{
    Matchmaker, PickOptions,
    action::{Action, NullActionExt},
    config::TerminalConfig,
    message::RenderCommand,
    nucleo::Worker,
    test,
    tui::IoStream,
};

#[tokio::test]
async fn test_backend_captures_rendered_output() {
    let _ = env_logger::try_init();
    test::clear();

    let worker = Worker::new_single_column();
    worker.append(["item1", "item2", "item3"]);

    let mut mm: Matchmaker<&str, &str> = Matchmaker::new_on_cloneable(worker);
    mm.config_tui(TerminalConfig {
        stream: IoStream::Test,
        ..Default::default()
    });

    // Headless: because the stream is `Test`, pick runs the event loop it
    // creates in optional (input-less) mode automatically.
    let mut opts = PickOptions::new();
    let render_tx = opts.render_tx();

    // Accept manually once the first frames have rendered.
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = render_tx.send(RenderCommand::Action(Action::Accept));
    });

    let picked = tokio::time::timeout(Duration::from_secs(10), mm.pick::<NullActionExt>(opts))
        .await
        .expect("pick timed out")
        .expect("pick should succeed");

    assert_eq!(picked, vec!["item1"]);

    let output = test::contents();
    assert!(
        output.contains("item1"),
        "rendered output should contain item1, got: {output:?}"
    );
    assert!(
        output.contains("item2"),
        "rendered output should contain item2, got: {output:?}"
    );
    assert!(
        output.contains("item3"),
        "rendered output should contain item3, got: {output:?}"
    );
}
