## TODO

- sane defaults for ui
  - it would be nice to have presets like full, simple, and minimal presets like fzf
  - it would be nice to have color presets too maybe
- Examples:
  - query change
  - frecency
  - api
- active columnn format: {!}
- header
  - from input (match results table widths)
  - Change StringorVec to StringAndVec
- multi format: {+}
- column change propogates to pickerquery
- dynamically adjusting column hide/filtering
  - column: column hide should be external, not on the column object
  - {_} to join together all visible column outputs
  - {+}
  - {!} current column
- configurable active and passive column colors
- benchmarks (what kinds of speed matter?)
- crossterm cannot read cursor info when piped, maybe we can be smarter about the minimum height by comparing with terminal size and rows moved up. Also, may need to clear artifacts.
- Add support for nucleo::Pattern in the matcher config
- Adaptable percentage (higher on smaller)
### Low priority

- a scroll action could dispatch between preview and results
- Should event handlers return an Option<Result<??>> to allow exiting the loop? if a use case comes up might be worth changing
- currently we choose not to have action handlers, only picker-event, is there a case for adding them?
- handler effects for better customizability
- refactor to better fit components into specific ratatui roles
- sometimes preview leaks, better autorefresh?
- (automatic) horizontal scrolling of results
- Should payloads be wrapped by formatstring
- interrupts may want the payload seperate from the enum


