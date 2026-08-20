#!/usr/bin/env bash
# shellcheck disable=SC2016 # single-quoted snippets expand at runtime (awk / inner bash -c)
# Emit one TSV row per item for the requested mode ($1):
#
#   session   every zellij session, active and otherwise (the initial start):
#             pane/tab are empty, title holds the state (EXITED / current).
#   current   every pane of the current session ($ZELLIJ_SESSION_NAME) except
#             the one we're sitting in.
#   other     every pane of sessions other than the current one (default).
#
# Row shape (all modes):  age \t session \t pane-id \t tab \t title
# EXITED sessions and the current session are dropped in "other" mode only.

mode="${1:-other}"
current="${ZELLIJ_SESSION_NAME:-}"

sessions=$(mktemp /tmp/mm-zellij-sessions.XXXXXX)
trap 'rm -f "$sessions"' EXIT

# name \t age \t state   for every session, EXITED included
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
}' | tr -d ' ' >"$sessions"

case "$mode" in
session)
    # newest first, so the current/active sessions land at the top
    awk -F'\t' -v OFS='\t' '{ rows[NR] = $2 "\t" $1 "\t\t\t" $3 }
         END { for (i = NR; i >= 1; i--) print rows[i] }' "$sessions"
    ;;
current)
    # drop the pane we're in ($ZELLIJ_PANE_ID); terminal ids
    # are unique among terminals session-wide, so the id guard can't over-match
    [ -n "$current" ] || exit 0
    age=$(awk -F'\t' -v n="$current" '$1 == n { print $2; exit }' "$sessions")
    zellij -s "$current" action list-panes --json 2>/dev/null |
        SESS="$current" AGE="$age" MYPANE="${ZELLIJ_PANE_ID:-}" \
            jq -r '.[]
                  | select(.is_plugin == false)
                  | select((env.MYPANE == "") or ((.id|tostring) != env.MYPANE))
                  | "\(env.AGE)\t\(env.SESS)\t\(.id|tostring)\t\(.tab_name)\t\(.title)"' |
        sort -k3,3n
    ;;
other)
    # panes of every non-EXITED session other than the current one, in parallel
    awk -F'\t' -v OFS='\t' -v cur="$current" '$3 != "EXITED" && $1 != cur { print $1, $2 }' "$sessions" |
        xargs -n 2 -P 10 bash -c '
            sess="$1"; age="$2"
            zellij -s "$sess" action list-panes --json 2>/dev/null |
            SESS="$sess" AGE="$age" \
            jq -r ".[] | select(.is_plugin == false) | \"\(env.AGE)\t\(env.SESS)\t\(.id|tostring)\t\(.tab_name)\t\(.title)\""
        ' _ | sort -k2,2 -k3,3n
    ;;
esac
