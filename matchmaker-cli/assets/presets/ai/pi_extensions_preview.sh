#!/usr/bin/env sh
# Preview pane for the pi extensions manager preset (pi_extensions.toml).
#
# Usage:
#   pi_extensions_preview.sh <source>        full preview: definition, install
#                                           location, and installed scope
#   pi_extensions_preview.sh --dir <source>  print just the install directory
#                                           (used by the README preview pane)

set -u

# extension_dir <source>
# Resolve the on-disk install location of a pi package/extension source.
#
# pi installs packages to different roots depending on scope:
#   global settings  -> ~/.pi/agent/
#   project settings -> <project>/.pi/        (project wins over global)
#
# Layout inside the root:
#   git sources  -> git/<host>/<user>/<repo>   (a pinned @ref is a checkout
#                                               ref, not a path segment)
#   npm sources  -> npm/node_modules/<name>    (a version pin is not part of
#                                               the package name)
#   local paths  -> used in place (absolute, ~-relative, or relative to the
#                   settings file's base)
#
# Prints the first existing install directory (project scope first, then
# global). Returns 1 when the source is not installed anywhere.
extension_dir() {
	src=$1
	agent_dir=$HOME/.pi/agent
	project_dir=$PWD/.pi

	# git_rel <source-without-git:-prefix> -> prints <host>/<user>/<repo>, ref stripped
	git_rel() {
		s=$1
		case $s in
		*://*) # https://host/user/repo@ref, ssh://git@host/user/repo
			host=${s#*://}
			host=${host%%/*}
			host=${host##*@} # drop userinfo (git@, token@)
			host=${host%%:*} # drop :port
			path=${s#*://}
			path=${path#*/}
			;;
		git@*:*) # git@host:user/repo@ref (scp-like)
			host=${s#git@}
			host=${host%%:*}
			path=${s#*:}
			;;
		*) # host/user/repo@ref (git: shorthand)
			host=${s%%/*}
			path=${s#*/}
			;;
		esac
		# hosts must be sane; anything else (e.g. a colon typo) is not a git source
		case $host in '' | *[!A-Za-z0-9._-]*) return 1 ;; esac
		[ -n "$path" ] || return 1
		path=${path%%@*} # strip the @ref (same first-@ rule pi uses)
		[ -n "$path" ] || return 1
		printf '%s/%s\n' "$host" "$path"
	}

	# npm_name <source> -> prints the package name without a version pin
	npm_name() {
		printf '%s' "${1#npm:}" | sed -e 's/@[^/@]*$//'
	}

	case $src in
	npm:*)
		name=$(npm_name "$src")
		for root in "$project_dir" "$agent_dir"; do
			if [ -d "$root/npm/node_modules/$name" ]; then
				printf '%s\n' "$root/npm/node_modules/$name"
				return 0
			fi
		done
		;;
	git:*)
		rel=$(git_rel "${src#git:}") || return 1
		for root in "$project_dir" "$agent_dir"; do
			if [ -d "$root/git/$rel" ]; then
				printf '%s\n' "$root/git/$rel"
				return 0
			fi
		done
		;;
	http://* | https://* | ssh://* | git://*)
		rel=$(git_rel "$src") || return 1
		for root in "$project_dir" "$agent_dir"; do
			if [ -d "$root/git/$rel" ]; then
				printf '%s\n' "$root/git/$rel"
				return 0
			fi
		done
		;;
	*)
		# local path: absolute, ~-relative, or relative to a settings base
		case $src in
		/*)
			[ -e "$src" ] && {
				printf '%s\n' "$src"
				return 0
			}
			;;
		~/*)
			d=$HOME/${src#~/}
			[ -e "$d" ] && {
				printf '%s\n' "$d"
				return 0
			}
			;;
		esac
		for root in "$agent_dir" "$project_dir" "$PWD"; do
			if [ -e "$root/$src" ]; then
				printf '%s\n' "$root/$src"
				return 0
			fi
		done
		;;
	esac
	return 1
}

# preview_full <source>: definition + install location + installed scope
preview_full() {
	name=$1
	MMX="${MM_EXTENSIONS_FILE:-$HOME/.pi/agent/mm_extensions}"
	GLOBAL="$HOME/.pi/agent/settings.json"
	LOCAL="$PWD/.pi/settings.json"

	defn=
	if [ -f "$MMX" ]; then
		while IFS= read -r line || [ -n "$line" ]; do
			case "$line" in
			'#'*)
				s="${line#"# "}"
				s="${s#"#"}"
				;;
			*) s="$line" ;;
			esac
			n=$(printf '%s' "${s%,}" | jq -r 'if type == "object" then .source else . end' 2>/dev/null || true)
			if [ "$n" = "$name" ]; then
				defn=$s
				break
			fi
		done <"$MMX"
	fi

	defined() {
		[ -f "$1" ] || return 1
		if jq -e . "$1" >/dev/null 2>&1; then
			jq -e --arg n "$name" '[.packages[]?, .extensions[]? | if type == "object" then (.source // null) else . end] | index($n) != null' "$1" >/dev/null 2>&1
		else
			# tolerate trailing commas from manual edits (jsonc-ish) before parsing
			perl -0777 -pe 's/,\s*([}\]])/$1/g' "$1" |
				jq -e --arg n "$name" '[.packages[]?, .extensions[]? | if type == "object" then (.source // null) else . end] | index($n) != null' >/dev/null 2>&1
		fi
	}

	if [ -n "$defn" ]; then
		printf '%s' "${defn%,}" | jq -c .
	fi
	echo "---"
	# install location: pi clones git sources to <root>/git/<host>/<user>/<repo>
	# (a pinned @ref is a checkout ref, not a path segment) and npm sources to
	# <root>/npm/node_modules/<name> (a version pin is not part of the name);
	# <root> is ~/.pi/agent for global settings and .pi/ for project settings.
	if dir=$(extension_dir "$name" 2>/dev/null); then
		echo "location: $dir"
		case "$name" in
		npm:*)
			jq -r '"version: " + (.version // "?") + (if (.description // "") != "" then " — " + .description else "" end)' "$dir/package.json" 2>/dev/null || true
			;;
		esac
	else
		case "$name" in
		npm:*) reason="(npm)" ;;
		*)
			reason=""
			if [ -n "$defn" ] && printf '%s' "${defn%,}" | jq -e '.lazy == true' >/dev/null 2>&1; then
				reason="(lazy)"
			fi
			;;
		esac
		echo "location: ✗${reason:+ $reason}"
	fi
	if defined "$GLOBAL" && defined "$LOCAL"; then
		echo "installed: global | local"
	elif defined "$GLOBAL"; then
		echo "installed: global"
	elif defined "$LOCAL"; then
		echo "installed: local"
	else
		echo "installed: ✗"
	fi
}

name=${1:-}
if [ "$name" = "--dir" ]; then
	name=${2:-}
	[ -n "$name" ] || exit 0
	extension_dir "$name" || exit 1
	exit 0
fi
[ -n "$name" ] || exit 0

preview_full "$name"
