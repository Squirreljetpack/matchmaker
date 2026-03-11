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
- `matcher.start.default_column` and `matcher.start.additional_commands`
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
