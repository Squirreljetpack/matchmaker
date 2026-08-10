use std::{
    ffi::OsString,
    process::Child,
    thread,
    time::{Duration, Instant},
};

use cba::broc::SHELL;

/// [`tokio::process::Command`] counterpart of `cba::broc::CommandExt::from_script`.
/// Mirrors the current patched-cba implementation: `shell[0] shell[1..] script`
/// when `shell` is non-empty, else `SHELL.0 SHELL.1 script` (plus an empty `$0`
/// on unix so subsequent args are fed to the script directly).
pub fn tokio_command_from_script(script: &str, shell: &[OsString]) -> tokio::process::Command {
    let (def_sh, def_arg) = &*SHELL;

    let mut ret = if shell.is_empty() {
        let mut ret = tokio::process::Command::new(def_sh);
        ret.arg(def_arg);
        ret
    } else {
        let mut ret = tokio::process::Command::new(&shell[0]);
        ret.args(&shell[1..]);
        ret
    };

    ret.arg(script);

    #[cfg(unix)]
    if shell.is_empty() {
        ret.arg("");
    }

    ret
}

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
