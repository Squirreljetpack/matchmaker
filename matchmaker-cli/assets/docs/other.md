# Queries

Matchmaker uses a powerful fuzzy matcher (based on Nucleo) to filter and rank items. It implements fzf-style scoring with smart case, consecutive match boosting, and start-of-word preference.

---

## How Matching Works

1. **Fuzzy Matching**: Characters match in order but not necessarily consecutively. Shorter gaps and consecutive runs score higher.
2. **Multiple Tokens**: Split by spaces. Each token must match independently (logical AND).
   - Example: `foo bar` matches items containing both `foo` and `bar`.
3. **Smart Case**:
   - Lowercase query → case-insensitive.
   - Uppercase letters → case-sensitive.

---

## Query Syntax and Operators

| Operator | Meaning           | Example                                    |
| :------- | :---------------- | :----------------------------------------- |
| `abc`    | Fuzzy match       | `abc` matches `alphabetic`                 |
| `'abc`   | Literal substring | `'foo` matches `hello foo` but not `f_o_o` |
| `^abc`   | Match prefix      | `^src` matches items starting with `src`   |
| `abc$`   | Match suffix      | `bar$` matches items ending with `bar`     |
| `^abc$`  | Exact match       | `^foo$` matches exactly `foo`              |
| `!abc`   | Exclude           | `foo !test` matches `foo` but not `test`   |
| `\`      | Escape space      | `foo\ bar` matches literal space           |

---

## Columns

### Configuration

- **`columns.split`**: Defines how input lines are parsed into columns.
  - `None`: No splitting (single column).
  - `Delimiter(regex)`: Splits line by a regex (e.g., `\s+` or `,`).
    - Capture groups are supported:
      - If the regex contains **named groups**, each named match is assigned to the column with the corresponding name.
      - If there are **unnamed groups**, matches are assigned to columns in sequence (first group → first column, etc.).
      - Example:

        ```regex
        (?P<name>\w+),(?P<age>\d+),(\w+)
        ```

        - `name` → column `"name"`
        - `age` → column `"age"`
        - Third unnamed group → third column in order
  - `Regexes([regex])`: Uses a sequence of regexes to capture specific parts of the line.
- **`columns.names`**: A list of column settings (`name`, `filter`, `hidden`).
  - If names are provided, you can filter by them.
  - If unspecified, columns are automatically named `1`, `2`, ...

*Note: Beware that any columns after `columns.max` are inaccessible!*

### Column Filtering

Filter a specific column using `%name` or any abbreviation thereof:

- `%path .toml`: Matches items where the `path` column ends with `.toml`.
- `helix %p .toml !lang`: Match `helix`, path ends in `.toml`, exclude `lang`.

---

### Testing presets

`--list` is the primary tool for developing and debugging a preset without a TTY.

### Bare `--list`

Runs the populating command (`start.command`) directly, so its stdout,
stderr and exit code are preserved:

```sh
mm --list
```

### `--list=<ARG>` — four modes

The value after `=` dispatches on its shape (the delimiter only wins when the
part before it is a number):

| Shape        | Mode                                                                                  |
| ------------ | ------------------------------------------------------------------------------------- |
| `N@alias`    | Execute the command actions bound to the semantic alias, formatted with the N-th item |
| `N-M`        | Run the M-th preview layout's command, formatted with the N-th item                   |
| `N:TEMPLATE` | Execute `TEMPLATE` formatted with the N-th item                                       |
| `TEMPLATE`   | Print `TEMPLATE` formatted with the first item (no execution)                         |

```sh
mm --list="3:open {path}"             # execute a template with the 4th item
mm --list="echo {}"                   # print the template (formatted, not run)
mm --list="2-0"                       # run preview layout 0 with the 3rd item
mm --list="0@toggle extension"        # run the alias's Execute/Become actions
```

- `N` (and the preview index `M`) are 0-based.
- The value must be joined with `=` — a separate argument after `--list` is parsed
  as a config override.
- `TEMPLATE` uses the full template syntax: see `mm --doc template`.
- Items are ordered exactly as the matcher would display them (with an empty query,
  all items are matched), so index `0` is the item Enter would accept first.
- The executed command inherits the `MM_*` / `FZF_*` environment variables
  (`MM_POS`, `MM_TOTAL_COUNT`, ...); preview commands additionally get
  `MM_PREVIEW_COMMAND`, mirroring the TUI.
- Alias execution looks at the action array bound to the trigger as-is (nested
  `@` aliases are **not** followed) and runs `Execute`/`ExecuteAsync`/
  `ExecuteThen`/`ExecuteSilent`, `Become`/`BecomeSilent`, and the CLI's
  `ExecuteOrConfirm`/`ExecuteAndQuit`/`BecomeOrConfirm`/`BecomeOrResume` actions
  in order, stopping at the first failure. Other actions are skipped.

*Note: `--list` never starts the TUI; `sync`, `mode` and `--no-read` are ignored.*

Action scripts execute under `$SHELL` unless `start.shell` or `preview.shell` are set.
Remember to escape any template specifiers, for example: `jq '((map(select(.type == "model_change")) | .[0]) // \{}) as $model | $model'`.

---

## Miscellaneous

### Exit codes

- 125: EventLoopClosed
- 404: No Match
- 400: No Match with a chunk error
- 11: Start Error
- 100: Signal to resume from BecomeOrConfirm (when emitted by spawned scripts)
