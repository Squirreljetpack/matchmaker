//! Execute lua payloads (`@file.lua`, `#!lua …`) on a fresh [`Lua`] per run.
//! Each execution owns its VM, so concurrent executions (ExecuteAsync tasks,
//! detached ExecuteSilent threads) never share or serialize on VM state.

mod formatter;

pub(crate) use formatter::LuaState;

use std::{
    cell::{Cell, RefCell},
    ffi::OsString,
    path::Path,
    rc::Rc,
};

use cba::broc::EnvVars;
use mlua::{Function, Lua, MultiValue, Table, Value, Variadic};

/// Run the lua file at `path` with `args` as varargs. The command's
/// environment (`MM_OVERRIDE`, `MM_PREVIEW_INDEX`, …) is exposed as the
/// global `env` table, the snapshot state as the global `state` table, and an
/// `os.exit(code)` that stops the script with `code` without terminating the
/// process.
pub(crate) fn run_file(
    path: &Path,
    args: Vec<OsString>,
    env: &EnvVars,
    state: &LuaState,
) -> Result<i32, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    let f = load_file(&lua, path)?;
    exit_code(f.call::<()>(Variadic::from(to_args(args))), exit.get())
}

/// Like [`run_file`], but returns the script's first value (converted to a
/// string) instead of its exit code. `None` means the script returned nothing.
pub(crate) fn run_file_value(
    path: &Path,
    args: Vec<OsString>,
    env: &EnvVars,
    state: &LuaState,
) -> Result<Option<String>, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    let f = load_file(&lua, path)?;
    value(f.call::<MultiValue>(Variadic::from(to_args(args))), exit.get())
}

/// Run inline lua source. Same VM, env, state, and exit semantics as
/// [`run_file`]; inline payloads receive no varargs — the item is read from
/// the `state` table.
pub(crate) fn run_inline(code: &str, env: &EnvVars, state: &LuaState) -> Result<i32, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    let f = load_inline(&lua, code)?;
    exit_code(f.call::<()>(()), exit.get())
}

/// Like [`run_inline`], but returns the script's first value (converted to a
/// string) instead of its exit code. `None` means the script returned nothing.
pub(crate) fn run_inline_value(
    code: &str,
    env: &EnvVars,
    state: &LuaState,
) -> Result<Option<String>, String> {
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    let f = load_inline(&lua, code)?;
    value(f.call::<MultiValue>(()), exit.get())
}

/// The exit code a completed script reports: the `os.exit` code when set,
/// `0` on success, or the error text otherwise.
#[cfg(feature = "mlua")]
fn exit_code(result: Result<(), mlua::Error>, exit: Option<i32>) -> Result<i32, String> {
    match (result, exit) {
        (_, Some(code)) => Ok(code),
        (Ok(()), None) => Ok(0),
        (Err(e), None) => Err(e.to_string()),
    }
}

/// The first value a script returned, converted to a string (`nil`/empty → `None`).
#[cfg(feature = "mlua")]
fn value(
    result: Result<MultiValue, mlua::Error>,
    exit: Option<i32>,
) -> Result<Option<String>, String> {
    match (result, exit) {
        // os.exit stopped the script; there is no return value to capture.
        (_, Some(_)) => Ok(None),
        (Ok(values), None) => {
            if let Some(v) = values.into_iter().next()
                && !matches!(v, Value::Nil)
            {
                v.to_string().map(Some).map_err(|e| e.to_string())
            } else {
                Ok(None)
            }
        }
        (Err(e), None) => Err(e.to_string()),
    }
}

#[cfg(feature = "mlua")]
fn to_args(args: Vec<OsString>) -> Vec<String> {
    args.iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect()
}

#[cfg(feature = "mlua")]
fn load_file(lua: &Lua, path: &Path) -> Result<Function, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read @{}: {e}", path.display()))?;
    lua.load(src)
        .set_name(format!("@{}", path.display()))
        .into_function()
        .map_err(|e| e.to_string())
}

#[cfg(feature = "mlua")]
fn load_inline(lua: &Lua, code: &str) -> Result<Function, String> {
    lua.load(code)
        .set_name("#!lua")
        .into_function()
        .map_err(|e| e.to_string())
}

