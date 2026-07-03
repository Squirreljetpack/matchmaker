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
  - (git) toast arguments

- Multiline + Capped + column highlight looks a tiny bit weird
  - try different combinations

- support alternate actions syntax(?): case insensitive, execute::content <- use rhai could be cool
- Picker overlay
- builder with intermediate type states for pick options + make state depend on context C and aext A
- CopyAsync causes a screen flicker somehow
- replace ansi-2-text for performance and correctness (i.e. man output)

# Previewer

- Offload large previews to disk
- Caching (?)
- debouncing (?)

# Perf

- benchmarks
  - (what kinds of speed matter?)
  - memory: (800000 items) mac home dir: fzf 137M vs sk 212 vs mm ~~509~~/309/(12-183?) <- btop giving some inaccurate readings

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

- ColumnChange event, set previewer to listen
- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro (not planned)
- case insensitive bitflags deserialization (probably requires ratatui pr)
- finalize non-exclusive columns: if the default query matches the default column or any in this set, include this result (wip)
- Non grapheme aware option to speed up rendering? This would require frizbee (and be required by?).
- Adaptable preview percentage (higher on smaller)
- ord field on prev layouts for better composability?
- switch preset can read remote from {$1} instead of assuming origin
- flicker-free reload: before interrupt save results or something + pause events? (not planned)
- Improve BecomeSilent to further reduce flickering (is it even possible?) (not planned)
- very very minor perf improvement, prevent duplicate dynamic handler calls somehow? (not planned)
- just ran into a facepalm due to previewsetting not having deny_unknown_settings, maybe it would be better to actually flatten
- support hijack rendering loop?
- more compatible keybinds: tagset for modes, provide alternates for key sequences which might not be available i.e. ? vs shift-?

# Rework column sizing and row rendering

while remaining > 0, render row.

allocate widths as follows, let n = total/#cols
allocate widths for cols with max < n.
Recompute n and allocate again.
When no more cols with max < n, distribute equally.

constant render:
Constant tick rate, only draw on tick + no events/status update since last tick
