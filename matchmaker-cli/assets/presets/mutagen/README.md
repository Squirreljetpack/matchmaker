## Mutagen session picker

### `mutagen` -- inspect and manage Mutagen sync sessions

- `@show` (`ctrl-s`): show detailed session information (`mutagen sync list -l <name>`)
- `@copy_all` (`ctrl-shift-y` / `alt-shift-y`): copy all conflicting file paths to clipboard
- `@copy` (`ctrl-y` / `alt-p`): copy selected session name to clipboard
- `alt-f` (`@flush`): flush (force synchronization cycle)
- `alt-p` (`@pause`): pause synchronization
- `alt-r` (`@resume`): resume synchronization
- `alt-shift-r` (`@reset`): reset sync history (clear merge state & rescan, with confirmation)
- `alt-c` (`@clean`): clean orphaned folders containing ignored files deleted remotely (non-interactive)
- `alt-v` (`@resolve`): interactive conflict resolver (diff, overwrite remote, overwrite from remote)
- `alt-a` (`@resolve-alpha`): resolve every conflict in favor of Alpha (non-interactive, with confirmation)
- `alt-b` (`@resolve-beta`): resolve every conflict in favor of Beta (non-interactive, with confirmation)
- `ctrl-d` (`@rm`): terminate session (with confirmation)
- main preview: `mutagen sync list <name>`
- secondary previews: detailed info (`-l`), conflicting files list
