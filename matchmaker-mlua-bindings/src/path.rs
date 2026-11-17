//! Path manipulation and query helpers exposed to the Lua VM (`path.*`).

use std::path::{Path, PathBuf};

use cba::bs::diff_paths;
use mlua::{Lua, Table};

/// Create the `path` module table.
pub fn create_module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // path.parent(path) -> string
    let parent_fn = lua.create_function(|_, p: String| {
        let path = Path::new(&p);
        match path.parent() {
            Some(parent) if parent.as_os_str().is_empty() => Ok(".".to_string()),
            Some(parent) => Ok(parent.to_string_lossy().into_owned()),
            None => Ok(String::new()),
        }
    })?;
    t.set("parent", parent_fn)?;

    // path.base(path) -> string
    let base_fn = lua.create_function(|_, p: String| {
        let path = Path::new(&p);
        Ok(path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone()))
    })?;
    t.set("base", base_fn)?;

    // path.stem(path) -> string
    let stem_fn = lua.create_function(|_, p: String| {
        let path = Path::new(&p);
        Ok(path
            .file_stem()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| p.clone()))
    })?;
    t.set("stem", stem_fn)?;

    // path.ext(path) -> string
    let ext_fn = lua.create_function(|_, p: String| {
        let path = Path::new(&p);
        Ok(path
            .extension()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default())
    })?;
    t.set("ext", ext_fn)?;

    // path.join(...) -> string
    let join_fn = lua.create_function(|_, parts: mlua::Variadic<String>| {
        let mut buf = PathBuf::new();
        for part in parts {
            buf.push(part);
        }
        Ok(buf.to_string_lossy().into_owned())
    })?;
    t.set("join", join_fn)?;

    // path.is_file(path) -> bool
    let is_file_fn = lua.create_function(|_, p: String| Ok(Path::new(&p).is_file()))?;
    t.set("is_file", is_file_fn)?;

    // path.is_dir(path) -> bool
    let is_dir_fn = lua.create_function(|_, p: String| Ok(Path::new(&p).is_dir()))?;
    t.set("is_dir", is_dir_fn)?;

    // path.is_empty(path) -> bool
    let is_empty_fn = lua.create_function(|_, p: String| {
        let path = Path::new(&p);
        if path.is_file() {
            Ok(std::fs::metadata(path)
                .map(|m| m.len() == 0)
                .unwrap_or(false))
        } else if path.is_dir() {
            Ok(std::fs::read_dir(path)
                .map(|mut d| d.next().is_none())
                .unwrap_or(false))
        } else {
            Ok(false)
        }
    })?;
    t.set("is_empty", is_empty_fn)?;

    // path.exists(path) -> bool
    let exists_fn = lua.create_function(|_, p: String| Ok(Path::new(&p).exists()))?;
    t.set("exists", exists_fn)?;

    // path.lexists(path) -> bool
    let lexists_fn =
        lua.create_function(|_, p: String| Ok(Path::new(&p).symlink_metadata().is_ok()))?;
    t.set("lexists", lexists_fn)?;

    // path.rel(target, base) -> string | nil
    let rel_fn = lua.create_function(|_, (target, base): (String, String)| {
        Ok(diff_paths(&target, &base).map(|p| p.to_string_lossy().into_owned()))
    })?;
    t.set("rel", rel_fn)?;

    // path.expand_tilde(path) -> string
    let expand_tilde_fn = lua.create_function(|_, p: String| Ok(cba::bath::expand_tilde(&p)))?;
    t.set("expand_tilde", expand_tilde_fn)?;

    Ok(t)
}
