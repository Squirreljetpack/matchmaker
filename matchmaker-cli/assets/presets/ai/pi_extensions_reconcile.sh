#!/usr/bin/env bash
# Reconcile the extension list (MMX) with pi's settings.
#
# Definitions present in pi's global (~/.pi/agent/settings.json) or project
# (.pi/settings.json) settings are enabled in MMX (appended when missing from
# the file); definitions not defined anywhere are commented out. If MMX does
# not exist yet it is created from the settings (empty when there are none).
#
# Usage: pi_extensions_reconcile.sh [MMX]
#   MMX defaults to $MM_EXTENSIONS_FILE, else ~/.pi/agent/mm_extensions

set -e

MMX="${1:-${MM_EXTENSIONS_FILE:-$HOME/.pi/agent/mm_extensions}}"
GLOBAL="$HOME/.pi/agent/settings.json"
LOCAL="$PWD/.pi/settings.json"

# collect every entry defined in the global or project settings as
# "<source>	<compact json>" pairs (global first, then local, deduped by source)
entries() {
	[ -f "$1" ] || return 0
	if jq -e . "$1" >/dev/null 2>&1; then
		jq -r '(.packages[]?, .extensions[]?) | if type == "object" then .source else . end as $s | select($s != "") | [$s, @json] | @tsv' "$1" 2>/dev/null || true
	else
		# tolerate trailing commas from manual edits (jsonc-ish) before parsing
		perl -0777 -pe 's/,\s*([}\]])/$1/g' "$1" |
			jq -r '(.packages[]?, .extensions[]?) | if type == "object" then .source else . end as $s | select($s != "") | [$s, @json] | @tsv' 2>/dev/null || true
	fi
	return 0
}

tmp=$(mktemp "${TMPDIR:-/tmp}/mm_reconcile.XXXXXX")
trap 'rm -f -- "$tmp" "$tmp.pairs" "$tmp.defined" "$tmp.names" "$tmp.out"' EXIT

{
	entries "$GLOBAL"
	entries "$LOCAL"
} | awk -F '\t' '!seen[$1]++' >"$tmp.pairs"
awk -F '\t' '{print $1}' "$tmp.pairs" | sort -u >"$tmp.defined"

: >"$tmp.names"

# keep enabled what the settings define, comment out the rest
if [ -f "$MMX" ]; then
	while IFS= read -r line || [ -n "$line" ]; do
		case "$line" in
		'#'*)
			stripped="${line#"# "}"
			stripped="${stripped#"#"}"
			;;
		*)
			stripped="$line"
			;;
		esac

		case "$stripped" in
		'' | *[![:space:]]*) ;;
		*)
			printf '%s\n' "$line"
			continue
			;;
		esac

		name=$(printf '%s' "${stripped%,}" | jq -r 'if type == "object" then .source else . end' 2>/dev/null || true)

		if [ -n "$name" ] && grep -qxF -- "$name" "$tmp.defined"; then
			# defined somewhere: make sure it is present and enabled
			printf '%s\n' "$name" >>"$tmp.names"
			case "$line" in '#'*) printf '%s\n' "$stripped" ;; *) printf '%s\n' "$line" ;; esac
		else
			# not defined anywhere: make sure it is disabled
			case "$line" in '#'*) printf '%s\n' "$line" ;; *) printf '# %s\n' "$line" ;; esac
		fi
	done <"$MMX" >"$tmp.out"
else
	: >"$tmp.out"
fi

# merge in definitions that are active in the settings but missing from mm_extensions
IFS="$(printf '\t')"
while read -r src entry; do
	if [ -n "$entry" ] && ! grep -qxF -- "$src" "$tmp.names"; then
		printf '%s,\n' "$entry" >>"$tmp.out"
	fi
done <"$tmp.pairs"
unset IFS

mkdir -p "$(dirname -- "$MMX")"
mv "$tmp.out" "$MMX"
echo "reconciled: $(awk '!/^#/ && NF' "$MMX" | wc -l | tr -d ' ') enabled of $(awk 'NF' "$MMX" | wc -l | tr -d ' ') definitions"
