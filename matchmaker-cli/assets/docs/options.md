# Command Line Options

Matchmaker allows you to override any configuration setting directly from the command line. Overrides are specified as key-value pairs following the standard arguments.

## Syntax

Overrides follow the pattern `path=value` or `path value`.

- **Hierarchical Paths**: Use dot notation to navigate the configuration structure (e.g., `results.style.fg`).
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
  - `start.input_separator` -> `n`
  - `start.output_template` -> `o`
  - `start.command` -> `x`
  - `start.command` -> `cmd`
  - `matcher.ansi` -> `a`
  - `matcher.sort_threshold` -> `t`
  - `columns.split` -> `d`
  - `preview.layout` -> `P`
  - `header.content` -> `h`
  - `matcher.sort` -> `S`
  - `query.initial` -> `Q`

For example to split input on space, use `mm d " "`.

# Miscellaneous

### Presets and Named Overrides (`-o` / `--override`)

The `-o` flag allows you to layer additional configuration files on top of your base config. This is allows for consistent keybindings and settings between many different workflows.

- **Relative Paths**: `-o` accepts an absolute path, but if you provide a relative path without an extension, Matchmaker will automatically look for a matching `.toml` file in the `presets` directory of your matchmaker configuration directory.
- **Example**: `mm -o git/status` will attempt to load `presets/git/status.toml` from the installation directory.
- **Source Field**: Overrides support a `source` field at the top level, allowing them to inherit from another preset (one level of recursion is supported).

### Listing Presets (`--presets`)

`mm --presets` recursively lists the full paths of installed `.toml` presets under the preset directory. Files named `base.toml` are omitted because they are inheritance parents rather than selectable presets.

### Values

