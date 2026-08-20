# Execution Models

Matchmaker supports multiple execution strategies for running commands, generating input rows, copying text, transforming state, and executing actions.

> [!IMPORTANT]
>
> Despite the breadth of the following section is, for the most part it only exists due to the need for matchmaker to act consistently with respect to lua support.
> You can almost always just use simple inline scripts (the *shell command* strategy), for which you should consult the [template docs](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/template.md) instead.

---

## Strategy Overview

Every command string (except preview layout commands) is classified into one of four execution strategies based on its prefix:

| Prefix / Shape        | Strategy            | Description                                                                                                                                                                                       |
| :-------------------- | :------------------ | :------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `command ...`         | **Shell Command**   | Run via the configured shell (`start.shell` or `$SHELL -c`). Supports `{}` [template expansions](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/template.md). |
| `@path [args...]`     | **Direct File**     | Directly invokes executable at `path` with `args`, bypassing shell interpretation. Relative paths resolve against the preset's directory.                                                         |
| `#!lua ...`           | **Inline Lua**      | Runs inline Lua script on the internal Lua runtime with access to global `state` and `env` tables.                                                                                                |
| `@file.lua [args...]` | **Lua Script File** | Runs `file.lua` on the internal Lua runtime, passing `args` as varargs (`...`). Relative paths resolve against the preset directory.                                                              |

> [!NOTE]
> Lua strategies require the `mlua` cargo feature (enabled by default). When disabled, `#!lua` falls back to Shell, and `@*.lua` falls back to Direct File.

> [!NOTE]
> Relative files resolve against the parent directory of `MM_OVERRIDE`. This is exactly the path of the first override passed with `-o`!

---

## Feature & Action Support Matrix

Different contexts in Matchmaker utilize command strategies in specific ways:

| Context / Action                                          | Shell | `@file` | `#!lua` | `@file.lua` | Output / Result Handling                                         |
| :-------------------------------------------------------- | :---: | :-----: | :-----: | :---------: | :--------------------------------------------------------------- |
| `Execute`, `ExecuteSilent`, `ExecuteAsync`, `ExecuteThen` |   ✓   |    ✓    |    ✓    |      ✓      | Exit code determining success / continuation                     |
| `ExecuteOrConfirm`, `ExecuteAndQuit`                      |   ✓   |    ✓    |    ✓    |      ✓      | Exit code determining prompt vs. quit policy                     |
| `Become`, `BecomeSilent`                                  |   ✓   |    ✓    |    —    |      —      | Replaces Matchmaker process (`exec`)                             |
| `BecomeOrConfirm`, `BecomeOrResume`                       |   ✓   |    ✓    |    —    |      —      | Replaces process with exit code recovery policy                  |
| `Copy`, `CopyAsync`                                       |   ✓   |    ✓    |    ✓    |      ✓      | Shell/File captures `stdout`; Lua uses script **return value**   |
| `Transform`, `TransformConfig`                            |   ✓   |    ✓    |    ✓    |      ✓      | Shell/File parses `stdout`; Lua parses script **return value**   |
| `start.command` & `Reload` / `ReloadNext` / `ReloadPrev`  |   ✓   |    ✓    |    ✓    |      ✓      | Shell/File reads piped `stdout`; Lua calls global `inject(line)` |
| `start.directory` (when `exec = true`)                    |   ✓   |    ✓    |    ✓    |      ✓      | Shell/File captures `stdout`; Lua uses script **return value**   |
| Dynamic `[envs]` (when `exec = true`)                     |   ✓   |    ✓    |    ✓    |      ✓      | Shell/File captures `stdout`; Lua uses script **return value**   |
| `--list` & `--list=<ARG>`                                 |   ✓   |    ✓    |    ✓    |      ✓      | Runs command directly outside TUI                                |
| **`preview.layout[].command`**                            | **✓** |  **—**  |  **—**  |    **—**    | **Strictly Shell execution with `{}` template expansion**        |

---

## Direct File Execution (`@`)

Commands prefixed with `@` execute a file directly without spawning an intermediary shell:

```toml
[binds]
"ctrl-o" = "Execute(@open_file.sh {file})"
"ctrl-y" = "Copy(@format_item.py '{name}' '{path}')"
```

- **Path Resolution**: Relative paths resolve against the directory of the first applied preset (`MM_OVERRIDE` parent). Absolute paths are used as-is.
- **Argument Preservation**: Arguments are split by whitespace. Single quotes (`'arg with spaces'`) preserve whitespace within arguments without shell interpolation.
- **Bypassing Shell Overhead**: Direct execution avoids shell startup time and shell escaping complexities.