/// Run a lua file whose `inject` global pushes lines into the picker. Used for
/// `start.command`/reload `@file.lua` payloads: the script supplies rows via
/// `inject(line)` instead of stdout. The push returns `Err` to fail the script
/// when the picker rejects a row.
#[cfg(feature = "mlua")]
pub(crate) fn run_file_inject<F>(
    path: &Path,
    args: Vec<OsString>,
    env: &EnvVars,
    state: &LuaState,
    push: F,
) -> Result<i32, String>
where
    F: FnMut(String) -> Result<(), String> + 'static,
{
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    set_inject(&lua, push)?;
    let f = load_file(&lua, path)?;
    exit_code(f.call::<()>(Variadic::from(to_args(args))), exit.get())
}

/// Inline counterpart of [`run_file_inject`], with the same `inject` global.
#[cfg(feature = "mlua")]
pub(crate) fn run_inline_inject<F>(
    code: &str,
    env: &EnvVars,
    state: &LuaState,
    push: F,
) -> Result<i32, String>
where
    F: FnMut(String) -> Result<(), String> + 'static,
{
    let exit = Rc::new(Cell::new(None));
    let lua = new_vm(env, state, &exit)?;
    set_inject(&lua, push)?;
    let f = load_inline(&lua, code)?;
    exit_code(f.call::<()>(()), exit.get())
}

/// Expose an `inject(line)` global that feeds lines to the picker. Errors from
/// the push fail the running script.
#[cfg(feature = "mlua")]
fn set_inject<F>(lua: &Lua, push: F) -> Result<(), String>
where
    F: FnMut(String) -> Result<(), String> + 'static,
{
    let push = RefCell::new(push);
    let f = lua
        .create_function(move |_, line: String| {
            push.borrow_mut()(line).map_err(mlua::Error::RuntimeError)
        })
        .map_err(|e| e.to_string())?;
    lua.globals().set("inject", f).map_err(|e| e.to_string())
}

