//! Command execution strategy classification, shared by every execution path
//! (Execute payloads, dynamic `[envs]`, `start.directory`, Transform, Copy).

use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};
#[cfg(feature = "mlua")]
use std::ffi::OsString;

use cba::{
    bait::ResultExt,
    bring::split::split_whitespace_preserve_single_quotes,
    broc::{CommandExt, EnvVars},
};

/// How a script string is executed.
pub enum CommandStrategy {
    /// Plain shell command (or `@path …` direct execution).
    Shell,
    /// `#!lua …` — inline lua source for the lua engine.
    #[cfg(feature = "mlua")]
    Lua(String),
    /// `@path [args…]` — direct execution without shell interpretation.
    File,
    /// `@file.lua [args…]` — lua script file; `args` are the script's varargs.
    #[cfg(feature = "mlua")]
    LuaFile { path: OsString, args: Vec<OsString> },
}

/// Classify a script string.
///
/// `#!lua` and `@*.lua` payloads are only recognized when the `mlua` feature
/// is enabled; otherwise they fall back to [`CommandStrategy::Shell`] and
/// [`CommandStrategy::File`].
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub fn classify(s: &str) -> CommandStrategy {
    #[cfg(feature = "mlua")]
    if let Some(rest) = s.strip_prefix("#!lua") && rest.starts_with(char::is_whitespace) {
        return CommandStrategy::Lua(rest.trim_start().to_owned());
    }

    let Some(rest) = s.strip_prefix('@') else {
        return CommandStrategy::Shell;
    };
    let mut words = split_whitespace_preserve_single_quotes(rest).into_iter();
    let Some(path) = words.next() else {
        return CommandStrategy::File;
    };
#[cfg(feature = "mlua")]
let args: Vec<OsString> = words.map(OsString::from).collect();
    #[cfg(feature = "mlua")]
    if Path::new(&path).extension().is_some_and(|e| e == "lua") {
        return CommandStrategy::LuaFile { path: path.into(), args };
    }
    CommandStrategy::File
}

/// Run a script with empty picker state and return its captured text (used
/// before the picker is up: dynamic `[envs]`, `start.directory`).
pub(crate) fn run_value(script: &str, envs: &EnvVars) -> Option<String> {
    run_value_state(script, envs, &crate::lua::LuaState::empty())
}

/// [`run_value`] with the current matchmaker state exposed to lua payloads as
/// the `state` table. Shell payloads ignore the state.
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub(crate) fn run_value_state(
    script: &str,
    envs: &EnvVars,
    state: &crate::lua::LuaState,
) -> Option<String> {
    match classify(script) {
        #[cfg(feature = "mlua")]
        CommandStrategy::Lua(code) => match crate::lua::run_inline_value(&code, envs, state)
        {
            Ok(value) => value,
            Err(e) => {
                log::error!("Lua command failed: {e}");
                None
            }
        },
        #[cfg(feature = "mlua")]
        CommandStrategy::LuaFile { path, args: file_args } => {
            let Some(file) = resolve_at_path(&path, envs) else {
                return None;
            };
            match crate::lua::run_file_value(&file, file_args, envs, state) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Lua script @{} failed: {e}", path.to_string_lossy());
                    None
                }
            }
        }
        CommandStrategy::Shell | CommandStrategy::File => {
            Command::from_script(script, &[])
                .envs(envs.iter().cloned())
                .read_to_string()
                ._elog()
        }
    }
}

/// Resolve an `@` path: a relative path resolves against the parent of
/// `MM_OVERRIDE`. Returns `None` to skip execution (already logged).
pub(crate) fn resolve_at_path(path: &OsStr, envs: &EnvVars) -> Option<PathBuf> {
    if Path::new(path).is_absolute() {
        return Some(Path::new(path).to_path_buf());
    }
    let Some(override_path) = envs.get("MM_OVERRIDE") else {
        log::error!(
            "MM_OVERRIDE not set; skipping @ command: {}",
            path.to_string_lossy()
        );
        return None;
    };
    Some(
        Path::new(override_path)
            .parent()
            .unwrap_or(Path::new(""))
            .join(path),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "mlua")]
    #[test]
    fn lua_prefix_accepts_any_whitespace() {
        for s in [
            "#!lua return 1",
            "#!lua  return 1",
            "#!lua\nreturn 1",
            "#!lua\r\nreturn 1",
            "#!lua\treturn 1",
        ] {
            assert!(
                matches!(classify(s), CommandStrategy::Lua(code) if code == "return 1"),
                "expected Lua for {s:?}"
            );
        }
        // a bare `#!lua` with no separator is not a lua marker
        assert!(matches!(classify("#!luaprint('x')"), CommandStrategy::Shell));
    }

    #[cfg(feature = "mlua")]
    #[test]
    fn classify_recognizes_lua_files() {
        assert!(matches!(
            classify("@clean.lua 'a b' c"),
            CommandStrategy::LuaFile { path, args } if path == "clean.lua" && args == ["a b", "c"]
        ));
    }
}

