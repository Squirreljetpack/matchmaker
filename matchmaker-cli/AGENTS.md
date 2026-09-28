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

## Config

- In **debug** builds `default_config_path()` resolves to
  `~/.config/matchmaker/dev.toml` (config_dir_impl uses `$MATCHMAKER_CONFIG_DIR`
  → `$HOME/.config/matchmaker`), and a debug `mm` run **without** `--config`
  auto-writes `assets/dev.toml` there. This shadows the embedded config.toml:
  config dumps and parse checks silently test dev.toml (which has no lua binds).
  Always pass `--config <path>` explicitly when inspecting effective config.

## Presets

- See `assets/plugins/SKILL.md`
