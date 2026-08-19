# Lua Scripts

Action payloads can be Lua instead of shell commands. Lua runs on a fresh
VM per execution (safe stdlibs only), so concurrent executions — `ExecuteAsync`
tasks, detached `ExecuteSilent` threads — never share or serialize on VM state.

Lua is compiled in via the `mlua` cargo feature (on by default). With the
feature off, `#!lua` payloads are treated as a shell comment (a silent no-op)
and `@*.lua` argument files are executed directly.

## `#!lua` — inline scripts

A payload that starts with `#!lua` followed by whitespace (space, tab, newline)
runs the rest as inline Lua:

```toml
"@open" = '''ExecuteOrConfirm(#!lua local p = state.current and state.current[1]
if p then os.execute(string.format("%q -- %q", os.getenv("EDITOR") or "vi", p)) end)'''
```

The script's exit code follows the same quit/prompt policy as the shell
commands it replaces (`handle_exit`): a clean return is code `0`; call
`os.exit(code)` to exit with `code` without terminating matchmaker itself.
A Lua error is logged and the action is skipped.

## `@file.lua` — script arguments

`@` payloads ending in `.lua` run the file on the Lua engine:

```terraform
"@open" = "ExecuteOrConfirm(@open_file.lua --flag)"
```

The remaining words become the script's varargs (`...`), preserved exactly as
written (single quotes keep their content together). Relative paths resolve
against the parent of `MM_OVERRIDE`. Inline payloads receive **no** varargs —
the current item is reached through the `state` table instead.

## Which actions run Lua

| Payload marker    | Actions                                                                                 |
| ----------------- | --------------------------------------------------------------------------------------- |
| `#!lua` / `@.lua` | `Execute`, `ExecuteSilent`, `ExecuteAsync` — exit-code based                            |
| `#!lua` / `@.lua` | `Copy` / `CopyAsync` — the script's **return value** is the copied text                 |
| `#!lua` / `@.lua` | `Transform` / `TransformConfig` — the **return value** feeds the action / config parser |
| `#!lua` / `@.lua` | dynamic `[envs]` values and `[start.directory]` — the **return value** is the string    |
| shell only        | `start.command`, `preview.command`, `Become` (it replaces the process)                  |

Where an action returns a value (copy, transform, `[envs]`, `start.directory`),
the value is the script's **first return value**, converted to a string. Nothing
is captured from standard output. `nil` (or a bare `return`) means "no value",
which skips the action or leaves the variable unset.

## The `state` table

The global `state` table is a snapshot of the picker at execution time. It is
built from the same column sources as the `{1}`/`{2}`/`{name}` template keys, so
it needs no shell-quoting or string interpolation.

| Field                                | Meaning                                                                                                                                 |
| ------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------- |
| `query`                              | the current input                                                                                                                       |
| `mode`                               | the mode tags, comma-joined                                                                                                             |
| `raw`                                | the whole current item line (nil when nothing is selected)                                                                              |
| `current`                            | the current item's columns: an array `[1]…[n]`, also keyed by configured column names (nil when nothing is selected)                    |
| `selected`                           | every selected item as a column array (the current item when the selection is empty — mirrors `{+0}`)                                   |
| `position`                           | index of the current item (0-based, same as `env.MM_POS`)                                                                               |
| `total`, `matched`, `selected_count` | item counts                                                                                                                             |
| `active`                             | 1-based index of the active column (so `state.current[state.active]` is its value); falls back to the primary column when not filtering |
| `args`                               | the run's command args (`$0`/`$1` template keys), 1-indexed array                                                                       |

Column arrays are keyed by both position and name, so `state.current[1]` and
`state.current.path` address the same cell. With no `[columns]` configured, a
single default column covers the whole item and `[1]` is the whole line.

```lua
#!lua local parts = {}
for _, it in ipairs(state.selected) do
  local c = it[1]
  if c ~= nil then parts[#parts + 1] = c end
end
return table.concat(parts, "\n")
```

Before the picker is up — `[envs]` and `start.directory` — `state` holds only
`mode` and `args`; the item fields are absent.

## The `env` table

The matchmaker command environment (`MM_*`, `FZF_*`, and any `[envs]` you
configure — `MM_QUERY`, `MM_MODE`, `MM_OVERRIDE`, …) is exposed as the global
`env` table. Use `os.getenv("NAME")` for the surrounding process environment
(the same values `$NAME` would see in a shell payload), e.g. `os.getenv("EDITOR")`.

## Interpreter notes

- `os.exit(code)` stops the script with `code` — it does **not** terminate the
  host, unlike stock Lua 5.4.
- To run an external command with arguments, use Lua 5.4's `%q`, which quotes
  and escapes each argument; there is no matchmaker quoting helper:

  ```lua
  os.execute(string.format("%q -- %q", path, state.current[1]))
  ```

## Mode-gated `lua^^` variants

The CLI appends a `lua` tag to the mode when the `mlua` feature is enabled,
so a bind can select a Lua payload *only when Lua is active* by writing a
mode-filtered alias:

```toml
"lua^^@accept" = "Accept"
"lua^^@copy" = '''CopyAsync(#!lua local t = {}
for _, it in ipairs(state.selected) do t[#t + 1] = it[1] end
return table.concat(t, "\n"))'''
```

While the mode contains a `lua` tag, `lua^^@copy` wins over the plain `@copy`
bind; without it, the plain bind applies. The payload's own `#!lua`/`@.lua`
marker always decides how it executes — the mode tag only chooses which payload
a trigger resolves to.
