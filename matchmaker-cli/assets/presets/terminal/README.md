## Zellij session launcher

Run `mm -o terminal/zellij_session` from inside a zellij session. Three list
modes, cycled with `ctrl-r` / `ctrl-shift-r` (or `alt-tab`):

| mode                   | items                                                                 |
| ---------------------- | --------------------------------------------------------------------- |
| `[sessions]` (initial) | every session, active and otherwise (state shown in the title column) |
| `[current]`            | every pane of the current session except the one you're in            |
| `[other]`              | every pane of sessions other than the current one                     |

- `<enter>`: attach/focus the selected session or pane (EXITED sessions can't
  be attached; use `@kill` to remove them)
- `@print`: print `session` or `session:pane`
- `@copy`: copy `session` or `session:pane` to the clipboard
- `@rm`: kill a session / close a pane (with confirmation)
- main preview: cached session layout KDL for a session, or the pane's live
  screen dump; right preview: every tab/pane index in that session

## Zellij layout picker

Run `mm -o terminal/zellij_layout` from inside a zellij session to apply a
single-tab layout to the current tab (or open it in a new one). The `name`
column is the only visible one -- `source` (user/builtin) and the user layout's
path are hidden columns:

- `<enter>`: apply the selected layout to the current tab
  (`zellij action override-layout <name> --apply-only-to-active-tab
  --retain-existing-terminal-panes`, with a y/N confirmation -- existing panes
  that don't fit are kept, not closed)
- `alt-enter` (`@accept_2`): open it in a new tab (`zellij action new-tab -l
  <name>`)
- `@copy`: copy the layout name
- preview: the layout's KDL via `zellij setup --dump-layout <name>`

Only layouts that affect a single tab are listed: user `*.kdl` files from the
config layout dir (at most one top-level `tab` block; swap variants skipped)
plus the single-tab built-ins `default`, `strider`, `compact`, `classic`.

<img src=".README.assets/Screenshot 2026-08-14 at 12.34.23 AM.png" alt="Screenshot 2026-08-14 at 12.34.23 AM" style="zoom:33%;" />
