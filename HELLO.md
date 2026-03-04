Hi all, been working on this for a while. Big fan of fzf, but I wanted to a more robust way to use it in my own applications, and Skim wasn't quite what I was looking for. Well, the natural progression was to turn it into a cli, and so was born matchmaker. I'd say it's close to feature-parity with fzf, in addition to being toml-configurable, and supporting a unique command-line syntax (which in my opinion is quite nice -- especially when binding shell-scripts where escaping special characters can get quite tricky, I'd be curious to know what you feel about it!), as well as a couple of features that fzf doesn't have, such as better support for cycling between multiple preview panes and support for priority-aware result sorting (i.e.: determining an item's resulting rank based on the incoming rank as well as similarity to the query: useful for something like frecency search).

Future goals include reaching full feature-parity with fzf, enhanced multi-column support (many possibilities here: editing, styles, output etc.), and performance improvements (a very far off goal would be for it to be able to handle something like the 1-billion-row challenge). There are a few points I have noticed where fzf is superior:

- fzf seems to be a little better at cold starts: this is due to a difference of between the custom fzf matching engine and nucleo -- the matching engine in Rust that matchmaker uses. I'm unlikely to change the _algorithm_ used in my nucleo fork, so if that matters to you, fzf is probably a better bet.
- fzf has some features like tracking the current item through query changes or displaying all results -- these will eventually be implemented but are low priority.
- Matchmaker supports similar system for event-triggered binds, and dynamic rebinding, but does not yet support fzf's --transform feature, which can trigger configuration changes based the output of shell scripts -- this is on the cards and will probably implemented in a different way. More importantly, I haven't tested this system too much myself, preferring to write more complicated logic using the library directly so I can't vouch for which approach is better.

I know that fzf is an entrenched tool (and for good reason), but personally, I believe matchmaker, being comparable in _most_ aspects, offers a few wins that make it a compelling alternative. One of my hopes is that the robust support for configuration enables a more robust method of developing and sharing useful fzf-like command-line interfaces for everything from git to docker to file navigation -- just copy a couple lines to your shell startup, or a single script to your PATH to get a full application with _your_ keybinds, _your_ preferred UI, and _your_ custom actions.

Using the library, I have built a few more cli applications that I find quite useful with this that I will be releasing in the following weeks, so keep your eyes peeled if you like this one :).

This has been a solo project so far, but contributions are very welcome! Anything from sample configurations, to documentation, feature suggestions, bug reports, even just your opinions on it will be very much appreciated.

Here is an example configuration file to give you a sense of what matchmaker supports. Of course, there's nothing stopping you from just trying it!

```toml
[tui]
percentage = 60
min = 10
max = 120

[ui]
tick_rate = 60

[input]

[results]
wrap = true

[previewer]
try_lossy = false

[preview]
show = true
wrap = true

[[preview.layout]]
command = "fs :tool lessfilter preview {}"
side = "right"
percentage = 40
min = 30
max = 120

[[preview.layout]]
side = "top"
percentage = 50
min = 5
max = 50

[binds]
# UI
"ctrl-c" = "Quit"
"esc" = "Quit"
"enter" = "Accept"
"ctrl-a" = "CycleAll"
"tab" = ["Toggle", "Down"]
"shift-backtab" = ["Toggle", "Up"]

# Edit
"right" = "ForwardChar"
"left" = "BackwardChar"
"ctrl-right" = "ForwardWord"
"ctrl-left" = "BackwardWord"
"backspace" = "DeleteChar"
"ctrl-h" = "DeleteWord"
"ctrl-u" = "Cancel"
"alt-a" = "QueryPos(0)"

# Navigation
"up" = "Up"
"down" = "Down"
"pageup" = "Up(10)"
"pagedown" = "Down(10)"
"shift-up" = "PreviewUp"
"shift-down" = "PreviewDown"
"ctrl-shift-up" = "PreviewHalfPageUp"
"ctrl-shift-down" = "PreviewHalfPageDown"

# Preview
"scrollup" = "Up"
"scrolldown" = "Down"
"shift+scrollup" = "PreviewUp"
"shift+scrolldown" = "PreviewDown"
"?" = "SwitchPreview"
"shift-?" = "SwitchPreview"
"alt-h" = "Help"
"alt-1" = "Pos(0)"

# Programmable
"ctrl-l" = "Execute(eval $FZF_PREVIEW_COMMAND | $PAGER)"
"ctrl-o" = "Execute($EDITOR {})"
"alt-o" = "Become($EDITOR {})"
"ctrl-r" = "Reload(find . -type f)"

[matcher]
select_1 = false

[start]
command = "fd --strip-cwd-prefix"
```
