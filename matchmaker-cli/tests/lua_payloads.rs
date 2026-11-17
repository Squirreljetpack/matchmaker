//! Cross-platform checks for the lua payload plumbing.
//!
//! Everything runs through `--list`, the non-interactive entry point, so the
//! assertions are identical on every OS: no shell, no external binary, no TTY.
//! That makes this the suite to run on Windows (see `%%WINDOWS/PROGRESS.md`).
//!
//! `MM_BIN` overrides the binary under test, for harnesses that run the test
//! executable outside of Cargo's own build tree.

use std::{
    path::Path,
    process::{Command, Output},
};

const PRESETS: &str = "tests/fixtures/presets";

fn mm() -> Command {
    let bin = std::env::var_os("MM_BIN").unwrap_or_else(|| env!("CARGO_BIN_EXE_mm").into());
    let mut cmd = Command::new(bin);
    cmd.arg("-q");
    cmd
}

fn run(preset: &str, arg: &str) -> Output {
    mm().arg("-o")
        .arg(format!("{PRESETS}/{preset}"))
        .arg(arg)
        .output()
        .expect("failed to spawn mm")
}

/// Runs `arg` and returns stdout and the exit code, failing on spawn errors.
///
/// Lua's `print` writes through the C runtime's text mode, so payload output
/// arrives CRLF-terminated on Windows while `inject` (a Rust write) uses `\n`;
/// normalise here so the assertions read the same on every platform.
fn stdout_of(preset: &str, arg: &str) -> (String, i32) {
    let out = run(preset, arg);
    assert!(
        out.status.code().is_some(),
        "mm was killed by a signal on {preset} {arg}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    (
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn lua_start_and_state_table() {
    // Named and indexed columns, the raw line, position, and the counters all
    // come from the same snapshot the TUI hands to lua.
    let (out, code) = stdout_of("lua_start.toml", "--list=0@accept");
    assert_eq!(code, 0, "stderr: {}", out);
    assert_eq!(out, "alpha|file-alpha.txt|1\talpha\tfile-alpha.txt|0|1|1\n");
}

#[test]
fn lua_exit_code_reaches_the_process() {
    let (out, code) = stdout_of("lua_start.toml", "--list=0@fail");
    assert_eq!(out, "");
    assert_eq!(code, 7);
}

#[test]
fn lua_start_rows_and_counters() {
    // Bare `--list` prints the injected rows in injection order.
    let (out, code) = stdout_of("lua_multi.toml", "--list");
    assert_eq!(code, 0, "stderr: {out}");
    assert_eq!(out, "1\talpha\n2\tbeta\n3\tgamma\n");

    // Counters do not depend on matcher ranking.
    let (out, code) = stdout_of("lua_multi.toml", "--list=1@counters");
    assert_eq!(code, 0, "stderr: {out}");
    assert_eq!(out, "3|3\n");

    // With no explicit selection the current item mirrors into `selected`, while
    // `selected_count` stays 0.
    let (out, code) = stdout_of("lua_multi.toml", "--list=2@selected");
    assert_eq!(code, 0, "stderr: {out}");
    assert_eq!(out, "1|0\n");
}

#[test]
fn lua_file_payloads_and_varargs() {
    // Both rows are reachable; which index holds which row is up to the
    // matcher, so compare the set.
    let mut seen = Vec::new();
    for n in 0..2 {
        let (out, code) = stdout_of("lua_file.toml", &format!("--list={n}@accept"));
        assert_eq!(code, 0, "stderr: {out}");
        seen.push(out);
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            "alpha|alpha|--flag|2\n".to_string(),
            "beta|beta|--flag|2\n".to_string()
        ]
    );
}

#[test]
fn lua_dynamic_envs_and_override() {
    let (out, code) = stdout_of("lua_envs.toml", "--list=0@accept");
    assert_eq!(code, 0, "stderr: {out}");
    let (greeting, override_path) = out.trim_end().split_once('|').expect("k=v pair");
    assert_eq!(greeting, "hi-from-lua");
    assert_eq!(
        Path::new(override_path)
            .file_name()
            .and_then(|s| s.to_str()),
        Some("lua_envs.toml")
    );
}
