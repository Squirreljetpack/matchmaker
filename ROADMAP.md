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
  - Fix exit_lite

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
  - id like to use either the line or table 'niche' of displayui but not sure which one

- mla feature?
- Picker overlay
- replace ansi-2-text for performance and correctness (i.e. man output)

# Previewer

- Offload large previews to disk/caching
- spawn with pty

# Perf

- benchmarks
  - (what kinds of speed matter?)
  - memory: (800000 items) mac home dir: fzf 137M vs sk 212 vs mm ~~509~~/309/(12-183?) <- btop giving some inaccurate readings

- Adaptive percentages, thresholds/interpolation is obvious but just doesn't feel "easy"

# Columns

- (fist: lowpri): execute: use of {\*} in place of {+}: execute once for each selected

# Bugs
- crossterm cannot detect modifiers on mouse events but we support binding it

### Low priority

- ColumnChange event, set previewer to listen
- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro (not planned)
- case insensitive bitflags deserialization (probably requires ratatui pr)
- nucleo fork
  - more column options (?)
  - Non grapheme aware option to speed up rendering? This would require frizbee (and be required by?).
- maybe generic context on mmstate so people don't need globals, doubtful tho
- README images should be unformly sized :(

- Adaptable preview percentage (higher on smaller)
- ord field on prev layouts for better composability?
- flicker-free reload: if empty don't update?
- very very minor perf improvement, prevent duplicate dynamic handler calls somehow? (not planned)
- just ran into a facepalm due to previewsetting not having deny_unknown_settings, maybe it would be better to actually flatten
- Indentation style setting: active or first or custom: decided on active.
