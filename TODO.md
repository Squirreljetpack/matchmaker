## TODO

- it would be nice to have presets like full, simple, and minimal presets like fzf
- it would be nice to have color presets too maybe
- Examples:
  - query change
  - frecency
  - api

- better hr styling (dim etc.)
- bind modes
- status/header click events
- ExecuteAsync: support chaining actions without blocking ui
- improve/test wrap_text and hscroll on non filtering
- Bottom scroll padding not working with --reverse (maybe we want to increase self.cursor if height before is insufficient).

# Previewer

- Offload large previews to disk
- Caching (?)
- debouncing (?)

# Perf

- benchmarks
  - (what kinds of speed matter?)
  - memory: (800000 items) mac home dir: fzf 137M vs sk 212 vs mm ~~509~~/309/(12-183?) <- btop giving some inaccurate readings

- group Segmented<T> Storage indices
- change nucleo to expose Index over to remove dependency on indexed<T>
- offload injector wrapper logic to column functions

# Columns
- (fist: lowpri): execute: use of {\*} in place of {+}: execute once for each selected

# Bugs

- When the cursor is not near the top (horizontal preview), the cursor doesn't get restored, and the stuff after not cleared
- if only current is highlighted, and current col is empty, cursor is invisible.. not sure best way to resolve this
- reverse scroll to end doesn't fill view


### Low priority

- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- sometimes preview leaks (on invalid unicode), better autorefresh?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro
- case insensitive bitflags deserialization (probably requires ratatui pr)
- finalize non-exclusive columns: if the default query matches the default column or any in this set, include this result (wip)
- I feel that having matcher and worker in seperate fields and supporting deny_unknown outweighs the minor confusion it could introduce
- Non grapheme aware option to speed up rendering? This would require frizbee (and be required by?).
- Adaptable preview percentage (higher on smaller)

- modes for binds? the event listener with a mode variable, which is initialized to
   "command" when mm is started using command, and "piped" when mm is started using
   stdin. Add an action SetMode(String), which sets the mode string. Actions now become vec<Option<String>, Actions> <- seems heavy handed
- renderloop optimization: pass available height?
- Mode: filters which binds activate (starts off as either "command", or "piped")

# Examples

(date; ps -ef) |
fzf --bind='ctrl-r:reload(date; ps -ef)'\
--header=$'Press CTRL-R to reload\n\n' --header-lines=2\
--preview='echo {}' --preview-window=down,3,wrap\
--layout=reverse --height=80% | awk '{print $2}' | xargs kill -9

Kubernetes

Git


