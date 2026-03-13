# Matchmaker Templating Rules

Matchmaker uses a template system for formatting output and executing commands.
Templates use `{}` placeholders with various modifiers to inject item data.
It's important note that only valid keys are replaced -- invalid keys are left alone. If you need it, you can also escape `{`.

## Modifiers

| Modifier | Description                                        |
| -------- | -------------------------------------------------- |
| `{}`     | Current item (shell-quoted)                        |
| `{=}`    | Current item (no quotes)                           |
| `{+}`    | All selected items (shell-quoted, space-separated) |
| `{-}`    | All selected items (no quotes, space-separated)    |

_Note: This outputs the original line (after possible trimming and ansi processing), and is not the same as {..} below._

## Column Specifics

You can specify a column by its name.

Note that the default column names (when `columns.names` is unspecified) are `1` … `columns.max`.

**Note: Column names must be alphanumeric.**

| Placeholder | Description                                       |
| ----------- | ------------------------------------------------- |
| `{col}`     | Column `col` of current item (shell-quoted)       |
| `{=col}`    | Column `col` of current item (raw)                |
| `{+col}`    | Column `col` of all selected items (shell-quoted) |
| `{-col}`    | Column `col` of all selected items (raw)          |

## Active Column

The active column is the one under the cursor in the input field (contolled via `%column_name` in the query).

## Ranges

Column ranges can be specified.

| Placeholder    | Description                                                |
| -------------- | ---------------------------------------------------------- |
| `{col1..col2}` | Columns from `col1` to `col2` (exclusive), space-separated |
| `{col..}`      | From column `col` to the end                               |
| `{..col}`      | From the first column to `col` (exclusive)                 |
| `{..}`         | All visible columns                                        |

Modifiers (`=`, `+`, `-`) also apply to ranges.

## Examples

### Configuration (`config.toml`)

#### Custom Preview Command

Use `{}` to pass the current item to a previewer like `bat` or `eza`.

```toml
[[preview.layout]]
    command = "bat --color=always {} || eza -T {}"
```

#### Keybindings with Shell Execution

You can bind keys to run shell commands.

```toml
[binds]
    "ctrl-o" = "Execute($EDITOR {})"     # Open in editor and return to matchmaker
    "alt-o"  = "Become($EDITOR {})"      # Open in editor and exit matchmaker
    "ctrl-y" = "Execute(echo -n {} | xclip -sel clip)" # Copy to clipboard
```

#### Output Formatting

Change how the selected item is printed to stdout when you press enter.

```toml
[start]
    output_template = "Selected: {}"
```

### Command Line (CLI)

#### Quick Output Wrap

Wrap the output in single quotes for use in shell scripts.

```bash
find . | mm o "'{}'"
```

#### Multi-Column Preview

If your input has columns (e.g., from `ls -l`), you can preview a specific column.

```bash
ls -l | mm d " +" m.max_columns=9 px "echo 'File: {=9}'" h.header_lines 1 m.default_column 9 h.content="|||"
```

_Note: `{=9}` uses the unquoted value of the 9th column (index 9)._

#### Active Column

The `{!}` placeholder refers to the column currently under focus (specified by `%column_name` in your query).

```bash
mm px "echo 'You are currently filtering on: {!}'"
```

#### Ranges

Join multiple columns together. `{2..}` joins the 2nd column to the end.

```bash
ls -l | mm d "[ +]" h.h 1 px "echo 'Metadata: {=2..}'"
```

_Note: `h.h` is short for `header.header_lines`._

#### Action on Selection

Execute a command with all selected items when pressing a custom key.

```bash
touch a b
mm b.ctrl-x "ExecuteSilent(rm {+}) Reload" x ls # Delete items then reload
```

## Status Line and Input Prompt Templates

The status line supports dynamic variables, and alignment.
With the `SetStyledStatus` action, it, along with the input prompt (by `SetStyledPrompt`), also supports styling.

### Styling

You can style parts of the status line and input prompt using the `{fg,bg,..modifiers:text}` syntax.

- `{red:Error:}` renders "Error:" in red.
- `{,,blue:Matchmaker}` renders "Matchmaker" in a blue background.
- `{red,bold,italic:Caution}` renders "Caution" in red, with bold and italic modifiers.

### Variables

The following variables are available and must be prefixed with a backslash (`\`):

| Variable | Description                                   |
| -------- | --------------------------------------------- |
| `\r`     | Current row index (0-indexed cursor position) |
| `\m`     | Number of matched items                       |
| `\t`     | Total number of items                         |

### Alignment

Use `\s` and `\S` to insert flexible whitespace for alignment.

- A single `\s` will expand to fill the remaining width of the terminal, pushing subsequent text to the right.
- If multiple `\s` are used, the available space is distributed equally between them.
- `\S` increases the distribution denominator without adding any whitespace.
