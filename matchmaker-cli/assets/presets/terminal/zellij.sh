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
#   zellij.sh session-preview <session> [state] [age]
#                                tab summary with pane titles for the preview pane;
#                                live list-panes for running, metadata.kdl for EXITED
#   zellij.sh dump-pane [session] [pane-id]
#                                screen dump of a pane or active pane in session
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

# Tab summary for the session picker preview.
# Running sessions use live list-panes data; EXITED sessions fall back to
# cached session-metadata.kdl, then to tab names from session-layout.kdl.
cmd_session_preview() {
    local sess=$1
    local state="" age=""
    if [ $# -ge 3 ]; then
        state=$2
        age=$3
    elif [ $# -eq 2 ]; then
        case "$2" in
            EXITED|current) state=$2 ;;
            *)              age=$2 ;;
        esac
    fi

    # header: italicize session name, session (created ... ago)
    printf '\033[3m%s\033[0m' "$sess"
    [ -n "$age" ] && printf ' (created %s ago)' "$age"
    [ -n "$state" ] && printf '  [%s]' "$state"
    printf '\n\n'

    # --- running: live tab summary via list-panes ---
    if [ "$state" != "EXITED" ]; then
        local tabs
        tabs=$(zellij -s "$sess" action list-panes --json 2>/dev/null | jq -r '
            [group_by(.tab_position)[]
             | {t: .[0].tab_name, p: .[0].tab_position,
                n: [.[] | select(.is_plugin == false) | .title]}]
            | sort_by(.p)
            | map("\u001b[1m\(.t)\u001b[0m:\n" + ([.n[] | "  " + .] | join("\n")))
            | join("\n\n")
        ' 2>/dev/null) || tabs=""
        if [ -n "$tabs" ]; then
            printf '%s\n' "$tabs"
            return
        fi
    fi

    # --- fallback 1: cached session-metadata.kdl ---
    local cache="${ZELLIJ_CACHE_DIR:-${XDG_CACHE_HOME:-$HOME/.cache}/zellij}"
    local meta=""
    if [ -d "$cache" ]; then
        for dir in "$cache"/*/; do
            [ -f "$dir/session_info/$sess/session-metadata.kdl" ] &&
                { meta="$dir/session_info/$sess/session-metadata.kdl"; break; }
        done
    fi
    if [ -n "$meta" ]; then
        awk '
function unquote(line,    i, rest, j) {
    i = index(line, "\"")
    if (i == 0) return ""
    rest = substr(line, i + 1)
    j = index(rest, "\"")
    if (j == 0) return rest
    return substr(rest, 1, j - 1)
}
/^tabs \{/  { sec = "tabs"; next }
/^panes \{/ { sec = "panes"; next }
/^}/ { sec = ""; next }
sec == "tabs" && /tab \{/ {
    blk = "tab"; t_pos = -1; t_name = ""; next
}
sec == "tabs" && blk == "tab" {
    if ($1 == "position") t_pos = $2 + 0
    if ($1 == "name") t_name = unquote($0)
}
sec == "tabs" && blk == "tab" && /^    }/ {
    if (t_pos >= 0) { tn[t_pos] = t_name; to[nt++] = t_pos }
    blk = ""
}
sec == "panes" && /pane \{/ {
    blk = "pane"; pp = ""; pt = ""; px = -1; next
}
sec == "panes" && blk == "pane" {
    if ($1 == "is_plugin") pp = $2
    if ($1 == "title") pt = unquote($0)
    if ($1 == "tab_position") px = $2 + 0
}
sec == "panes" && blk == "pane" && /^    }/ {
    if (pp == "false" && px >= 0) {
        idx = pc[px]++
        panes[px, idx] = pt
    }
    blk = ""
}
END {
    for (i = 0; i < nt; i++) {
        p = to[i]; n = pc[p] + 0
        if (i > 0) printf "\n"
        printf "\033[1m%s\033[0m:\n", tn[p]
        for (j = 0; j < n; j++) {
            printf "  %s\n", panes[p, j]
        }
    }
}' "$meta"
        return
    fi

    # --- fallback 2: tabs & panes from layout KDL ---
    local layout=""
    if [ -d "$cache" ]; then
        for dir in "$cache"/*/; do
            [ -f "$dir/session_info/$sess/session-layout.kdl" ] &&
                { layout="$dir/session_info/$sess/session-layout.kdl"; break; }
        done
    fi
    if [ -n "$layout" ]; then
        awk '
function get_attr(line, attr,    s, m) {
    if (match(line, attr "=\"[^\"]*\"")) {
        s = substr(line, RSTART, RLENGTH)
        sub("^" attr "=\"", "", s)
        sub("\"$", "", s)
        return s
    }
    return ""
}
BEGIN {
    tcount = 0
}
/^[ \t]*new_tab_template/ { in_template = 1; next }
/^[ \t]*tab[ \t]/ || /^[ \t]*tab\{/ {
    if (in_template) next
    in_tab = 1
    tname = get_attr($0, "name")
    if (tname == "") tname = "Tab #" (tcount + 1)
    tabs[tcount] = tname
    tab_panes_cnt[tcount] = 0
    tcount++
    next
}
in_tab && /plugin location=/ { next }
in_tab && /^[ \t]*pane([ \t]|$)/ {
    if (in_template) next
    if ($0 ~ /plugin[ \t]/ || $0 ~ /size=1[ \t]+borderless=true/) next
    pname = get_attr($0, "name")
    pcmd = get_attr($0, "command")
    pcwd = get_attr($0, "cwd")
    item = pname
    if (item == "" && pcmd != "") item = pcmd
    if (item == "" && pcwd != "") item = pcwd
    if (item == "") item = "terminal"
    
    cur_t = tcount - 1
    idx = tab_panes_cnt[cur_t]++
    tab_panes[cur_t, idx] = item
}
END {
    if (tcount == 0) {
        print "  (no tabs found in saved layout)"
    } else {
        for (i = 0; i < tcount; i++) {
            if (i > 0) printf "\n"
            printf "\033[1m%s\033[0m:\n", tabs[i]
            cnt = tab_panes_cnt[i]
            for (j = 0; j < cnt; j++) {
                printf "  %s\n", tab_panes[i, j]
            }
        }
    }
}' "$layout"
    else
        echo "  (no cached data)"
    fi
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

cmd_dump_pane() {
    local sess=${1:-}
    local pane_id=${2:-}
    if [ -z "$pane_id" ] && [ -n "$sess" ]; then
        pane_id=$(zellij -s "$sess" action list-panes --json 2>/dev/null | jq -r '.[] | select(.is_focused == true and .is_plugin == false) | .id' 2>/dev/null) || pane_id=""
    fi
    if [ -n "$sess" ]; then
        if [ -n "$pane_id" ]; then
            zellij -s "$sess" action dump-screen --pane-id "$pane_id" 2>/dev/null && return
        fi
        zellij -s "$sess" action dump-screen 2>/dev/null
    else
        if [ -n "$pane_id" ]; then
            zellij action dump-screen --pane-id "$pane_id" 2>/dev/null && return
        fi
        zellij action dump-screen 2>/dev/null
    fi
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
    session-preview)   shift; cmd_session_preview "$@" ;;
    dump-pane)         shift; cmd_dump_pane "$@" ;;
    has-current-panes) cmd_has_current_panes ;;
    has-other-panes)   cmd_has_other_panes ;;
    --pane-rows)       shift; pane_rows "$@" ;;   # internal: xargs probe entry point
    *)
        echo "usage: zellij.sh {sessions|current|other|layouts|session-preview|dump-pane|has-current-panes|has-other-panes}" >&2
        exit 2
        ;;
esac
