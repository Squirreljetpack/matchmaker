# Processes

Browse the process table, inspect a process tree, its resource usage and its
sockets, then signal or kill the selected processes.

## Usage

```sh
mm -o ps
mm --download ps
```

Requirements: `ps`, plus `pstree` (falling back to `ps --forest`), `top`, and
`lsof` for the previews. A missing tool leaves its preview tab empty.

Rows come from `ps -eo pgid,pid,user,%cpu,vsz,rss,etime,stat,command`, split on
runs of spaces, and the command is the default search column. The header is the
label `[procs]`, so the column order is not otherwise on screen. A second header
tab lists listening and connected sockets instead, and the previews show the
process tree, live resource usage (`top -b -p`), and the selected process's open
files and sockets (`lsof -p`).

The abbreviation columns keep `ps`'s own names: `pgid` is the process group id
(the group leader is normally the shell that started the job), `vsz` and `rss`
are virtual and resident memory in KiB, `etime` is elapsed wall-clock time since
start, and `stat` carries the process state and its flags (`S` sleeping, `R`
running, `Z` zombie, `+` in the foreground, `s` session leader).

`<enter>` prints the selected PIDs, one per line, so the picker also works when
piped. `@return` prints the active column of the selection, and `@copy` copies
the PIDs.

The kill bindings signal the selected PIDs, their group leader, or their whole
process group. A single row is signalled immediately; a multi-row selection
asks for confirmation first.

| Bind           | Target                            | Signal    |
| -------------- | --------------------------------- | --------- |
| `ctrl-k`       | selection                         | `SIGTERM` |
| `alt-k`        | selection                         | `SIGKILL` |
| `ctrl-shift-k` | selection's group leader (`pgid`) | `SIGTERM` |
| `alt-shift-k`  | selection's group leader (`pgid`) | `SIGKILL` |
| `ctrl-g`       | selection's process group         | `SIGTERM` |
| `alt-g`        | selection's process group         | `SIGKILL` |

A process counts as gone once it exits or becomes a zombie. Each target is
waited on for `MM_KILL_TIMEOUT` seconds (default `1`); survivors are reported
as `Failed:`.

![Process picker](./.README.assets/ps.png)
