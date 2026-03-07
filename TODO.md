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
- Offload large previews to disk
- better hr styling (dim etc.)
- Previewer debouncing
- read/write state which integrates with shell outputs and set_ actions

- https://github.com/saghen/frizbee

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
