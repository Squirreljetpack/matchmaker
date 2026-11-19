# Linux presets

## Preference panes

Find and launch the settings panels installed on your desktop. The preset reads
XDG `.desktop` entries and presents the ones registered as Settings panels, so
you do not have to remember whether a panel lives in System Settings, GNOME
Tweaks, or a vendor application.

### Usage

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
`Settings` when it has no comment; the command is shown in the preview.
`<enter>` runs the selected panel's `Exec` line and exits Matchmaker, so this
picker is a launcher rather than a manager. The desktop entry is opened as-is;
no settings are modified by the preset itself.

## Users

Inspect local accounts and their last login.

```sh
mm -o linux/users
```

Requirements: `getent`, `id`, and `last`; `lastlog` is used when installed.

Rows come from `getent passwd | cut -d: -f1`, so every account in the user
database is listed, including system accounts with `/usr/bin/nologin` shells.
The preview shows the account's groups, its passwd fields (uid, gid, GECOS,
home, shell), and its last login: `lastlog -u` when that command exists, or the
most recent `last -n 1` entry otherwise. `lastlog` is deprecated in util-linux
and missing from some distributions, which is why the fallback is always
attempted.

Enter accepts the user name; nothing in the preset modifies accounts or logs.

## Libraries

Browse shared libraries from the dynamic linker cache, or from a filesystem
scan.

```sh
mm -o linux/libs
```

Requirements: `ldconfig`, `fd`, and `path-links` (for the preview).

Rows are the `ldconfig -p` lines with the header, footer, and leading tabs
removed: `libz.so.1 (libc6,x86-64) => /usr/lib/libz.so.1`. `@next`/`@prev`
reload from `fd -t f -t l '\.(so|dll|dylib)(\.[\d\.]+)?$' /` instead, whose
rows are plain absolute paths; the prompt reports the active source (`ldconfig>`
or `find *.so>`), and `-t l` keeps the versioned soname symlinks (`libz.so.1`)
that a plain `-t f` filter skips.

The preview runs `path-links` on the part after `=>`, or on the whole row when
there is none, and lists the symlink chain. Enter accepts that same path, one
path per line for a multi-selection, so the `name (arch) =>` prefix never
reaches the output.
