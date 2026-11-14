# English dictionary

An offline English dictionary built from Wiktionary data. Search by spelling,
inflection, synonym, or etymology and read the definitions in the preview
without leaving the terminal.

## Usage

```sh
mm -o dict/en
mm -o dict/en -- word
```

The second form prefills the query with `word`.

Requirements: `zstd` at runtime. The build script also needs `python3`; use
`curl` or a browser to fetch the source data.

The shipped `english.zst` is already built. To regenerate it, download the
English JSONL from [kaikki.org](https://kaikki.org/dictionary/English/) and run
the extractor from this directory:

```sh
curl -L -o english.jsonl \
  https://kaikki.org/dictionary/English/kaikki.org-dictionary-English.jsonl
python3 extract_dictionary.py -c synonyms -c etymology_text -z 19 \
  -o english.zst english.jsonl
```

The source is large (several GB); the extractor streams it and writes a
tab-separated, NUL-delimited table. The preset decompresses the `.zst` on
startup, so the dictionary remains usable without network access.

Each row is one word and part of speech; multiple senses appear on separate
lines in the definition. Synonyms and etymology are searchable and the preview
shows the full entry with both.

Accepting a row prints the full tab-separated entry, so the preset also works as
a fuzzy word lookup in a pipeline. The data file is selected through `ACTIVE_DICT`
(`english` by default); to add another language, place `<name>.zst` beside
`english.zst` and set `ACTIVE_DICT` to that name.
