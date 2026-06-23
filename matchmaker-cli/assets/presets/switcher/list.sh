#!/usr/bin/env bash
set -euo pipefail

active_window_id="$(xdotool getactivewindow 2>/dev/null || true)"
active_window_id_hex=""
if [[ -n "$active_window_id" ]]; then
    active_window_id_hex=$(printf '0x%08x' "$active_window_id")
fi

wmctrl -lxp | while IFS= read -r line; do
    read -r window_id _desktop pid wm_class _host title <<<"$line"
    [[ -z "${window_id:-}" || -z "${title:-}" ]] && continue
    [[ -n "$active_window_id_hex" && "$window_id" == "$active_window_id_hex" ]] && continue

    app="${wm_class##*.}"
    [[ -z "$app" || "$app" == "$wm_class" ]] && app="$wm_class"

    printf '%s\n%s\n%s\n%s\0' "$title" "$app" "$pid" "$window_id"
done
