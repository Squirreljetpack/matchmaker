## Zellij pickers

Four presets, connected into a ring with `@next` / `@prev` (cycle with
`ctrl-shift-n` / `ctrl-shift-p` or `alt-tab`):

Presets are skipped in the cycle when empty.

### `session` -- switch sessions

Every session, active and otherwise.

- `<enter>`: attach/focus the selected session (EXITED sessions can't be
  attached; use `@rm` to remove them)
- `@print`: print the name of every selected session
- `@copy`: copy the name of every selected session to the clipboard
- `@rm`: kill the selected session(s) (with confirmation)
- `alt-n` (`@new`): start a new session with zellij's generated name
- main preview: tab summary with pane titles
- secondary preview: active pane screen dump (active sessions) / session KDL layout (inactive sessions)

### `current` -- panes of the current session

Every pane of the current session except the one you're in.

### `other` -- panes of other sessions

Every pane of every other active session.

For both pane pickers:

- `<enter>`: focus the selected pane (`current`) or switch to it in its
  session (`other`; from outside zellij this attaches to that session
  with the pane focused)
- `@print`: print `session<TAB>pane` for every selected pane
- `@copy`: copy `session:pane` for every selected pane to the clipboard
- `@rm`: close the selected pane(s) (with confirmation)
- `alt-n` (`@new`): start a new session with zellij's generated name
- main preview: the pane's live screen dump

## Zellij layout picker

Apply a single-tab layout.

- `<enter>`: apply the selected layout to the current tab, keeping panes that
  don't fit (`zellij action override-layout <name> --apply-only-to-active-tab
  --retain-existing-terminal-panes`)
- `alt-enter` (`@return`): apply it replacing the current tab's panes --
  terminal panes that don't fit are closed (no confirmation, no undo)
- `ctrl-n`: open it in a new tab
- preview: the layout's KDL via `zellij setup --dump-layout <name>`

The picker lists user `*.kdl` files from the config layout dir (at most one
top-level `tab` block; swap variants skipped). Extra layout names can be passed
as arguments to `zellij.sh layouts` to include them; a bare name resolves to
the user layout dir first, then zellij's built-ins.

<img src=".README.assets/Screenshot 2026-08-14 at 12.34.23 AM.png" alt="Screenshot 2026-08-14 at 12.34.23 AM" style="width:416px;" />
