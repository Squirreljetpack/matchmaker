---
name: matchmaker-presets
description: Creates, reviews, debugs, and ports Matchmaker (mm) TOML presets. Use when designing a fuzzy-search workflow, adding previews or keybindings, adapting commands for another shell, or testing a preset with mm.
compatibility: Requires the Matchmaker CLI and a TOML preset; Python-based presets additionally require Python 3.9+ unless they deliberately target another interpreter.
---

# Matchmaker preset authoring

Use this skill to build a complete, portable Matchmaker preset rather than a single command-line override. A preset turns a stream of lines into a small workflow: it defines how input is produced, how rows are split and matched, what previews show, and what actions keys trigger.

## Source of truth

Before inventing configuration keys, consult the version of Matchmaker being used:

```sh
mm --doc options
mm --doc binds
mm --doc template
mm --dump-config
```

If the request is ambiguous, clarify the input source, accepted output, target platforms, and whether the preset may modify files or execute destructive commands before writing it. You may be able to list existing presets on the user's system with `mm --presets`.

## Preset model

A preset is layered onto Matchmaker's base configuration. It normally contains these sections in this order:

1. `source` (only when inheriting from another preset)
2. `[start]` and optional `[envs]`
3. `[columns]`, `[matcher]`, and UI sections such as `[query]`, `[results]`, `[header]`, and `[footer]`
4. `[previewer]` and `[preview]`
5. `[binds]`, with concrete key binds first and semantic aliases afterward

Use section comments to separate the input, UI, preview, and action portions. Keep the preset self-contained: a reader should be able to understand its input command, row format, preview behavior, and side effects without finding undocumented shell functions elsewhere. Shipped presets use both compact inline commands and sibling helper scripts; choose the simpler form that remains readable and testable.

### Input and output

- `start.command` produces the rows consumed by Matchmaker. Make its output deterministic and keep diagnostics off stdout; stdout is the data stream.
- Use `start.input_separator` when input items are separated by something other than newlines.
- Use `[envs]` for stable values shared by commands. An environment entry can also be command-backed with `value = ...`, `exec = true`, and `force = true` when deriving the value or requiring it to exist is intentional.
- Use `columns.split` (`'\t'`, `csv`, `tsv`, or a regex) when rows contain multiple fields. Give important fields names with `columns.names`; names must be alphanumeric.
- Set `start.trim`, `start.ansi`, and `start.skip_invalid_lines` deliberately rather than relying on defaults.
- Choose an explicit `start.output_template`, `start.output_separator`, or `start.on_accept` when the accepted value should differ from the displayed row.
- `start.command` and values in `[envs]` are **not** template-expanded. Templates belong in preview commands, bind actions, and output/accept hooks.
- Treat a preset's input command as a public interface. Quote paths, handle empty output, and return a useful non-zero status when the source cannot be read.

### Inheritance and overrides

`source` can inherit from another preset. Override only the fields that are intentionally different. Collections such as `preview.layout` are merged by position and then appended; binds override existing keys. Test an inherited preset with the same commands as a standalone preset so defaults do not hide a missing field.

## Templates

Matchmaker formats templates before executing preview and action commands. Formatted values are shell-quoted by Matchmaker unless the raw modifier is used.

| Template                                  | Meaning                                          |
| ----------------------------------------- | ------------------------------------------------ |
| `{}` or `{0}`                             | Current item / primary column, quoted            |
| `{=}` or `{=0}`                           | Current item / primary column, unquoted          |
| `{+}` or `{+0}`                           | All selected items, quoted and space-separated   |
| `{-}` or `{-0}`                           | All selected items, unquoted and space-separated |
| `{name}`, `{=name}`, `{+name}`, `{-name}` | Named-column forms                               |
| `{$0}`, `{$1}`                            | Trailing command-line arguments                  |
| `{2..}`, `{..name}`                       | Column ranges                                    |
| `{#}`, `{!}`                              | Current absolute index / active column           |

