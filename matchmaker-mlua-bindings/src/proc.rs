//! Subprocess execution helpers exposed to the Lua VM (`proc.*`).

use std::collections::HashMap;
use std::process::{Command, Stdio};

use cba::broc::CommandExt;
use mlua::{ExternalResult, Lua, Table};

/// Create the `proc` module table.
pub fn create_module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // proc.run(prog, [args], [opts]) -> { ok = bool, code = number, stdout = string, stderr = string }
    let run_fn = lua.create_function(
        |lua, (prog, args, opts): (String, Option<Vec<String>>, Option<Table>)| {
            let mut cmd = Command::new(&prog);
            if let Some(args) = args {
                cmd.args(args);
            }
            apply_opts(&mut cmd, opts.as_ref())?;
            let stdin_data = match opts.as_ref() {
                Some(opts) => opts.get::<Option<String>>("stdin")?,
                None => None,
            };

            if stdin_data.is_some() {
                cmd.stdin(Stdio::piped());
            } else {
                cmd.stdin(Stdio::null());
            }
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd
                .spawn()
                .map_err(|e| mlua::Error::RuntimeError(format!("failed to spawn '{prog}': {e}")))?;

            // Feed stdin from a separate thread: writing the whole payload
            // before reading stdout deadlocks once the child fills its stdout
            // pipe while waiting for more input.
            let writer = stdin_data.and_then(|data| {
                child.stdin.take().map(|mut stdin| {
                    std::thread::spawn(move || {
                        use std::io::Write;
                        let _ = stdin.write_all(data.as_bytes());
                    })
                })
            });

            let output = child.wait_with_output().into_lua_err()?;
            if let Some(writer) = writer {
                let _ = writer.join();
            }

            let res = lua.create_table()?;
            res.set("ok", output.status.success())?;
            res.set("code", output.status.code().unwrap_or(-1))?;
            res.set(
                "stdout",
                String::from_utf8_lossy(&output.stdout).into_owned(),
            )?;
            res.set(
                "stderr",
                String::from_utf8_lossy(&output.stderr).into_owned(),
            )?;

            Ok(res)
        },
    )?;
    t.set("run", run_fn)?;

    // proc.spawn(prog, [args], [opts]) -> { ok = bool, pid = number }
    let spawn_fn = lua.create_function(
        |lua, (prog, args, opts): (String, Option<Vec<String>>, Option<Table>)| {
            let mut cmd = Command::new(&prog);
            if let Some(args) = args {
                cmd.args(args);
            }
            apply_opts(&mut cmd, opts.as_ref())?;
            cmd.stdin(Stdio::null());
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            let child = cmd
                .spawn()
                .map_err(|e| mlua::Error::RuntimeError(format!("failed to spawn '{prog}': {e}")))?;

            let pid = child.id();
            // Reap in the background: dropping a `Child` leaves a zombie on unix
            // and a leaked handle on windows, and one long TUI session may spawn
            // any number of children.
            std::thread::spawn(move || {
                let mut child = child;
                let _ = child.wait();
            });

            let res = lua.create_table()?;
            res.set("ok", true)?;
            res.set("pid", pid)?;
            Ok(res)
        },
    )?;
    t.set("spawn", spawn_fn)?;

    // proc.exec(prog, [args], [opts]) -> never returns
    let exec_fn = lua.create_function(
        |_, (prog, args, opts): (String, Option<Vec<String>>, Option<Table>)| -> mlua::Result<()> {
            let mut cmd = Command::new(&prog);
            if let Some(args) = args {
                cmd.args(args);
            }
            apply_opts(&mut cmd, opts.as_ref())?;
            // Replaces the process on unix; spawns, waits and exits with the
            // child's status on windows.
            cmd._exec()
        },
    )?;
    t.set("exec", exec_fn)?;

    // proc.has(prog) -> bool
    let has_fn = lua.create_function(|_, prog: String| Ok(cba::broc::has(&prog)))?;
    t.set("has", has_fn)?;

    Ok(t)
}

/// Apply the `cwd` and `env` options shared by `proc.run`, `proc.spawn` and
/// `proc.exec`.
fn apply_opts(cmd: &mut Command, opts: Option<&Table>) -> mlua::Result<()> {
    let Some(opts) = opts else {
        return Ok(());
    };
    if let Some(cwd) = opts.get::<Option<String>>("cwd")? {
        cmd.current_dir(cwd);
    }
    if let Some(env) = opts.get::<Option<HashMap<String, String>>>("env")? {
        cmd.envs(env);
    }
    Ok(())
}
