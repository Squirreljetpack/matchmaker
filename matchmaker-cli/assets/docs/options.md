# Command Line Options

Matchmaker allows you to override configuration settings directly from the command line. Overrides are specified as key-value pairs following the standard arguments.

## Syntax

Overrides follow the pattern `path=value` or `path value`.

- **Hierarchical Paths**: Use dot notation to navigate the configuration structure (e.g., `results.style.fg`).
- **Flattened Fields**: Several major configuration blocks are "flattened," meaning their children can be accessed as top-level keys.
- **Section Shortcuts**: Most sections have a single-letter alias:
  - `matcher` -> `m`, `columns` -> `c`, `exit` -> `e`
  - `query` -> `q`, `results` -> `r`, `preview` -> `p`
  - `header` -> `h`, `footer` -> `f`, `binds` -> `b`, `start` -> `s`
- **Field Shortcuts**: Common nested fields have short aliases:
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
  - `matcher.sort.threshold` -> `t`
  - `columns.split` -> `d`
  - `preview.layout` -> `P`
  - `header.content` -> `h`
  - `matcher.sort` -> `S`
  - `query.initial` -> `Q`

For example to split input on space, use `mm d " "`.

# Miscellaneous

### Presets and Named Overrides (`-o` / `--override`)

The `-o` flag allows you to layer additional configuration files on top of your base config. This is allows for consistent keybindings and settings between many different workflows.

- **Relative Paths**: `-o` accepts a path. A relative path without an extension is resolved inside the `presets` directory of your matchmaker configuration directory. It checks, in order: `<path>/main.toml`, `<path>/<os>.main.toml`, `<path>.toml`, then `<path>.<os>.toml`.
- **Example**: `mm -o git/status` loads `presets/git/status.toml`; `mm -o unicode` loads `presets/unicode/main.toml`.
- **Source Field**: An override may set a top-level `source` path to inherit another preset, resolved relative to the overriding file. Only one level is followed, and `source` is ignored in the main config and in nested overrides.
- **`MM_OVERRIDE`**: Each applied override exports `MM_OVERRIDE` with its canonical path (unless your config already sets it), so `@` scripts and `$MM_OVERRIDE` resolve relative to the preset.

### Values

