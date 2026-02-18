## TODO

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
- case insensitive bitflags deserialization?
- Offload large previews to disk
- (automatic) horizontal scrolling of results

# Bugs

- Too many execute can sometimes crash event loop
- Preview sometimes disappears?
- Indexing can break?

### Low priority

- refactor to better fit components into specific ratatui roles
- sometimes preview leaks (on invalid unicode), better autorefresh?
- input_rhs_prompt
