Hi all, been working on this for a while. I love fzf, but I wanted to a more robust way to use it in my own applications than what fzf's command-line interface provides and Skim wasn't quite what I was looking for. I'd say it's close to feature-parity with fzf, in addition to being toml-configurable, and supporting a unique command-line syntax (which in my opinion is quite nice -- especially when binding shell-scripts where escaping special characters can get quite tricky, I'd be curious to know what you feel about it!), as well as a couple of features that fzf doesn't have, such as better support for cycling between multiple preview panes and support for priority-aware result sorting (i.e.: determining an item's resulting rank based on the incoming rank as well as similarity to the query: useful for something like frecency search).

I know that fzf is an entrenched tool (and for good reason), but personally, I believe matchmaker, being comparable in *most* aspects, offers a few wins that make it a compelling alternative. One of my hopes is that the robust support for configuration enables a more robust method of developing and sharing useful fzf-like command-line interfaces for everything from git to docker to file navigation -- just copy a couple lines to your shell startup, or a single script to your PATH to get a full application with *your* keybinds, *your* preferred UI, and *your* custom actions.

But my main motive for this project has always been using it as a library: if you like matchmaker, keep your eyes peeled as I have a few interesting TUIs I have built using it lined up for release in the coming weeks :)

Future goals include reaching full feature-parity with fzf, enhanced multi-column support (many possibilities here: editing, styles, output etc.), and performance improvements (a very far off goal would be for it to be able to handle something like the 1-billion-row challenge). There are a few points I have noticed where fzf is superior:

- fzf seems to be a little better at cold starts: this is due to a difference of between the custom fzf matching engine and nucleo -- the matching engine in Rust that matchmaker uses. I'm unlikely to change the *algorithm* used in my nucleo fork, so if that matters to you, fzf is probably a better bet.
- fzf has some features like tracking the current item through query changes or displaying all results -- these will eventually be implemented but are low priority.
- Matchmaker supports similar system for event-triggered binds, and dynamic rebinding, but does not yet support fzf's --transform feature, which can trigger configuration changes based the output of shell scripts -- this is on the cards and will probably implemented in a different way. More importantly, I haven't tested this system too much myself, preferring to write more complicated logic using the library directly so I can't vouch for which approach is better.

This has been a solo project so far, but contributions are very welcome! Anything from sample configurations, to documentation, feature suggestions, bug reports, even just your opinions on it will be very much appreciated.


# Random AI generated FAQ

## How does the current mode affect how semantic triggers (bind aliases) are resolved?

In **Matchmaker**, the interaction between **modes** and **semantic triggers** (prefixed with `@`) follows the same hierarchical resolution logic as physical keybinds. Here is a summary of how the current mode affects their resolution:

### 1. Scoped Lookup (Namespace Partitioning)
Semantic triggers are stored in the same bind map as keys and events. When a semantic action (e.g., `@accept`) is executed, the system performs a lookup for a corresponding trigger. The current application mode acts as the primary namespace:
*   **Mode-Specific:** Matchmaker first looks for `current_mode^^@trigger_name`.
*   **Global Fallback:** If no mode-specific bind is found, it falls back to the global `@trigger_name` definition.

### 2. Resolution Context
The resolution is determined by the **active mode at the time of activation**, not necessarily the mode where the keybind was originally defined.
*   If you have a global bind `ctrl-a = "@foo"`, and you press `ctrl-a` while in `vim` mode, the system will look for `vim^^@foo` first, even though the `ctrl-a` bind itself is global.
*   This allows you to define "abstract" behaviors for keys that automatically adapt to the current mode.

### 3. "Interface" Pattern
This mechanism allows semantic triggers to act as an **interface** that different modes "implement" differently:
```toml
[binds]
# Global alias usage
"enter" = "@accept"

# Mode-specific alias implementations
"@accept"       = "Accept"             # Default behavior
"insert^^@accept" = ["Print(msg)", "Accept"] # Behavior in 'insert' mode
```
In this example, the `enter` key doesn't need to be rebound for every mode; it simply points to `@accept`, which resolves to the appropriate logic based on the current mode.

### 4. Static vs. Dynamic Resolution
*   **Static (Startup):** At application start, Matchmaker runs a `resolve_semantics` pass. This flattens aliases where possible to improve performance, but it respects the mode hierarchy (e.g., a bind defined in `vim^^` will be resolved using `vim^^` semantic aliases if they exist).
*   **Dynamic (Runtime):** Because semantic triggers can be rebound at runtime using the `Bind(@alias = ...)` action, any physical key pointing to that alias will immediately reflect the new behavior. If a bind is made to a mode-specific alias (e.g. `Bind(vim^^@foo = ...)`), it only affects the resolution when the application is in that mode.

### Summary Table
| Activation Mode | Physical Bind | Semantic Alias Target | Resulting Action |
| :--- | :--- | :--- | :--- |
| `default` | `ctrl-x = "@foo"` | `@foo = "A"` | Executes **"A"** |
| `vim` | `ctrl-x = "@foo"` | `@foo = "A"`, `vim^^@foo = "B"` | Executes **"B"** |
| `vim` | `vim^^ctrl-x = "@foo"` | `@foo = "A"`, `vim^^@foo = "B"` | Executes **"B"** |
| `emacs` | `ctrl-x = "@foo"` | `@foo = "A"`, `vim^^@foo = "B"` | Executes **"A"** (Fallback) |
