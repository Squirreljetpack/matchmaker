# matchmaker-cli / AGENTS.md

Guidance for working on the matchmaker-cli crate (the `mm` binary). These are
correct as of the lua-support work; re-verify before relying on them.

## Crate shape & testing

- The crate is **bin-only** (no lib target); there is no `--lib` test target.
  Run CLI tests with `cargo test -p matchmaker-cli --bin mm`, and the whole
  workspace with `cargo test --workspace` (matchmaker-cli + matchmaker-lib +
  matchmaker-partial).
- config assets embed through `include_str!` at `matchmaker-cli/assets/`
  (config.toml on unix, win.config.toml on windows, dev.toml in debug builds).
  A rebuild is triggered whenever those files change.

## Config & effective-config testing gotchas

- In **debug** builds `default_config_path()` resolves to
  `~/.config/matchmaker/dev.toml` (config_dir_impl uses `$MATCHMAKER_CONFIG_DIR`
  → `$HOME/.config/matchmaker`), and a debug `mm` run **without** `--config`
  auto-writes `assets/dev.toml` there. This shadows the embedded config.toml:
  config dumps and parse checks silently test dev.toml (which has no lua binds).
  Always pass `--config <path>` explicitly when inspecting effective config.
- `--dump-config` behaves differently by stdout: with a TTY it writes the
  default config to the config **file** path and exits (nothing on stdout);
  only with **piped** stdout does it serialize the effective config to stdout.

## Binds & mode tags

- Mode-filtered triggers use the on-disk/formatter convention
  `<mode_filter>^^<trigger>` (e.g. `0,1^^@accept`, `lua^^@open`) — mode first.
  `Trigger::from_str` splits on `^^`; `Display`/serialize must emit the same
  order (a past bug had them reversed, breaking config round-trips, `--dump-config`
  and runtime re-binds once the first `^^` binds landed).
- A semantic trigger must only have **one** non-empty-mode alias: `resolve_alias`
  iterates the bind map in HashMap order and returns the first non-empty-mode
  match, so two competing mode-filtered aliases (e.g. `0,1^^@accept` next to
  `lua^^@accept`) pick a nondeterministic winner when both modes match.
  Bindings with an empty mode filter always act as the deterministic fallback.
- The CLI builds the mode tag stack (matchmaker-lib `MODE`) from TTY detection
  or the `start.mode` override, appending `win` under `#[cfg(windows)]` and
  `lua` under `#[cfg(feature = "mlua")]`, comma-joined. `set_mode` splits on
  commas and filters empty segments, so appending tags is safe even for a
  fully-piped (empty) base. The `lua` tag is what activates `lua^^` bind
  variants; with the mlua feature disabled no `lua` tag exists and those binds
  are simply never active.

## Lua support

- `#!lua`-prefixed payloads (any whitespace after the prefix, not just a
  single space) run through mlua; `@*.lua` argument files are executed as lua,
  with args split preserving single quotes and file paths resolved relative to
  the parent of `MM_OVERRIDE`. Both are fresh per-run VMs.
- Lua payloads read matchmaker state from a `state` global table
  (query/mode/raw/current{1..n,named}/selected/position/total/matched/
  selected_count/active/args) — inline payloads get **no** `...` vararg.
  The `env` global holds only `FZF_*`/`MM_*` vars plus configured `[envs]`
  (make_env_vars), **not** the process environment: shell payloads can use
  `$EDITOR` via spawn inheritance, but lua payloads must use stock
  `os.getenv("EDITOR")` for process env vars.
- Lua support is return-value only: stdout is never captured, `os.exit` does
  not terminate the host, and shell-safe exec uses lua 5.4 `%q` quoting
  (no quote helper). This is the documented contract in assets/docs/lua.md
  (`mm --doc lua`).
- The mlua dependency is optional behind the default `mlua` feature; every
  feature-gated path must keep `--no-default-features` builds warning-free
  (targeted `#[cfg_attr(...)]` allows on classify/run_value stubs, etc.).
