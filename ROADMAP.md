## TODO
- it would be nice to have presets like full, simple, and minimal presets like fzf
- it would be nice to have color presets too maybe
- better hr styling (dim etc.)
- vty to support animated previews/sixel (will that do the trick? otherwise pipe should be more efficient).
- move preset screenshots into preset directory
- improve restore: exit(clear: Option<bool>)
  - None: move up by input.y-area.y
  - true: move to area.y and clear
  - false: nothing
  - config.clear_on_exit: None -> true

- git/restore has a weird heading bug [history] -> [stash]y], i think its from ratatui tho i have no idea how it happens

- Code examples:
  - query change
  - frecency
  - api


- Does render refs (statusUI, overlay?) improve performance?

- toast action:
  - toast config:
    - trigger on cycle
  - (git) toast arguments

- Indentation style setting: active or first or custom? 

- support alternate actions syntax?: execute::content <- use rhai/lua could be cool

- Picker overlay

- builder with intermediate type states for pick options + make state depend on context C and aext A

- replace ansi-2-text for performance and correctness (i.e. man output)
- Fix exit_lite

# Previewer

- Offload large previews to disk/caching
- spawn with pty

# Perf

- benchmarks
  - (what kinds of speed matter?)
  - memory: (800000 items) mac home dir: fzf 137M vs sk 212 vs mm ~~509~~/309/(12-183?) <- btop giving some inaccurate readings

# Columns

- (fist: lowpri): execute: use of {\*} in place of {+}: execute once for each selected
- constraint: Min/Percent, use header to set min width?

# Bugs

- When the cursor is not near the top (horizontal preview), the cursor doesn't get restored, and the stuff after not cleared
- if only current is highlighted, and current col is empty, cursor is invisible.. not sure best way to resolve this
- crossterm (can fail to) detect modifiers on mouse events

### Low priority

- ColumnChange event, set previewer to listen
- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro (not planned)
- case insensitive bitflags deserialization (probably requires ratatui pr)
- nucleo fork
  - more column options (?)
  - Non grapheme aware option to speed up rendering? This would require frizbee (and be required by?).

- Adaptable preview percentage (higher on smaller)
- ord field on prev layouts for better composability?
- flicker-free reload: if empty don't update?
- very very minor perf improvement, prevent duplicate dynamic handler calls somehow? (not planned)
- just ran into a facepalm due to previewsetting not having deny_unknown_settings, maybe it would be better to actually flatten

