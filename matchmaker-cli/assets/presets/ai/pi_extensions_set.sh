#!/usr/bin/env bash
# Apply the enabled extension definitions to pi's settings.
#
# Replaces the `packages` array in the target settings file with the enabled
# (uncommented) definitions from the extension list (MMX), keeping the rest
# of the file intact. Trailing commas from manual edits are tolerated.
#
# Usage: pi_extensions_set.sh global|local [MMX]
#   global  -> write $HOME/.pi/agent/settings.json
#   local   -> write $PWD/.pi/settings.json
#   MMX     defaults to $MM_EXTENSIONS_FILE, else ~/.pi/agent/mm_extensions

set -e

case "${1:-}" in
global)
	SETTINGS="$HOME/.pi/agent/settings.json"
	;;
local)
	SETTINGS="$PWD/.pi/settings.json"
	;;
*)
	echo "usage: $0 global|local [MMX]" >&2
	exit 2
	;;
esac

MMX="${2:-${MM_EXTENSIONS_FILE:-$HOME/.pi/agent/mm_extensions}}"

[ -f "$MMX" ] || {
	echo "error: mm_extensions not found: $MMX" >&2
	exit 1
}
[ -f "$SETTINGS" ] || {
	echo "error: pi settings not found: $SETTINGS" >&2
	exit 1
}

tmp=$(mktemp "${TMPDIR:-/tmp}/mm_settings.XXXXXX")
trap 'rm -f -- "$tmp" "$tmp.enabled" "$tmp.out"' EXIT

# assemble the enabled (uncommented) definitions into a JSON array
{
	echo "["
	grep -v '^[[:space:]]*#' "$MMX" |
		grep -v '^[[:space:]]*$' |
		sed -E '$ s/,[[:space:]]*$//'
	echo "]"
} >"$tmp.enabled"

jq -e . "$tmp.enabled" >/dev/null || {
	echo "error: enabled extensions do not form valid JSON" >&2
	exit 1
}

# replace the `packages` array (keep the rest of the settings intact)
if jq -e . "$SETTINGS" >/dev/null 2>&1; then
	jq --slurpfile exts "$tmp.enabled" '.packages = $exts[0]' "$SETTINGS" >"$tmp.out" 2>/dev/null || true
else
	# tolerate trailing commas from manual edits (jsonc-ish) before parsing
	perl -0777 -pe 's/,\s*([}\]])/$1/g' "$SETTINGS" |
		jq --slurpfile exts "$tmp.enabled" '.packages = $exts[0]' >"$tmp.out" 2>/dev/null || true
fi

[ -s "$tmp.out" ] || {
	echo "error: failed to update $SETTINGS" >&2
	exit 1
}

mv "$tmp.out" "$SETTINGS"
echo "wrote $(jq '.packages | length' "$SETTINGS") extension(s) to $SETTINGS"
