//! Native filesystem and metadata operations exposed to the Lua VM (`fs.*`).

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use mlua::{ExternalResult, Lua, Table, Value};

/// Create the `fs` module table.
pub fn create_module(lua: &Lua) -> mlua::Result<Table> {
    let t = lua.create_table()?;

    // fs.stat(path) -> table | nil
    let stat_fn = lua.create_function(|lua, p: String| {
        let path = Path::new(&p);
        let md = match fs::metadata(path) {
            Ok(m) => m,
            Err(_) => return Ok(None),
        };
        let sym_md = fs::symlink_metadata(path).ok();
        let is_symlink = sym_md.as_ref().map(|s| s.is_symlink()).unwrap_or(false);

        let res = lua.create_table()?;
        res.set("size", md.len())?;
        res.set("is_file", md.is_file())?;
        res.set("is_dir", md.is_dir())?;
        res.set("is_symlink", is_symlink)?;
        res.set("readonly", md.permissions().readonly())?;

        let mtime = md
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        res.set("mtime", mtime)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = md.permissions().mode();
            res.set("mode", mode)?;
            res.set("executable", mode & 0o111 != 0)?;
        }
        #[cfg(not(unix))]
        {
            res.set("mode", 0u32)?;
            res.set("executable", false)?;
        }

        Ok(Some(res))
    })?;
    t.set("stat", stat_fn)?;

    // fs.chmod(path, mode) -> bool
    let chmod_fn = lua.create_function(|_, (p, mode_val): (String, Value)| {
        let path = Path::new(&p);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let new_mode = match mode_val {
                Value::Integer(n) => n as u32,
                Value::Number(n) => n as u32,
                Value::String(s) => {
                    let str_val = s.to_str()?;
                    if str_val == "+x" || str_val == "-x" {
                        let curr = fs::metadata(path).into_lua_err()?.permissions().mode();
                        if str_val == "+x" {
                            curr | 0o111
                        } else {
                            curr & !0o111
                        }
                    } else {
                        u32::from_str_radix(str_val.trim_start_matches("0o"), 8).map_err(|_| {
                            mlua::Error::RuntimeError(format!("invalid chmod mode: {str_val}"))
                        })?
                    }
                }
                _ => {
                    return Err(mlua::Error::RuntimeError(
                        "mode must be an integer, octal string, '+x', or '-x'".into(),
                    ));
                }
            };
            fs::set_permissions(path, fs::Permissions::from_mode(new_mode)).into_lua_err()?;
            Ok(true)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode_val);
            Ok(false)
        }
    })?;
    t.set("chmod", chmod_fn)?;

    // fs.mkdir(path, [recursive = true]) -> bool
    let mkdir_fn = lua.create_function(|_, (p, recursive): (String, Option<bool>)| {
        let path = Path::new(&p);
        if recursive.unwrap_or(true) {
            fs::create_dir_all(path).into_lua_err()?;
        } else {
            fs::create_dir(path).into_lua_err()?;
        }
        Ok(true)
    })?;
    t.set("mkdir", mkdir_fn)?;

    // fs.readdir(path) -> table of strings
    let readdir_fn = lua.create_function(|lua, p: String| {
        let path = Path::new(&p);
        let rd = fs::read_dir(path).into_lua_err()?;
        let res = lua.create_table()?;
        for (i, e) in rd.flatten().enumerate() {
            res.raw_seti(i + 1, e.file_name().to_string_lossy().into_owned())?;
        }
        Ok(res)
    })?;
    t.set("readdir", readdir_fn)?;

    // fs.read(path) -> string
    let read_fn =
        lua.create_function(|_, p: String| fs::read_to_string(Path::new(&p)).into_lua_err())?;
    t.set("read", read_fn)?;

    // fs.write(path, content) -> bool
    let write_fn = lua.create_function(|_, (p, content): (String, String)| {
        fs::write(Path::new(&p), content).into_lua_err()?;
        Ok(true)
    })?;
    t.set("write", write_fn)?;

    // fs.remove(path, [recursive = false]) -> bool
    let remove_fn = lua.create_function(|_, (p, recursive): (String, Option<bool>)| {
        let path = Path::new(&p);
        let md = fs::symlink_metadata(path).into_lua_err()?;
        if md.is_symlink() {
            // Unlink the link itself; a symlinked directory must not be walked.
            #[cfg(windows)]
            if fs::remove_file(path).is_err() {
                fs::remove_dir(path).into_lua_err()?;
            }
            #[cfg(not(windows))]
            fs::remove_file(path).into_lua_err()?;
        } else if md.is_dir() {
            if recursive.unwrap_or(false) {
                fs::remove_dir_all(path).into_lua_err()?;
            } else {
                fs::remove_dir(path).into_lua_err()?;
            }
        } else {
            fs::remove_file(path).into_lua_err()?;
        }
        Ok(true)
    })?;
    t.set("remove", remove_fn)?;

    // fs.copy(src, dst) -> bool
    let copy_fn = lua.create_function(|_, (src, dst): (String, String)| {
        fs::copy(Path::new(&src), Path::new(&dst)).into_lua_err()?;
        Ok(true)
    })?;
    t.set("copy", copy_fn)?;

    // fs.rename(src, dst) -> bool
    let rename_fn = lua.create_function(|_, (src, dst): (String, String)| {
        fs::rename(Path::new(&src), Path::new(&dst)).into_lua_err()?;
        Ok(true)
    })?;
    t.set("rename", rename_fn)?;

    // fs.symlink(src, dst) -> bool
    let symlink_fn = lua.create_function(|_, (src, dst): (String, String)| {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(Path::new(&src), Path::new(&dst)).into_lua_err()?;
            Ok(true)
        }
        #[cfg(windows)]
        {
            let src_p = Path::new(&src);
            if src_p.is_dir() {
                std::os::windows::fs::symlink_dir(src_p, Path::new(&dst)).into_lua_err()?;
            } else {
                std::os::windows::fs::symlink_file(src_p, Path::new(&dst)).into_lua_err()?;
            }
            Ok(true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(mlua::Error::RuntimeError(
                "symlink unsupported on platform".into(),
            ))
        }
    })?;
    t.set("symlink", symlink_fn)?;

    // fs.readlink(path) -> string
    let readlink_fn = lua.create_function(|_, p: String| {
        fs::read_link(Path::new(&p))
            .map(|pb| pb.to_string_lossy().into_owned())
            .into_lua_err()
    })?;
    t.set("readlink", readlink_fn)?;

    // fs.canonicalize(path) -> string
    let canonicalize_fn = lua.create_function(|_, p: String| {
        fs::canonicalize(Path::new(&p))
            .map(|pb| pb.to_string_lossy().into_owned())
            .into_lua_err()
    })?;
    t.set("canonicalize", canonicalize_fn)?;

    // fs.tempdir([prefix]) -> string
    let tempdir_fn = lua.create_function(|_, prefix: Option<String>| {
        fresh_temp_path(prefix.as_deref(), |p| fs::create_dir(p)).into_lua_err()
    })?;
    t.set("tempdir", tempdir_fn)?;

    // fs.tempfile([prefix]) -> string
    let tempfile_fn = lua.create_function(|_, prefix: Option<String>| {
        fresh_temp_path(prefix.as_deref(), |p| {
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(p)
                .map(drop)
        })
        .into_lua_err()
    })?;
    t.set("tempfile", tempfile_fn)?;

    Ok(t)
}

/// Create an unused path under the temp dir by attempting `create` until it
/// does not collide with an existing entry.
fn fresh_temp_path(
    prefix: Option<&str>,
    create: impl Fn(&Path) -> std::io::Result<()>,
) -> std::io::Result<String> {
    let prefix = prefix.unwrap_or("tmp_");
    // The prefix is a file-name stem under the temp dir, so only a path
    // separator (or, on windows, a drive prefix) can move the result out of it.
    // A `..` inside the stem is harmless because the generated suffix always
    // follows it, and rejecting it would refuse prefixes like `v1..2_`.
    let escapes =
        prefix.chars().any(std::path::is_separator) || (cfg!(windows) && prefix.contains(':'));
    if escapes {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid temp prefix",
        ));
    }
    for _ in 0..64 {
        let p = std::env::temp_dir().join(format!("{prefix}{:x}", fastrand::u64(..)));
        match create(&p) {
            Ok(()) => {
                // Lua strings are UTF-8: a replacement-character path would
                // address a different directory than the one just created.
                return p.to_str().map(str::to_owned).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("temp path is not valid UTF-8: {}", p.display()),
                    )
                });
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(std::io::Error::other("could not create a unique temp path"))
}
