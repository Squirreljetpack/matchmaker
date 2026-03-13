# Command Line Options

Matchmaker allows you to override any configuration setting directly from the command line. Overrides are specified as key-value pairs following the standard arguments.

## Syntax

Overrides follow the pattern `path=value` or `path value`.

- **Hierarchical Paths**: Use dot notation to navigate the configuration structure (e.g., `results.fg`).
- **Flattened Fields**: Several major configuration blocks are "flattened," meaning their children can be accessed as top-level keys.
- **Shortcuts**: Many common fields have short aliases:
  - `binds` -> `b`
  - `start` -> `s`
  - `header.header_lines` -> `h.h`
  - `results.reverse` -> `r.r`
  - `results.wrap` -> `r.w`
  - `preview.layout` -> `p.l`
  - `preview.initial` -> `p.i`

- **Absolute Aliases**: The following common paths can be accessed directly:
  - `preview.layout.command` -> `px`
  - `start.input_separator` -> `i`
  - `start.output_template` -> `o`
  - `start.command` -> `x`
  - `start.command` -> `cmd`
  - `start.command` -> `command`
  - `start.ansi` -> `a`
  - `start.trim` -> `t`
  - `columns.split` -> `d`
  - `preview.layout` -> `P`
  - `header.content` -> `h`

### Collections (Lists/Vectors)

Fields that are collections (like `preview.layout` or `binds`) are treated as **additive leaf nodes**.

1. **Adding Elements**: Each time a collection path is specified, a new partial element is added to that collection.
2. **Merging**: When the configuration is finalized:
   - The first $N$ overrides for a collection are merged into the first $N$ elements of the base configuration (from your config file). (Or in the case of of binds, existing keys are overridden).
   - Any additional overrides are appended as new elements.

### Leaf values

