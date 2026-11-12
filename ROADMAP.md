## TODO

- move preset screenshots into preset directory

- improve restore: exit(clear: Option<bool>)
  - None: move up by input.y-area.y
  - true: move to area.y and clear
  - false: nothing
  - config.clear_on_exit: None -> true
  - Fix exit_lite

- mla feature?
- replace ansi-2-text for performance and correctness (i.e. man output)

- chafa has better character renderer, if our protocol is half_blocks, mm --image would technically work better by preferring chafa

# Previewer

- Offload large previews to disk/caching
- spawn with pty (animated previews?)

# Perf

- benchmarks
  - (what kinds of speed matter?)
  - memory: (800000 items) mac home dir: fzf 137M vs sk 212 vs mm ~~509~~/~~309~~/(12-183?) <- btop giving some inaccurate readings

- Adaptive percentages maybe, thresholds/interpolation is obvious but just doesn't feel "easy"

# Bugs

- crossterm cannot detect modifiers on mouse events but we support binding it
- [x] git/restore has a weird heading bug [history] -> [stash]y], i think its from ratatui tho i have no idea how it happens
- zellij session preview, sometimes the header line autowraps when wrap is false tho the command producing it never does. ratatui bug maybe?
- chinese characters (i.e. in preview) can break layout, probably also a ratatui/term bug related to widths

### Low priority

- ColumnChange event, set previewer to listen
- partial should be under #[cfg] but that breaks field level attributes, i don't think there is a solution as we cannot use derive macro (not planned)
- case insensitive bitflags deserialization (probably requires ratatui pr)
- ord/name field on prev layouts for better composability?
- Indentation style setting: active or first or custom: decided on active.
- Does render refs (statusUI, overlay?) improve performance?