If a "leaf" value contains multiple settings (like a [border](#border-settings) or a bind with multiple actions), you can specify them within a single string joined by `,` (which can be escaped by doubling: `,,`).

A few illustrative (but not very practical) examples:

```bash
# Example:
# If you started with one preview layout, the following overrides the first preview layout to just display hi and have a minimum width of 3, and adds two new ones. It also sets 3 binds.
mm p.l command=ls p.l "x=echo hi,min=3" b "ctrl-c=Quit,?=preview(echo hi)" b.ctrl-a cancel

# Example:
# Setting the column splitting delimiter
mm m.c.split "\w+,/\w+" # Sets the field: columns.split = Split::Regexes([Regex('\w'), Regex('/\w+')])

# Note that the same effect is NOT achieved by specifying mm m.c.split "\w+" m.c.split "/\w+" in this case:
# both declare a single (delimiter) regex, and the second command overwrites the first.
```

Note however, that when declaring a bind, you should prefer to use `mm b.ctrl-x "ExecuteSilent(rm {+}),Reload"` over `mm b "ctrl-x=ExecuteSilent(rm {+})"`, since as you can see, the second format doesn't support chained actions, while the first does.

Bool values can be specified with true, false, or "".

```bash
# Example:
# Enable result wrapping and scroll wrapping
mm p.w= r.r=
```

### Collections (Lists/Vectors)

Two of the collections: `preview.layout` and `columns.names`, are consumed additively:

1. **Adding Elements**: Each time a collection path is specified, a new partial element is added to that collection.
2. **Merging**: When the configuration is finalized:
   - The first $N$ overrides for a collection are merged into the first $N$ elements of the base configuration (from your config file). (Or in the case of of binds, existing keys are overridden).
   - Any additional overrides are appended as new elements.

### Colors and Modifiers

All colors and modifiers come from ratatui:

- <https://ratatui.rs/examples/style/colors/>
- <https://docs.rs/ratatui/latest/ratatui/style/struct.Modifier.html>

## Available Options

### Start (`start.`, `s`)

- `command`: (string or object) The command that generates items.
  - Absolute alias: `x`, `cmd`.
  - The command may be a shell command, an `@file` referenced relative to the
    preset directory (e.g. `"@list_sessions.sh"`), or a Lua payload
    (`#!lua …` inline or `@file.lua`) that pushes rows through the `inject`
    global (see `docs --lua`).
  - If an object:
    - `command`: (string) The shell command.
    - `separator`: (char) Input separator (overrides `start.input_separator` for this command).
- `is`, `input_separator`: (char) Character separating input items.
  - Absolute alias: `n`.
- `os`, `output_separator`: (string) String separating output selections.
- `ot`, `output_template`, `output`: (string) Template string used to print results.
  - Absolute alias: `o`.
- `on_accept`: (String) Template to execute on accepted items. Exclusive with output_template, output_separator.
- `sync`: (bool) Whether to wait for the command to finish before starting.
- `ax`, `additional_commands`: ([String]) Additional commands that can be cycled through using the ReloadNext action.
- `mode`: (string) The initial mode of the application. Default values (`0,1`, `0`, `1`, or `""`) depend on whether stdin and stdout are connected to /dev/tty.
- `d`, `directory`: (string) Change directory context.
  - `~` is resolved to home directory.
  - If an object:
    - `value`: (string) The directory path or command resolving to the directory path.
    - `exec`: (bool) If true, the directory is read from the stdout of the executed value.
    - `force`: (bool) If true, startup aborts when an executed directory lookup or directory change fails.
- `save_orphans`: (bool) Don't kill the last populating command when reloading.
- `skip_invalid_lines`: (bool) If false, aborts program when encountering an invalid utf-8 input line.
- `shell`: ([string]) Interpreter arguments used to execute start/action scripts, for example `["bash", "-c"]`. An empty list uses `$SHELL` (or `/bin/sh`).

### Exit (`exit.`, `e`)

- `first`: (bool) Exit automatically if there is only one match.
- `allow_empty`: (bool) Allow returning without any items selected.
- `abort_empty`: (bool) Abort if no items are provided.
- `last_key_path`: (path, optional) File in which to write the last processed key. Set to an empty path to disable it.

### Matcher (`matcher.`, `m`)

- `normalize`: (bool) Enable/disable normalization of characters (e.g., matching 'e' with 'é').
- `ignore_case`: (bool) Enable/disable case-insensitive matching.
- `prefer_prefix`: (bool) Prioritize matches that start with the query.
- `match_paths`: (bool) Enable path-aware matching.
- `trim`: (bool) Trim whitespace from input lines.
  - Absolute alias: `t`.
- `ansi`: (bool) Parse ansi codes from input.
  - Absolute alias: `a`.
- `sanitize`: (bool) Sanitize the input text/string from text_preprocessor.
- `require_column`: (integer, optional) Skip input lines whose specified column is empty.
- `raw`: (bool) Enable raw mode where non-matching items are also displayed in a dimmed color. (unimplemented)
- `track`: (bool) Track the current selection when the result list is updated. (unimplemented)
- `sort.reverse`: (bool) Reverse the order of the input.
- `sort.mode`: (string) Sort mode: `None` (default), `Lexicographic`, or `Numeric`.
- `sort.column`: (string) Name of the column to sort by; empty uses the primary column.
- `sort.threshold`: (number) Similarity threshold within which item order is preserved (0 to always sort).

### Columns (`columns.`, `c`)

- `s`, `split`: Defines how the input line is divided into columns. This can be `None`, a single `Delimiter` regex, a list of `Regexes`, or a CSV/TSV token.
  - **No Splitting** (`null`): The entire line is treated as a single column.
  - **Single Regex** (`"regex"`):
    - **No Capture Groups**: The regex is treated as a delimiter. Columns are the segments *between* matches.
    - **Unnamed Capture Groups**: If the regex contains capture groups (e.g., `(\d+) (\w+)`), each group's match becomes a column in order.
    - **Named Capture Groups**: If the regex contains named groups (e.g., `(?P<size>\d+) (?P<name>\w+)`), matches are mapped to columns with matching names defined in `columns.names`.
  - **Multiple Regexes** (`"[re1] [re2].."`): Each regex is searched independently; the match becoming the corresponding column.
  - **CSV/TSV** (`"csv"` / `"tsv"`): Splits comma/tab separated values following RFC 4180 rules
- `names`, `n`: List of column names/settings. A name may also be given as a bare string.
  - `name`: (string) Name of the column. Must be alphanumeric.
  - `ignore`: (bool) Ignore the column for matching.
  - `hidden`: (bool) Hide the column in the results table.
  - `options`: Column matching/display options, such as `Optional` or `OrUseDefault`.
- `max_columns`, `max`: (number) Maximum number of autogenerated columns, bounded to 1–16.
- `default`, `i`: (string or integer) The default column by name or zero-based index (default: first column).

### UI & Rendering

#### Global UI (`ui.`)

- `tick_rate`: (number) Refresh rate of the UI (default 60).
- `mouse_events`: (bool) Toggle mouse events (default true).
- `mouse_scroll_debounce_ms`: (number) Debounce mouse scroll events in milliseconds. Set to 0 to disable debouncing.
- `border`: [Border Settings](#border-settings).

#### Query Bar (`query.`, `q`)

- `prompt`: (string) The prompt prefix (default "> ").
- `initial`: (string) Initial text in the input bar.
- `style`: [Style Settings](#style-settings) for the input text.
- `prompt_style`: [Style Settings](#style-settings) for the prompt.
- `scroll_padding`: (bool) Maintain padding when moving the cursor in the query bar.
- `cursor`: Cursor style.
- `border`: [Border Settings](#border-settings).
- `reset_cursor_on_query_change`: (bool) Reset cursor to initial position on query change (experimental).

#### Results Table (`results.`, `r`)

- `multi_prefix`: (string) Prefix for multi-selected items.
- `default_prefix`: (string) Prefix for normal items.
- `current_prefix`: (string) Prefix for the current item.
- `multi`: (bool) Enable multiple selections.
- `style`: [Style Settings](#style-settings) (default).
- `inactive_style`, `inactive`: [Style Settings](#style-settings) for inactive columns.
- `inactive_current_style`, `inactive_current`: [Style Settings](#style-settings) for the current item in inactive columns.
- `match_style`, `match`: [Style Settings](#style-settings) for matching characters.
- `current_style`, `current`: [Style Settings](#style-settings) for the highlighted item.
- `prefix_style`, `prefix`: [Style Settings](#style-settings) for the prefix of the active column.
- `inactive_prefix_style`, `inactive_prefix`: [Style Settings](#style-settings) for the prefix of inactive columns.
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls how current item styles apply across the row.
- `scroll_wrap`, `cycle`: (bool) Wrap selection when reaching the end of the list.
- `scroll_padding`, `sp`: (number) Number of items to keep visible above/below the selection.
- `r`, `reverse`: (bool or null) Whether to reverse the list order; null leaves the default behavior.
- `w`, `wrap`: (bool) Enable line wrapping for result items.
- `width_overrides`: ([number]) Initial column width overrides.
- `min_width`: (number) Minimum column width.
- `min_width_from_cols`: (bool) Derive the minimum width from the configured columns.
- `column_spacing`: (number) Spacing between columns.
- `right_align_last`: (bool) Right-align the last column.
- `acp`, `active_column_min_percentage_hint`: (number, optional) Minimum percentage hint for the active column.
- `max_height`: (number) Maximum row height. VScroll/Preview can still show the full result; 0 disables the limit.
- `show_skipped`: (bool) Show rows skipped by the result-height limit.
- `vscroll_current_only`: (bool) Restrict vertical scrolling to the current row.
- `uniformly_truncate_columns`: (bool) Truncate row columns uniformly instead of truncating each column independently.
- `v`, `vertical`, `stacked_columns`: (bool) Display columns stacked vertically instead of across.
- `separator`, `hr`: (`none`, `empty`, `light`, `normal`, `heavy`, `dashed`) Separator between rows.
- `separator_style`: [Style Settings](#style-settings) for the row separator.
- `autoscroll`, `a`: Control how the results table scrolls horizontally to keep matches in view.
  - `enabled`: (bool) Enable/disable horizontal autoscroll.
  - `initial_preserved`: (number) Number of characters at the start of the line to always keep visible.
  - `context`: (number) Number of characters to show around the match.
  - `end`: (bool) Whether to autoscroll to the end of the line.
  - `always`: (bool) Enable autoscroll even when wrapping is enabled.
- `rct`, `resize_col_thresholds`: ([number, number]) Pixel deltas required before a column's preferred width is recomputed.
- `border`: [Border Settings](#border-settings).

#### Status Line (`status.`)

- `style`: [Style Settings](#style-settings).
- `show`: (bool) Show/hide the status line.
- `match_indent`: (bool) Indent the status to match the results table.
- `template`: (string) The following replacements are available:
  - `\r` -> current result index
  - `\m` -> match count
  - `\t` -> total item count
  - `\s` -> available whitespace / count appearances
  - `\S` -> increment the count denominator without displaying whitespace
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls the width used for whitespace expansion.
- `interactions`: ([index, action]) Define interactive regions. See [Interactions](template.md#interaction-regions).

#### Preview Panel (`preview.`, `p`)

- `show`: (bool or number) Toggle the preview window, or show it only when the relevant available dimension exceeds the given number.
- `scroll_wrap`: (bool) Enable scroll wrapping in preview.
- `wrap`: (bool) Enable line wrapping in preview.
- `layout`: List of preview settings. This path overrides the existing preview layouts in order.
  - Absolute alias: `l`.
  - `x`, `command`: Command to run for preview. `{}` is replaced by the item.
    - Absolute alias: `px`.
  - `layout` *(flattened)*:
    - `side`: `top`, `bottom`, `left`, `right`.
    - `percentage`: Percentage of the screen to occupy.
    - `min`, `max`: Pixel constraints for the preview size. Setting `max` to 0 disables a preview layout.
    - `scroll` *(flattened)*: Initial scroll settings for this layout. See the initial scroll settings below.
    - `border`: Optional per-layout [Border Settings](#border-settings).
- `border`: [Border Settings](#border-settings).
- `initial`, `i`: Control the initial scroll offset of the preview window.
  - `index` (string or integer, optional) – Extract the initial display index `n` of the preview window from this column. `n` lines are skipped after the header lines are consumed.
  - `o`, `offset` (integer) – Adjust the initial scroll index relative to `index`.
  - `p`, `percentage` (0-100) – How far from the bottom of the preview window the scroll offset should appear.
  - `h`, `header_lines` (number) – Keep the top N lines as a fixed header so that they are always visible.
  - `t`, `tail` (bool) – Start with the scroll at the bottom of the preview window.
- `initial_layout`: (number) Index of the initially selected preview layout.
- `trim_ends`: (bool) Trim leading and trailing newlines from preview output.
- `reevaluate_show_on_resize`: (bool) Re-evaluate the `show` condition after a resize.
- `drag_width`: (integer, optional) Width of the divider strip used for mouse resizing. `0` disables drag resizing; omitted uses the preview border width.

### Previewer (`previewer.`)

- `try_lossy`: (bool) Enable lossy UTF-8 conversion for preview command output.
- `delay_clear`: (bool) If true, prevents clearing the preview window until the new command starts producing output (default true).
- `debounce_ms`: (number) Debounce delay for preview commands in milliseconds (default 50).
- `max_procs`: (number) Maximum number of concurrent preview processes (default 4).
- `always_trigger`: (bool) If false, skips running the preview command if it is the same as the last one executed (default true).
- `shell`: (list of strings) The shell used to execute preview commands (e.g., `["sh", "-c"]`).
- `trim_commands`: (bool) Trim whitespace from preview commands.
- `help`: Help display settings.
  - `colors`: Optional help colors with `section`, `key`, and `value` color fields.
  - `seq_brackets`: Optional pair of characters used to display key sequences.
  - `hide_semantic`: (bool) Hide semantic triggers in help (default true).
  - `quote_traces`: (bool) Quote trace messages in help (default true).
  - `max_item_len`: (number) Maximum length of a help item before ellipsizing (default 50).
  - `ellipsize_center`: (bool) Ellipsize the center of help items (default false).
  - `event_trigger_prefix`: (string) Prefix for event triggers (default "#").
  - `show_events`: (bool) Show event triggers in help (default false).
  - `sort_fn_last`: (bool) Put function key (F1, F2, etc.) bindings at the end of the list (default true).
  - `combine_keys`: (bool) Combine multiple keys triggering the same action into a comma-separated list (default true).
- `cache`: (number) Reserved for future use.

### Pager (`pager.`)

Configures the pager used by the `ShowPreview` action (pages the current
preview command fullscreen and always resumes the picker afterwards). Only
present when the `pager` feature is enabled; without it `ShowPreview` falls
back to the external pager chain `MM_PAGER` → `$PAGER` → `less` → `more`.

- `line_numbers`, `ln`: (bool) Show line numbers in the pager (default false).
- `follow`: (bool) Start the pager in follow mode, auto-scrolling as new
  output arrives (default false).
- `prompt`: (string, optional) Footer prompt text shown by the pager.
- `horizontal_scroll`: (bool) Always enable horizontal scrolling
  (default false; `Ctrl+h` still toggles it).

### Header & Footer (`header.`, `footer.`, `h`, `f`)

- `content`: (string or list) Static content to display.
  - Absolute alias: `h`.
- `style`: [Style Settings](#style-settings).
- `match_indent`: (bool) Indent content to match the results table.
- `wrap`: (bool) Enable line wrapping.
- `row_connection`: Controls the effective width of the displayed content. See [Results Table](#results-table-results-r) for variants.
- `h`, `header_lines`: (number, header only) Number of lines to read from input for the header.
- `interactions`: ([index, action]) Define interactive regions per line. See [Interactions](template.md#interaction-regions).
- `border`: [Border Settings](#border-settings).

### TUI Settings (`tui.`)

- `stream`: (`Stdout` or `BufferedStderr`) Output stream selection consumed by the client.
- `restore_fullscreen`: (bool) Restore fullscreen on exit.
- `extended_keys`: (bool) Enable enhanced keyboard support.
- `sleep_ms`: (number) Delay in milliseconds before resizing.
- `clear_on_exit`: (bool) Clear the TUI screen after selection.
- `clear_after_execute`: (bool) Clear after execute actions. This setting is currently unimplemented.
- `layout` *(flattened)*: Constraints for non-fullscreen mode.
  - `percentage`: Height of the terminal used.
  - `min`, `max`: Pixel constraints.
- `osc52`: (bool). Execute the `Copy` action using the OSC52 protocol. If false, the `Copy` command pipes to `CLIPcmd` from `envs` (default: true).
- `copy_trailing_newline`: (bool) Preserve a trailing newline in copied command output (default: false).

### Style Settings

Several UI components have a `style` block (or similar, like `prompt_style`):

- `fg`: (color) Foreground color.
- `bg`: (color) Background color.
- `modifier`: (modifier) Style modifier (e.g., `BOLD`, `ITALIC`, `DIM`, joined by `|`).

### Border Settings

Most UI components have a `border` block:

- `type`: See <https://docs.rs/matetui/latest/matetui/ratatui/widgets/enum.BorderType.html>.
- `color`: CSS-style colors or named colors (e.g., `blue`, `red`, `#ff0000`).
- `bg`: Background color of the bordered area.
- `sides`: Which sides to show (e.g., `TOP | BOTTOM | LEFT | RIGHT`). Empty string for none.
- `padding`: Padding inside the border. Can be 1 value (all), 2 (vertical, horizontal), or 4 (top, right, bottom, left).
- `title`: Optional text to display on the border.
- `title_modifier`: Style modifier for the title.
- `modifier`: Style modifier applied to the bordered area.

### Key Binds (`binds.`, `b`)

See `mm --doc binds`.
