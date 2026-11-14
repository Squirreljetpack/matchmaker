# Obsidian vaults

Choose an Obsidian vault, then open it in Obsidian or jump straight into a file
picker or heading search. The preset keeps a small cache of vault paths so it
starts without walking your home directory.

## Usage

```sh
mm -o obsidian/vaults
```

Requirements: [fist](https://github.com/Squirreljetpack/fist) (`fs`), Obsidian,
and a shell. `jq` is optional and improves URI escaping in `ob-open`.

Generate the vault cache once before the first run, from this preset directory:

```sh
export OBSIDIAN_HOME="$HOME/Documents/Obsidian"
./ob-vaults -r
```

`ob-vaults` lists one directory level under `OBSIDIAN_HOME` and also includes
the content directory hardcoded at the top of the script; edit `populate_cache`
there to match your own layout. The cache lives in `.__vaults` next to the
preset (override it with `OBSIDIAN_VAULTS_FILE`) and is regenerated
automatically when missing or empty. The preset adds its own directory to
`PATH`, so the helper scripts resolve after download.

Each row is a vault directory present in the cache; missing directories are
skipped. `<enter>` opens `obsidian://open?vault=<name>` for the selected
vault(s). The `right` binding opens an `fs` browser rooted at the vault.
`@open` opens the vault path with the platform opener, `ctrl-d` reveals it in
the file manager, and `@copy` copies the path.

`ctrl-f` starts `ob-files` for the selected vaults. That picker lists markdown
files and opens them with `ob-open`, which resolves the file's vault and sends
an Obsidian URI (falling back to the default application for files outside a
vault). `ctrl-r` searches headings across the selected vaults with `fs` and
`rg`, using `ob-open` as the opener. Both actions leave the vault picker and
can be resumed with the usual back navigation.
