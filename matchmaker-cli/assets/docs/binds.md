# Key Binds

Key Binds allow you to map user input (Triggers) to one or more operations (Actions).

## Triggers

A trigger is the event that activates a binding. Matchmaker supports keyboard, mouse, and semantic aliases.

### Keyboard

Standard key names and combinations are supported. Matchmaker uses a very human format:

- **Single Keys**: `enter`, `esc`, `space`, `tab`, `backspace`, `up`, `down`, `left`, `right`, `pageup`, `pagedown`, `home`, `end`, `f1` through `f12`.
- **Characters**: `a`, `b`, `c`, `1`, `2`, `3`, `?`, `/`, etc.
- **Modifiers**: `ctrl-`, `alt-`, `shift-`, `super-` (e.g., `ctrl-c`, `alt-enter`, `shift-up`).
- **Combinations**: `ctrl-alt-del`.

Get your key name with `mm --test-keys`.

**Example:**
`ctrl-s = "Select"` (Bind Ctrl+S to the Select action)

### Mouse

Mouse events can be bound with modifiers:

- **Buttons**: `left`, `middle`, `right`.
- **Scrolling**: `scrollup`, `scrolldown`, `scrollleft`, `scrollright`.
- **Modifiers**: `ctrl+left`, `alt+scrollup`, `shift+right`.

**Example:**
`alt+scrollup = "Up(5)"` (Bind Alt+ScrollUp to move the cursor up 5 lines)

### Semantic Triggers

Semantic triggers (prefixed with `@`) act as named aliases for a group of actions. They allow you to define a sequence of operations once and trigger it from multiple keys or events, or even from other actions.

**Defining a Semantic Trigger:**
You define a semantic trigger by binding it to one or more actions in your configuration:

```toml
[binds]
"@my_macro" = [
  "ExecuteSilent(echo 'Starting...')",
  "Filtering(true)",
  "SetPrompt(working> )",
]
```

**Using a Semantic Trigger:**
This trigger then becomes a valid action:

```toml
[binds]
"ctrl-x" = "@my_macro"
"Start" = "@my_macro"
```

When sharing a command-line matchmaker command, you can also define your actions using these semantic triggers, allowing consumers to use their preferred binds for similar actions across different applications.

*Note: You can also dynamically rebind semantic triggers at runtime using the `Bind` action. For an advanced example, scroll to the bottom.*

### Events

Actions can be bound to events:

#### Lifecycle

- `Start` – Triggered when the application starts.
- `Complete` – Triggered when the application is about to exit.
- `Synced` – Triggered when the matcher completes its first synchronization.
- `Resynced` – Triggered when the matcher finishes processing the current state again.

#### Input & Cursor

- `QueryChange` – Triggered whenever the input query changes.
- `CursorChange` – Triggered when the selection cursor moves.

#### Preview & Overlay

- `PreviewChange` – Triggered when the preview content updates.
- `PreviewSet` – Triggered when preview content is explicitly set.
- `OverlayChange` – Triggered when the overlay content changes.

#### Window

- `Resize` – Triggered when the terminal window is resized.
- `Refresh` – Triggered when a full UI redraw occurs.

#### Control

- `Pause` – Triggered when the system enters a paused state.
- `Resume` – Triggered when execution resumes from a paused state.

Scroll to the bottom for some examples.

---

## Actions

Actions are the operations performed when a trigger is activated.

### Selection

| Action            | Description                                                |
| ----------------- | ---------------------------------------------------------- |
| `Select`          | Add the current item to the selections.                    |
| `Deselect`        | Remove the current item from the selections.               |
| `Toggle`          | Toggle the selection state of the current item.            |
| `CycleAll`        | Toggle selection for all items in the current view.        |
| `ClearSelections` | Clear all active selections.                               |
| `Accept`          | Accept the current selection and exit.                     |
| `Quit(code)`      | Exit Matchmaker with the specified exit code (default: 1). |

### Navigation

