## TODO

- Fault tolerance
- sane defaults for ui
  - it would be nice to have presets like full, simple, and minimal presets like fzf
  - it would be nice to have color presets too maybe
- Examples:
  - query change
  - frecency
  - api
- column change propogates to pickerquery
- dynamically adjusting column hide/filtering
  - column: column hide should be external, not on the column object
  - formatter:
  - {_} to join together all visible column outputs
  - {+}
  - {!} current column
- configurable active and passive column colors
- benchmarks (what kinds of speed matter?)
- Add support for nucleo::Pattern in the matcher config
- Adaptable percentage (higher on smaller)
- status template
- header extraction option in cli
- case insensitive bitflags deserialization?

# Bugs

- Too many execute will crash event loop
- Preview sometimes disappears?
- Indexing can break?

### Low priority

- a scroll action could dispatch between preview and results
- Should event handlers support exiting the loop? if a use case comes up might be worth changing
- refactor to better fit components into specific ratatui roles
- sometimes preview leaks, better autorefresh?
- (automatic) horizontal scrolling of results
- too many actions at once (execute/reload) can cause crash sometimes
- input_rhs_prompt,
