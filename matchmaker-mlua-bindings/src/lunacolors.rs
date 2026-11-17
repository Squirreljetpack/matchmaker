//! Bundled `lunacolors` styling library.

use mlua::{Lua, Table};

/// Build the `lunacolors` module table by evaluating the bundled Lua source.
pub fn create_module(lua: &Lua) -> mlua::Result<Table> {
    lua.load(include_str!("lunacolors.lua")).eval()
}