If a "leaf" value contains multiple settings (like a [border](#border-settings) or a bind with multiple actions), you can specify them within a single string using `key=value` pairs or nested dot notation.

A few contrived examples:

```bash
# Example:
# If you started with one preview layout, the following overrides the first preview layout, and adds two new ones. It also sets 3 binds.
mm p.l command=ls p.l "x=bye min=3" b "ctrl-c=Quit ?=preview(echo hi)" b.ctrl-a cancel

# Example:
# Setting the column splitting delimiter
mm m.c.split "\w+ /\w+" # Sets the field: columns.split = Split::Regexes([Regex('\w'), Regex('/\w+')])
# or even shorter, using the absolute alias
mm d "[ ]" # split on space

# Example:
# Enable wrapping
mm p.w= r.r= # bool values are specified with true, false, or "" (true).
```

Individual values within the word specifying the leaf are split by whitespace.

- Whitespace splitting is disabled by a single level of parenthesis `()`, or brackets `{}`, and continues until the opening token is matched.
- The opening bracket `{}` is not included in the final output, while `()` is.
- Braces within can be escaped from contributing to the nesting level with `\`.

For example, `(( )) [one word] [\{}` splits into `(( ))`, `one word`, `[`.

### Beware!

1. All values are split following the above rule. If you are setting a single value with whitespaces, make sure to encapsulate it with `{..}`!

```shell
ls -l | mm d "[ +]" h.h 1 px "echo 'Metadata: {=..3}'" # Set the delimiter, header_lines, and preview command
```

2. When declaring a bind, it's recommended to use `mm b.ctrl-x "ExecuteSilent(rm {+}) Reload"` over `mm b "ctrl-x=ExecuteSilent(rm {+})"`, since the second format doesn't support chained actions.

## Colors and Modifiers

All colors and modifiers come from ratatui:

- https://ratatui.rs/examples/style/colors/
- https://docs.rs/ratatui/latest/ratatui/style/struct.Modifier.html

## Available Options

### Start (`start.`, `s`)

- `command`: (string) The shell command used to generate items.
  - Absolute alias: `x`.
- `input_separator`: (char) Character separating input items.
  - Absolute alias: `i`.
- `os`, `output_separator`: (string) String separating output selections.
- `output_template`: (string) Template string used to print results.
  - Absolute alias: `o`.
- `sync`: (bool) Whether to wait for the command to finish before starting.
- `trim`: (bool) Trim whitespace from input lines.
  - Absolute alias: `t`.
- `ansi`: (bool) Parse ansi codes from input.
  - Absolute alias: `a`.
- `ax`, `additional_commands`: ([String]) Additional commands that can be cycled through using the ReloadNext action
- `ansi`: (bool) Parse ansi codes from input.
  - Absolute alias: `a`.

### Exit (`exit.`, `e`)

- `select_1`: (bool) Exit automatically if there is only one match.
- `allow_empty`: (bool) Allow returning without any items selected.
- `abort_empty`: (bool) Abort if no items are provided.

### Matcher (`matcher.`, `m`)

- `normalize`: (bool) Enable/disable normalization of characters (e.g., matching 'e' with 'é').
- `ignore_case`: (bool) Enable/disable case-insensitive matching.
- `prefer_prefix`: (bool) Prioritize matches that start with the query.

#### Worker _(flattened)_

- `sort_threshold`: (number) Number of items above which sorting is disabled for performance.
- `raw`: Enable raw mode where non-matching items are also displayed in a dimmed color. (unimplemented)
- `track`: Track the current selection when the result list is updated. (unimplemented)
- `reverse`: Reverse the order of the input (unimplemented)

### Columns (`columns.`, `c`)

- `s`, `split`: Defines how the input line is divided into columns. This can be `None`, a single `Delimiter` regex, or a list of `Regexes`.
  - **No Splitting** (`null`): The entire line is treated as a single column.
  - **Delimiter Regex** (`"regex"`):
    - **No Capture Groups**: The regex is treated as a delimiter. Columns are the segments _between_ matches.
    - **Unnamed Capture Groups**: If the regex contains capture groups (e.g., `(\d+) (\w+)`), each group's match becomes a column in order.
    - **Named Capture Groups**: If the regex contains named groups (e.g., `(?P<size>\d+) (?P<name>\w+)`), matches are mapped to columns with matching names defined in `columns.names`.
  - **Multiple Regexes** (`"[re1] [re2].."`): Each regex is searched independently; the match becoming the corresponding column.
- `names`, `n`: List of column names/settings.
  - `name`: (string) Name of the column.
    - Must be alphanumeric.
- `max_columns`: (number) Maximum number of autogenerated columns (1-indexed).
- `default_column`: (string) The name of the default column (default: first column).

### UI & Rendering

#### Global UI (`ui.`)

- `tick_rate`: (number) Refresh rate of the UI (default 60).
- `border`: [Border Settings](#border-settings).

#### Input Bar (`input.`, `i`)

- `prompt`: (string) The prompt prefix (default "> ").
- `initial`: (string) Initial text in the input bar.
- `fg`: (color) Foreground color of the input text.
- `modifier`: (modifier) Style modifier for the input text.
- `prompt_fg`: (color) Foreground color of the prompt.
- `prompt_modifier`: (modifier) Style modifier for the prompt.
- `cursor`: Cursor style.
- `border`: [Border Settings](#border-settings).

#### Results Table (`results.`, `r`)

- `multi_prefix`: (string) Prefix for multi-selected items.
- `default_prefix`: (string) Prefix for normal items.
- `current_prefix`: (string) Prefix for the currently highlighted item.
- `fg`: (color) Default foreground color.
- `modifier`: (modifier) Default style modifier.
- `match_fg`: (color) Foreground color for matching characters.
- `match_modifier`: (modifier) Style for matching characters.
- `current_fg`: (color) Foreground color of the highlighted item.
- `current_bg`: (color) Background color of the highlighted item.
- `current_modifier`: (modifier) Style of the highlighted item.
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls how current item styles apply across the row.
- `scroll_wrap`: (bool) Wrap selection when reaching the end of the list.
- `scroll_padding`: (number) Number of items to keep visible above/below the selection.
- `r`, `reverse`: (When) When to reverse the list order (`Always`, `Never`, `Auto`).
- `w`, `wrap`: (bool) Enable line wrapping for result items.
- `min_wrap_width`: (number) Minimum column width when wrapping.
- `column_spacing`: (number) Spacing between columns.
- `right_align_last`: (bool) Right-align the last column.
- `v`, `vertical`, `stacked_columns`: (bool) Display columns stacked vertically instead of across.
- `hr`, `horizontal_separator`: (none, empty, light, normal, heavy, dashed): Show a seperator between rows (Currently only limited to one column).
- `autoscroll`: Control how the results table scrolls horizontally to keep matches in view.
  - Alias: `a`.
  - `enabled`: (bool) Enable/disable horizontal autoscroll.
  - `initial_preserved`: (number) Number of characters at the start of the line to always keep visible.
  - `context`: (number) Number of characters to show around the match.
  - `end`: (bool) Whether to autoscroll to the end of the line.
- `right_align_last`: (bool) Right-align the last column.
- `border`: [Border Settings](#border-settings).

#### Status Line (`status.`)

- `fg`: (color) Color of the status line.
- `modifier`: (modifier) Style of the status line.
- `show`: (bool) Show/hide the status line.
- `template`: (string) The following replacements are available:
  - `\r` -> current index
  - `\c` -> current column
  - `\m` -> match count
  - `\t` -> total count
  - `\s` -> Available whitespace / #count
  - `\S` -> Increments the count denominator without displaying whitespace

#### Preview Panel (`preview.`, `p`)

- `show`: (bool) Toggle the preview window.
- `scroll_wrap`: (bool) Enable scroll wrapping in preview.
- `wrap`: (bool) Enable line wrapping in preview.
- `layout`: List of preview settings. This path overrides the existing preview layouts in order.
  - Absolute alias: `l`.
  - `x`, `command`: Command to run for preview. `{}` is replaced by the item.
    - Absolute alias: `px`.
  - `layout` _(flattened)_:
    - `side`: `top`, `bottom`, `left`, `right`.
    - `percentage`: Percentage of the screen to occupy.
    - `min`, `max`: Pixel constraints for the preview size.
- `border`: [Border Settings](#border-settings).
- `initial`: Control the initial scroll offset of the preview window.
  - Alias: `i`.
  - `index` (string, optional) – Extract the initial display index `n` of the preview window from this column. `n` lines are skipped after the header lines are consumed.
  - `o`, `offset` (integer) – Adjust the initial scroll index relative to `index`.
  - `p`, `percentage` (0-100) – How far from the bottom of the preview window the scroll offset should appear.
  - `h`, `header_lines` (number) – Keep the top N lines as a fixed header so that they are always visible.

#### Header & Footer (`header.`, `footer.`, `h`, `f`)

- `content`: (string or list) Static content to display.
  - Absolute alias: `h`.
- `fg`, `modifier`: Style of the text.
- `match_indent`: (bool) Indent content to match the results table.
- `wrap`: (bool) Enable line wrapping.

<!-- - `row_connection`: See Results Table.  -->

- `t`, `header_lines`: (number, header only) Number of lines to read from input for the header.
- `border`: [Border Settings](#border-settings).

### TUI Settings (`tui.`)

- `restore_fullscreen`: (bool) Restore fullscreen on exit.
- `redraw_on_resize`: (bool) Redraw the UI when the terminal is resized.
- `extended_keys`: (bool) Enable enhanced keyboard support.
- `sleep_ms`: (number) Delay in milliseconds before resizing.
- `clear_on_exit`: (bool) Clear the TUI screen after selection.
- `layout` _(flattened)_: Constraints for non-fullscreen mode.
  - `percentage`: Height of the terminal used.
  - `min`, `max`: Pixel constraints.

### Border Settings

Most UI components have a `border` block:

- `type`: See https://docs.rs/matetui/latest/matetui/ratatui/widgets/enum.BorderType.html.
- `color`: CSS-style colors or named colors (e.g., `blue`, `red`, `#ff0000`).
- `bg`: Background color of the bordered area.
- `sides`: Which sides to show (e.g., `TOP | BOTTOM | LEFT | RIGHT`). Empty string for none.
- `padding`: Padding inside the border. Can be 1 value (all), 2 (vertical, horizontal), or 4 (top, right, bottom, left).
- `title`: Optional text to display on the border.
- `title_modifier`: Style modifier for the title.

### Key Binds (`binds.`, `b`)

See [webpage](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/docs/binds.md) or `--doc binds`.