If a "leaf" value contains multiple settings (like a [frame](#frame-settings) or a bind with multiple actions), you can specify them within a single string joined by `,` (which can be escaped by doubling: `,,`).

A few illustrative (but not very practical) examples:

```bash
# If you started with one preview layout, the following overrides the first preview layout to just display hi and have a minimum width of 3, and adds two new ones. It also sets 3 binds.
mm p.l command=ls p.l "x=echo hi,min=3" b "ctrl-c=Quit,?=preview(echo hi)" b.ctrl-a cancel

# Setting the column splitting delimiter
mm c.split "\w+,/\w+" # Sets the field: columns.split = Split::Regexes([Regex('\w'), Regex('/\w+')])

# Note that the same effect is NOT achieved by specifying mm c.split "\w+" c.split "/\w+" in this case:
# both declare a single (delimiter) regex, and the second command overwrites the first.
```

Note however, that when declaring a bind, you should prefer to use `mm b.ctrl-x "ExecuteSilent(rm {+}),Reload"` over `mm b "ctrl-x=ExecuteSilent(rm {+})"`, since as you can see, the second format doesn't support chained actions, while the first does.

Bool values can be specified with true, false, or "".

```bash
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

### Environment (`envs.`)

Environment variables injected into the picker and its child commands. An entry may be a string or an object:

- `value`: (string) The variable value. An empty value is skipped, leaving any existing process variable untouched.
- `exec`: (bool) Run `value` as a command (using the normal command strategies) and use its trimmed output as the value.
- `force`: (bool) Override a variable that already exists in the process environment. Without it, an existing process variable wins.

```toml
[envs]
EDITOR = "nvim"
GIT_PAGER = { value = "delta", force = true }
BRANCH = { value = "git branch --show-current", exec = true }
```

### Matcher (`matcher.`, `m`)

#### Preprocessing

- `ansi`: (bool, alias `a`) Parse ANSI codes from input.
- `trim`: (bool, alias `t`) Trim whitespace from input lines. The absolute alias `t` is reserved for `matcher.sort.threshold`, so use `m.t` for trimming.
- `sanitize`: (bool, alias `s`) Sanitize the input text from the text preprocessor.
- `require_column`: (integer, optional) Skip input lines whose specified column is empty.

#### Backend

- `backend`: `nucleo` (default) or `frizbee`.
- With the default `nucleo` backend:
  - `normalize`: (bool) Normalize characters (e.g. match 'e' with 'é').
  - `ignore_case`: (bool) Case-insensitive matching.
  - `prefer_prefix`: (bool) Prioritize matches that start with the query.
  - `match_paths`: (bool) Path-aware matching.
- With `backend = "frizbee"` (typo-tolerant matching):
  - `max_typos`: (integer, optional) Maximum edit distance.
  - Scoring weights (integers): `match_score`, `mismatch_penalty`, `gap_open_penalty`, `gap_extend_penalty`, `prefix_bonus`, `capitalization_bonus`, `matching_case_bonus`, `exact_match_bonus`, `delimiter_bonus`.

```toml
[matcher]
backend = "frizbee"
max_typos = 1
exact_match_bonus = 16
```

#### Matching and Sorting

- `raw`: (bool, alias `r`) Display non-matching items dimmed. (unimplemented)
- `track`: (bool) Track the current selection when the result list is updated. (unimplemented)
- `sort.reverse`: (bool) Reverse the order of the input.
- `sort.mode`: (string) Sort mode: `None` (default), `Lexicographic`, or `Numeric`.
- `sort.column`: (string) Name of the column to sort by; empty uses the primary column.
- `sort.threshold`: (number or bool) Similarity threshold within which item order is preserved: `0` always sorts; `true` is `0`; `false` uses the maximum threshold (keep input order). Absolute alias: `t`.

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
  - `options`: Matching options joined by `|`:
    - `Optional`: when patterns apply to several columns, failing this column does not reject the item (unless it is the only column with a pattern).
    - `OrUseDefault`: when no column-specific query is given, match this column using the primary column's query.
- `max_columns`, `max`: (number) Maximum number of autogenerated columns, bounded to 1–16.
- `default`, `i`: (string or integer) The default column by name or zero-based index (default: first column).

### UI & Rendering

#### Global UI (`ui.`)

- `tick_rate`: (number) Refresh rate of the UI (default 60).
- `mouse_events`: (bool) Toggle mouse events (default true).
- `mouse_scroll_debounce_ms`: (number) Debounce mouse scroll events in milliseconds. Set to 0 to disable debouncing.
- `frame`, `f`: [Frame Settings](#frame-settings).
- `outer_frame`, `of`: [Frame Settings](#frame-settings) drawn around the entire UI.

#### Query Bar (`query.`, `q`)

- `prompt`: (string) The prompt prefix (default "> ").
- `initial`, `i`: (string) Initial text in the input bar. Absolute alias: `Q`.
- `style`: [Style Settings](#style-settings) for the input text.
- `prompt_style`: [Style Settings](#style-settings) for the prompt.
- `scroll_padding`: (bool) Maintain padding when moving the cursor in the query bar.
- `word_boundaries`: ([char]) Additional characters that stop word movement (whitespace always does), e.g. `[".", "/"]`.
- `cursor`: Cursor style.
- `frame`, `f`: [Frame Settings](#frame-settings).
- `reset_cursor_on_query_change`: (bool) Reset cursor to initial position on query change, keeping the cursor disabled if it was disabled (experimental).

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
- `prefix_inactive_style`, `prefix_inactive`: [Style Settings](#style-settings) for the prefix of inactive columns.
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls how current item styles apply across the row.
- `scroll_wrap`, `cycle`, `c`: (bool) Wrap selection when reaching the end of the list.
- `scroll_padding`, `sp`: (number) Number of items to keep visible above/below the selection.
- `r`, `reverse`: (bool or null) Whether to reverse the list order; null leaves the default behavior.
- `w`, `wrap`: (bool) Enable line wrapping for result items.
- `width_overrides`: ([number]) Initial column width overrides.
- `min_width`: (number) Minimum column width.
- `min_width_from_cols`: (bool) Derive the minimum width from the configured columns.
- `column_spacing`: (number) Spacing between columns.
- `right_align_last`, `ra`: (bool) Right-align the last column.
- `acp`, `active_column_min_percentage_hint`: (number, optional) Minimum percentage hint for the active column.
- `max_height`: (number) Maximum row height. VScroll/Preview can still show the full result; 0 disables the limit.
- `show_skipped`: (bool) Show rows skipped by the result-height limit.
- `vscroll_current_only`: (bool) Restrict vertical scrolling to the current row.
- `delay_clear`: (bool) Keep the previous results visible while a reload is still loading input instead of clearing the table; it is cleared once a result arrives or the worker settles.
- `uniformly_truncate_columns`: (bool) Truncate row columns uniformly instead of truncating each column independently.
- `v`, `vertical`, `stacked_columns`: (bool) Display columns stacked vertically instead of across.
- `separator`, `hr`: (`none`, `empty`, `light`, `normal`, `heavy`, `dashed`) Separator between rows.
- `separator_style`: [Style Settings](#style-settings) for the row separator.
- `autoscroll`, `a`: Control how the results table scrolls horizontally to keep matches in view.
  - `enabled`, `a`: (bool) Enable/disable horizontal autoscroll.
  - `initial_preserved`, `i`: (number) Number of characters at the start of the line to always keep visible.
  - `context`, `c`: (number) Number of characters to show around the match.
  - `end`, `e`: (bool) Whether to autoscroll to the end of the line.
  - `always`: (bool) Enable autoscroll even when wrapping is enabled.
- `rct`, `resize_col_thresholds`: ([number, number]) Pixel deltas required before a column's preferred width is recomputed.
- `frame`, `f`: [Frame Settings](#frame-settings).

#### Status Line (`status.`)

- `style`: [Style Settings](#style-settings).
- `show`: (bool) Show/hide the status line.
- `match_indent`: (bool) Indent the status to match the results table.
- `template`, `t`: (string) The following replacements are available:
  - `\r` -> current result row (1-based, `-` when there is no current row)
  - `\m` -> match count
  - `\t` -> total item count
  - `\s` -> available whitespace / count appearances
  - `\S` -> increment the count denominator without displaying whitespace
- `row_connection`: `Disjoint`, `Capped`, or `Full`. Controls the width used for whitespace expansion.
- `interactions`: Either `[index, action]` regions or an ordered action list mapped to `i`/`interactive` template spans. See [Interactions](template.md#interaction-regions).

#### Toast (`toast.`)

The toast log is drawn at the outer edge of the UI (below the footer in the default layout) and replaces footer rows before it takes any from the results table. It stays hidden while the log is empty. The mouse wheel scrolls the toast when the cursor is over it; scrolling up stops following new messages until you scroll back to the bottom.

- `frame`, `f`: [Frame Settings](#frame-settings) (default: top border only, black background, light yellow border).
- `max_height`: (number) Maximum number of message rows shown; the frame adds its own rows (default 5).

#### Preview Panel (`preview.`, `p`)

- `show`: (bool or number) Toggle the preview window, or show it only when the relevant available dimension is at least the given number.
- `scroll_wrap`, `cycle`, `c`: (bool) Enable scroll wrapping in preview.
- `wrap`: (bool) Enable line wrapping in preview.
- `layout`, `l`: List of preview settings. This path overrides the existing preview layouts in order.
  - `x`, `command`: Command to run for preview. `{}` is replaced by the item.
    - Absolute alias: `px`.
  - Layout fields *(flattened into each entry)*:
    - `side`: `top`, `bottom`, `left`, `right`.
    - `percentage`: Percentage of the screen to occupy.
    - `min`, `max`: Size constraints in cells. `max = 0` disables the layout. A negative value is relative to the total size: the pane is capped at `total - abs(value)`, reserving that many cells for the results pane.
    - `initial` (alias `scroll`): Per-layout overrides for the initial scroll settings below.
    - `frame`: Per-layout [Frame Settings](#frame-settings); provided keys are layered over `preview.frame`.
- `frame`, `f`: [Frame Settings](#frame-settings).
- `initial`, `i` (alias `scroll`): Control the initial scroll offset of the preview window.
  - `index` (string or integer, optional) – Extract the initial display index `n` of the preview window from this column. `n` lines are skipped after the header lines are consumed.
  - `o`, `offset` (integer) – Adjust the initial scroll index relative to `index`. Defaults to `-1` when `index` is set, matching 1-indexed tools like `rg`.
  - `p`, `percentage` (0-100) – How far from the bottom of the preview window the scroll offset should appear.
  - `h`, `header_lines` (number) – Keep the top N lines (default 0) as a fixed header so that they are always visible. Ignored while an `<MM_IMAGE>` image is shown; the image header's own `header_lines` decides how many lines sit above it.
  - `t`, `tail` (bool) – Start with the scroll at the bottom of the preview window.
- `initial_layout`: (number) Index of the initially selected preview layout.
- `trim_ends`: (bool) Trim leading and trailing newlines from preview output.
- `reevaluate_show_on_resize`: (bool) Re-evaluate the `show` condition after a resize.
- `drag_width`: (integer, optional) Width of the divider strip used for mouse resizing. `0` disables drag resizing; omitted uses the preview frame width.
- `image`: How `<MM_IMAGE>` preview images are displayed. See `mm --doc image` for the full guide.
  - `protocol`: Graphics protocol to render with: `auto`, `halfblocks`, `sixel`, `kitty`, `iterm2`, or `none` to disable images (default `auto`). Anything but `auto` wins over what the terminal reported, so a protocol the terminal cannot render is drawn as garbage rather than replaced; use `auto` to go by detection.
  - `horizontal_fit`, `vertical_fit`: How each axis is fitted to the preview pane: `contain` (shrink to the pane, never enlarge), `fill` (scale to the pane extent, may enlarge) or `crop` (leave unconstrained and clip). Defaults to `horizontal_fit = "contain"`, `vertical_fit = "crop"`. Either may name its own extent in cells instead of the pane, e.g. `"fill:16"`, with `0` meaning the pane. The image keeps its proportions, so the two axes resolve together; `mm --doc image` lists the combinations.
  - `filter`: Resampling filter used when the image has to be scaled: `nearest`, `triangle`, `catmull_rom`, `gaussian` or `lanczos3`. Defaults to `triangle`.
  - `zoom`: Magnification of `1.0` or more (default `1.0`). The image's rows grow with it and less of its width is shown at a time; the vertical preview scroll zooms it by tenths while it is on screen.
  - `scroll`: Initial `[vertical, horizontal]` offset in cells (default `[0, 0]`): where the preview starts, and how far the image is panned.
  - `header_lines`: Lines of the text after the image payload drawn above the image instead of after it (default 0). The image is fitted into the pane height that remains under them.

### Previewer (`previewer.`)

- `try_lossy`: (bool) Enable lossy UTF-8 conversion for preview command output.
- `delay_clear`: (bool) If true, prevents clearing the preview window until the new command starts producing output (default true).
- `debounce_ms`: (number) Debounce delay for preview commands in milliseconds (default 50).
- `max_procs`: (number) Maximum number of concurrent preview processes (default 4).
- `always_trigger`: (bool) If false, skips running the preview command if it is the same as the last one executed (default true).
- `shell`: (list of strings) The shell used to execute preview commands (e.g., `["sh", "-c"]`).
- `trim_commands`: (bool) Trim whitespace from preview commands.
- `images`: (bool) Whether to parse `<MM_IMAGE>` image headers from preview output (default true). This is disabled automatically when `preview.image.protocol` is `none`.
- `help`: Help display settings.
  - `colors`: Optional help colors with `section`, `key`, and `value` color fields.
  - `seq_brackets`: Optional pair of characters used to display key sequences.
  - `hide_semantic`: (bool) Hide semantic triggers in help (default true).
  - `quote_traces`: (bool) Quote trace messages in help (default false).
  - `trace_modifier`: (modifier) Style modifier for trace descriptions in help (default "ITALIC").
  - `alias_modifier`: (modifier, optional) Style modifier for alias triggers in help, stripping the leading '@' (default "ITALIC").
  - `max_item_len`: (number) Maximum length of a help item before ellipsizing (default 50).
  - `ellipsize_center`: (bool) Ellipsize the center of help items (default false).
  - `event_trigger_prefix`: (string) Prefix for event triggers (default "#").
  - `show_events`: (bool) Show event triggers in help (default false).
  - `sort_fn_last`: (bool) Put function key (F1, F2, etc.) bindings at the end of the list (default true).
  - `combine_keys`: (bool) Combine multiple keys triggering the same action into a comma-separated list (default true).
- `cache`: (number) Reserved for future use.
- `command_args`: ([string]) Extra arguments reserved for preview commands (currently unused).

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
- `smart_case`: (bool) Case-sensitive search only when the query contains
  uppercase characters (default false).
- `copy_on_select`: (bool) Copy a mouse selection when the button is released
  instead of waiting for the copy key (default false).

### Markdown (`markdown.`)

How markdown is rendered by `mm --doc` and by the preset docs paged from `@help`.

- `variant`: (variant) Base skin for the terminal background: `neutral` (default), `dark`, or `light`. The variant only shifts the code colors; the rest of the skin is shared.
- `bold`, `header`, `inline_code`, `code_block`, `bullet`: [style settings](#style-settings) for those elements. `header` applies to every heading level, `bullet` is the style of list bullets. Omitted fields keep termimad's defaults.

Blocks fenced with the `mermaid` tag (flowcharts, sequence, state, class, ER, pie, gantt, mindmap, …) are drawn as Unicode box diagrams sized to the available width, styled by `code_block`. A block that Mermaid cannot parse is left as source code.

```toml
[markdown]
variant = "dark"
bold.fg = "LightYellow"
header.modifier = "BOLD | ITALIC"
```

### Header & Footer (`header.`, `footer.`, `h`, `f`)

- `content`: (string or list) Static content to display.
  - Absolute alias: `h`.
- `style`: [Style Settings](#style-settings).
- `match_indent`: (bool) Indent content to match the results table.
- `wrap`: (bool) Enable line wrapping.
- `row_connection`: Controls the effective width of the displayed content. See [Results Table](#results-table-results-r) for variants.
- `h`, `header_lines`: (number, header only) Number of lines to read from input for the header.
- `interactions`: ([index, action]) Define interactive regions per line. See [Interactions](template.md#interaction-regions).
- `frame`, `f`: [Frame Settings](#frame-settings).

### TUI Settings (`tui.`)

- `stream`: Output stream selection consumed by the client: `Auto` (default; pick automatically), `Stdout`, `BufferedStderr`, `Tty`, or `Test` (tests only).
- `restore_fullscreen`: (bool) Restore fullscreen on exit.
- `extended_keys`: (bool) Enable enhanced keyboard support.
- `sleep_ms`: (number) Delay in milliseconds before resizing.
- `clear_on_exit`: (bool) Clear the TUI screen after selection.
- `clear_after_execute`: (bool) Clear after execute actions. This setting is currently unimplemented.
- `layout` *(flattened)*: Constraints for non-fullscreen mode.
  - `percentage`, `p`: Height of the terminal used (percentage).
  - `min`, `max`: Height constraints in rows.
  - `scroll`: (`Aggressive`, `Lazy`, `Expansive` or `Compact`) How the terminal is scrolled up when the requested height does not fit below the cursor (default: `Aggressive`).
- `osc52`: (bool) Execute copy actions using the OSC52 protocol (default: true).
- `copy_trailing_newline`: (bool) Preserve a trailing newline in copied command output (default: false).

Scroll strategies:

| Strategy     | Scrolls when       | Final size                         |
| ------------ | ------------------ | ---------------------------------- |
| `Aggressive` | below `percentage` | capped at `percentage`             |
| `Lazy`       | below `min`        | capped at `percentage`             |
| `Expansive`  | below `percentage` | all rows below cursor, up to `max` |
| `Compact`    | below `min`        | just enough for `min`              |

### Style Settings

Several UI components have a `style` block (or similar, like `prompt_style`):

- `fg`: (color) Foreground color.
- `bg`: (color) Background color.
- `modifier`: (modifier) Style modifier (e.g., `BOLD`, `ITALIC`, `DIM`, joined by `|`).

### Frame Settings

Most UI components have a `frame` block. It draws an optional border and applies padding to the component's inner area.

- `border_type` (alias `type`): Border glyphs, e.g. `Plain`, `Rounded`, `Double`, `Thick`. See <https://docs.rs/ratatui/latest/ratatui/widgets/enum.BorderType.html>.
- `sides`: Which sides to draw (`TOP | BOTTOM | LEFT | RIGHT`, or `ALL`). An empty string (`sides = ""`) draws none. When omitted, all sides are drawn if `border_type` or a border color/modifier is set; otherwise none.
- `padding`: Space inside the frame. Accepts a number (all sides), `[horizontal, vertical]`, `[top, right, bottom, left]`, or an object such as `{ top = 1, left = 2 }` (omitted sides default to 0).
- `title`: Optional text to display on the top border.
- `title_style`: [Style Settings](#style-settings) applied to the title.
- `style`: [Style Settings](#style-settings) applied to the framed container area.
- The border glyph style is supplied by the frame's own `fg`, `bg`, and `modifier` keys:

  ```toml
  [preview.frame]
  border_type = "Rounded"
  fg = "#3B4261"
  ```

### Key Binds (`binds.`, `b`)

See `mm --doc binds`.
