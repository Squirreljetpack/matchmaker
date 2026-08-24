mod exit;
mod strategy;

pub use exit::ExitDetails;
pub use strategy::{CommandStrategy, classify};

use std::{
    ffi::OsString,
    process::{Command, Stdio},
};

use cba::{
    bait::ResultExt,
    broc::{CommandExt, EnvVars, SHELL, tty_or_inherit},
};

// ---------------------------------------------------------------------------
// Process execution helpers (Non-Lua)
// ---------------------------------------------------------------------------

/// Execute a process command and capture its standard output as a `String`.
pub fn run_command_capture(mut command: Command) -> Option<String> {
    command.read_to_string()._elog()
}

/// Build a `std::process::Command` for a `CommandStrategy::Shell` or `CommandStrategy::File`.
pub(crate) fn build_command(strategy: &CommandStrategy, shell: &[OsString]) -> Option<Command> {
    match strategy {
        CommandStrategy::Shell(cmd) => Some(Command::from_script(cmd, shell)),
        CommandStrategy::File { path, args, direct } => {
            // direct: exec the file itself, letting the kernel honor its
            // shebang (requires the executable bit)
            if *direct {
                let mut cmd = Command::new(path);
                cmd.args(args);
                return Some(cmd);
            }
            let mut cmd = Command::new(
                shell
                    .first()
                    .cloned()
                    .unwrap_or_else(|| OsString::from(SHELL.0.as_str())),
            );
            cmd.arg(path);
            cmd.args(args);
            Some(cmd)
        }
        #[cfg(feature = "mlua")]
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Lua execution helpers (Feature-gated under "mlua")
// ---------------------------------------------------------------------------

/// Run an inline Lua script or file as an execution target, blocking the thread.
#[cfg(feature = "mlua")]
pub fn run_lua_status(
    strategy: &CommandStrategy,
    envs: &EnvVars,
    lua_state: &crate::lua::LuaState,
) -> Option<ExitDetails> {
    match strategy {
        CommandStrategy::Lua(code) => match crate::lua::run_inline(code, envs, lua_state) {
            Ok(code) => Some(ExitDetails::code(code)),
            Err(e) => {
                log::error!("Lua command failed: {e}");
                Some(ExitDetails::error())
            }
        },
        CommandStrategy::LuaFile { path, args } => {
            match crate::lua::run_file(std::path::Path::new(path), args.clone(), envs, lua_state) {
                Ok(code) => Some(ExitDetails::code(code)),
                Err(e) => {
                    log::error!("Lua script @{} failed: {e}", path.to_string_lossy());
                    Some(ExitDetails::error())
                }
            }
        }
        _ => None,
    }
}

/// Run an inline Lua script or file and capture its return value or stdout as a `String`.
#[cfg(feature = "mlua")]
pub fn run_lua_capture(
    strategy: &CommandStrategy,
    envs: &EnvVars,
    lua_state: &crate::lua::LuaState,
) -> Option<String> {
    match strategy {
        CommandStrategy::Lua(code) => match crate::lua::run_inline_value(code, envs, lua_state) {
            Ok(value) => value,
            Err(e) => {
                log::error!("Lua command failed: {e}");
                None
            }
        },
        CommandStrategy::LuaFile { path, args } => {
            match crate::lua::run_file_value(
                std::path::Path::new(path),
                args.clone(),
                envs,
                lua_state,
            ) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Lua script @{} failed: {e}", path.to_string_lossy());
                    None
                }
            }
        }
        _ => None,
    }
}

/// Run an inline Lua script or file and stream/inject rows using a pusher callback.
#[cfg(feature = "mlua")]
pub fn run_lua_inject<F>(
    strategy: &CommandStrategy,
    envs: &EnvVars,
    lua_state: &crate::lua::LuaState,
    push: F,
) -> Result<i32, String>
where
    F: FnMut(String) -> Result<(), String> + 'static,
{
    match strategy {
        CommandStrategy::Lua(code) => crate::lua::run_inline_inject(code, envs, lua_state, push),
        CommandStrategy::LuaFile { path, args } => crate::lua::run_file_inject(
            std::path::Path::new(path),
            args.clone(),
            envs,
            lua_state,
            push,
        ),
        _ => Err("cannot inject rows from a non-Lua command strategy".into()),
    }
}

// ---------------------------------------------------------------------------
// Unified execution dispatchers
// ---------------------------------------------------------------------------

/// Execute a strategy synchronously to completion, inheriting terminal stdio.
/// Returns the normalized [`ExitDetails`], or `None` if the command failed to spawn or resolve.
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub fn run_execute(
    strategy: &CommandStrategy,
    shell: &[OsString],
    envs: &EnvVars,
    #[cfg(feature = "mlua")] lua_state: &crate::lua::LuaState,
) -> Option<ExitDetails> {
    #[cfg(feature = "mlua")]
    if strategy.is_lua() {
        return run_lua_status(strategy, envs, lua_state);
    }
    let mut command = build_command(strategy, shell)?;
    command
        .envs(envs.as_strs())
        .stdin(tty_or_inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = command.status()._elog()?;
    Some(ExitDetails::of(status))
}

/// Run a command strategy silently in a detached background thread.
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub fn run_execute_silent(
    strategy: CommandStrategy,
    shell: Vec<OsString>,
    envs: EnvVars,
    #[cfg(feature = "mlua")] lua_state: crate::lua::LuaState,
) {
    std::thread::spawn(move || {
        #[cfg(feature = "mlua")]
        if strategy.is_lua() {
            run_lua_status(&strategy, &envs, &lua_state);
            return;
        }
        if let Some(mut command) = build_command(&strategy, &shell) {
            command.envs(envs.as_strs()).stdin(tty_or_inherit());
            let _ = command._spawn();
        }
    });
}

/// Execute a strategy synchronously and capture its standard output / returned value as a `String`.
/// Used by `Transform`, `TransformConfig`, `Copy`, `CopyAsync`, `[envs]`, and `start.directory`.
#[cfg_attr(not(feature = "mlua"), allow(unused_variables))]
pub fn run_capture(
    strategy: &CommandStrategy,
    envs: &EnvVars,
    #[cfg(feature = "mlua")] lua_state: &crate::lua::LuaState,
) -> Option<String> {
    #[cfg(feature = "mlua")]
    if strategy.is_lua() {
        return run_lua_capture(strategy, envs, lua_state);
    }
    let mut command = build_command(strategy, &[])?;
    command.envs(envs.as_strs());
    run_command_capture(command)
}

/// Run an injection strategy (for `start.command`, `Reload`, `--list`).
#[cfg_attr(not(feature = "mlua"), allow(unused_variables, dead_code))]
pub fn run_inject<F>(
    strategy: &CommandStrategy,
    envs: &EnvVars,
    #[cfg(feature = "mlua")] lua_state: &crate::lua::LuaState,
    push: F,
) -> Result<i32, String>
where
    F: FnMut(String) -> Result<(), String> + 'static,
{
    #[cfg(feature = "mlua")]
    if strategy.is_lua() {
        return run_lua_inject(strategy, envs, lua_state, push);
    }
    #[cfg(not(feature = "mlua"))]
    {
        Err("the mlua feature is disabled".into())
    }
    #[cfg(feature = "mlua")]
    {
        Err("cannot inject rows from a non-Lua command strategy".into())
    }
}
