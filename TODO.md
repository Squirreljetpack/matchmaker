## TODO

- it would be nice to have presets like full, simple, and minimal presets like fzf
- it would be nice to have color presets too maybe
- Examples:
  - query change
  - frecency
  - api

- Add support for nucleo::Pattern in the matcher config
- Adaptable percentage (higher on smaller)
- better hr styling (dim etc.)
- The former is important in that it allows executing commands without breaking tui layout
- vscroll has a bind but is not implemented in results
- status/header click events
- ExecuteAsync: support chaining actions without blocking ui

# Previewer

- Offload large previews to disk
- Caching (?)
- debouncing (?)

# Perf

- benchmarks
  - (what kinds of speed matter?)
- https://github.com/saghen/frizbee

# Columns

- (fist: lowpri): execute: use of {\*} in place of {+}: execute once for each selected

# Bugs

- Too many execute can sometimes crash event loop (cannot replicate)
- Preview sometimes disappears (cannot replicate)
- Indexing can break?
- When the cursor is not near the top (horizontal preview), the cursor doesn't get restored, and the stuff ater not cleared

### Low priority

- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- sometimes preview leaks (on invalid unicode), better autorefresh?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro
- case insensitive bitflags deserialization (probably requires ratatui pr)
- a better design for horizontal result scrolling?
  - better reset
  - better determination of where it applies
  - autohscroll interferes with manual scroll: better for results to return, per text, the first match index on each line (!)
- no option for a set of non-exclusive columns: if the default query matches the default column or any in this set, include this result (wip)