| Action         | Description                                                          |
| -------------- | -------------------------------------------------------------------- |
| `Up(n)`        | Move selection cursor up by `n` lines (default: 1).                  |
| `Down(n)`      | Move selection cursor down by `n` lines (default: 1).                |
| `Pos(idx)`     | Move selection cursor to absolute index `idx`. `-1` for end.         |
| `HalfPageUp`   | Scroll the results list up by half the height of the results pane.   |
| `HalfPageDown` | Scroll the results list down by half the height of the results pane. |
| `HScroll(n)`   | Horizontally scroll the active column by `n`. `0` to reset.          |
| `VScroll(n)`   | Vertically scroll down the current result by `n`. `0` to reset.      |
| `ToggleWrap`   | Toggle line wrapping for the results list.                           |

### Preview

| Action                | Description                                               |
| --------------------- | --------------------------------------------------------- |
| `CyclePreview`        | Cycle through available preview layouts.                  |
| `Preview(cmd)`        | Show/hide preview using the provided shell command.       |
| `SetPreview(idx)`     | Set preview layout to index `idx`.                        |
| `SwitchPreview(idx)`  | Switch to layout `idx`, or toggle it if already active.   |
| `TogglePreviewWrap`   | Toggle line wrapping in the preview window.               |
| `PreviewUp(n)`        | Scroll the preview window up by `n` lines (default: 1).   |
| `PreviewDown(n)`      | Scroll the preview window down by `n` lines (default: 1). |
| `PreviewHalfPageUp`   | Scroll the preview up by half a page.                     |
| `PreviewHalfPageDown` | Scroll the preview down by half a page.                   |
| `Help(section)`       | Display the specified help section in the preview.        |

### Columns

| Action              | Description                                |
| ------------------- | ------------------------------------------ |
| `NextColumn`        | Move focus to the next column.             |
| `PrevColumn`        | Move focus to the previous column.         |
| `SwitchColumn(col)` | Focus column specified by name or index.   |
| `ToggleColumn(col)` | Toggle visibility of the specified column. |
| `ShowColumn(col)`   | Ensure the specified column is visible.    |

### Input & Search

| Action            | Description                                                  |
| ----------------- | ------------------------------------------------------------ |
| `ForwardChar`     | Move cursor one character forward.                           |
| `BackwardChar`    | Move cursor one character backward.                          |
| `ForwardWord`     | Move cursor one word forward.                                |
| `BackwardWord`    | Move cursor one word backward.                               |
| `DeleteChar`      | Delete the character under the cursor.                       |
| `DeleteWord`      | Delete the word before the cursor.                           |
| `DeleteLineStart` | Delete from cursor to the start of the line.                 |
| `DeleteLineEnd`   | Delete from cursor to the end of the line.                   |
| `Cancel`          | Clear the current input query.                               |
| `SetQuery(s)`     | Replace the input query with `s`.                            |
| `QueryPos(n)`     | Move the input cursor to position `n`.                       |
| `Filtering(bool)` | Toggle or set whether input filters results (default: true). |
| `CycleSort`       | Cycle through sorting stability levels.                      |

### Binds (Dynamic)

| Action                  | Description                               |
| ----------------------- | ----------------------------------------- |
| `Bind(trigger=actions)` | Define or overwrite a binding at runtime. |
| `Unbind(trigger)`       | Remove a binding.                         |
| `PushBind(t=a)`         | Append an action to an existing binding.  |
| `PopBind(t)`            | Remove the last action from a binding.    |

### UI Customization

| Action               | Description                |
| -------------------- | -------------------------- |
| `SetHeader(s)`       | Update the header content. |
| `SetFooter(s)`       | Update the footer content. |
| `SetPrompt(s)`       | Update the input prompt.   |
| `SetStatus(s)`       | Update the status line.\*  |
| `SetStyledPrompt(s)` | Update the input prompt.\* |
| `SetStyledStatus(s)` | Update the status line.\*  |

\* See --doc template

### Programmable

