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
  - If unspecified, columns are automatically named `1`, `2`, ... (or `0`, `1`, ... if `names_from_zero` is true).

### Column Filtering

Filter a specific column using `%name` or any abbreviation thereof:

- `%path .toml`: Matches items where the `path` column ends with `.toml`.
- `helix %p .toml !lang`: Match `helix`, path ends in `.toml`, exclude `lang`.