---

## Lua Runtime (`mlua`)

Each Lua execution runs on a fresh, isolated Lua 5.4 VM. Concurrent actions (such as `ExecuteAsync` background tasks or `ExecuteSilent` threads) never share or block on VM state.

### Inline Scripts (`#!lua`)

A payload starting with `#!lua` followed by whitespace runs the rest of the string as Lua code:

```toml
[binds]
"@open" = '''ExecuteOrConfirm(#!lua local p = state.current and state.current[1]
if p then
  os.execute(string.format("%q -- %q", os.getenv("EDITOR") or "vi", p))
end)'''
```

### Script Files (`@file.lua`)

A `@` path ending in `.lua` executes the file within the Lua engine:

```toml
[binds]
"@process" = "ExecuteAsync(@process_item.lua --fast)"
```

Remaining words become varargs (`...`) passed to the script.

### Input Stream Ingestion (`inject`)

For `start.command` and `Reload`, Lua scripts do not write to standard output. Instead, rows are pushed into Matchmaker using the global `inject(line)` function:

```toml
[start]
command = '''#!lua
for _, item in ipairs({ "apple", "banana", "cherry" }) do
  inject(item)
end'''
```

Or via a sibling file:

```toml
[start]
command = "@list_items.lua"
```

### The `state` Table

The global `state` table provides an execution-time snapshot of the picker:

| Field                  | Description                                                          |
| :--------------------- | :------------------------------------------------------------------- |
| `state.query`          | Current user search query string                                     |
| `state.mode`           | Comma-joined active mode tags                                        |
| `state.raw`            | Complete raw line of the current item (`nil` if nothing selected)    |
| `state.current`        | Current item columns array `[1]...[n]`, also keyed by column names   |
| `state.selected`       | Array of selected items (each item is a column array; mirrors `{+}`) |
| `state.position`       | 0-based index of current cursor item                                 |
| `state.total`          | Total item count                                                     |
| `state.matched`        | Matched item count                                                   |
| `state.selected_count` | Selected item count                                                  |
| `state.active`         | 1-based index of the focused column                                  |
| `state.args`           | Trailing CLI arguments passed after `--` (1-indexed array)           |

```lua
#!lua
local selected_paths = {}
for _, item in ipairs(state.selected) do
  if item.path then table.insert(selected_paths, item.path) end
end
return table.concat(selected_paths, "\n")
```

### The `env` Table

Matchmaker exposes environment variables (`MM_*`, `FZF_*`, and custom `[envs]`) via the global `env` table:

```lua
local override_dir = env.MM_OVERRIDE and env.MM_OVERRIDE:match("(.*/)")
```

To access the parent process environment variables (such as `$EDITOR` or `$HOME`), use standard Lua `os.getenv("EDITOR")`.

### Mode-Gated Lua Binds

Because Matchmaker appends a `lua` mode tag when `mlua` is enabled, you can provide Lua implementations that gracefully fall back when compiled without Lua:

```toml
[binds]
"@copy" = "Copy(echo -n {+})"
"lua^^@copy" = '''Copy(#!lua local t = {}
for _, it in ipairs(state.selected) do t[#t + 1] = it[1] end
return table.concat(t, "\n"))'''
```

---

## Exit Policies & Lifecycle Handling

When executing commands synchronously within the TUI, Matchmaker applies distinct post-execution policies depending on the action used:

| Action             | Success (`code == 0`) | Non-Zero Exit                | Interrupted / Ctrl+C / Signal                |
| :----------------- | :-------------------- | :--------------------------- | :------------------------------------------- |
| `Execute`          | Returns to TUI        | Returns to TUI               | Returns to TUI                               |
| `ExecuteOrConfirm` | Returns to TUI        | Displays confirmation prompt | Returns to TUI (no prompt on signal)         |
| `ExecuteAndQuit`   | Quits Matchmaker      | Returns to TUI               | Returns to TUI                               |
| `Become`           | Replaces process      | Replaces process             | Replaces process                             |
| `BecomeOrConfirm`  | Quits Matchmaker      | Displays confirmation prompt | Resumes Matchmaker (on Ctrl+C or code `100`) |
| `BecomeOrResume`   | Quits Matchmaker      | Resumes Matchmaker           | Quits with error (on crash/abnormal exit)    |

> [!NOTE]
> Custom helper scripts invoked via `BecomeOrConfirm` can exit with code `100` to instruct Matchmaker to resume the interactive picker without relying on user interrupts.

