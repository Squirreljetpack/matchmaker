## TODO
- sane defaults for ui
- color theme, match_fg configuration
- Examples:
    - query change
    - frecency
    - api
- header/footer/bg
- active columnn format: {!}
- multi format: {+}
- column change propogates to pickerquery
- dynamically adjusting column hide/filtering
    - column: column hide should be external, not on the column object
    - {_} to join together all visible column outputs
    - {+}
    - {!} current column
- configurable active and passive column colors
- ensure uniform fg/bg config on widgets


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