Prefer quoted forms when passing values as arguments. Use raw forms only when the receiving program explicitly expects a larger expression or a delimiter-free value. Never build a shell command by concatenating untrusted row data yourself when a Matchmaker template can do the quoting.

## Alternate interpreters (optional)

Most shipped presets use the user's shell and POSIX-compatible commands. A preset may select an explicit interpreter with `[start].shell` or `[previewer].shell`; when it does, every command governed by that setting must be written in that interpreter's language. Setting a Python shell while leaving shell conditionals, pipes, or variable assignments in the command is a syntax error.

When an alternate interpreter receives a Matchmaker template, remember that the inserted value is still shell-quoted. For Python, parse a multi-value placeholder with `shlex.split` rather than a plain `.split()`:

```python
import shlex
items = shlex.split(r"""{+1}""")
```

TOML literal strings (`'''...'''`) pass backslashes through unchanged. Write the source language's intended escapes once; do not double them merely because the source is inside a TOML literal. Keep interpreter-specific guidance short and document the required executable (`python`, `python3`, `pwsh`, etc.) when the preset is not using the default shell.

With the default shell, prefer portable POSIX commands when the preset targets macOS and Linux. Do not assume GNU-only flags (`stat -c`, `sed -i`, `grep -P`, `readlink -f`) on macOS. If Windows is a target, either use an explicit cross-platform interpreter or provide a Windows-compatible command rather than silently relying on `/bin/sh`.

## Previews

Use one or more `[[preview.layout]]` entries for distinct views. Keep each command focused:

- A full definition/details view should show the selected item's relevant fields and report missing installations clearly.
- A source or documentation view can use `bat` when available, with a plain `cat`/Python fallback.

Note that `[preview]` and `[previewer]` are different sections. Consult `mm --doc options`.

## Binds and actions

Use semantic aliases for workflows that have more than one trigger:

```toml
[binds]
"ctrl-r" = "@reload-source"
"@reload-source" = ["Reload", "Cancel"]
"?" = "SwitchPreview"
```

Guidelines:

- Prefer an action array for ordered operations such as `['ExecuteAsync(...)', 'Reload']`.
- Use `Execute` for a command that should return to Matchmaker, `Become` when the external program should replace Matchmaker, and `ExecuteOrConfirm` when a failure needs user confirmation.
- Use `ExecuteSilent`/`ExecuteAsync` only when detached or asynchronous behavior is intentional.
- Make destructive operations opt-in and obvious. Confirm the exact selected paths before using `rm`, overwriting files, or changing persistent configuration.
- Keep aliases composable. A key should trigger an alias when the same workflow may later be attached to another key, event, or interaction region.
- Use `mm --test-keys` instead of guessing terminal key names.

When a bind needs all selected rows, use `{+}`/`{+column}`. There is no environment variable containing the selection list; `MM_SELECT_COUNT` only provides the count. A common Python pattern is:

```python
import shlex
names = shlex.split(r"""{+1}""")
```

If a bind uses a second selected column for state, parse both lists and reject a mixed-state selection before changing anything. Preserve the original file's line endings, blank lines, and trailing newline when performing a targeted rewrite.

## Scripts next to a preset

For non-trivial logic, prefer a small script beside the preset over a giant inline command. Resolve sibling files from `MM_OVERRIDE`, not from the process working directory:

```python
import os
import subprocess
import sys
from pathlib import Path

preset = Path(os.environ["MM_OVERRIDE"])
script = preset.with_name("helper.py")
subprocess.run([sys.executable, str(script), "arg"], check=False)
```

Use the standard library when portability matters. Send data rows only to stdout; send progress, warnings, and diagnostics to stderr. Use atomic same-directory temporary files plus `os.replace`/`Path.replace` for updates so a failed write does not leave a truncated settings file. Validate JSON/TOML-derived input before replacing a user's configuration.

