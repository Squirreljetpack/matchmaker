## Zellij pickers

Four presets, connected into a ring with `@next` / `@prev` (cycle with
`ctrl-shift-n` / `ctrl-shift-p` or `alt-tab`):

Presets are skipped in the cycle when empty

### `zellij_session` -- switch sessions

Every session, active and otherwise.

- `<enter>`: attach/focus the selected session (EXITED sessions can't be
  attached; use `@kill` to remove them)
- `@print`: print the session name
- `@copy`: copy the session name to the clipboard
- `@rm`: kill the selected session (with confirmation)
- main preview: cached session layout KDL

### `zellij_current` -- panes of the current session

Every pane of the current session except the one you're in.

### `zellij_other` -- panes of other sessions

Every pane of every other active session.

For both pane pickers:

- `<enter>`: focus the selected pane (`zellij_current`) or switch to it in its
  session (`zellij_other`)
- `@print`: print `session<TAB>pane`
- `@copy`: copy `session:pane` to the clipboard
- `@rm`: close the selected pane (with confirmation)
- main preview: the pane's live screen dump

## Zellij layout picker

Apply a single-tab layout.

- `<enter>`: apply the selected layout to the current tab, keeping panes that
  don't fit (`zellij action override-layout <name> --apply-only-to-active-tab
  --retain-existing-terminal-panes`)
- `alt-enter` (`@accept_2`): apply it replacing the current tab's panes --
  terminal panes that don't fit are closed (no confirmation, no undo)
- `ctrl-n`: open it in a new tab
- `ctrl-o` (default `@open`): open the layout's KDL file in `$EDITOR` (the
  file path is column 1; built-in layouts have none)
- preview: the layout's KDL via `zellij setup --dump-layout <name>`

user `*.kdl` files from the
config layout dir (at most one top-level `tab` block; swap variants skipped).
Extra layout names can be passed as arguments to `zellij.sh layouts` to
include them; a bare name resolves to the user layout dir first, then
zellij's built-ins.

<img src=".README.assets/Screenshot 2026-08-14 at 12.34.23 AM.png" alt="Screenshot 2026-08-14 at 12.34.23 AM" style="zoom:33%;" />
