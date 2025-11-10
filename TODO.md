## TODO
- Examples:
    - query change
    - frecency
    - api
- header/footer/bg
- Should i rename types/traits named Picker* to MM* or Match* for consistency with crate name?

### Low priority
- a scroll action could dispatch between preview and results
- Should event handlers return an Option<Result<??>> to allow exiting the loop? if a use case comes up might be worth changing
- currently we choose not to have action handlers, only picker-event, is there a case for adding them?
- handler effects for better customizability
- refactor to better fit components into specific ratatui roles
- sometimes preview leaks, better autorefresh?
- (automatic) horizontal scrolling of results