# System packages

Browse installed and repository packages, inspect metadata, owned files, and
reverse dependencies, open project homepages, and install or remove packages
through your distro's package manager.

## Usage

```sh
mm -o packages/pacman
mm -o packages/apt
mm -o packages/dnf
```

Requirements: `sudo`, `xdg-open` for `@open`, and optionally `bat` for
syntax-highlighted package metadata previews.

Shared binds across all package pickers: `<enter>` installs the selected
package(s), `@rm` prompts before removing them, `@open` opens the package's
homepage URL, and `@refresh` (`F5` / `alt-u`) syncs the package database.

## Presets

### `pacman`

Browse and manage Arch Linux packages.

Requirements: `pacman` (plus `paru` or `yay` when installed, and optionally
`pactree` / `paccache` from `pacman-contrib`).

Cycles across three tabs with `@reload` and `@reload_2`: explicitly installed
(`pacman -Qe`), all installed (`pacman -Q`), and all sync repository packages
(`pacman -Sl`). Previews show `pacman -Qi`/`-Si` metadata, `pacman -Ql`/`-Fl`
file lists, and reverse dependencies via `pactree -r`. `<enter>` installs via
`paru`/`yay` if available or `sudo pacman -S --needed`, `@rm` runs
`sudo pacman -Rns` after confirmation, and `ctrl-x` (`@clean`) prunes the
package cache.

### `apt`

Browse and manage Debian/Ubuntu packages.

Requirements: `dpkg-query`, `apt-cache`, `dpkg`, and `apt-get`.

Cycles between installed packages (`dpkg-query`) and all known packages
(`apt-cache pkgnames`). Previews show `apt-cache show`, `dpkg -L`, and
`apt-cache rdepends --installed`. `<enter>` runs `sudo apt-get install`;
`@rm` prompts before running `sudo apt-get remove`.

### `dnf`

Browse and manage Fedora/RHEL packages.

Requirements: `rpm` and `dnf`.

Cycles between installed RPMs (`rpm -qa`) and available repository packages
(`dnf list --available`). Previews show `rpm -qi`/`dnf info`, `rpm -ql`, and
installed reverse dependencies via `dnf repoquery --whatrequires`. `<enter>`
runs `sudo dnf install`; `@rm` prompts before running `sudo dnf remove`.
