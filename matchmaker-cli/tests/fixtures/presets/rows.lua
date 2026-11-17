-- Populating command for lua_file.toml: rows are `id<TAB>name`.
for _, row in ipairs({ "1\talpha", "2\tbeta" }) do
  inject(row)
end
