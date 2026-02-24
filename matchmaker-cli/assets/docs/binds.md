# Key Binds

Key Presses, Mouse Input, and Events can all be bound to actions.

## CLI

Actions can be sequences:

- `key=Action`: e.g., `ctrl-c=Quit`.
- `key=[Action1, Action2]`: Sequence of actions.

```
# Bind a single action: 
mm b 'key=action'.
# Bind an action sequence:
mm b.key 'action1 action2'
```

## Triggers

- **Keyboard**: Standard key names (`enter`, `esc`, `up`, `down`, `left`, `right`, `tab`, `backspace`, etc.) and combinations (`ctrl-a`, `alt-enter`, `shift-up`).
- **Mouse**: `left`, `middle`, `right`, `scrollup`, `scrolldown`, `scrollleft`, `scrollright`. Modifiers can be added: `ctrl+left`, `alt+scrollup`.

## Actions

- **Selection**: `Select`, `Deselect`, `Toggle`, `CycleAll`, `ClearSelections`, `Accept`, `Quit(code)`.
- **Navigation**: `Up(n)`, `Down(n)`, `Pos(pos)`, `ForwardChar`, `BackwardChar`, `ForwardWord`, `BackwardWord`, `QueryPos(pos)`.
- **Preview**: `CyclePreview`, `Preview(cmd)`, `Help(section)`, `SetPreview(idx)`, `SwitchPreview(idx)`, `PreviewUp(n)`, `PreviewDown(n)`, `PreviewHalfPageUp`, `PreviewHalfPageDown`, `ToggleWrapPreview`.
- **Input/Edit**: `SetQuery(str)`, `Cancel` (clear query), `DeleteChar`, `DeleteWord`, `DeleteLineStart`, `DeleteLineEnd`, `HistoryUp`, `HistoryDown`, `ToggleWrap`.
- **UI/Display**: `SetHeader(str)`, `SetFooter(str)`, `SetPrompt(str)`, `Column(idx)`, `CycleColumn`, `Redraw`, `Overlay(idx)`.
- **System**: `Execute(cmd)`, `Become(cmd)` (replace matchmaker with command), `Reload(cmd)`, `Print(str)`.

## Source

[Basic actions](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-lib/src/action.rs), [Extended](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/src/action.rs),