`MM_OVERRIDE` identifies the first applied override and is the reliable anchor for preset-local helpers. It may not be set when a helper is run directly, so provide a deliberate fallback or fail with an actionable message.

## Patterns in shipped presets

Use the repository's presets as style references instead of treating one workflow as canonical:

- `rg.toml` shows a focused source command, ANSI input, regex capture columns, a raw output template, and a query-driven reload flow.
- `git/status.toml` shows inheritance with `source = 'base.toml'`, NUL-delimited input, several preview layouts, semantic aliases, and confirmation around destructive actions.
- `docker/containers.toml` shows sibling helper scripts, command-backed environment values, named columns, and previews that reuse a dispatcher.
- `csv.toml` shows the smallest useful preset: discover input, select a parser, set a header, and preview the active field.
- `ai/pi_sessions.toml` shows a larger multi-column browser with generated metadata, several preview layouts, and helper programs resolved relative to `MM_OVERRIDE`.

Copy the pattern, not the incidental command names. Keep the preset's assumptions explicit and avoid adding UI or abstraction that the workflow does not need.

## A practical authoring workflow

1. **Define the contract.** Record the source command, row format, primary column, accepted output, side effects, target shells, and external dependencies.
2. **Build the smallest input preset.** Make `start.command` produce clean rows and verify it independently.
3. **Add columns and matching.** Configure `columns.split`, names, default column, sort behavior, and ANSI/trim handling.
4. **Add previews.** Start with one reliable layout, then add alternate layouts. Configure `[previewer].shell` if the layout interpreter differs from the default.
5. **Add safe actions.** Put reusable workflows behind semantic aliases and use templates for selected values.
6. **Add scripts only where needed.** Keep helpers focused, use `MM_OVERRIDE` for sibling paths, and separate stdout data from stderr diagnostics.
7. **Exercise non-interactively.** Use `mm --list` to run the start command, `mm --list='N-M'` to render a preview layout, and `mm --list='N@alias'` to exercise a bind alias. The exact `--list` forms are shown by `mm --help`.
8. **Exercise interactively.** Check empty input, one row, many rows, long rows, quotes, backslashes, Unicode, multiple selections, missing files, and command failures.
9. **Check portability and idempotency.** Test on every promised OS/interpreter, run file-rewriting actions twice, and confirm the second run makes no further changes.
10. **Document dependencies.** State required commands, environment variables, config locations, keybindings, and destructive behavior next to the preset or in its README.

Useful checks:

```sh
mm --help
mm --doc options
mm --doc binds
mm --doc template
mm -o path/to/preset.toml --list
mm -o path/to/preset.toml --list='0-0'
mm -o path/to/preset.toml --list='0@alias'
```

Use a temporary `HOME`, settings directory, and input file for tests that mutate state. Do not test a new reconciliation or settings-writing preset directly against a user's real configuration until the creation, existing-file, malformed-input, blank-line, and idempotency cases pass in a throwaway directory.

## Common failure modes

- **Rows are polluted with diagnostics:** a start command wrote status text to stdout. Move it to stderr.
- **A Python preset reports a syntax error:** shell code is still present, or a TOML literal string contains doubled backslashes.
- **Preview works in one layout but not another:** `[start].shell` was set but `[previewer].shell` was not.
- **A selected path is split incorrectly:** use `{+column}` plus `shlex.split`, not a plain `.split()`.
- **A template is printed literally:** it was placed in `start.command` or `[envs]`, where templates are not expanded.
- **The helper cannot be found:** use `MM_OVERRIDE` and `Path.with_name`, not `$PWD` or an assumed install directory.
- **An update destroys a file:** validate the complete new content and write atomically in the target file's directory.
- **A command works on Linux but not macOS:** it depends on GNU utilities, Bash-only syntax, or a non-portable `stat`/`sed`/`grep` flag.
- **A command works only in one environment:** document its required tools, shell, variables, working directory, and config files, or provide a fallback.
