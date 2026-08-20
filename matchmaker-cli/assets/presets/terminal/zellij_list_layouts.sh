#!/usr/bin/env bash
# Emit one TSV row per single-tab zellij layout:  name \t source \t path
#
#   name     layout name -- addressable by bare name, eg.
#            `zellij --layout <name>`, `zellij action override-layout <name>`,
#            `zellij action new-tab -l <name>`, `zellij setup --dump-layout <name>`
#   source   "user" | "builtin"
#   path     absolute path of a user layout ("" for built-in layouts)
#
# Only layouts that affect a single tab are listed: user layouts whose KDL has
# at most one top-level `tab` block, plus the single-tab built-ins
# (default, strider, compact, classic -- the ones `setup --dump-layout` renders).

layout_dir=$(zellij setup --check 2>/dev/null | sed -n 's/^\[LAYOUT DIR\]: "\(.*\)"/\1/p')
[ -n "$layout_dir" ] || layout_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zellij/layouts"

if [ -d "$layout_dir" ]; then
    # user layouts first (single-tab only; swap variants skipped)
    find "$layout_dir" -maxdepth 1 -type f -name '*.kdl' 2>/dev/null | sort |
        while IFS= read -r f; do
            base="${f##*/}"
            case "$base" in *.swap.kdl) continue ;; esac
            tabs=$(awk 'match($0, /^[ \t]*tab([ \t]|$)/) && index($0, "{") { c++ } END { print c+0 }' "$f")
            [ "$tabs" -le 1 ] || continue
            printf '%s\t%s\t%s\n' "${base%.kdl}" user "$f"
        done
fi
for name in default strider compact classic; do
    printf '%s\t%s\t%s\n' "$name" builtin ""
done
