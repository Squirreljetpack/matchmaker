//! Portable built-in Lua modules for host applications embedding matchmaker.
//!
//! [`register_modules`] installs the shared module set (`path`, `util`, `proc`,
//! `fs`, `lunacolors`) on a Lua VM as globals and in `package.preload`. Host
//! applications append their own host-specific functions — clipboard access,
//! trash, editor launching, MIME detection — to the registered tables, so
//! `require(...)` sees them too.

pub mod fs;
pub mod input;
pub mod lunacolors;
pub mod path;
pub mod proc;
pub mod util;

use mlua::{Lua, Table};

/// Register a module both as a global and in `package.preload` for
/// `require(...)`.
///
/// The preload loader returns a clone of the same table, so methods added to
/// the global later are visible through `require(...)`.
fn install_module(lua: &Lua, name: &str, module: Table) -> mlua::Result<()> {
    lua.globals().set(name, module.clone())?;
    let package: Table = lua.globals().get("package")?;
    let preload: Table = package.get("preload")?;
    preload.set(name, lua.create_function(move |_, ()| Ok(module.clone()))?)?;
    Ok(())
}

/// Register the bundled `lunacolors` library as global `lunacolors` (and
/// `Lunacolors`) and in `package.preload`.
pub fn register_lunacolors(lua: &Lua) -> mlua::Result<()> {
    let module = lunacolors::create_module(lua)?;
    install_module(lua, "lunacolors", module.clone())?;
    install_module(lua, "Lunacolors", module)
}

/// Register the bundled `path`, `util`, `proc`, `fs`, and `lunacolors` modules
/// both as globals and in `package.preload` for `require(...)`.
///
/// `input` is only registered when `is_interactive` is true: it reads from and
/// draws on the controlling terminal, and mutates the screen through
/// matchmaker's picker, which background and queued runs must not do.
pub fn register_modules(lua: &Lua, is_interactive: bool) -> mlua::Result<()> {
    install_module(lua, "path", path::create_module(lua)?)?;
    install_module(lua, "util", util::create_module(lua)?)?;
    install_module(lua, "proc", proc::create_module(lua)?)?;
    install_module(lua, "fs", fs::create_module(lua)?)?;
    register_lunacolors(lua)?;

    if is_interactive {
        install_module(lua, "input", input::create_module(lua)?)?;
    }

    Ok(())
}
