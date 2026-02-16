# m&m [![Crates.io](https://img.shields.io/crates/v/match-maker)](https://crates.io/crates/match-maker) [![License](https://img.shields.io/github/license/squirreljetpack/matchmaker)](https://github.com/squirreljetpack/matchmaker/blob/main/LICENSE)

Matchmaker is a fuzzy searcher, powered by nucleo and written in rust.

(pitch + fzf credit: todo)

## Features

- Matching with [nucleo](https://github.com/helix-editor/nucleo).
- Declarative configuration sourced from a [toml file](./matchmaker-cli/assets/config.toml), as well as an intuitive syntax for cli overrides.
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
> The default input and preview commands rely on fd, bat and eza. For an optimal experience, install them or update your configuration.

## Configuration

To begin, you can dump the default configuration to a file:

```sh
matchmaker --dump-config
```

The default locations are in order:

- `~/.config/matchmaker/config.toml` (If the folder exists already).
- `{PLATFORM_SPECIFIC_CONFIG_DIRECTORY}/matchmaker` (Generally the same as above when on linux)

### Keybindings

All actions must be defined in your `config.toml`.

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
