# Tabby model selector

List the models, LoRAs, and embeddings available to a
[Tabby](https://github.com/TabbyML/tabby) server, including local model
directories, and load or unload them from one picker.

## Usage

```sh
mm -o misc/tabby-select
```

Requirements: `zsh`, `curl`, `jq`, and `fd`. The preview can use `fs`, `fx`,
`ds`, and `$PAGER` when installed. Set `TABBY_API_KEY`; the picker refuses to
start without it. The server defaults to `http://localhost:5000/tabby`; set
`TS_ADDRESS` to another base address. Local files are read from
`$LLMdir` (default `~/models`), specifically `models/exl2`, `loras`, and
`embeddings`.

The start command merges local entries with the API's model, LoRA, and embedding
lists. Local entries win when a name appears on both sides. The type column
distinguishes local `model`, `lora`, and `embedding` entries from their `api-*`
counterparts.

`<enter>` loads the selection after confirmation. API entries are loaded by
name. Local entries build a request from defaults and read an optional
`tabby.json` beside the model or in the LoRA/embedding directory for overrides.
`ctrl-u` unloads a model, `alt-u` unloads an embedding, and `alt-shift-u`
unloads a LoRA.

The preview shows the local directory when `fs` is available, the API status,
and the raw response from the Tabby `/v1/model` endpoint. `alt-l` previews a
local entry's `README.md` with `$PAGER`, `f2` shows disk usage, and `?` cycles
preview layouts.
