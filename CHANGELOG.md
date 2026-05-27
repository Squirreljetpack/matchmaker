## [0.0.38] - 2026-05-27

### 🐛 Bug Fixes

- Rendering bug

### 💼 Other

- cursor_next/prev now returns whether they caused a wraparound

### ⚙️ Miscellaneous Tasks

- Update release

## [0.0.37] - 2026-05-27

### ⚙️ Miscellaneous Tasks

- Update packaging naming

## [0.0.36] - 2026-05-27

### 🚀 Features

- Update win config

### ⚙️ Miscellaneous Tasks

- Setup cargo dist

## [0.0.35] - 2026-05-26

### 🚀 Features

- Adjust default colors for better visibility
- ExecuteAsync

### ⚙️ Miscellaneous Tasks

- Add gif

## [0.0.34] - 2026-05-26

### 🚀 Features

- Defaults for PAGER and EDITOR

### 🐛 Bug Fixes

- Bugfixes and improvements

## [0.0.33] - 2026-05-25

### 🚀 Features

- Refactor directory to use EnvValue, docs update
- Preview resizing (drag, expand, shrink)

### ⚙️ Miscellaneous Tasks

- Update screenshots

## [0.0.32] - 2026-05-25

### 🚀 Features

- Prefix_styles

### 🐛 Bug Fixes

- Preset tweaks
- fix BecomeSilent to correctly exit

## [0.0.32] - 2026-05-25

### 🚀 Features

- results.prefix_styles

### 🐛 Bug Fixes

- Preset tweaks
- fix BecomeSilent incorrect exit

## [0.0.31] - 2026-05-25

### 🚀 Features

- Doc update

### ⚙️ Miscellaneous Tasks

- Dep updates, preset tweaks

## [0.0.30] - 2026-05-25

### 🚀 Features

- Git-grep preset + more previewer configuration settings
- Support disabling of preview layout
- Git presets
- Preset downloading
- Better no_match/empty semantics
- Ssh, ps presets
- PushHeader, command.directory, improved ssh presets
- Env config, various improvements
- Improvements to preview tail, presets, and others
- Doc updates
- Trigger mode
- Bug fixes
- Doc updates, read MM_INDEX from env
- Config inheritance
- RunPreview action
- Improve help display
- Source envs for initial command
- Kube presets

### 🐛 Bug Fixes

- Various bug fixes and improvements
- Bugfixes
- Incorrect COLUMNS for preview
- Doc update
- More flexible download
- Click indexing
- Execute display + improved presets

### ⚙️ Miscellaneous Tasks

- Update ci
## [0.0.29] - 2026-05-18
## [0.0.29] - 2026-05-18

### 🚀 Features

- Clickable headers and status
- optimize mem footprint
- named overrides (`-o`)
- configurable previewer shell executor

## [0.0.27] - 2026-05-17

### 🚀 Features

- Smoother preview switching

## [0.0.26] - 2026-05-17

### 🚀 Features

- Preliminary windows support windows

### 🐛 Bug Fixes

- Correctly clear screen on exit for all layouts
- Autoscroll bugs
- Autoscroll oob panic

## [0.0.25] - 2026-05-15

### 🚀 Features

- Enable experimental features (i.e. sort controls) for github build
- Optional colors

### 🐛 Bug Fixes

- Don't use wildcard versions

### ⚙️ Miscellaneous Tasks

- Add justfile task runner

## [0.0.24] - 2026-03-24

### 🚀 Features

- Add command_input_separator specifically for splitting only non-piped input

### 🐛 Bug Fixes

- fix broken default config.toml splitting on null separator even for piped input

## [0.0.23] - 2026-03-22

### 🚀 Features

- Max_height
- Refactor styles to StyleSetting
- mimalloc

### 🐛 Bug Fixes

- Previewchange now emits correctly
- fixed broken config.toml

## [0.0.22] - 2026-03-18

- deps update
- refactor

## [0.0.21] - 2026-03-17

### 🚀 Features

- --override (layered configs)
- finish implementing header wrapping config option

## [0.0.20] - 2026-03-15

### 🐛 Bug Fixes

- Invisible columns

## [0.0.19] - 2026-03-14

### 🚀 Features

- reverted template braces [] -> {} for fzf compatibility
- improved docs

## [0.0.18] - 2026-03-13

### 🚀 Features

- moved autoscroll options to results.autoscroll
- autoscroll.end (--keep-right) in fzf.

## [0.0.17] - 2026-03-13

### 🚀 Features

- Rename alias prefix :: -> @.
- Update configs to work with newer syntax.

## [0.0.16] - 2026-03-13

### 🚀 Features

- PageUp -> HalfPageUp for more flexibility

### 🐛 Bug Fixes

- Fix panic in PreviewLayout
- Fix bug causing width sizer to not run

## [0.0.14] - 2026-03-11

- Regex capture groups
- `--doc` to display comprehensive help
- Improve (finalized) width sizing and autoscrolling
- Improved rg example and column switching

## [0.0.13] - 2026-03-11

### 🚀 Features

- Span template shrinkers, doc updates
- Reworked semantic triggers now behave like action aliases
- new actions: Transform, PrintKey, Store
- new example: ripgrep (in options.md)
- cli values now split on ||| instead of nesting level
- support StatusLine template in SetPrompt

## [0.0.12] - 2026-03-09

### 🚀 Features

- Cleaner help display
- Column styles
- Finalize templating
- `start.default_column` and `start.additional_commands`
- ExecuteSilent action
- various bugfixes and documentation

### Performance

- Streamline AppendOnly (preview synchronization) using arc-swap

## [0.0.10] - 2026-03-07

### 💼 Other

- fix cli parsing regressions

## [0.0.9] - 2026-03-07

### 🚀 Features

- Auto-scroll to first match index
- Hscroll
- Semantic aliases in keybinds
- Previewer pausing

### 💼 Other

- matchmaker-partial: support attr(clear) to clear all field attributes.
- various bugfixes

### 🚜 Refactor

- Switch to hashmaps for binds + value sort for display

## [0.0.8] - 2026-02-24

### 🚀 Features

- New actions
- dynamic rebinding
- --last-key now displays the last recorded key
- support --no-multi
- support various toggle/set actions (filtering, sorting, header and more).
- Enhance status line styling
- various bugfixes
- per-preview-layout borders
- hidden columns
- bugfixes
- Richer status line (support template and styling)

## [0.0.7] - 2026-02-22

### 🚀 Features

- matchmaker-partial: support recursive set in collections
- matchmaker-cli: support direct override of preview command (alias: px)
- matchmaker-cli: new aliases: see options.md

### 🚜 Refactor

- Move start and exit configs out from under MatcherConfig to top level

## [0.0.6] - 2026-02-22

### 🚀 Features

- Status template

### 🚜 Refactor

- Lints

## [0.0.4] - 2026-02-19

- Bugfix and documentation updates
- Align version cli and library versions

## [0.0.2] - 2026-02-18

- Various bugfixes and improvements
- New configuration options:
  - PreviewScrollSetting
  - print_template

## [0.0.1] - 2026-02-16

- Re-release as workspace crates.