/// Create the per-run VM: all safe stdlibs, the `env` table, the `state`
/// table, and an `os.exit` that records the code and stops the script (lua
/// 5.4's `os.exit` would terminate the host process).
#[cfg(feature = "mlua")]
fn new_vm(env: &EnvVars, state: &LuaState, exit: &Rc<Cell<Option<i32>>>) -> Result<Lua, String> {
    let lua = Lua::new();

    let table = lua.create_table().map_err(|e| e.to_string())?;
    for (k, v) in env.iter() {
        table.set(k.clone(), v.clone()).map_err(|e| e.to_string())?;
    }
    lua.globals().set("env", table).map_err(|e| e.to_string())?;

    lua.globals()
        .set(
            "state",
            build_state_table(&lua, state).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;

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

/// Materialize a [`LuaState`] as the global `state` table. Items are arrays of
/// column values, also indexed by their configured column names.
#[cfg(feature = "mlua")]
fn build_state_table(lua: &Lua, state: &LuaState) -> mlua::Result<Table> {
    let s = lua.create_table()?;

    s.set("query", state.query.as_str())?;
    s.set("mode", state.mode.as_str())?;
    s.set("total", state.total)?;
    s.set("matched", state.matched)?;
    s.set("selected_count", state.selected_count)?;
    s.set("active", state.active + 1)?;

    let args = lua.create_table()?;
    for (i, a) in state.args.iter().enumerate() {
        args.set(i + 1, a.as_str())?;
    }
    s.set("args", args)?;

    if let Some(raw) = &state.raw {
        s.set("raw", raw.as_str())?;
    }
    if let Some(position) = state.position {
        s.set("position", position)?;
    }
    if let Some(current) = &state.current {
        s.set("current", columns_table(lua, current)?)?;
    }

    let selected = lua.create_table()?;
    for (i, item) in state.selected.iter().enumerate() {
        selected.set(i + 1, columns_table(lua, item)?)?;
    }
    s.set("selected", selected)?;

    Ok(s)
}

/// One item as a lua array of column values, with configured column names set
/// as additional string keys (so `current[1]` and `current.path` both work).
#[cfg(feature = "mlua")]
fn columns_table(lua: &Lua, columns: &[(String, String)]) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (i, (name, value)) in columns.iter().enumerate() {
        t.set(i + 1, value.as_str())?;
        if !name.is_empty() {
            t.set(name.as_str(), value.as_str())?;
        }
    }
    Ok(t)
}

#[cfg(all(test, feature = "mlua"))]
mod tests {
    use super::*;
    use cba::env_vars;

    #[test]
    fn run_inline_returns_exit_code() {
        let env = EnvVars::default();
        let state = LuaState::empty();
        assert_eq!(run_inline("return", &env, &state).unwrap(), 0);
        assert_eq!(run_inline("os.exit(3)", &env, &state).unwrap(), 3);
        assert_eq!(run_inline("os.exit()", &env, &state).unwrap(), 0);
        assert_eq!(run_inline("os.exit(false)", &env, &state).unwrap(), 1);
        assert!(run_inline("error('boom')", &env, &state).is_err());
        assert!(run_inline("this is not lua", &env, &state).is_err());
    }

    #[test]
    fn run_inline_value_returns_first_value() {
        let env = EnvVars::default();
        let state = LuaState::empty();
        assert_eq!(run_inline_value("return 42", &env, &state).unwrap(), Some("42".into()));
        assert_eq!(run_inline_value("return nil", &env, &state).unwrap(), None);
        assert_eq!(run_inline_value("return", &env, &state).unwrap(), None);
        assert_eq!(run_inline_value("return 'a', 'b'", &env, &state).unwrap(), Some("a".into()));
        assert!(run_inline_value("error('boom')", &env, &state).is_err());
    }

    #[test]
    fn inline_lua_has_no_varargs_and_sees_state() {
        let env = EnvVars::default();
        let state = LuaState {
            query: "my query".into(),
            args: vec!["-o".into(), "out.txt".into()],
            ..LuaState::empty()
        };
        // `...` is empty for inline payloads: the item comes from `state`.
        assert_eq!(run_inline_value("return ...", &env, &state).unwrap(), None);
        assert_eq!(
            run_inline_value("return state.query .. '/' .. state.args[2]", &env, &state).unwrap(),
            Some("my query/out.txt".into())
        );
    }

    #[test]
    fn run_file_passes_varargs_and_env() {
        let path = std::env::temp_dir().join("mm_lua_engine_test.lua");
        std::fs::write(
            &path,
            r#"assert(env.MM_OVERRIDE == '/x', env.MM_OVERRIDE or 'nil')
assert((...) == 'a b', (...))
assert(select('#', ...) == 1)
assert(state.total == 0, 'state table present')"#,
        )
        .unwrap();
        let env = env_vars!("MM_OVERRIDE" => "/x");
        assert_eq!(
            run_file(&path, vec![OsString::from("a b")], &env, &LuaState::empty()).unwrap(),
            0
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn run_file_value_returns_first_value() {
        let path = std::env::temp_dir().join("mm_lua_engine_test_val.lua");
        std::fs::write(&path, "return \"hello\", 42").unwrap();
        let env = EnvVars::default();
        assert_eq!(
            run_file_value(&path, vec![], &env, &LuaState::empty()).unwrap(),
            Some("hello".into())
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn run_file_inject_pushes_lines_and_returns_exit_code() {
        let path = std::env::temp_dir().join("mm_lua_engine_inject_test.lua");
        std::fs::write(
            &path,
            "inject(env.MM_OVERRIDE); inject('a\\tb'); os.exit(7)",
        )
        .unwrap();
        let env = env_vars!("MM_OVERRIDE" => "/x");
        let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let out = lines.clone();
        assert_eq!(
            run_file_inject(&path, vec![], &env, &LuaState::empty(), {
                move |l| {
                    out.borrow_mut().push(l.to_string());
                    Ok(())
                }
            })
            .unwrap(),
            7
        );
        assert_eq!(*lines.borrow(), vec!["/x", "a\tb"]);
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn run_inline_inject_pushes_lines() {
        let env = EnvVars::default();
        let state = LuaState::empty();
        let lines = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let out = lines.clone();
        let code = "inject('a'); inject('b'); inject('c\\nd')";
        let _ = run_inline_inject(code, &env, &state, move |l| {
            out.borrow_mut().push(l.to_string());
            Ok(())
        })
        .unwrap();
        assert_eq!(*lines.borrow(), vec!["a", "b", "c\nd"]);
    }

    #[test]
    fn inject_errors_fail_the_script() {
        let env = EnvVars::default();
        let state = LuaState::empty();
        let err = run_inline_inject("inject('x')", &env, &state, |_| {
            Err("picker closed".into())
        })
        .unwrap_err();
        assert!(err.contains("picker closed"), "unexpected error: {err}");
    }

    #[test]
    fn run_file_surfaces_script_errors() {
        let path = std::env::temp_dir().join("mm_lua_engine_test_err.lua");
        std::fs::write(&path, "error('boom')").unwrap();
        let err = run_file(&path, vec![], &EnvVars::default(), &LuaState::empty()).unwrap_err();
        assert!(err.contains("boom"), "unexpected error: {err}");
        std::fs::remove_file(&path).ok();
    }
}
