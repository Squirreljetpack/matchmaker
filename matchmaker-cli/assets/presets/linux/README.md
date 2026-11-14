# Linux preference panes

Find and launch the settings panels installed on your desktop. The preset reads
XDG `.desktop` entries and presents the ones registered as Settings panels, so
you do not have to remember whether a panel lives in System Settings, GNOME
Tweaks, or a vendor application.

## Usage

```sh
mm -o linux/prefpanes
```

Requirements: `bash`, `find`, `grep`, and `xargs`. No setup is needed beyond
having desktop entries in one of the standard application directories.

The populating command searches `/usr/share/applications`,
`/usr/local/share/applications`, and `~/.local/share/applications` for
`.desktop` files whose `Categories` contain `Settings`. It reads the first
`Name`, `Comment`, and `Exec` fields and skips entries that have no name or no
executable.

When `XDG_CURRENT_DESKTOP` is set, entries are filtered by their `OnlyShowIn`
and `NotShowIn` lists. If the variable is unset, every Settings entry is shown,
including panels intended for a different desktop environment. The `Exec`
field's `%`-style field codes are stripped before the command is displayed.

One row is one panel. The description column is the entry's comment, or
`Settings` when it has no comment; the command is shown in the preview. `<enter>` runs the selected panel's `Exec` line and exits
Matchmaker, so this picker is a launcher rather than a manager. The desktop
entry is opened as-is; no settings are modified by the preset itself.
