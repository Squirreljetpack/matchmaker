# Pager

`ShowPreview` (default binds: `F11` and `ctrl-l`) pages the current preview fullscreen using [`minus`](https://docs.rs/minus).

minus draws on stdout when it is a terminal; when stdout is redirected (e.g.
`mm | head`) the pager draws on the controlling tty (`/dev/tty`) instead, so
piping or redirecting `mm`'s output never mixes pager UI into it.

The pager starts immediately: it does not wait for the preview command to
finish, and output streams in as the command produces it. `ShowPreview(cmd)`
pages an arbitrary template instead of the current preview command.

### `pager.` config section

| Key                       | Type   | Description                 |
| ------------------------- | ------ | --------------------------- |
| `pager.line_numbers`      | bool   | Show line numbers           |
| `pager.follow`            | bool   | Stick to the newest output  |
| `pager.prompt`            | string | Footer prompt text          |
| `pager.horizontal_scroll` | bool   | Enable horizontal scrolling |
| `pager.smart_case`        | bool   | Case-sensitive search unless the query has uppercase |

### minus keybindings

These are the defaults of minus 5.7. A keybinding table for all of
minus's actions lives in the [minus documentation](https://docs.rs/minus/latest/minus/).

| Key                            | Action                                      |
| ------------------------------ | ------------------------------------------- |
| `q`, `Ctrl-c`                  | Quit the pager (always resumes the picker)  |
| `j` / `↓`, `k` / `↑`           | Scroll one line (prefix a number to repeat) |
| `Space`, `PageDown` / `PageUp` | Scroll one page                             |
| `u`, `Ctrl-u` / `d`, `Ctrl-d`  | Scroll half a page                          |
| `g` / `G`                      | Go to the top / bottom (or to line N)       |
| `Enter`                        | Scroll N lines                              |
| `h` / `←`, `l` / `→`           | Scroll horizontally                         |
| `Ctrl-l`                       | Toggle line numbers                         |
| `Ctrl-f`                       | Toggle follow mode                          |
| `Ctrl + H`                     | Toggle line wrap                            |
| `/`, `?`                       | Search forward / backward                   |
| `n`, `p`                       | Jump to the next / previous match           |

## Without the `pager` feature

Without the feature, `ShowPreview` pipes the command output through the
external pager chain `MM_PAGER` → `$PAGER` → `less` → `more`.
