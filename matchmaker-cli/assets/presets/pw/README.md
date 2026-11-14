# Passwords

Browse login items from Bitwarden, 1Password, or Proton Pass and copy
passwords, usernames, or TOTP codes without printing secrets in the item list
or default previews.

## Usage

```sh
mm -o pw/bitwarden
mm -o pw/1password
mm -o pw/protonpass
mm -o pw/protonpass -- "Personal"
```

Requirements: `jq` for parsing each manager's JSON output. Sign in or unlock the
corresponding CLI session before launching a preset. The optional vault name
after `--` for `protonpass` restricts `pass-cli item list` to that vault.

- `bitwarden`: `rbw`, unlocked with `rbw unlock`.
- `1password`: `op` with an active session.
- `protonpass`: `pass-cli`, authenticated.

All pickers mask secret fields in both preview layouts and fetch secrets on
demand when copying or when explicitly revealed with `@show`.

`<enter>` copies the password and exits; `@copy` (`ctrl-y`) copies it without
quitting. Both schedule a 15-second background clipboard clear when `wl-copy`,
`pbcopy`, or `xclip` is available, and `@clear` (`ctrl-k`) clears the clipboard
immediately. `@copy_user` (`alt-u`) copies the username, `@copy_otp` (`ctrl-t`)
copies the current TOTP code, `@open` opens the entry's first URL in your
browser, and `@return` (`alt-enter`) prints the item ID.

## Presets

### `bitwarden`

Search your Bitwarden vault via `rbw`.

`ctrl-x` (`@sync`) runs `rbw sync` and reloads the list.

### `1password`

Search your 1Password items via `op`.

### `protonpass`

Search active Proton Pass login items via `pass-cli`.
