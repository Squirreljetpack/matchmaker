#!/usr/bin/env bash
set -euo pipefail

minimize=false
window_id=""

while (($#)); do
    case "$1" in
        --minimize)
            minimize=true
            shift
            ;;
        --id)
            window_id="${2:-}"
            shift 2
            ;;
        *)
            window_id="$1"
            shift
            ;;
    esac
done

if [[ -z "$window_id" ]]; then
    echo "Error: window id is required." >&2
    exit 1
fi

active_window_id="$(xdotool getactivewindow)"
active_window_title="$(xdotool getwindowname "$active_window_id")"

wmctrl -ia "$window_id"

if [[ "$minimize" == true && -n "$active_window_id" ]]; then
    xdotool windowminimize "$active_window_id"
fi
