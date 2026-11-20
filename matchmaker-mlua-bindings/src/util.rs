//! Utility functions exposed to the Lua VM (`util.*`).

use std::time::Duration;

use mlua::{ExternalResult, Lua, LuaSerdeExt, Table, Value};

/// Create the `util` module table.
pub fn create_module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // util.sleep(ms)
    let sleep_fn = lua.create_function(|_, ms: u64| {
        std::thread::sleep(Duration::from_millis(ms));
        Ok(())
    })?;
    t.set("sleep", sleep_fn)?;

    // util.uuid() -> UUID v4 string
    let uuid_fn = lua.create_function(|_, ()| {
        let mut bytes = [0u8; 16];
        for b in &mut bytes {
            *b = fastrand::u8(..);
        }
        // UUID v4 variant
        bytes[6] = (bytes[6] & 0x0f) | 0x40;
        bytes[8] = (bytes[8] & 0x3f) | 0x80;
        Ok(format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            bytes[0],
            bytes[1],
            bytes[2],
            bytes[3],
            bytes[4],
            bytes[5],
            bytes[6],
            bytes[7],
            bytes[8],
            bytes[9],
            bytes[10],
            bytes[11],
            bytes[12],
            bytes[13],
            bytes[14],
            bytes[15]
        ))
    })?;
    t.set("uuid", uuid_fn)?;

    // util.timestamp([format])
    let timestamp_fn = lua.create_function(|_, fmt: Option<String>| {
        let now = jiff::ZonedDateTime::now();
        match fmt.as_deref() {
            None => Ok(now.to_string()),
            Some("unix") | Some("epoch") => Ok(now.timestamp().as_second().to_string()),
            Some("ms") => Ok(now.timestamp().as_millisecond().to_string()),
            Some(f) => {
                let mut out = String::new();
                std::fmt::write(&mut out, format_args!("{}", now.strftime(f))).map_err(|e| {
                    mlua::Error::RuntimeError(format!("invalid timestamp format: {e}"))
                })?;
                Ok(out)
            }
        }
    })?;
    t.set("timestamp", timestamp_fn)?;

    // util.json_encode(value) -> string
    let json_encode_fn =
        lua.create_function(|_, val: Value| serde_json::to_string(&val).into_lua_err())?;
    t.set("json_encode", json_encode_fn)?;

    // util.json_decode(text) -> value
    let json_decode_fn = lua.create_function(|lua, text: String| {
        let v: serde_json::Value = serde_json::from_str(&text).into_lua_err()?;
        lua.to_value(&v)
    })?;
    t.set("json_decode", json_decode_fn)?;

    // util.toml_encode(value) -> string
    let toml_encode_fn =
        lua.create_function(|_, val: Value| toml::to_string(&val).into_lua_err())?;
    t.set("toml_encode", toml_encode_fn)?;

    // util.toml_decode(text) -> value
    let toml_decode_fn = lua.create_function(|lua, text: String| {
        let v: toml::Value = toml::from_str(&text).into_lua_err()?;
        lua.to_value(&v)
    })?;
    t.set("toml_decode", toml_decode_fn)?;

    // util.shq(str) -> string
    let shq_fn = lua.create_function(|_, s: String| Ok(shell_words::quote(&s).into_owned()))?;
    t.set("shq", shq_fn)?;

    // util.split_args(line) -> { "prog", "arg", … }
    //
    // POSIX word splitting, i.e. the inverse of `shq`: quotes group words and
    // are removed, backslashes escape. It exists for values a shell would
    // otherwise expand, such as `$EDITOR="code --wait"`.
    let split_args_fn = lua.create_function(|lua, line: String| {
        let words = shell_words::split(&line).into_lua_err()?;
        lua.create_sequence_from(words)
    })?;
    t.set("split_args", split_args_fn)?;

    Ok(t)
}
