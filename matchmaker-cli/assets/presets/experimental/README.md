# Experimental desktop controls

Pickers for the desktop services around a session: Bluetooth devices, the Niri
compositor, MPRIS media players, and PipeWire audio nodes. Each picker wraps the
service's CLI, with changes confirmed before they run.

## Usage

```sh
mm -o experimental/bluetooth
mm -o experimental/niri
mm -o experimental/playerctl
mm -o experimental/wireplumber
```

## Presets

### `bluetooth`

Connect, pair, trust, scan, and remove Bluetooth devices.

Requirements: `bluetoothctl` (BlueZ) and `timeout`.

Rows come from `bluetoothctl devices`, with state flags derived from the
Connected, Paired, and Trusted views. A second tab lists only connected devices;
`@reload` and `@reload_2` cycle the tabs. `<enter>` connects or disconnects the
selected device after confirmation. `@trust` (`ctrl-t`) toggles trust, `@pair`
(`ctrl-p`) pairs, `@scan` (`ctrl-b`) scans for eight seconds, and `@rm` removes
the device after confirmation. `@copy` copies the MAC address. The preview runs
`bluetoothctl info`.

### `niri`

Run Niri actions, or focus and close windows.

Requirements: `niri` and `jq` for the windows tab.

The actions tab is parsed from `niri msg action --help`; the windows tab reads
`niri msg -j windows`. `<enter>` runs the selected action, or focuses the window
from the windows tab. Actions that take arguments are sent without them and will
fail. `@rm` closes the selected window from the windows tab and reports an error
on the actions tab. `@copy` copies the action name or window id. The preview
shows the action's help or the window's JSON.

### `playerctl`

Control MPRIS media players.

Requirements: `playerctl`.

Rows come from `playerctl -a metadata`, one per active player. `<enter>` toggles
play/pause and reloads the list. `@next_track` (`ctrl-n`), `@prev_track`
(`ctrl-p`), and `@stop` (`ctrl-x`) control the selected player; `@copy` copies
the title, and `@copy_all` copies `artist - title`. The preview shows the
player's full metadata.

### `wireplumber`

Set the default PipeWire node, mute it, or change its volume.

Requirements: `wpctl` (WirePlumber).

Rows come from the Audio section of `wpctl status`: sinks, sources, and active
streams. Devices and filters are skipped. `<enter>` sets the selected node as the default after confirmation.
`@mute` (`ctrl-m`) toggles mute, and `@vol_up` / `@vol_down` (`ctrl-up` /
`ctrl-down`, or `alt-k` / `alt-j`) change the volume by 5%, limiting the maximum
to 150%. `@copy` copies the node id. The preview shows `wpctl get-volume` and
the node's `wpctl inspect` output.
