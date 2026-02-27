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
  - {\_} to join together all visible column outputs
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
- better hr styling (dim etc.)
- partial should be under #[cfg] but that breaks field level attributes, is there a solution?
- Previewer debouncing

# Bugs

- Too many execute can sometimes crash event loop
- Preview sometimes disappears?
- Indexing can break?
- When the cursor is not near the top (horizontal preview), the cursor doesn't get restored, and the stuff ater not cleared
### Low priority

- refactor to better fit components into specific ratatui roles so the ui can be embedded?
- sometimes preview leaks (on invalid unicode), better autorefresh?
- input_rhs_prompt
- status template substitution via shell command
- find out why previewchange event doesn't seem to fire as often as it should