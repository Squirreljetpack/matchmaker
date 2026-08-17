//! Fullscreen paging of command output for the `ShowPreview` action.
//!
//! With the `pager` feature the output is streamed into an interactive `minus`
//! pager when stdout is a tty; otherwise (or without the feature) it is piped
//! into the external pager chain `MM_PAGER -> $PAGER -> less -> more`,
//! displayed on the controlling tty so a redirected stdout is not polluted.

use std::{
    env,
    ffi::OsString,
    io::Write,
    process::{Child, ChildStdout, Command, Stdio},
    time::Duration,
};

use cba::broc::tty_or_inherit;
use log::info;

use crate::config::PagerConfig;
use crate::register::wait_with_timeout;

#[cfg(feature = "pager")]
use std::{
    io::{BufRead, BufReader},
    sync::{Arc, Mutex},
    thread,
};

#[cfg(feature = "pager")]
use minus::{hooks::Hook, LineNumbers, Pager};

/// Common pager configuration for `ShowPreview`: line numbers, follow mode,
/// horizontal scroll, prompt, and the no-op `PostPagerExit` id-1 hook that
/// keeps `q` from exiting the whole process (it is pre-populated by minus
/// with a callback that exits the app; the former
/// `set_exit_strategy(PagerQuit)` is deprecated).
#[cfg(feature = "pager")]
fn configure_pager(pager: &Pager, cfg: &PagerConfig) {
    let _ = pager.set_line_numbers(if cfg.line_numbers {
        LineNumbers::Enabled
    } else {
        LineNumbers::Disabled
    });
    if cfg.follow {
        let _ = pager.follow_output(true);
    }
    if cfg.horizontal_scroll {
        let _ = pager.horizontal_scroll(true);
    }
    let prompt = cfg
        .prompt
        .clone()
        .unwrap_or_else(|| "/ or ? to search".to_string());
    let _ = pager.set_prompt(prompt);
    let _ = pager.remove_hook(Hook::PostPagerExit, 1);
    let _ = pager.add_hook(Hook::PostPagerExit, 1, Box::new(|_| {}));
}

/// Stream `stdout` into an interactive `minus` pager, killing the preview
/// command when the user quits early. The pager starts immediately and output
/// streams in live.
#[cfg(feature = "pager")]
pub(crate) fn minus_page(stdout: ChildStdout, child: Child, cfg: &PagerConfig) {
    let pager = Pager::new();
    configure_pager(&pager, cfg);

    // Kill the preview command when the user quits the pager early; it is
    // reaped below once its stdout closes.
    let child_handle = Arc::new(Mutex::new(Some(child)));
    let hook_handle = child_handle.clone();
    let _ = pager.add_hook(
        Hook::PostPagerExit,
        2,
        Box::new(move |_| {
            if let Some(c) = hook_handle.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        }),
    );

    let pager_for_thread = pager.clone();
    let pager_thread = thread::spawn(move || {
        let _ = minus::dynamic_paging(pager_for_thread);
    });

    let mut stdout = BufReader::new(stdout);
    let mut line = String::new();
    loop {
        line.clear();
        match stdout.read_line(&mut line) {
            Ok(0) => break, // EOF: the command finished (or was killed)
            Ok(_) => {
                // The channel closes when the pager quits; stop feeding.
                if pager.push_str(line.clone()).is_err() {
                    break;
                }
            }
            Err(e) => {
                log::error!("ShowPreview: failed reading preview output: {e}");
                break;
            }
        }
    }

    let _ = pager_thread.join();
    // Reap the preview command: it either finished or was killed above.
    if let Some(child) = child_handle.lock().unwrap().take() {
        wait_with_timeout(child, Duration::from_secs(5));
    }
}

/// Pipe `stdout` into the external pager chain `MM_PAGER -> $PAGER -> less ->
/// more`. The pager displays on the controlling tty (`/dev/tty` when
/// available) so a redirected stdout is not polluted with pager UI.
pub(crate) fn external_pager(stdout: ChildStdout, child: Child) {
    let Some(pager_path) = resolve_pager() else {
        log::error!("ShowPreview: no pager found (set MM_PAGER or $PAGER)");
        return;
    };

    let mut pager = match Command::new(pager_path.clone())
        .stdin(Stdio::from(stdout))
        .stdout(tty_or_inherit())
        .env("PG_FORCE_TTY", "true")
        .spawn()
    {
        Ok(pager) => pager,
        Err(e) => {
            log::error!("ShowPreview: failed to spawn pager {pager_path:?}: {e}");
            return;
        }
    };
    match pager.wait() {
        Ok(status) => info!("ShowPreview: pager {pager_path:?} exited with {status}"),
        Err(e) => log::error!("ShowPreview: failed waiting for pager {pager_path:?}: {e}"),
    }
    // The preview command likely SIGPIPEd once the pager finished; reap it.
    wait_with_timeout(child, Duration::from_secs(5));
}

/// Page static text (e.g. a `Set` preview payload) fullscreen; no command
/// runs. With the `pager` feature plus a tty stdout this uses `minus`;
/// otherwise the text is fed to the external pager on the controlling tty.
pub(crate) fn page_text(text: &str, cfg: &PagerConfig) {
    #[cfg(not(feature = "pager"))]
    let _ = cfg;
    let use_minus = cfg!(feature = "pager") && atty::is(atty::Stream::Stdout);
    if use_minus {
        #[cfg(feature = "pager")]
        return minus_text(text, cfg);
    }
    external_text(text);
}

/// Page static text in an interactive `minus` pager on the current thread.
#[cfg(feature = "pager")]
pub(crate) fn minus_text(text: &str, cfg: &PagerConfig) {
    let pager = Pager::new();
    configure_pager(&pager, cfg);
    let _ = pager.set_text(text);
    let _ = minus::dynamic_paging(pager);
}

/// Feed static text to the external pager on the controlling tty.
fn external_text(text: &str) {
    let Some(pager_path) = resolve_pager() else {
        log::error!("ShowPreview: no pager found (set MM_PAGER or $PAGER)");
        return;
    };

    let mut pager = match Command::new(pager_path.clone())
        .stdin(Stdio::piped())
        .stdout(tty_or_inherit())
        .env("PG_FORCE_TTY", "true")
        .spawn()
    {
        Ok(pager) => pager,
        Err(e) => {
            log::error!("ShowPreview: failed to spawn pager {pager_path:?}: {e}");
            return;
        }
    };
    if let Some(mut stdin) = pager.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
        let _ = stdin.flush();
    }
    match pager.wait() {
        Ok(status) => info!("ShowPreview: pager {pager_path:?} exited with {status}"),
        Err(e) => log::error!("ShowPreview: failed waiting for pager {pager_path:?}: {e}"),
    }
}

/// Resolve the external pager executable: `MM_PAGER` -> `$PAGER` -> `less` ->
/// `more`.
fn resolve_pager() -> Option<OsString> {
    for name in ["MM_PAGER", "PAGER"] {
        if let Some(pager) = env::var_os(name).filter(|p| !p.is_empty()) {
            return Some(pager);
        }
    }
    for name in ["less", "more"] {
        if let Ok(path) = which::which(name) {
            return Some(path.into_os_string());
        }
    }
    None
}
