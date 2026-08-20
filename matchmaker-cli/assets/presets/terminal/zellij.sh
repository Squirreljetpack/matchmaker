#!/usr/bin/env bash
# shellcheck disable=SC2016 # single-quoted snippets expand at runtime (awk / jq filters)
# All shell logic behind the terminal/zellij_* presets -- one subcommand per
# function the presets need, TSV rows on stdout (\t = tab separator):
#
#   zellij.sh sessions           every session, active and otherwise (newest first)
#                                age \t session \t "" \t "" \t state(EXITED|current|"")
#   zellij.sh current            panes of the current session except ours, by pane id
#                                age \t session \t pane-id \t tab \t title
#   zellij.sh other              panes of every non-EXITED, non-current session,
#                                by session then pane id
#                                age \t session \t pane-id \t tab \t title
#   zellij.sh layouts [name...]  single-tab layouts from the user layout dir,
#                                plus any extra layout names to include
#                                file("" for non-user) \t name \t source(user|builtin)
#   zellij.sh has-current-panes  no output; exit 0 iff `current` would list at
#                                least one pane (ring navigation skips it otherwise)
#   zellij.sh has-other-panes    no output; exit 0 iff `other` would list at
#                                least one pane (ring navigation skips it otherwise)

set -u

# capture at top level: some interpreters (zsh's FUNCTION_ARGZERO) redefine $0
# to the function name inside functions; absolutize so the --pane-rows re-exec
# works regardless of how we were invoked
script=$0
case $script in
    /*) ;;
    *) script=$PWD/$script ;;
esac

current="${ZELLIJ_SESSION_NAME:-}"

# name \t age \t state, one row per session, EXITED included
list_sessions() {
    zellij list-sessions --no-formatting 2>/dev/null | awk '
    {
        name = $1
        state = ""
        if ($0 ~ /\(EXITED/) state = "EXITED"
        else if ($0 ~ /\(current\)/) state = "current"
        age = $0
        sub(/^[^[]*\[Created /, "", age)
        sub(/ ago\].*$/, "", age)
        print name "\t" age "\t" state
    }' | tr -d ' '
}

# one age \t session \t id \t tab \t title row per terminal pane of $1 ($2 =
# its age); pass a third arg to drop the pane we're in ($ZELLIJ_PANE_ID --
# terminal ids are unique session-wide, so the id guard can't over-match)
pane_rows() {
    local sess=$1 age=$2
    local filter='select(.is_plugin == false)'
    if [ $# -ge 3 ]; then
        filter='select(.is_plugin == false)
                | select((env.MYPANE == "") or ((.id|tostring) != env.MYPANE))'
    fi
    zellij -s "$sess" action list-panes --json 2>/dev/null |
        SESS="$sess" AGE="$age" MYPANE="${ZELLIJ_PANE_ID:-}" \
            jq -r ".[]
                  | $filter
                  | \"\(env.AGE)\t\(env.SESS)\t\(.id|tostring)\t\(.tab_name)\t\(.title)\""
}

cmd_sessions() {
    # newest first, so the current/active sessions land at the top
    list_sessions | awk -F'\t' -v OFS='\t' '{ rows[NR] = $2 "\t" $1 "\t\t\t" $3 }
         END { for (i = NR; i >= 1; i--) print rows[i] }'
}

cmd_current() {
    [ -n "$current" ] || exit 0
    age=$(list_sessions | awk -F'\t' -v n="$current" '$1 == n { print $2; exit }')
    pane_rows "$current" "$age" mine | sort -k3,3n
}

cmd_other() {
    # panes of every non-EXITED session other than the current one, probed in
    # parallel; each probe re-execs this script (--pane-rows) so pane_rows
    # runs under the interpreter the shebang picked, whatever shell we were
    # started with
    list_sessions |
        awk -F'\t' -v OFS='\t' -v cur="$current" '$3 != "EXITED" && $1 != cur { print $1, $2 }' |
        xargs -r -n 2 -P 10 "$script" --pane-rows |
        sort -k2,2 -k3,3n
}

cmd_layouts() {
    # Only user layouts that affect a single tab (KDL with at most one
    # top-level `tab` block; swap variants skipped). Any name arguments are
    # included as-is: a bare name resolves to the user layout dir first, then
    # zellij's embedded defaults -- the same resolution `override-layout` /
    # `setup --dump-layout` use.
    layout_dir=$(zellij setup --check 2>/dev/null | sed -n 's/^\[LAYOUT DIR\]: "\(.*\)"/\1/p')
    [ -n "$layout_dir" ] || layout_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zellij/layouts"

    if [ -d "$layout_dir" ]; then
        find "$layout_dir" -maxdepth 1 -type f -name '*.kdl' 2>/dev/null | sort |
            while IFS= read -r f; do
                base="${f##*/}"
                case "$base" in *.swap.kdl) continue ;; esac
                tabs=$(awk 'match($0, /^[ \t]*tab([ \t]|$)/) && index($0, "{") { c++ } END { print c+0 }' "$f")
                [ "$tabs" -le 1 ] || continue
                printf '%s\t%s\t%s\n' "$f" "${base%.kdl}" user
            done
    fi
    for name in "$@"; do
        if [ -f "$layout_dir/$name.kdl" ]; then
            printf '%s\t%s\t%s\n' "$layout_dir/$name.kdl" "$name" user
        else
            printf '%s\t%s\t%s\n' "" "$name" builtin
        fi
    done
}

cmd_has_current_panes() {
    [ -n "$current" ] || exit 1
    zellij -s "$current" action list-panes --json 2>/dev/null |
        MYPANE="${ZELLIJ_PANE_ID:-}" jq -e '.[]
            | select(.is_plugin == false)
            | select((env.MYPANE == "") or ((.id|tostring) != env.MYPANE))' >/dev/null
}

cmd_has_other_panes() {
    [ -n "$(list_sessions |
        awk -F'\t' -v OFS='\t' -v cur="$current" '$3 != "EXITED" && $1 != cur { print $1, $2 }' |
        xargs -r -n 2 -P 10 "$script" --pane-rows)" ]
}

case "${1:-}" in
    sessions)          cmd_sessions ;;
    current)           cmd_current ;;
    other)             cmd_other ;;
    layouts)           cmd_layouts "${@:2}" ;;
    has-current-panes) cmd_has_current_panes ;;
    has-other-panes)   cmd_has_other_panes ;;
    --pane-rows)       shift; pane_rows "$@" ;;   # internal: xargs probe entry point
    *)
        echo "usage: zellij.sh {sessions|current|other|layouts|has-current-panes|has-other-panes}" >&2
        exit 2
        ;;
esac