| Action             | Description                                                  |
| ------------------ | ------------------------------------------------------------ |
| `Execute(cmd)`     | Run a shell command (replaces TUI).                          |
| `ExecuteSilent(c)` | Run a shell command in the background.                       |
| `Become(cmd)`      | Exit Matchmaker and execute the command.                     |
| `Reload(cmd)`      | Rerun the item generation command or a new one.              |
| `ReloadNext(n)`    | Cycle through `additional_commands` defined at startup.      |
| `Transform(cmd)`   | Run command and parse its output as a stream of Actions.     |
| `Print(s)`         | Print a string to stdout on exit.                            |
| `@name`            | Execute the actions associated with semantic trigger `name`. |

### UI & Display

| Action           | Description                                |
| ---------------- | ------------------------------------------ |
| `SetHeader(str)` | Set the header text (pass empty to clear). |
| `SetFooter(str)` | Set the footer text (pass empty to clear). |
| `SetPrompt(str)` | Set the input prompt text.                 |
| `SetStatus(str)` | Set the status line template.              |

### Programmable

| Action           | Description                                    |
| ---------------- | ---------------------------------------------- |
| `Execute(cmd)`   | Run a shell command and continue.              |
| `Become(cmd)`    | Replace Matchmaker with the specified command. |
| `Reload(cmd)`    | Reload items by running the specified command. |
| `Print(str)`     | Print the specified string to stdout.          |
| `PrintKey`       | Print the activating key.                      |
| `Store(str)`     | Store a string in the state (`MM_STORE`).      |
| `Transform(cmd)` | Run command and parse output as actions.       |

### Other & Experimental

| Action            | Description                                                           |
| ----------------- | --------------------------------------------------------------------- |
| `Filtering(bool)` | Enable or disable query filtering.                                    |
| `CycleSort`       | Cycle through result sorting modes (`Full`/ `Mixed` / `None`).        |
| `ReloadNext(idx)` | Reload using the next command in `matcher.start.additional_commands`. |
| `Overlay(idx)`    | Activate the UI overlay at index `idx`.                               |
| `Redraw`          | Force a complete UI redraw.                                           |

---

## Sequences and CLI

Multiple actions can be executed in sequence by using an array:

- **TOML**: `ctrl-x = ["Cancel", "Quit"]`
- **CLI**: `mm b "ctrl-x=Cancel|||Quit"`

### CLI Overrides

When overriding binds from the command line, use the `b` (or `binds`) prefix:

```bash
# Bind a single action:
mm b 'alt-enter=Accept'

# Bind multiple actions to one key:
mm b.ctrl-s='Select|||Down'

# Some action parameters are optional
mm b.ctrl-q 'SwitchPreview'
```

#### Advanced Example: Switching between Ripgrep and MM

You can mimic `fzf`'s [ripgrep example](https://github.com/junegunn/fzf/blob/master/ADVANCED.md) as follows:

```toml
[query]
prompt_style.fg = "Red"

[start]
command = 'rg --column --line-number --no-heading --color=always --smart-case "$FZF_QUERY"'
ansi = true

[binds]
"Start" = "@enter_rg"
"@enter_rg" = [ # Reload on query change, disable reparsing, update bind
  "Filtering(false)",
  '''Bind(QueryChange = Reload)''',
  # Prompt indicator (
  '''Transform(
    [[ -n "$MM_QUERY" ]] &&
    prompt="($MM_QUERY)" ||
    prompt="rg>"

    echo "SetPrompt($prompt )"
    echo "SetQuery($MM_STORE)"
    echo "Store($MM_QUERY)"
)''',
  "Bind(@reload = @enter_mm)",
]
"@enter_mm" = [
  "Filtering(true)",
  "Unbind(QueryChange)",
  '''Transform(
	[[ -n "$MM_QUERY" ]] &&
	prompt="($MM_QUERY)" ||
	prompt="mm"

    echo "SetPrompt({blue,italic:$prompt })"
    echo "SetQuery($MM_STORE)"
    echo "Store($MM_QUERY)"
)
''',
  "Bind(@reload = @enter_rg)",
]

"ctrl-r" = "@reload"
```

This example is simplified to demonstrate the special actions `Bind`, `Store`, `Transform`, and `Semantic`. You can find the full version at https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-cli/assets/rg.toml
