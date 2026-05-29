use cba::broc::SHELL;

pub fn tokio_command_from_script(script: &str) -> tokio::process::Command {
    let (shell, arg) = &*SHELL;

    let mut ret = tokio::process::Command::new(shell);

    ret.arg(arg).arg(script).arg(""); //

    ret
}

use std::process::Child;
use std::thread;
use std::time::{Duration, Instant};

pub(crate) fn wait_with_timeout(mut child: Child, timeout: Duration) {
    let start = Instant::now();

    let handle = thread::spawn(move || {
        let _ = child.wait();
    });

    while start.elapsed() < timeout {
        if handle.is_finished() {
            return;
        }

        thread::sleep(Duration::from_millis(10));
    }

    log::warn!("CLIPcmd timed out");

    // there is a crate for this but for simplicity just forget about it
    // let _ = child.kill();
}
