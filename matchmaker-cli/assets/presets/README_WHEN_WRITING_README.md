# Preset READMEs

This file is the contributor guide for the shipped preset collection. Each
folder preset has its own `README.md`, which is its help page: Matchmaker pages
the file from `@help` (`alt-h`) while the preset is active, and GitHub uses it as
the folder's landing page.

Write for two readers:

- someone deciding whether the preset fits their workflow;
- someone already using it who needs to know where rows come from, what
  accepting a row does, and which side effects to expect.

The effective bind map is already shown by `@help`, so a README does not need to
reproduce it. Describe the workflow first; document individual actions only when
they are non-obvious or destructive.

## Required structure

Every folder preset README has:

1. One `#` title.
2. A one-paragraph summary.
3. A `## Usage` section showing how to launch it. In a multi-preset folder, list
   every preset's invocation here, along with requirements and one-time setup.
4. A description of how the preset works once launched. This is the body of the
   README, not a section of its own.

Single-preset folders use the preset name as the title. Multi-preset folders use
the collection name as the title, list the presets under `## Presets`, and give
each preset an `###` section. In a multi-preset folder, the invocations belong in
the folder-level `## Usage`; each `###` describes what the preset does once it is
running. Use `##` for top-level sections and do not skip heading levels.

Scope each statement to the section that owns it. Anything that applies across
presets — shared requirements, shared setup, preview conventions, a ring order
— belongs in the folder-level sections and must not be buried inside one
preset's `###`. Conversely, how a preset produces rows, what accepting a row
does, and its own `@next` / `@prev` transitions are specific to that preset and
belong in its `###`.

### Summary

Say what the preset searches or manages and when to use it. Name the input
source (a command, a file, a running service) and the intended outcome. Keep it
to one to three sentences; put platform limits and prerequisites in the usage
section instead of burying them here.

### Usage

`## Usage` is its own section. Show the canonical invocation with the shipped
path, plus any arguments or environment variables the user can pass. In a
multi-preset folder, list one invocation per preset, with argument variants
alongside. For example:

```sh
mm -o zellij/session
mm -o zellij/layout
mm -o zellij/layout -- my-layout
```

Prefer one invocation that works from a fresh download. Do not describe the
internals of the populating command here; that belongs in the description.

Requirements and one-time setup belong in this section, after the invocation:
external executables, OS versions, permissions, environment variables, and
config locations. Include copy-pasteable setup commands, and state when a
dependency is optional and what degrades without it.

### How it works

Describe the workflow once the preset is running; the invocation stays in the
usage section. This is the part `@help` cannot infer from the configuration.
Cover the points that apply:

- **Input**: the command, file, or service that produces rows, and how
  arguments reach it.
- **Rows**: what one row represents and what `<enter>` / `@accept` returns or
  opens. Do not restate columns whose headers already say it; describe a column
  only when its name alone is not enough, such as a terse status code or a
  computed field. Hidden columns and the default column usually need no mention.
- **State**: files, environment variables, caches, or configuration the preset
  reads or writes, and whether writes are atomic and reversible.
- **Side effects**: processes started, files deleted, settings changed, and
  which actions ask for confirmation.
- **Lifecycle**: reloads, modes, empty states, and that preset's own `@next` /
  `@prev` transitions to sibling presets. A ring order that spans the folder is
  a cross-preset fact and belongs at folder level.

If a row's meaning is not obvious from the UI, say so. Do not make the reader
open the TOML or a helper script to understand the workflow, and omit anything
the UI already makes clear.

## Multi-preset folders

List the pickers under `## Presets`, in the order they appear in the picker or
ring, with one `###` section each. Give each one a one-line purpose and the
details specific to that preset. Anything shared by several presets is a
folder-level fact: state it once above `## Presets` instead of repeating it or
describing it from inside one preset. Preset bodies start after launch; the
invocations are collected in `## Usage`.

````markdown
## Usage

```sh
mm -o zellij/session
mm -o zellij/current
mm -o zellij/layout
mm -o zellij/other
```

## Presets

### `session`

Switch between Zellij sessions.

Every session is listed, running or exited...
````

Avoid repeating the same bind list under every preset.

## Actions are optional

`@help` already shows the effective bind map, including aliases and their
descriptions. Do not duplicate it. When actions are worth documenting:

- prefer workflow descriptions over key tables;
- call out destructive or irreversible actions and their confirmations;
- use one format for keys and aliases, such as `` `@alias` (`key`) ``;
- omit defaults such as `@accept`, `@open`, `@copy`, and `@reload` unless the
  preset changes their behavior.

## Screenshots

Put images in the preset's `.README.assets/` directory and link them with a
relative path:

```markdown
![Zellij session picker](./.README.assets/session-picker.png)
```

- Use descriptive kebab-case filenames (`session-picker.png`), not camera or
  timestamp names (`image-20260810081144100.png`).
- Give every image useful alt text.
- One representative screenshot per preset is enough; prefer the main view over
  every preview layout.
- Keep images local to the repository. Do not link to a personal branch, a
  GitHub blob URL, or an external host.
- `mm --download` fetches top-level preset files and skips images, so the README
  must remain useful without them.

For a fixed display width, the repository uses HTML image tags:

```html
<img
  src="./.README.assets/session-picker.png"
  alt="Zellij session picker"
  style="width:416px;"
/>
```

## Formatting

- Use ATX headings (`#`, `##`, `###`) in sentence case.
- Put identifiers in backticks: preset names, paths, commands, environment
  variables, keys, and aliases.
- Fence code with a language (`sh`, `toml`, `markdown`, `html`, `text`).
- Use `-` for unordered lists and keep list items to one idea.
- Wrap prose near 80 columns; do not wrap code spans, links, or image tags.
- Use a table only for genuinely tabular reference data, such as the theme
  catalogue.
- Do not add badges, HTML layouts, or decorative rules; the file is also read as
  plain text in a pager.

## Preset file conventions

This section covers the TOML beside the README. See
[`../plugins/SKILL.md`](../plugins/SKILL.md) for the full authoring guide.

Recommended definition order:

1. `source` (only when inheriting)
2. `[start]` and `[envs]`
3. `[columns]` and `[matcher]`
4. UI sections: `[tui]`, `[query]`, `[results]`, `[header]`, `[footer]`, `[exit]`
5. `[previewer]` and `[preview]`
6. `[binds]`, with concrete binds first and semantic aliases after

Separate groups with a comment rule:

```toml
[matcher]
sort = 40

# --- UI ---

[tui]
percentage = 100
```

## Distribution

`mm --download <folder>` downloads a folder's top-level files;
`mm --download <file>.toml` downloads a single preset. `mm --download` with no
argument downloads everything, and `mm -o presets` opens the preset picker.

Executability after download is heuristic, not git-based: files with a valid
shebang or a known binary signature get the executable bit. Images and
subdirectories are not downloaded.

## Before committing

- [ ] The README starts with a single `#` title and a short summary.
- [ ] Every invocation runs from a fresh download and is listed in the usage
      section, together with requirements and one-time setup.
- [ ] The description explains input, row meaning, accept behavior, and side
      effects once the preset is running, without restating obvious columns.
- [ ] Shared requirements and cross-preset behavior stay at folder level, and
      each preset's `###` covers only that preset.
- [ ] Referenced commands, helper scripts, and paths exist in the folder.
- [ ] Images live in `.README.assets/`, have descriptive names and alt text, and
      are not the only explanation of the workflow.
- [ ] The file still reads well as plain text in `$PAGER`.
- [ ] `mm -o <preset>` and `@help` (`alt-h`) work from the documented state.
