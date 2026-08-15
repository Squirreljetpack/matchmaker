//! Execute lua payloads (`@file.lua`, `#!lua …`) on a fresh [`Lua`] per run.
//! Each execution owns its VM, so concurrent executions (ExecuteAsync tasks,
//! detached ExecuteSilent threads) never share or serialize on VM state.

use std::{ffi::OsString, path::Path};

use cba::broc::EnvVars;

#[cfg(feature = "mlua")]
use std::{cell::Cell, rc::Rc};

#[cfg(feature = "mlua")]
use mlua::{Lua, MultiValue, Table, Value, Variadic};

/// Run the lua file at `path` with `args` as varargs. The command's
/// environment (`MM_OVERRIDE`, `MM_PREVIEW_COMMAND`, …) is exposed as the
/// global `env` table; the script's `os.exit(code)` stops the script with
/// `code` without terminating the process.
#[cfg(feature = "mlua")]
pub(crate) fn run_file(path: &Path, args: Vec<OsString>, env: &EnvVars) -> Result<i32, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, &exit)?;
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read @{}: {e}", path.display()))?;
    let f = lua
        .load(src)
        .set_name(format!("@{}", path.display()))
        .into_function()
        .map_err(|e| e.to_string())?;
    let args: Vec<String> = args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let result = f.call::<MultiValue>(Variadic::from(args));
    exit_code(result, exit.get())
}

/// Run inline lua source. Same VM, env, and exit semantics as [`run_file`].
#[cfg(feature = "mlua")]
pub(crate) fn run_inline(code: &str, env: &EnvVars) -> Result<i32, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, &exit)?;
    let result = lua.load(code).set_name("#!lua").exec();
    exit_code(result, exit.get())
}

#[cfg(not(feature = "mlua"))]
pub(crate) fn run_file(path: &Path, _args: Vec<OsString>, _env: &EnvVars) -> Result<i32, String> {
    Err(format!(
        "the mlua feature is disabled; cannot run @{}",
        path.display()
    ))
}

#[cfg(not(feature = "mlua"))]
pub(crate) fn run_inline(_code: &str, _env: &EnvVars) -> Result<i32, String> {
    Err("the mlua feature is disabled; cannot run #!lua commands".into())
}

#[cfg(feature = "mlua")]
fn exit_code<T>(result: Result<T, mlua::Error>, exit: Option<i32>) -> Result<i32, String> {
    match (result, exit) {
        (_, Some(code)) => Ok(code),
        (Ok(_), None) => Ok(0),
        (Err(e), None) => Err(e.to_string()),
    }
}

/// Create the per-run VM: all safe stdlibs, the `env` table, and an
/// `os.exit` that records the code and stops the script (lua 5.4's `os.exit`
/// would terminate the host process).
#[cfg(feature = "mlua")]
fn new_vm(env: &EnvVars, exit: &Rc<Cell<Option<i32>>>) -> Result<Lua, String> {
    let lua = Lua::new();
    let table = lua.create_table().map_err(|e| e.to_string())?;
    for (k, v) in env.iter() {
        table.set(k.clone(), v.clone()).map_err(|e| e.to_string())?;
    }
    lua.globals().set("env", table).map_err(|e| e.to_string())?;

    let exit = exit.clone();
    let os_exit = lua
        .create_function(move |_, status: Option<Value>| -> mlua::Result<()> {
            let code = match status {
                Some(Value::Boolean(true)) | None => 0,
                Some(Value::Boolean(false)) => 1,
                Some(Value::Integer(n)) => n as i32,
                _ => 0,
            };
            exit.set(Some(code));
            Err(mlua::Error::RuntimeError(format!("exit {code}")))
        })
        .map_err(|e| e.to_string())?;
    let os: Table = lua.globals().get("os").map_err(|e| e.to_string())?;
    os.set("exit", os_exit).map_err(|e| e.to_string())?;
    Ok(lua)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cba::env_vars;

    #[test]
    fn run_inline_returns_exit_code() {
        let env = EnvVars::default();
        assert_eq!(run_inline("return", &env).unwrap(), 0);
        assert_eq!(run_inline("os.exit(3)", &env).unwrap(), 3);
        assert_eq!(run_inline("os.exit()", &env).unwrap(), 0);
        assert_eq!(run_inline("os.exit(false)", &env).unwrap(), 1);
        assert!(run_inline("error('boom')", &env).is_err());
        assert!(run_inline("this is not lua", &env).is_err());
    }

    #[test]
    fn run_file_passes_varargs_and_env() {
        let path = std::env::temp_dir().join("mm_lua_engine_test.lua");
        std::fs::write(
            &path,
            r#"assert(env.MM_OVERRIDE == '/x', env.MM_OVERRIDE or 'nil')
assert((...) == 'a b', (...))
assert(select('#', ...) == 1)"#,
        )
        .unwrap();
        let env = env_vars!("MM_OVERRIDE" => "/x");
        assert_eq!(
            run_file(&path, vec![OsString::from("a b")], &env).unwrap(),
            0
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn run_file_surfaces_script_errors() {
        let path = std::env::temp_dir().join("mm_lua_engine_test_err.lua");
        std::fs::write(&path, "error('boom')").unwrap();
        let err = run_file(&path, vec![], &EnvVars::default()).unwrap_err();
        assert!(err.contains("boom"), "unexpected error: {err}");
        std::fs::remove_file(&path).ok();
    }
}
