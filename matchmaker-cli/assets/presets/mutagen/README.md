# Mutagen sessions

Inspect and manage [Mutagen](https://mutagen.io/) sync sessions from one list.
The picker is useful when several folders are syncing continuously and you need
to see which session is paused, conflicted, or stuck.

## Usage

```sh
mm -o mutagen
```

Requirements: `mutagen`, `python3` for the conflict resolver, and `ssh`/`scp`
when a session has a remote endpoint. The diff viewer uses `$PAGER` and `diff`.

Rows come from `mutagen sync list`; each row is one session, named by its
configured name (or identifier when it has none). Accepting a row prints the
session name. The preview has three tabs: the short status from
`mutagen sync list`, a live `mutagen sync monitor`, and the detailed
configuration from `mutagen sync list -l`.

The actions map directly to Mutagen commands. `@flush` forces a synchronization
cycle, `@pause` and `@resume` control the session, and `@reset` clears the
merge state and rescans. `@rm` terminates the session after a confirmation;
terminating a session does not delete either endpoint's files.

Conflicts are resolved separately. `@resolve` walks each conflict and offers
to keep Alpha (the local side) or Beta (the remote side), show a diff, skip it,
or quit. For an orphaned folder whose other side was deleted but which still
contains untracked files, it also offers to delete the orphan. `@resolve-alpha`
and `@resolve-beta` perform the same choice for every conflict without
prompting per file, then flush the affected sessions. All three paths overwrite
or delete files directly and ask for confirmation before starting.

`@clean` only removes orphaned folders that contain untracked or ignored files
after the opposite side deleted the parent directory; it is not a general
conflict resolver. `@copy_all` copies the conflicting paths, and `@copy` copies
the session name.

Mutagen owns the sync state; this preset keeps no cache of its own. Actions
that change a session reload the list, so the displayed state reflects the
result of the command.
