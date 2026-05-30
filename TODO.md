## TODO

- it would be nice to have presets like full, simple, and minimal presets like fzf
- it would be nice to have color presets too maybe
- better hr styling (dim etc.)
- improve/test wrap_text and hscroll on non filtering
- Bottom scroll padding not working with --reverse (maybe we want to increase self.cursor if height before is insufficient).
- vty to support animated previews/sixel (will that do the trick? otherwise pipe should be more efficient).
- improve restore: exit(clear: Option<bool>)
  - None: move up by input.y-area.y
  - true: move to area.y and clear
  - false: nothing
  - config.clear_on_exit: None -> true

- Code examples:
  - query change
  - frecency
  - api

- sort by column
- toast action:
  - toast config:
    - trigger on cycle

- Multiline + Capped + column highlight looks a tiny bit weird
  - try different combinations

- unaligned headings (!!!): need to optimize make_table and worker.results fn, I think we should break worker.results down into a method which returns a row instead creating them all at once using iter.filter_map().
- support alternate actions syntax(?): case insensitive, execute::content <- use rhai could be cool

- Display rework
- Picker overlay
- builder with intermediate type states for pick options + make state depend on context C and aext A

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
- constraint: Min/Percent, use header to set min width?

# Bugs

- When the cursor is not near the top (horizontal preview), the cursor doesn't get restored, and the stuff after not cleared
- if only current is highlighted, and current col is empty, cursor is invisible.. not sure best way to resolve this
- reverse scroll to end doesn't fill view
- crossterm (can fail to) detect modifiers on mouse events

### Low priority

- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- sometimes preview leaks (on invalid unicode), better autorefresh?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro
- case insensitive bitflags deserialization (probably requires ratatui pr)
- finalize non-exclusive columns: if the default query matches the default column or any in this set, include this result (wip)
- I feel that having matcher and worker in seperate fields and supporting deny_unknown outweighs the minor confusion it could introduce
- Non grapheme aware option to speed up rendering? This would require frizbee (and be required by?).
- Adaptable preview percentage (higher on smaller)
- ord field on prev layouts for better composability?

- modes for binds? the event listener with a mode variable, which is initialized to
  "command" when mm is started using command, and "piped" when mm is started using
  stdin. Add an action SetMode(String), which sets the mode string. Actions now become vec<Option<String>, Actions> <- seems heavy handed
- renderloop optimization: pass available height?
- descriptions to override help actions
- switch preset can read remote from {$1} instead of assuming origin
- flicker-free reload: before interrupt save results or something + pause events?
- Improve BecomeSilent to further reduce flickering (is it even possible?)
- reload should send to preview_tx (why did i add this?)
- very very minor perf improvement, prevent duplicate dynamic handler calls somehow?
- just ran into a facepalm due to previewsetting not having deny_unknown_settings, maybe it would be better to actually flatten
- support hijack rendering?


