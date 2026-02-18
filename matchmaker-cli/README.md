# m&m [![Crates.io](https://img.shields.io/crates/v/matchmaker-cli)](https://crates.io/crates/matchmaker-cli) [![License](https://img.shields.io/github/license/squirreljetpack/matchmaker/LICENSE)](https://github.com/squirreljetpack/matchmaker/blob/main/matchmaker-cli/LICENSE)

Matchmaker is a fuzzy searcher, powered by nucleo and written in rust.

![screen1](https://github.com/Squirreljetpack/matchmaker/blob/main/matchmaker-lib/assets/screen1.png)

## Features

- Matching with [nucleo](https://github.com/helix-editor/nucleo).
- Declarative configuration which can be sourced from a [toml file](./matchmaker-cli/assets/config.toml), or overridden using an intuitive [syntax](./matchmaker-cli/assets/docs/options.md) for specifying command line options.
- Interactive preview supports color, scrolling, wrapping, multiple layouts, and even entering into an interactive view.
- [FZF](https://github.com/junegunn/fzf)-inspired actions.
- Column support: Split input lines into multiple columns, that you can dynamically search, filter, highlight, return etc.
- Available as a rust library to use in your own code.

## Installation

```sh
cargo install matchmaker-cli
```

Pass it some items:

```sh
find . | mm
```

> [!NOTE]
> The [default](./matchmaker-cli/assets/config.toml) input and preview commands rely on fd, bat and eza. For an optimal experience, install them or update your configuration.

## Configuration

To begin, you can dump the default configuration to a file:

```sh
matchmaker --dump-config
```

The default locations are in order:

- `~/.config/matchmaker/config.toml` (If the folder exists already).
- `{PLATFORM_SPECIFIC_CONFIG_DIRECTORY}/matchmaker` (Generally the same as above when on linux)

Matchmaker options are [hierarchical](./matchmaker-lib/src/config.rs) but most categories are flattened to the top level:

```toml
[preview]
    show = true
    wrap = true
    header_lines = 3 # sticky the top 3 lines

# Full specification of (the default values of) a single layout. Multiple layouts can be specified.
[[preview.layout]]
    command    = ""
    side       = "right"
    percentage = 60
    min        = 30
    max        = 120
```

They can also be specified on the command line, where abbreviations are supported:

```sh
mm --config ~/.config/matchmaker/alternate.toml p.l "cmd=[echo {}] p=50 max=20" cmd "ls" o "'{}'"

# 1. Start mm with an alternate config, as well as with the following overrides:
# 2. List the contents of the current directory by executing `ls`
# 3. Show the current item name in the preview pane
# 4. Set a preferred percentage of 50 for the preview pane, and a maximum column width of 20 for the preview pane
# 5. Output the result wrapped in single quotes
```

### Keybindings

Actions can be defined in your `config.toml` or on the command line.

The list of currently supported actions can be found [here](./matchmaker-lib/src/action.rs) or from `mm --options`.

To get the names of keys, type `mm --test-keys`.

In addition to keys, actions can also be bound to Events and Crossterm events (check your default config for details).

## CLI

See [here](./matchmaker-cli/assets/docs/options.md) for the command-line syntax.

Matchmaker aims to achieve feature-parity with fzf (though not necessarily by the same means). If there's any specific feature that you'd like to see, open an issue!

## Library

Matchmaker can also be used as a library.

```sh
cargo add matchmaker
```

### Example

Here is how to use `Matchmaker` to select from a list of strings.

```rust
use matchmaker::nucleo::{Indexed, Worker};
use matchmaker::{MatchError, Matchmaker, Result, Selector};

#[tokio::main]
async fn main() -> Result<()> {
    let items = vec!["item1", "item2", "item3"];

    let worker = Worker::new_single_column();
    worker.append(items);
    let selector = Selector::new(Indexed::identifier);
    let mm = Matchmaker::new(worker, selector);

    match mm.pick_default().await {
        Ok(v) => {
            println!("{}", v[0]);
        }
        Err(err) => match err {
            MatchError::Abort(1) => {
                eprintln!("cancelled");
            }
            _ => {
                eprintln!("Error: {err}");
            }
        },
    }

    Ok(())
}
```

For more information, check out the [examples](./matchmaker-lib/examples/) and [Architecture.md](./matchmaker-lib/ARCHITECTURE.md)

# See also

- [junegunn/fzf](https://github.com/junegunn/fzf)
- [helix-editor/nucleo](https://github.com/helix-editor/nucleo)
- [skim-rs/skim](https://github.com/skim-rs/skim)
- [autobib/nucleo-picker](https://github.com/autobib/nucleo-picker)
- [alexpasmantier/television](https://github.com/alexpasmantier/television)
- [helix-editor/helix](https://github.com/helix-editor/helix)
- [Canop/crokey](https://github.com/Canop/crokey)
